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
    Checkpoint, DownloadEngine, ErrorCode, Job, JobOptions, Outcome, Progress, RateLimiter, Result,
    ResumeState,
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
}

fn job(url: &str, dir: &Path, resume: Option<ResumeState>, connections: u32) -> Run {
    let recorder = Arc::new(Recorder::default());
    let cancel = CancellationToken::new();
    let progress = Arc::new(Progress::default());
    let job = Job {
        id: Uuid::new_v4(),
        source: url.into(),
        save_dir: dir.to_path_buf(),
        file_name: None,
        resume,
        options: JobOptions {
            connections,
            ..JobOptions::default()
        },
        cancel: cancel.clone(),
        progress: progress.clone(),
        task_limit: RateLimiter::new(None),
        global_limit: RateLimiter::new(None),
        checkpoint: recorder.clone(),
    };
    Run {
        job,
        recorder,
        cancel,
        progress,
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
