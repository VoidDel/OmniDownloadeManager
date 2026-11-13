//! The manager against a fake engine that "downloads" in steps.

use std::sync::atomic::AtomicU64;

use super::*;

/// Counts `step` bytes every `delay` up to `size`; resumes from the saved segment.
struct FakeEngine {
    size: u64,
    step: u64,
    delay: Duration,
    fail_at: Option<u64>,
    /// Bytes already done when the last run started.
    resumed_from: AtomicU64,
}

impl FakeEngine {
    fn new(size: u64, step: u64, delay_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            size,
            step,
            delay: Duration::from_millis(delay_ms),
            fail_at: None,
            resumed_from: AtomicU64::new(0),
        })
    }
}

#[async_trait]
impl DownloadEngine for FakeEngine {
    fn kind(&self) -> TaskKind {
        TaskKind::Http
    }

    async fn probe(&self, source: &str, _: &JobOptions) -> Result<ProbeInfo> {
        Ok(ProbeInfo {
            final_url: source.into(),
            file_name: filename::from_url(source).unwrap_or_default(),
            total: Some(self.size),
            resumable: true,
            content_type: None,
            etag: None,
            last_modified: None,
            ..Default::default()
        })
    }

    async fn run(&self, job: Job) -> Outcome {
        let size = self.size;
        let name = job
            .resume
            .as_ref()
            .map(|r| r.file_name.clone())
            .or(job.file_name.clone())
            .unwrap_or_else(|| filename::from_url(&job.source).unwrap());
        let mut done = job.resume.as_ref().map_or(0, ResumeState::downloaded);
        self.resumed_from.store(done, Ordering::SeqCst);
        let state = |done| ResumeState {
            file_name: name.clone(),
            total: Some(size),
            resumable: true,
            segments: vec![Segment {
                start: 0,
                end: Some(size - 1),
                downloaded: done,
            }],
            ..Default::default()
        };
        job.checkpoint.started(&state(done)).await.unwrap();
        job.progress.set_total(Some(size));
        job.progress.set_downloaded(done);
        // Connections as the engines take them: from the options, changed while it runs.
        job.conns.begin(job.options.conn_mode(), 64, 4);
        job.progress.set_connections(job.conns.target());
        while done < size {
            tokio::select! {
                _ = job.cancel.cancelled() => return Outcome::Stopped(state(done)),
                _ = tokio::time::sleep(self.delay) => {}
            }
            done = (done + self.step).min(size);
            job.progress.set_downloaded(done);
            job.progress.set_connections(job.conns.target());
            job.checkpoint.save(&state(done)).await.unwrap();
            if self.fail_at.is_some_and(|f| done >= f) {
                let e = Error::new(ErrorCode::Network, "connection reset");
                return Outcome::Failed(e, Some(state(done)));
            }
        }
        Outcome::Completed {
            file_name: name,
            total: size,
        }
    }
}

fn new_task(url: &str, start: bool) -> NewTask {
    NewTask {
        source: url.into(),
        save_dir: std::env::temp_dir().to_string_lossy().into_owned(),
        options: TaskOptions::default(),
        start,
        probe: None,
    }
}

async fn manager(engine: Arc<FakeEngine>, db: Db, max_concurrent: u32) -> TaskManager {
    let config = EngineConfig {
        max_concurrent,
        ..Default::default()
    };
    TaskManager::open(db, vec![engine], config).await.unwrap()
}

/// Polls until `pred` holds for task `id`.
async fn wait_for(m: &TaskManager, id: TaskId, pred: impl Fn(&Task) -> bool) -> Task {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let t = m.get(id).expect("task exists");
        if pred(&t) {
            return t;
        }
        assert!(Instant::now() < deadline, "timed out; task is {t:#?}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_a_task_to_completion_and_publishes_it() {
    let db = Db::memory().await.unwrap();
    let m = manager(FakeEngine::new(1000, 100, 2), db.clone(), 3).await;
    let mut events = m.subscribe();
    let t = m
        .add(vec![new_task("https://example.com/a.iso", true)])
        .await
        .unwrap()
        .remove(0);
    assert_eq!(t.file_name, "a.iso");
    assert_eq!(t.category, crate::Category::DiskImage);
    let done = wait_for(&m, t.id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(done.downloaded_bytes, 1000);
    assert!(done.started_at.is_some() && done.completed_at.is_some());

    let row = db.get_task(&t.id.to_string()).await.unwrap().unwrap();
    assert_eq!(row.status, "completed");
    assert!(db.segments(&t.id.to_string()).await.unwrap().is_empty());

    let mut seen = Vec::new();
    while let Ok(e) = events.try_recv() {
        match e {
            TaskEvent::Added { .. } => seen.push("added".to_owned()),
            TaskEvent::Changed { task } => seen.push(task.status.as_str().to_owned()),
            _ => {}
        }
    }
    seen.dedup();
    assert_eq!(seen, ["added", "probing", "downloading", "completed"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_no_more_than_max_concurrent() {
    let db = Db::memory().await.unwrap();
    let m = manager(FakeEngine::new(500, 100, 2), db, 2).await;
    let ids: Vec<TaskId> = m
        .add(
            (0..5)
                .map(|i| new_task(&format!("https://e.com/{i}.bin"), true))
                .collect(),
        )
        .await
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let list = m.list();
        let running = list.iter().filter(|t| t.status.is_running()).count();
        assert!(running <= 2, "{running} running");
        if list.iter().all(|t| t.status == TaskStatus::Completed) {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    // List order is kept.
    let order: Vec<TaskId> = m.list().iter().map(|t| t.id).collect();
    assert_eq!(order, ids);
}

#[tokio::test(flavor = "multi_thread")]
async fn pauses_and_resumes_from_saved_progress() {
    let db = Db::memory().await.unwrap();
    let engine = FakeEngine::new(100_000, 1000, 5);
    let m = manager(engine.clone(), db.clone(), 3).await;
    let id = m
        .add(vec![new_task("https://e.com/big.bin", true)])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.downloaded_bytes >= 3000).await;
    m.pause(&[id]).await.unwrap();
    let paused = wait_for(&m, id, |t| t.status == TaskStatus::Paused).await;
    assert!(paused.downloaded_bytes >= 3000);
    let saved = db.segments(&id.to_string()).await.unwrap();
    assert_eq!(saved[0].downloaded as u64, paused.downloaded_bytes);

    m.resume(&[id]).await.unwrap();
    wait_for(&m, id, |t| t.status == TaskStatus::Downloading).await;
    assert_eq!(
        engine.resumed_from.load(Ordering::SeqCst),
        paused.downloaded_bytes
    );
    m.pause(&[id]).await.unwrap();
    wait_for(&m, id, |t| t.status == TaskStatus::Paused).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn moves_tasks_through_the_recycle_bin() {
    let db = Db::memory().await.unwrap();
    let m = manager(FakeEngine::new(1000, 10, 1), db.clone(), 3).await;
    let id = m
        .add(vec![new_task("https://e.com/x.zip", false)])
        .await
        .unwrap()[0]
        .id;
    assert_eq!(m.get(id).unwrap().status, TaskStatus::Paused);
    m.remove(&[id], false).await.unwrap();
    assert_eq!(m.get(id).unwrap().status, TaskStatus::Removed);
    m.restore(&[id]).await.unwrap();
    assert_eq!(m.get(id).unwrap().status, TaskStatus::Paused);

    let mut events = m.subscribe();
    m.purge(&[id], false).await.unwrap();
    assert!(m.get(id).is_none());
    assert!(db.get_task(&id.to_string()).await.unwrap().is_none());
    assert!(matches!(events.recv().await.unwrap(), TaskEvent::Deleted { ids } if ids == [id]));
}

#[tokio::test(flavor = "multi_thread")]
async fn removes_tasks_with_their_files() {
    // The fake engine writes nothing: the files are put there by hand.
    let dir = tempfile::tempdir().unwrap();
    let in_dir = |name: &str| NewTask {
        save_dir: dir.path().to_string_lossy().into_owned(),
        ..new_task(&format!("https://e.com/{name}"), true)
    };

    // A finished download: its file goes, the task waits in the recycle bin and comes back
    // finished (its file missing).
    std::fs::write(dir.path().join("done.bin"), b"abc").unwrap();
    let m = manager(FakeEngine::new(3, 3, 1), Db::memory().await.unwrap(), 3).await;
    let done = m.add(vec![in_dir("done.bin")]).await.unwrap()[0].id;
    wait_for(&m, done, |t| t.status == TaskStatus::Completed).await;
    m.remove(&[done], true).await.unwrap();
    assert_eq!(m.get(done).unwrap().status, TaskStatus::Removed);
    assert!(!dir.path().join("done.bin").exists());
    m.restore(&[done]).await.unwrap();
    assert_eq!(m.get(done).unwrap().status, TaskStatus::Completed);

    // A running one stops first; its partial file goes and it would start over.
    let m = manager(
        FakeEngine::new(1_000_000, 100, 5),
        Db::memory().await.unwrap(),
        3,
    )
    .await;
    let running = m.add(vec![in_dir("part.bin")]).await.unwrap()[0].id;
    let t = wait_for(&m, running, |t| t.status == TaskStatus::Downloading).await;
    std::fs::write(t.partial_path(), b"partial").unwrap();
    m.remove(&[running], true).await.unwrap();
    let t = wait_for(&m, running, |t| t.status == TaskStatus::Removed).await;
    assert!(!t.partial_path().exists());
    assert_eq!(t.downloaded_bytes, 0);
    m.restore(&[running]).await.unwrap();
    assert_eq!(m.get(running).unwrap().status, TaskStatus::Paused);
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_running_task_stops_it_first() {
    let db = Db::memory().await.unwrap();
    let m = manager(FakeEngine::new(1_000_000, 100, 5), db, 3).await;
    let id = m
        .add(vec![new_task("https://e.com/r.bin", true)])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.status == TaskStatus::Downloading).await;
    m.remove(&[id], false).await.unwrap();
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Removed).await;
    assert_eq!(t.prev_status, Some(TaskStatus::Paused));
    m.restore(&[id]).await.unwrap();
    assert_eq!(m.get(id).unwrap().status, TaskStatus::Paused);
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_a_finished_task_again() {
    // The finished file was deleted: restarting starts over from the first byte.
    let engine = FakeEngine::new(1000, 100, 2);
    let m = manager(engine.clone(), Db::memory().await.unwrap(), 3).await;
    let id = m
        .add(vec![new_task("https://example.com/again.iso", true)])
        .await
        .unwrap()[0]
        .id;
    let done = wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert!(done.completed_at.is_some());
    // The time it spent downloading (10 steps of 2 ms): its average speed.
    assert!(done.active_ms >= 10, "active {} ms", done.active_ms);
    m.restart(&[id]).await.unwrap();
    let again = m.get(id).unwrap();
    assert!(matches!(
        again.status,
        TaskStatus::Queued | TaskStatus::Probing | TaskStatus::Downloading
    ));
    assert_eq!(again.downloaded_bytes, 0);
    assert_eq!(again.completed_at, None);
    assert_eq!((again.active_ms, again.peak_bps), (0, 0));
    wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(engine.resumed_from.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn records_failures_and_retries_them() {
    let db = Db::memory().await.unwrap();
    let engine = Arc::new(FakeEngine {
        fail_at: Some(300),
        ..Arc::into_inner(FakeEngine::new(1000, 100, 1)).unwrap()
    });
    let m = manager(engine, db, 3).await;
    let id = m
        .add(vec![new_task("https://e.com/f.bin", true)])
        .await
        .unwrap()[0]
        .id;
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Failed).await;
    assert_eq!(t.error.unwrap().code, ErrorCode::Network);
    assert_eq!(t.downloaded_bytes, 300);
    m.resume(&[id]).await.unwrap();
    let t = m.get(id).unwrap();
    assert!(t.error.is_none() && t.status != TaskStatus::Failed);
}

#[tokio::test(flavor = "multi_thread")]
async fn resumes_running_downloads_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("omni.db");
    let id = {
        let m = manager(
            FakeEngine::new(1_000_000, 1000, 5),
            Db::open(&path).await.unwrap(),
            3,
        )
        .await;
        let id = m
            .add(vec![new_task("https://e.com/restart.bin", true)])
            .await
            .unwrap()[0]
            .id;
        wait_for(&m, id, |t| t.downloaded_bytes >= 5000).await;
        m.shutdown().await;
        id
    };
    let engine = FakeEngine::new(1_000_000, 1000, 5);
    let m = manager(engine.clone(), Db::open(&path).await.unwrap(), 3).await;
    wait_for(&m, id, |t| t.status == TaskStatus::Downloading).await;
    assert!(engine.resumed_from.load(Ordering::SeqCst) >= 5000);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_links_it_cannot_fetch() {
    let m = manager(FakeEngine::new(1, 1, 1), Db::memory().await.unwrap(), 3).await;
    let e = m
        .add(vec![new_task("ftp://e.com/a", true)])
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported);
    let e = m.add(vec![new_task("not a url", true)]).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidUrl);
    let e = m
        .add(vec![NewTask {
            save_dir: " ".into(),
            ..new_task("https://e.com/a", true)
        }])
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidRequest);
    assert!(m.list().is_empty());
}

#[test]
fn measures_speed_over_a_window() {
    let mut m = SpeedMeter::default();
    let t0 = Instant::now();
    assert_eq!(m.sample(t0, 0), 0);
    assert_eq!(m.sample(t0 + Duration::from_secs(1), 1000), 1000);
    assert_eq!(m.sample(t0 + Duration::from_secs(2), 3000), 1500);
    // Old samples fall out of the window.
    let s = m.sample(t0 + Duration::from_secs(5), 3000);
    assert_eq!(s, 0);
    // A restart (fewer bytes) starts a new window.
    assert_eq!(m.sample(t0 + Duration::from_secs(6), 0), 0);
}

/// A torrent that is complete at once and then seeds, uploading 100 bytes every 10 ms until it
/// is cancelled or has uploaded `stop_at`.
struct FakeSeeder {
    stop_at: Option<u64>,
    runs: AtomicU64,
}

impl FakeSeeder {
    fn new(stop_at: Option<u64>) -> Arc<Self> {
        Arc::new(Self {
            stop_at,
            runs: AtomicU64::new(0),
        })
    }
}

#[async_trait]
impl DownloadEngine for FakeSeeder {
    fn kind(&self) -> TaskKind {
        TaskKind::Torrent
    }

    async fn probe(&self, _: &str, _: &JobOptions) -> Result<ProbeInfo> {
        Ok(ProbeInfo::default())
    }

    async fn run(&self, job: Job) -> Outcome {
        const SIZE: u64 = 1000;
        self.runs.fetch_add(1, Ordering::SeqCst);
        let state = ResumeState {
            file_name: "t".into(),
            total: Some(SIZE),
            resumable: true,
            segments: vec![Segment {
                start: 0,
                end: Some(SIZE - 1),
                downloaded: SIZE,
            }],
            ..Default::default()
        };
        job.checkpoint.started(&state).await.unwrap();
        job.progress.set_total(Some(SIZE));
        job.progress.set_downloaded(SIZE);
        job.checkpoint.seeding(&state).await.unwrap();
        loop {
            tokio::select! {
                _ = job.cancel.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
            let up = job.progress.uploaded() + 100;
            job.progress.set_uploaded(up);
            if self.stop_at.is_some_and(|s| up >= s) {
                break;
            }
        }
        job.checkpoint.save(&state).await.unwrap();
        Outcome::Completed {
            file_name: "t".into(),
            total: SIZE,
        }
    }
}

const MAGNET: &str = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=t";

async fn mixed(seeder: Arc<FakeSeeder>, db: Db, max_concurrent: u32) -> TaskManager {
    let config = EngineConfig {
        max_concurrent,
        ..Default::default()
    };
    let engines: Vec<Arc<dyn DownloadEngine>> = vec![seeder, FakeEngine::new(1000, 100, 20)];
    TaskManager::open(db, engines, config).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn seeding_frees_the_slot_and_pausing_ends_it() {
    let m = mixed(FakeSeeder::new(None), Db::memory().await.unwrap(), 1).await;
    let torrent = m.add(vec![new_task(MAGNET, true)]).await.unwrap()[0].clone();
    assert_eq!(torrent.kind, TaskKind::Torrent);
    assert_eq!(torrent.file_name, "t");
    let seeding = wait_for(&m, torrent.id, |t| t.status == TaskStatus::Seeding).await;
    assert!(seeding.completed_at.is_some());
    // The only slot is free for another download while the torrent seeds.
    let http = m
        .add(vec![new_task("https://example.com/b.bin", true)])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, http, |t| t.status == TaskStatus::Completed).await;
    wait_for(&m, torrent.id, |t| t.uploaded_bytes > 0).await;
    m.pause(&[torrent.id]).await.unwrap();
    let done = wait_for(&m, torrent.id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(done.completed_at, seeding.completed_at);
    assert!(done.uploaded_bytes > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn seeding_ends_when_the_engine_is_done() {
    let m = mixed(FakeSeeder::new(Some(500)), Db::memory().await.unwrap(), 3).await;
    let id = m.add(vec![new_task(MAGNET, true)]).await.unwrap()[0].id;
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert!(t.uploaded_bytes >= 500);
}

#[tokio::test(flavor = "multi_thread")]
async fn seeding_goes_on_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("omni.db");
    let seeder = FakeSeeder::new(None);
    let m = mixed(seeder.clone(), Db::open(&path).await.unwrap(), 3).await;
    let id = m.add(vec![new_task(MAGNET, true)]).await.unwrap()[0].id;
    wait_for(&m, id, |t| {
        t.status == TaskStatus::Seeding && t.uploaded_bytes >= 300
    })
    .await;
    m.shutdown().await;

    let seeder = FakeSeeder::new(None);
    let m = mixed(seeder.clone(), Db::open(&path).await.unwrap(), 3).await;
    let before = m.get(id).unwrap().uploaded_bytes;
    assert!(before >= 300, "uploaded bytes were saved");
    wait_for(&m, id, |t| {
        t.status == TaskStatus::Seeding && t.uploaded_bytes > before
    })
    .await;
    assert_eq!(seeder.runs.load(Ordering::SeqCst), 1);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn checks_a_finished_file_against_its_checksum() {
    // The fake engine writes nothing: the "downloaded" files are put there beforehand.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("good.bin"), b"abc").unwrap();
    std::fs::write(dir.path().join("bad.bin"), b"abc").unwrap();
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let wrong = crate::Checksum::parse(&"0".repeat(64)).unwrap();
    let m = manager(FakeEngine::new(3, 3, 1), Db::memory().await.unwrap(), 3).await;
    let task = |name: &str, sum: &crate::Checksum| NewTask {
        save_dir: dir.path().to_string_lossy().into_owned(),
        options: TaskOptions {
            checksum: Some(sum.clone()),
            ..Default::default()
        },
        ..new_task(&format!("https://example.com/{name}"), true)
    };
    let added = m
        .add(vec![
            task("good.bin", &crate::Checksum::parse(ABC_SHA256).unwrap()),
            task("bad.bin", &wrong),
        ])
        .await
        .unwrap();
    let good = wait_for(&m, added[0].id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(good.error, None);
    let bad = wait_for(&m, added[1].id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(bad.error.map(|e| e.code), Some(ErrorCode::ChecksumMismatch));
    // The finished file stays; giving the right value clears the mismatch.
    let fixed = m
        .set_checksum(
            added[1].id,
            crate::Checksum::parse(&format!("sha256:{ABC_SHA256}")),
        )
        .await
        .unwrap();
    assert_eq!(fixed.error, None);
    let digests = m
        .digests(added[1].id, vec![crate::HashAlgo::Sha256])
        .await
        .unwrap();
    assert_eq!(
        digests,
        vec![(crate::HashAlgo::Sha256, ABC_SHA256.to_owned())]
    );
    // Wrong again, then downloaded again: the file that failed goes first.
    let again = m.set_checksum(added[1].id, Some(wrong)).await.unwrap();
    assert_eq!(
        again.error.map(|e| e.code),
        Some(ErrorCode::ChecksumMismatch)
    );
    m.restart(&[added[1].id]).await.unwrap();
    assert!(!dir.path().join("bad.bin").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn changes_the_connections_of_a_running_task() {
    let db = Db::memory().await.unwrap();
    let m = manager(FakeEngine::new(100_000, 10, 5), db.clone(), 3).await;
    let id = m
        .add(vec![new_task("https://example.com/c.bin", true)])
        .await
        .unwrap()[0]
        .id;
    // Tuned by default: it starts at 4.
    wait_for(&m, id, |t| t.connections == 4).await;
    m.set_task_connections(id, Some(12)).await.unwrap();
    let t = wait_for(&m, id, |t| t.connections == 12).await;
    assert_eq!(t.options.connections, Some(12));
    assert_eq!(t.options.auto_connections, Some(false));
    // Saved: the next run starts with them.
    let saved = db.get_task(&id.to_string()).await.unwrap().unwrap();
    assert!(
        saved.options_json.contains("\"autoConnections\":false"),
        "{}",
        saved.options_json
    );
    m.set_task_connections(id, None).await.unwrap();
    let t = m.get(id).unwrap();
    assert_eq!(
        (t.options.connections, t.options.auto_connections),
        (None, Some(true))
    );
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn moves_tasks_in_the_list_and_the_queue() {
    let db = Db::memory().await.unwrap();
    let m = manager(FakeEngine::new(300, 100, 2), db.clone(), 1).await;
    let ids: Vec<TaskId> = m
        .add(
            (0..5)
                .map(|i| new_task(&format!("https://e.com/{i}.bin"), false))
                .collect(),
        )
        .await
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    let order = |m: &TaskManager| -> Vec<usize> {
        m.list()
            .iter()
            .map(|t| ids.iter().position(|id| *id == t.id).unwrap())
            .collect()
    };
    let mut events = m.subscribe();
    // The last two (given out of order) before the first: they keep their own order.
    m.reorder(&[ids[4], ids[3]], ids[0], false).await.unwrap();
    assert_eq!(order(&m), [3, 4, 0, 1, 2]);
    // One after another.
    m.reorder(&[ids[3]], ids[1], true).await.unwrap();
    assert_eq!(order(&m), [4, 0, 1, 3, 2]);
    // Each move is published once, with the whole order.
    let mut published = Vec::new();
    while let Ok(e) = events.try_recv() {
        if let TaskEvent::Reordered { ids } = e {
            published.push(ids);
        }
    }
    assert_eq!(published.len(), 2);
    assert_eq!(
        published[1],
        m.list().iter().map(|t| t.id).collect::<Vec<_>>()
    );
    // Next to itself, or to a task that is not there: refused, nothing changes.
    assert!(m.reorder(&[ids[0]], ids[0], true).await.is_err());
    assert!(m.reorder(&[ids[0]], Uuid::new_v4(), true).await.is_err());
    assert_eq!(order(&m), [4, 0, 1, 3, 2]);
    // New tasks still go to the end.
    let late = m
        .add(vec![new_task("https://e.com/late.bin", false)])
        .await
        .unwrap()[0]
        .id;
    assert_eq!(m.list().last().unwrap().id, late);
    // The queue follows the list: with one slot, the tasks run in the new order.
    m.resume(&ids).await.unwrap();
    let mut started: Vec<TaskId> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while started.len() < ids.len() {
        for t in m.list() {
            if t.started_at.is_some() && !started.contains(&t.id) {
                started.push(t.id);
            }
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert_eq!(started, [ids[4], ids[0], ids[1], ids[3], ids[2]]);
    // And it is kept across a restart.
    m.shutdown().await;
    let db = Db::memory().await.unwrap();
    let m = manager(FakeEngine::new(300, 100, 2), db.clone(), 1).await;
    let ids: Vec<TaskId> = m
        .add(
            (0..3)
                .map(|i| new_task(&format!("https://e.com/{i}.bin"), false))
                .collect(),
        )
        .await
        .unwrap()
        .iter()
        .map(|t| t.id)
        .collect();
    m.reorder(&[ids[2]], ids[0], false).await.unwrap();
    let rows = db.list_tasks().await.unwrap();
    let saved = |id: TaskId| {
        rows.iter()
            .find(|r| r.id == id.to_string())
            .unwrap()
            .sort_order
    };
    assert_eq!([saved(ids[2]), saved(ids[0]), saved(ids[1])], [0, 1, 2]);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_an_account_and_an_answer_about_a_taken_name() {
    let db = Db::memory().await.unwrap();
    let m = manager(FakeEngine::new(300, 100, 2), db.clone(), 1).await;
    // A password in the link moves into the options: the link shown is without it.
    let t = m
        .add(vec![new_task("https://bob:s%40cret@e.com/a.bin", false)])
        .await
        .unwrap()
        .remove(0);
    assert_eq!(t.source, "https://e.com/a.bin");
    assert_eq!(t.options.username.as_deref(), Some("bob"));
    assert_eq!(t.options.password.as_deref(), Some("s@cret"));
    m.set_credentials(t.id, Some(" alice ".into()), Some("pw".into()))
        .await
        .unwrap();
    let o = m.get(t.id).unwrap().options;
    assert_eq!(
        (o.username.as_deref(), o.password.as_deref()),
        (Some("alice"), Some("pw"))
    );
    // No name: no password either.
    m.set_credentials(t.id, Some("  ".into()), Some("pw".into()))
        .await
        .unwrap();
    let o = m.get(t.id).unwrap().options;
    assert_eq!((o.username, o.password), (None, None));

    m.set_conflict(t.id, crate::ConflictPolicy::Overwrite)
        .await
        .unwrap();
    assert_eq!(
        m.get(t.id).unwrap().options.conflict,
        Some(crate::ConflictPolicy::Overwrite)
    );
    assert!(
        m.set_conflict(t.id, crate::ConflictPolicy::Ask)
            .await
            .is_err()
    );
    // The task's answer wins over the settings.
    let config = EngineConfig {
        conflict: crate::ConflictPolicy::Ask,
        ..Default::default()
    };
    let o = JobOptions::resolve(&m.get(t.id).unwrap().options, &config);
    assert_eq!(o.conflict, crate::ConflictPolicy::Overwrite);
    let o = JobOptions::resolve(&TaskOptions::default(), &config);
    assert_eq!(o.conflict, crate::ConflictPolicy::Ask);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn exports_the_list_and_imports_it_elsewhere() {
    let m = manager(FakeEngine::new(300, 100, 1), Db::memory().await.unwrap(), 3).await;
    let done = m
        .add(vec![new_task("https://e.com/done.iso", true)])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, done, |t| t.status == TaskStatus::Completed).await;
    let mut secret = new_task("https://e.com/secret.zip", false);
    secret.options.cookies = Some("session=abc".into());
    secret.options.headers.insert("X-Token".into(), "t".into());
    secret.options.username = Some("bob".into());
    secret.options.password = Some("pw".into());
    secret.options.connections = Some(4);
    secret.options.proxy = Some(crate::ProxyConfig::Custom {
        url: "socks5://u:p@proxy.example:1080".into(),
    });
    let waiting = m.add(vec![secret]).await.unwrap()[0].id;
    let binned = m
        .add(vec![new_task("https://e.com/binned.bin", false)])
        .await
        .unwrap()[0]
        .id;
    m.remove(&[binned], false).await.unwrap();

    // The recycle bin stays behind; secrets too, unless asked for.
    let plain = m.export(None, false).await;
    assert_eq!((plain.app.as_str(), plain.version), ("omni-download", 1));
    let names: Vec<&str> = plain.tasks.iter().map(|t| t.file_name.as_str()).collect();
    assert_eq!(names, ["done.iso", "secret.zip"]);
    assert!(plain.tasks[0].completed && !plain.tasks[1].completed);
    let o = &plain.tasks[1].options;
    assert_eq!(
        (&o.cookies, &o.password, o.headers.len()),
        (&None, &None, 0)
    );
    assert_eq!(o.username.as_deref(), Some("bob"));
    assert_eq!(o.connections, Some(4));
    assert_eq!(
        o.proxy,
        Some(crate::ProxyConfig::Custom {
            url: "socks5://proxy.example:1080".into()
        })
    );
    let json = serde_json::to_string(&plain).unwrap();
    assert!(!json.contains("abc") && !json.contains("\"pw\""), "{json}");
    let full = m.export(Some(&[waiting]), true).await;
    assert_eq!(full.tasks.len(), 1);
    assert_eq!(
        full.tasks[0].options.cookies.as_deref(),
        Some("session=abc")
    );
    assert_eq!(full.tasks[0].options.password.as_deref(), Some("pw"));

    // Into another list: finished stays finished, the rest waits paused from the first byte.
    let other = manager(FakeEngine::new(300, 100, 1), Db::memory().await.unwrap(), 3).await;
    let file: TaskExport = serde_json::from_str(&json).unwrap();
    assert_eq!(file, plain);
    let mut events = other.subscribe();
    let summary = other.import(file.clone()).await.unwrap();
    assert_eq!((summary.added, summary.skipped), (2, 0));
    let list = other.list();
    assert_eq!(list.len(), 2);
    assert_eq!(
        (
            list[0].status,
            list[0].downloaded_bytes,
            list[0].total_bytes
        ),
        (TaskStatus::Completed, 300, Some(300))
    );
    assert!(list[0].completed_at.is_some());
    assert_eq!(
        (list[1].status, list[1].downloaded_bytes, list[1].started_at),
        (TaskStatus::Paused, 0, None)
    );
    assert_eq!(list[1].options.connections, Some(4));
    let mut added = 0;
    while let Ok(e) = events.try_recv() {
        added += u32::from(matches!(e, TaskEvent::Added { .. }));
    }
    assert_eq!(added, 2);
    // Again: all there already. A link nothing fetches is skipped too.
    let mut again = file;
    again.tasks.push(ExportedTask {
        source: "gopher://e.com/x".into(),
        save_dir: "/downloads".into(),
        file_name: "x".into(),
        completed: false,
        total_bytes: None,
        created_at: 0,
        completed_at: None,
        options: TaskOptions::default(),
    });
    let summary = other.import(again.clone()).await.unwrap();
    assert_eq!((summary.added, summary.skipped), (0, 3));
    assert_eq!(other.list().len(), 2);
    // Not our file, or a newer format: refused whole.
    again.app = "something-else".into();
    assert!(other.import(again.clone()).await.is_err());
    again.app = "omni-download".into();
    again.version = 99;
    assert!(other.import(again).await.is_err());
    // A hand-written file needs only the link, the folder and the name.
    let minimal: TaskExport = serde_json::from_str(
        r#"{"app":"omni-download","version":1,"exportedAt":0,
            "tasks":[{"source":"https://e.com/min.bin","saveDir":"/d","fileName":"min.bin"}]}"#,
    )
    .unwrap();
    assert_eq!(other.import(minimal).await.unwrap().added, 1);
    m.shutdown().await;
    other.shutdown().await;
}
