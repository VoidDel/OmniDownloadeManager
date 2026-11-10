//! The HTTP engine against a local server.

mod common;

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use common::{assert_content, start};
use omnidl_core::{
    Checkpoint, ConnControl, ConnMode, DownloadEngine, ErrorCode, Job, JobOptions, Outcome,
    Progress, RateLimiter, Result, ResumeState,
};
use omnidl_http::HttpEngine;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MIB: u64 = 1 << 20;

/// Keeps what the engine reports.
#[derive(Default)]
struct Recorder {
    started: Mutex<Option<ResumeState>>,
    saved: Mutex<Option<ResumeState>>,
    saves: AtomicUsize,
}

#[async_trait]
impl Checkpoint for Recorder {
    async fn started(&self, s: &ResumeState) -> Result<()> {
        *self.started.lock() = Some(s.clone());
        *self.saved.lock() = Some(s.clone());
        Ok(())
    }

    async fn save(&self, s: &ResumeState) -> Result<()> {
        *self.saved.lock() = Some(s.clone());
        self.saves.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct Run {
    job: Job,
    recorder: Arc<Recorder>,
    cancel: CancellationToken,
    progress: Arc<Progress>,
    conns: Arc<ConnControl>,
}

fn job(url: &str, dir: &Path, resume: Option<ResumeState>, connections: u32) -> Run {
    let recorder = Arc::new(Recorder::default());
    let cancel = CancellationToken::new();
    let progress = Arc::new(Progress::default());
    let conns = Arc::new(ConnControl::default());
    let job = Job {
        id: Uuid::new_v4(),
        source: url.into(),
        save_dir: dir.to_path_buf(),
        file_name: None,
        resume,
        options: JobOptions {
            connections,
            auto_connections: false,
            ..JobOptions::default()
        },
        cancel: cancel.clone(),
        progress: progress.clone(),
        task_limit: RateLimiter::new(None),
        global_limit: RateLimiter::new(None),
        checkpoint: recorder.clone(),
        conns: conns.clone(),
    };
    Run {
        job,
        recorder,
        cancel,
        progress,
        conns,
    }
}

fn engine() -> HttpEngine {
    HttpEngine::with_retry_delay(Duration::from_millis(20))
}

fn completed(o: Outcome) -> (String, u64) {
    match o {
        Outcome::Completed { file_name, total } => (file_name, total),
        other => panic!("not completed: {other:?}"),
    }
}

/// Waits until `pred` holds or panics after 20 s.
async fn until(pred: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !pred() {
        assert!(Instant::now() < deadline, "timed out");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_over_many_connections() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 24 * MIB + 12345;
    let r = job(&server.url(&format!("/file/{size}")), dir.path(), None, 8);
    let (name, total) = completed(engine().run(r.job).await);
    assert_eq!((name.as_str(), total), (size.to_string().as_str(), size));
    assert_content(&dir.path().join(&name), size);
    assert!(!dir.path().join(format!("{name}.odm")).exists());
    assert!(
        server.stats.max_open.load(Ordering::SeqCst) > 1,
        "used several connections"
    );
    let started = r.recorder.started.lock().clone().unwrap();
    assert!(started.resumable);
    assert_eq!(started.segments.len(), 8);
    assert_eq!(started.etag.as_deref(), Some("\"v0\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn uses_one_connection_without_range_support() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 5 * MIB;
    let r = job(
        &server.url(&format!("/norange/{size}")),
        dir.path(),
        None,
        8,
    );
    let (name, _) = completed(engine().run(r.job).await);
    assert_content(&dir.path().join(name), size);
    let started = r.recorder.started.lock().clone().unwrap();
    assert!(!started.resumable);
    assert_eq!(started.segments.len(), 1, "one connection");
    // The probe (whose one-byte request got the whole file, dropped unread) and the download.
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn probes_with_a_ranged_get_when_head_is_refused() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 3 * MIB;
    let r = job(&server.url(&format!("/nohead/{size}")), dir.path(), None, 4);
    let (name, _) = completed(engine().run(r.job).await);
    assert_content(&dir.path().join(name), size);
    assert!(r.recorder.started.lock().as_ref().unwrap().resumable);
}

#[tokio::test(flavor = "multi_thread")]
async fn retries_connections_that_break_off() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 4 * MIB;
    let r = job(&server.url(&format!("/flaky/{size}")), dir.path(), None, 4);
    let (name, _) = completed(engine().run(r.job).await);
    assert_content(&dir.path().join(name), size);
    assert!(
        server.stats.flaky_left.load(Ordering::SeqCst) <= 0,
        "bodies broke off"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pauses_and_resumes_where_it_stopped() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 8 * MIB;
    let url = server.url(&format!("/slow/{size}"));
    let r = job(&url, dir.path(), None, 4);
    let progress = r.progress.clone();
    let cancel = r.cancel.clone();
    let run = tokio::spawn(async move { engine().run(r.job).await });
    until(|| progress.downloaded() >= MIB).await;
    cancel.cancel();
    let state = match run.await.unwrap() {
        Outcome::Stopped(s) => s,
        other => panic!("{other:?}"),
    };
    let done = state.downloaded();
    assert!(done >= MIB && done < size, "{done}");
    assert_eq!(state.segments.len(), 4);

    server.stats.ranges.lock().clear();
    let r = job(&url, dir.path(), Some(state.clone()), 4);
    let (name, _) = completed(engine().run(r.job).await);
    assert_eq!(name, state.file_name);
    assert_content(&dir.path().join(name), size);
    // Every request of the second run continued a segment, none restarted one.
    let starts = server.stats.ranges.lock().clone();
    let segment_starts: Vec<u64> = state.segments.iter().map(|s| s.start).collect();
    assert!(
        starts.iter().all(|s| !segment_starts.contains(s)
            || state
                .segments
                .iter()
                .any(|g| g.start == *s && g.downloaded == 0)),
        "{starts:?} vs {:?}",
        state.segments
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn resumes_from_the_last_checkpoint_after_a_crash() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 32 * MIB;
    let url = server.url(&format!("/slow/{size}"));
    let r = job(&url, dir.path(), None, 4);
    let recorder = r.recorder.clone();
    let run = tokio::spawn(async move { engine().run(r.job).await });
    until(|| recorder.saves.load(Ordering::SeqCst) >= 1).await;
    // Kill it: no final flush, no outcome.
    run.abort();
    let _ = run.await;
    let state = recorder.saved.lock().clone().unwrap();
    assert!(state.downloaded() > 0);

    let r = job(&url, dir.path(), Some(state), 4);
    let (name, _) = completed(engine().run(r.job).await);
    assert_content(&dir.path().join(name), size);
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_to_resume_a_changed_file() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let url = server.url(&format!("/changing/{}", 8 * MIB));
    let r = job(&url, dir.path(), None, 2);
    let progress = r.progress.clone();
    let cancel = r.cancel.clone();
    let run = tokio::spawn(async move { engine().run(r.job).await });
    until(|| progress.downloaded() >= 256 << 10).await;
    cancel.cancel();
    let Outcome::Stopped(state) = run.await.unwrap() else {
        panic!("not stopped")
    };
    server.stats.version.store(1, Ordering::SeqCst);
    let r = job(&url, dir.path(), Some(state), 2);
    match engine().run(r.job).await {
        Outcome::Failed(e, Some(kept)) => {
            assert_eq!(e.code, ErrorCode::RemoteChanged);
            assert!(
                kept.downloaded() > 0,
                "progress kept for the user to decide"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fails_at_once_on_not_found() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let r = job(&server.url("/missing"), dir.path(), None, 4);
    let t = Instant::now();
    match engine().run(r.job).await {
        Outcome::Failed(e, _) => {
            assert_eq!(e.code, ErrorCode::NotFound);
            assert_eq!(e.status, Some(404));
        }
        other => panic!("{other:?}"),
    }
    assert!(t.elapsed() < Duration::from_secs(2), "no retries");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn names_files_from_the_server_and_numbers_duplicates() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let info = engine()
        .probe(&server.url("/redirect"), &JobOptions::default())
        .await
        .unwrap();
    assert_eq!(info.file_name, "测试 文件.bin");
    assert!(
        info.final_url.ends_with("/named/1000"),
        "{}",
        info.final_url
    );
    assert_eq!(info.total, Some(1000));
    assert!(info.resumable);

    let first = completed(
        engine()
            .run(job(&server.url("/redirect"), dir.path(), None, 4).job)
            .await,
    );
    let second = completed(
        engine()
            .run(job(&server.url("/redirect"), dir.path(), None, 4).job)
            .await,
    );
    assert_eq!(first.0, "测试 文件.bin");
    assert_eq!(second.0, "测试 文件 (1).bin");
    assert_content(&dir.path().join(&second.0), 1000);
    // Dated like the server's copy.
    let modified = std::fs::metadata(dir.path().join(&first.0))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        httpdate::fmt_http_date(modified),
        "Wed, 21 Oct 2015 07:28:00 GMT"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_bodies_of_unknown_length_and_empty_files() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 3 * MIB + 7;
    let (name, total) = completed(
        engine()
            .run(
                job(
                    &server.url(&format!("/chunked/{size}")),
                    dir.path(),
                    None,
                    8,
                )
                .job,
            )
            .await,
    );
    assert_eq!(total, size);
    assert_content(&dir.path().join(name), size);

    let empty = tempfile::tempdir().unwrap();
    let (name, total) = completed(
        engine()
            .run(job(&server.url("/file/0"), empty.path(), None, 8).job)
            .await,
    );
    assert_eq!(total, 0);
    assert_content(&empty.path().join(name), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_to_the_speed_limit() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let r = job(&server.url(&format!("/file/{}", MIB)), dir.path(), None, 4);
    r.job.task_limit.set_rate(Some(400 << 10));
    let t = Instant::now();
    completed(engine().run(r.job).await);
    // 1 MiB at 400 KiB/s is 2.56 s, less the 0.25 s burst.
    let took = t.elapsed().as_secs_f64();
    assert!(took > 2.0, "{took}");
}

/// Phase 1 acceptance: a large file over 8 connections, killed at about 40 % and resumed from
/// its last checkpoint. Run on purpose (it writes 5 GiB by default; `OMNIDL_LARGE_GIB` changes
/// that):
/// `cargo test -p omnidl-http --release --test download -- --ignored large_file --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "writes gigabytes"]
async fn large_file() {
    let gib: u64 = std::env::var("OMNIDL_LARGE_GIB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let size = gib << 30;
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let url = server.url(&format!("/file/{size}"));

    let r = job(&url, dir.path(), None, 8);
    let recorder = r.recorder.clone();
    let progress = r.progress.clone();
    let t0 = Instant::now();
    let run = tokio::spawn(async move { engine().run(r.job).await });
    while progress.downloaded() < size * 2 / 5 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    run.abort();
    let _ = run.await;
    let first = t0.elapsed();
    let state = recorder.saved.lock().clone().unwrap();
    let kept = state.downloaded();
    println!(
        "killed after {:.1} s at {} MiB ({} MiB/s, {} connections); checkpoint kept {} MiB",
        first.as_secs_f64(),
        progress.downloaded() >> 20,
        (progress.downloaded() as f64 / first.as_secs_f64()) as u64 >> 20,
        state.segments.len(),
        kept >> 20,
    );

    let r = job(&url, dir.path(), Some(state), 8);
    let t1 = Instant::now();
    let (name, total) = completed(engine().run(r.job).await);
    let second = t1.elapsed();
    println!(
        "resumed and finished {} MiB in {:.1} s ({} MiB/s)",
        (size - kept) >> 20,
        second.as_secs_f64(),
        ((size - kept) as f64 / second.as_secs_f64()) as u64 >> 20,
    );
    assert_eq!(total, size);
    let t2 = Instant::now();
    assert_content(&dir.path().join(name), size);
    println!("verified in {:.1} s", t2.elapsed().as_secs_f64());
}

/// A job for `url` with `mirrors`.
fn mirrored(url: &str, mirrors: Vec<String>, dir: &Path) -> Run {
    let mut r = job(url, dir, None, 8);
    r.job.options.mirrors = mirrors;
    r
}

#[tokio::test(flavor = "multi_thread")]
async fn spreads_the_connections_over_mirrors() {
    let (link, mirror) = (start().await, start().await);
    let dir = tempfile::tempdir().unwrap();
    let size = 16 * MIB;
    let path = format!("/file/{size}");
    let r = mirrored(&link.url(&path), vec![mirror.url(&path)], dir.path());
    let (name, total) = completed(engine().run(r.job).await);
    assert_eq!(total, size);
    assert_content(&dir.path().join(name), size);
    let ranged = |s: &common::Server| s.stats.ranges.lock().len();
    assert!(ranged(&link) > 0, "the link served ranges");
    assert!(ranged(&mirror) > 0, "the mirror served ranges");
}

#[tokio::test(flavor = "multi_thread")]
async fn leaves_out_mirrors_of_another_file_or_that_fail() {
    let (link, other) = (start().await, start().await);
    let dir = tempfile::tempdir().unwrap();
    let size = 8 * MIB;
    let path = format!("/file/{size}");
    let mirrors = vec![
        // Another length, missing, no ranges, not HTTP: all left out before the download.
        other.url(&format!("/file/{}", size + 1)),
        other.url("/missing"),
        other.url(&format!("/norange/{size}")),
        "ftp://example.com/x".into(),
    ];
    let r = mirrored(&link.url(&path), mirrors, dir.path());
    let (name, _) = completed(engine().run(r.job).await);
    assert_content(&dir.path().join(name), size);
    assert!(
        other.stats.ranges.lock().is_empty(),
        "no ranges asked of the bad mirrors"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn carries_on_when_a_mirror_breaks_off() {
    let (link, mirror) = (start().await, start().await);
    let dir = tempfile::tempdir().unwrap();
    let size = 8 * MIB;
    // `/flaky` breaks its first bodies off early: that mirror is left out on its first failure.
    let r = mirrored(
        &link.url(&format!("/file/{size}")),
        vec![mirror.url(&format!("/flaky/{size}"))],
        dir.path(),
    );
    let (name, _) = completed(engine().run(r.job).await);
    assert_content(&dir.path().join(name), size);
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_an_hls_stream_into_one_file() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let r = job(&server.url("/hls/plain/5/index.m3u8"), dir.path(), None, 4);
    let (name, total) = completed(engine().run(r.job).await);
    // `index` says nothing: the folder's name.
    assert_eq!(name, "5.ts");
    assert_eq!(total, 5 * common::HLS_SEG);
    assert_eq!(
        std::fs::read(dir.path().join(name)).unwrap(),
        common::hls_content(5)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn decrypts_aes_128_segments() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let r = job(&server.url("/hls/enc/4/index.m3u8"), dir.path(), None, 4);
    let (name, _) = completed(engine().run(r.job).await);
    assert_eq!(
        std::fs::read(dir.path().join(name)).unwrap(),
        common::hls_content(4)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn takes_the_best_variant_of_a_master_playlist() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let r = job(&server.url("/hls/plain/6/master.m3u8"), dir.path(), None, 4);
    let (_, total) = completed(engine().run(r.job).await);
    assert_eq!(
        total,
        6 * common::HLS_SEG,
        "the 6-segment variant, not the 1-segment one"
    );
    let probe = engine()
        .probe(
            &server.url("/hls/plain/6/master.m3u8"),
            &JobOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(probe.file_name, "6.ts");
    assert_eq!(probe.total, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn will_not_record_a_live_stream() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let r = job(&server.url("/hls/live/3/index.m3u8"), dir.path(), None, 4);
    match engine().run(r.job).await {
        Outcome::Failed(e, _) => assert_eq!(e.code, ErrorCode::Unsupported),
        other => panic!("not refused: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn resumes_an_hls_stream_after_the_segments_written() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let url = server.url("/hls/slow/10/index.m3u8");
    let r = job(&url, dir.path(), None, 2);
    let progress = r.progress.clone();
    let cancel = r.cancel.clone();
    let run = tokio::spawn(async move { engine().run(r.job).await });
    until(|| progress.downloaded() >= 3 * common::HLS_SEG).await;
    cancel.cancel();
    let state = match run.await.unwrap() {
        Outcome::Stopped(s) => s,
        other => panic!("not stopped: {other:?}"),
    };
    let done = state.segments.len() as u64;
    assert!((3..10).contains(&done), "{done} segments kept");
    let before = server.stats.requests.load(Ordering::SeqCst);
    let r = job(&url, dir.path(), Some(state), 2);
    let (name, total) = completed(engine().run(r.job).await);
    assert_eq!(total, 10 * common::HLS_SEG);
    assert_eq!(
        std::fs::read(dir.path().join(name)).unwrap(),
        common::hls_content(10)
    );
    // The playlist again, and only the segments not written yet.
    let asked = server.stats.requests.load(Ordering::SeqCst) - before;
    assert_eq!(asked as u64, 1 + (10 - done));
}

// ----------------------------------------------------------------- connections that change

fn spawn(job: Job) -> tokio::task::JoinHandle<Outcome> {
    tokio::spawn(async move { engine().run(job).await })
}

/// Tuned connections, up to `max`, measured over half-second windows (instead of two
/// seconds) so that the tests take seconds.
fn tuned(r: &mut Run, max: u32) {
    r.job.options.auto_connections = true;
    r.job.options.auto_max_connections = max;
    r.conns = Arc::new(ConnControl::with_window(Duration::from_millis(500)));
    r.job.conns = r.conns.clone();
}

/// Fixed connections, measured over half-second windows.
fn quick(r: &mut Run) {
    r.conns = Arc::new(ConnControl::with_window(Duration::from_millis(500)));
    r.job.conns = r.conns.clone();
}

/// The connection counts of a running download, sampled every 100 ms until it ends.
async fn sample(
    run: tokio::task::JoinHandle<Outcome>,
    progress: Arc<Progress>,
) -> (Outcome, Vec<u32>) {
    let mut seen = Vec::new();
    let mut run = run;
    loop {
        tokio::select! {
            o = &mut run => return (o.unwrap(), seen),
            _ = tokio::time::sleep(Duration::from_millis(100)) => seen.push(progress.connections()),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_the_connections_while_downloading() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 24 * MIB + 777;
    let r = job(&server.url(&format!("/slow/{size}")), dir.path(), None, 2);
    let (progress, conns) = (r.progress.clone(), r.conns.clone());
    let run = spawn(r.job);
    until(|| progress.connections() == 2).await;
    // More: new connections split the segments with the most left.
    conns.set_mode(ConnMode::Fixed(6));
    until(|| progress.connections() == 6).await;
    // Fewer: the others leave their segments to the one that stays.
    conns.set_mode(ConnMode::Fixed(1));
    until(|| progress.connections() == 1).await;
    conns.set_mode(ConnMode::Fixed(12));
    let (name, total) = completed(run.await.unwrap());
    assert_eq!(total, size);
    assert_content(&dir.path().join(&name), size);
    assert!(server.stats.max_open.load(Ordering::SeqCst) >= 6);
}

#[tokio::test(flavor = "multi_thread")]
async fn tunes_the_connections_up_to_what_the_server_allows() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 32 * MIB;
    let mut r = job(
        &server.url(&format!("/capped/6/{size}")),
        dir.path(),
        None,
        8,
    );
    tuned(&mut r, 32);
    let (name, total) = completed(engine().run(r.job).await);
    assert_eq!(total, size);
    assert_content(&dir.path().join(&name), size);
    let stats = &server.stats;
    // From 4 it doubled to 8, two were refused, and it kept to 6 from then on.
    assert!(stats.refused.load(Ordering::SeqCst) >= 1, "it tried more");
    assert_eq!(stats.max_open.load(Ordering::SeqCst), 6);
    assert!(r.conns.target() <= 6, "{}", r.conns.target());
}

#[tokio::test(flavor = "multi_thread")]
async fn opens_more_connections_on_a_server_that_limits_each() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    // 256 KiB/s per connection and no limit in all: every connection adds speed.
    let size = 24 * MIB;
    let mut r = job(
        &server.url(&format!("/throttled/256/0/{size}")),
        dir.path(),
        None,
        8,
    );
    tuned(&mut r, 16);
    let progress = r.progress.clone();
    let started = Instant::now();
    let (o, seen) = sample(spawn(r.job), progress).await;
    let (name, total) = completed(o);
    assert_eq!(total, size);
    assert_content(&dir.path().join(&name), size);
    // 4, 8, then the most allowed.
    assert_eq!(server.stats.max_open.load(Ordering::SeqCst), 16, "{seen:?}");
    // Steady connections are kept: only the splits at the end cut bodies short.
    let cut = server.stats.cut.load(Ordering::SeqCst);
    eprintln!(
        "steady: {} requests, {cut} cut",
        server.stats.requests.load(Ordering::SeqCst)
    );
    assert!(cut <= 20, "{cut}");
    // 4 connections alone would take 24 s, 16 take 6.
    assert!(
        started.elapsed() < Duration::from_secs(14),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn settles_where_the_server_bandwidth_runs_out() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    // 512 KiB/s per connection, 3 MiB/s in all: 6 connections fill the line, more only wait.
    let size = 60 * MIB;
    let mut r = job(
        &server.url(&format!("/throttled/512/3072/{size}")),
        dir.path(),
        None,
        8,
    );
    tuned(&mut r, 32);
    let progress = r.progress.clone();
    let (o, seen) = sample(spawn(r.job), progress).await;
    let (name, total) = completed(o);
    assert_eq!(total, size);
    assert_content(&dir.path().join(&name), size);
    // It tried more (8, 16), went back, and spent the second half near 6, not at 16 or 32.
    assert!(
        server.stats.max_open.load(Ordering::SeqCst) >= 8,
        "{seen:?}"
    );
    let late = &seen[seen.len() / 2..];
    let avg = late.iter().sum::<u32>() as f64 / late.len() as f64;
    assert!((4.5..=9.0).contains(&avg), "{avg:.1}: {seen:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn replaces_a_connection_that_lags() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 48 * MIB;
    let r = job(
        &server.url(&format!("/lagging/{size}")),
        dir.path(),
        None,
        4,
    );
    let run = spawn(r.job);
    // One connection gets 40 kB/s while the others get 1 MB/s: within seconds it is dropped
    // and a new connection takes over its segment (long before the others run out of work and
    // split it).
    let stats = server.stats.clone();
    let deadline = Instant::now() + Duration::from_secs(11);
    while stats.lag_cut.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "the slow connection was kept");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (name, total) = completed(run.await.unwrap());
    assert_eq!(total, size);
    assert_content(&dir.path().join(&name), size);
}

#[tokio::test(flavor = "multi_thread")]
async fn renews_connections_that_a_server_slows_down_after_a_while() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    // Each connection: 4 MiB/s for its first MiB, then 256 KiB/s.
    let size = 32 * MIB;
    let mut r = job(
        &server.url(&format!("/burst/4096/256/1024/{size}")),
        dir.path(),
        None,
        4,
    );
    quick(&mut r);
    let started = Instant::now();
    let (name, total) = completed(spawn(r.job).await.unwrap());
    assert_eq!(total, size);
    assert_content(&dir.path().join(&name), size);
    let (requests, cut) = (
        server.stats.requests.load(Ordering::SeqCst),
        server.stats.cut.load(Ordering::SeqCst),
    );
    eprintln!(
        "burst: {:?}, {requests} requests, {cut} cut",
        started.elapsed()
    );
    // Kept, 4 connections would take about 30 s; renewed, each runs fast again.
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
    assert!(cut >= 10, "{cut}");
}

// ------------------------------------------------------------------------- user agents

#[tokio::test(flavor = "multi_thread")]
async fn tries_another_user_agent_when_refused() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let size = 3 * MIB;
    let r = job(&server.url(&format!("/picky/{size}")), dir.path(), None, 4);
    assert!(
        r.job.options.user_agent.contains("Mozilla"),
        "a browser's by default"
    );
    let (name, total) = completed(engine().run(r.job).await);
    assert_eq!(total, size);
    assert_content(&dir.path().join(&name), size);
    let agents = server.stats.agents.lock().clone();
    assert!(
        agents.iter().any(|a| a.contains("Mozilla")),
        "the configured one first: {agents:?}"
    );
    // Then the app's own name, which the server takes, for every request after.
    let last = agents.last().unwrap();
    assert!(last.starts_with("OmniDownload/"), "{agents:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_the_user_agent_of_a_task_with_cookies() {
    let server = start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut r = job(&server.url("/picky/1000"), dir.path(), None, 1);
    // A browser's cookies go with that browser's user agent: no other is tried.
    r.job.options.cookies = Some("session=1".into());
    r.job.options.user_agent_fixed = true;
    match engine().run(r.job).await {
        Outcome::Failed(e, _) => assert_eq!(e.code, ErrorCode::Forbidden),
        other => panic!("not refused: {other:?}"),
    }
    let agents = server.stats.agents.lock().clone();
    assert!(agents.iter().all(|a| a.contains("Mozilla")), "{agents:?}");
}
