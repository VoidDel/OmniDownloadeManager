//! The manager with the real engine and an on-disk database: quitting mid-download and opening
//! again continues where it stopped (docs/development-plan.md Phase 1 acceptance).

mod common;

use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use common::{assert_content, start};
use omnidl_core::{EngineConfig, NewTask, Task, TaskId, TaskManager, TaskOptions, TaskStatus};
use omnidl_http::HttpEngine;
use omnidl_storage::Db;

const MIB: u64 = 1 << 20;

async fn open(db: &Path) -> TaskManager {
    let engine = Arc::new(HttpEngine::with_retry_delay(Duration::from_millis(20)));
    TaskManager::open(
        Db::open(db).await.unwrap(),
        vec![engine],
        EngineConfig::default(),
    )
    .await
    .unwrap()
}

async fn wait_for(m: &TaskManager, id: TaskId, pred: impl Fn(&Task) -> bool) -> Task {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let t = m.get(id).unwrap();
        if pred(&t) {
            return t;
        }
        assert!(Instant::now() < deadline, "timed out: {t:#?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn continues_after_quitting_mid_download() {
    let server = start().await;
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let db = data.path().join("omni.db");
    let size = 16 * MIB;

    let id = {
        let m = open(&db).await;
        let added = m
            .add(vec![NewTask {
                source: server.url(&format!("/slow/{size}")),
                save_dir: downloads.path().to_string_lossy().into_owned(),
                options: TaskOptions {
                    connections: Some(4),
                    ..Default::default()
                },
                start: true,
                probe: None,
            }])
            .await
            .unwrap();
        let id = added[0].id;
        let t = wait_for(&m, id, |t| t.downloaded_bytes >= 2 * MIB).await;
        assert_eq!(t.status, TaskStatus::Downloading);
        assert_eq!(t.file_name, size.to_string());
        assert!(t.resumable);
        m.shutdown().await;
        id
    };
    let before = server.stats.ranges.lock().len();

    let m = open(&db).await;
    let t = m.get(id).unwrap();
    assert!(
        t.downloaded_bytes >= 2 * MIB,
        "progress survived: {}",
        t.downloaded_bytes
    );
    let done = wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(done.downloaded_bytes, size);
    assert_content(&downloads.path().join(&done.file_name), size);
    // The second run asked only for what was missing.
    let ranges = server.stats.ranges.lock().clone();
    assert!(
        ranges[before..].iter().all(|&s| s > 0),
        "{:?}",
        &ranges[before..]
    );
    let detail = m.detail(id).await.unwrap();
    assert!(
        detail.segments.is_empty(),
        "segments are dropped when complete"
    );
    m.shutdown().await;
    assert!(server.stats.open.load(Ordering::SeqCst) == 0);
}
