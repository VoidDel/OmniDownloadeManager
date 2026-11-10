//! The engine (through the task manager) against an in-process FTP server (libunftp over a
//! temporary folder): anonymous and with an account, several sessions, pause and resume with
//! REST, FTPS, a missing file and a file that changed.

use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use libunftp::{ServerBuilder, auth::AnonymousAuthenticator};
use omnidl_core::{
    DownloadEngine, EngineConfig, ErrorCode, NewTask, Task, TaskId, TaskManager, TaskOptions,
    TaskStatus,
};
use omnidl_ftp::FtpEngine;
use omnidl_storage::Db;
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, Principal};
use unftp_sbe_fs::Filesystem;

const MIB: usize = 1 << 20;

fn content(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u32).wrapping_mul(2_654_435_761).to_le_bytes()[i % 4] ^ seed)
        .collect()
}

/// One account.
#[derive(Debug)]
struct Account;

#[async_trait]
impl Authenticator for Account {
    async fn authenticate(
        &self,
        username: &str,
        creds: &Credentials,
    ) -> Result<Principal, AuthenticationError> {
        if username == "bob" && creds.password.as_deref() == Some("s3cret") {
            Ok(Principal {
                username: username.into(),
            })
        } else {
            Err(AuthenticationError::BadPassword)
        }
    }
}

struct Server {
    port: u16,
    root: PathBuf,
}

impl Server {
    fn url(&self, scheme: &str, path: &str) -> String {
        format!("{scheme}://127.0.0.1:{}/{path}", self.port)
    }
}

/// Serves `root` on a free port; with an account only `bob` gets in, with `tls` FTPS is
/// offered (explicit, `AUTH TLS`).
async fn serve(root: &Path, account: bool, tls: Option<(PathBuf, PathBuf)>) -> Server {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = root.to_path_buf();
    let auth: Arc<dyn Authenticator + Send + Sync> = if account {
        Arc::new(Account)
    } else {
        Arc::new(AnonymousAuthenticator)
    };
    let mut builder = ServerBuilder::with_authenticator(
        Box::new(move || Filesystem::new(dir.clone()).unwrap()),
        auth,
    )
    .greeting("omni test server");
    if let Some((cert, key)) = tls {
        builder = builder.ftps(cert, key);
    }
    let server = builder.build().unwrap();
    tokio::spawn(async move { server.listen(format!("127.0.0.1:{port}")).await });
    let deadline = Instant::now() + Duration::from_secs(10);
    while tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err()
    {
        assert!(Instant::now() < deadline, "the FTP server did not start");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Server {
        port,
        root: root.to_path_buf(),
    }
}

async fn manager() -> TaskManager {
    let engines: Vec<Arc<dyn DownloadEngine>> = vec![Arc::new(FtpEngine::with_retry_delay(
        Duration::from_millis(50),
    ))];
    let config = EngineConfig {
        max_retries: 2,
        ..Default::default()
    };
    TaskManager::open(Db::memory().await.unwrap(), engines, config)
        .await
        .unwrap()
}

fn new_task(source: &str, dir: &Path, options: TaskOptions) -> NewTask {
    NewTask {
        source: source.into(),
        save_dir: dir.to_string_lossy().into_owned(),
        options,
        start: true,
        probe: None,
    }
}

async fn wait_for(m: &TaskManager, id: TaskId, what: impl Fn(&Task) -> bool) -> Task {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let t = m.get(id).unwrap();
        if what(&t) {
            return t;
        }
        assert!(Instant::now() < deadline, "timed out; the task is {t:#?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

struct Dirs {
    _root: tempfile::TempDir,
    served: PathBuf,
    downloads: PathBuf,
}

fn dirs() -> Dirs {
    let root = tempfile::tempdir().unwrap();
    let d = Dirs {
        served: root.path().join("served"),
        downloads: root.path().join("downloads"),
        _root: root,
    };
    std::fs::create_dir_all(d.served.join("pub")).unwrap();
    std::fs::create_dir_all(&d.downloads).unwrap();
    d
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_anonymously_over_several_sessions() {
    let d = dirs();
    let data = content(6 * MIB + 777, 1);
    std::fs::write(d.served.join("pub/ubuntu 24.04.iso"), &data).unwrap();
    let s = serve(&d.served, false, None).await;
    let m = manager().await;
    let url = s.url("ftp", "pub/ubuntu%2024.04.iso");

    let probe = m.probe(&url, &TaskOptions::default()).await.unwrap();
    assert_eq!(probe.file_name, "ubuntu 24.04.iso");
    assert_eq!(probe.total, Some(data.len() as u64));
    assert!(probe.resumable);
    assert!(probe.last_modified.is_some());

    // Slow enough (8 MiB/s) to see the sessions at work.
    let options = TaskOptions {
        connections: Some(4),
        speed_limit: Some(8 * MIB as u64),
        ..Default::default()
    };
    let id = m
        .add(vec![new_task(&url, &d.downloads, options)])
        .await
        .unwrap()[0]
        .id;
    let most = std::cell::Cell::new(0);
    let t = wait_for(&m, id, |t| {
        most.set(most.get().max(t.connections));
        t.status == TaskStatus::Completed
    })
    .await;
    assert!(most.get() > 1, "several sessions ran ({})", most.get());
    assert_eq!(t.downloaded_bytes, data.len() as u64);
    assert_eq!(
        std::fs::read(d.downloads.join("ubuntu 24.04.iso")).unwrap(),
        data
    );
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn logs_in_with_an_account() {
    let d = dirs();
    let data = content(MIB + 5, 2);
    std::fs::write(d.served.join("pub/report.pdf"), &data).unwrap();
    let s = serve(&d.served, true, None).await;
    let m = manager().await;

    let wrong = format!("ftp://bob:nope@127.0.0.1:{}/pub/report.pdf", s.port);
    let id = m
        .add(vec![new_task(&wrong, &d.downloads, TaskOptions::default())])
        .await
        .unwrap()[0]
        .id;
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Failed).await;
    assert_eq!(t.error.unwrap().code, ErrorCode::AuthRequired);

    let right = format!("ftp://bob:s3cret@127.0.0.1:{}/pub/report.pdf", s.port);
    let added = m
        .add(vec![new_task(&right, &d.downloads, TaskOptions::default())])
        .await
        .unwrap();
    // The password left the link for the (sealed) options.
    assert_eq!(added[0].source, s.url("ftp", "pub/report.pdf"));
    assert_eq!(added[0].options.username.as_deref(), Some("bob"));
    assert_eq!(added[0].options.password.as_deref(), Some("s3cret"));
    wait_for(&m, added[0].id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(std::fs::read(d.downloads.join("report.pdf")).unwrap(), data);

    // The account given apart from the link works the same.
    let options = TaskOptions {
        username: Some("bob".into()),
        password: Some("s3cret".into()),
        file_name: Some("copy.pdf".into()),
        ..Default::default()
    };
    let id = m
        .add(vec![new_task(
            &s.url("ftp", "pub/report.pdf"),
            &d.downloads,
            options,
        )])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(std::fs::read(d.downloads.join("copy.pdf")).unwrap(), data);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pauses_and_resumes_where_it_stopped() {
    let d = dirs();
    let data = content(4 * MIB, 3);
    std::fs::write(d.served.join("pub/big.bin"), &data).unwrap();
    let s = serve(&d.served, false, None).await;
    let m = manager().await;
    let options = TaskOptions {
        connections: Some(2),
        speed_limit: Some(MIB as u64),
        ..Default::default()
    };
    let id = m
        .add(vec![new_task(
            &s.url("ftp", "pub/big.bin"),
            &d.downloads,
            options,
        )])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.downloaded_bytes >= MIB as u64).await;
    m.pause(&[id]).await.unwrap();
    let paused = wait_for(&m, id, |t| t.status == TaskStatus::Paused).await;
    assert!(paused.downloaded_bytes >= MIB as u64 && paused.downloaded_bytes < data.len() as u64);
    assert!(paused.resumable);

    m.set_task_speed_limit(id, None).await.unwrap();
    m.resume(&[id]).await.unwrap();
    // REST: it goes on from the saved bytes, not from zero.
    let again = wait_for(&m, id, |t| {
        matches!(t.status, TaskStatus::Downloading | TaskStatus::Completed)
    })
    .await;
    assert!(again.downloaded_bytes >= paused.downloaded_bytes);
    wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(std::fs::read(d.downloads.join("big.bin")).unwrap(), data);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn will_not_mix_a_changed_file() {
    let d = dirs();
    let data = content(3 * MIB, 4);
    let path = d.served.join("pub/nightly.zip");
    std::fs::write(&path, &data).unwrap();
    let s = serve(&d.served, false, None).await;
    let m = manager().await;
    let options = TaskOptions {
        speed_limit: Some(MIB as u64),
        ..Default::default()
    };
    let id = m
        .add(vec![new_task(
            &s.url("ftp", "pub/nightly.zip"),
            &d.downloads,
            options,
        )])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.downloaded_bytes > 0).await;
    m.pause(&[id]).await.unwrap();
    wait_for(&m, id, |t| t.status == TaskStatus::Paused).await;
    std::fs::write(&path, content(3 * MIB + 10, 5)).unwrap();
    m.resume(&[id]).await.unwrap();
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Failed).await;
    assert_eq!(t.error.unwrap().code, ErrorCode::RemoteChanged);
    // Restarting downloads the new version from the beginning.
    m.restart(&[id]).await.unwrap();
    wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(
        std::fs::read(d.downloads.join("nightly.zip"))
            .unwrap()
            .len(),
        3 * MIB + 10
    );
    let _ = &s.root;
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_a_missing_file() {
    let d = dirs();
    let s = serve(&d.served, false, None).await;
    let m = manager().await;
    let e = m
        .probe(&s.url("ftp", "pub/nothing.iso"), &TaskOptions::default())
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    let e = m
        .probe(&s.url("ftp", "pub/"), &TaskOptions::default())
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidUrl);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_over_ftps() {
    let d = dirs();
    let cert =
        rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    let cert_file = d.served.join("../cert.pem");
    let key_file = d.served.join("../key.pem");
    std::fs::write(&cert_file, cert.cert.pem()).unwrap();
    std::fs::write(&key_file, cert.signing_key.serialize_pem()).unwrap();
    let data = content(2 * MIB + 3, 6);
    std::fs::write(d.served.join("pub/secret.tar"), &data).unwrap();
    let s = serve(&d.served, false, Some((cert_file, key_file))).await;
    let m = manager().await;
    let url = s.url("ftps", "pub/secret.tar");

    // A self-signed certificate does not verify…
    let e = m.probe(&url, &TaskOptions::default()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Tls, "{e:?}");
    // …unless the task accepts it.
    let options = TaskOptions {
        insecure_tls: Some(true),
        connections: Some(2),
        ..Default::default()
    };
    let probe = m.probe(&url, &options).await.unwrap();
    assert_eq!(probe.total, Some(data.len() as u64));
    let id = m
        .add(vec![new_task(&url, &d.downloads, options)])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(std::fs::read(d.downloads.join("secret.tar")).unwrap(), data);
    m.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_a_folder_with_its_subfolders() {
    let d = dirs();
    std::fs::create_dir_all(d.served.join("pub/set/sub one")).unwrap();
    std::fs::write(d.served.join("pub/set/a.bin"), content(1000, 1)).unwrap();
    std::fs::write(d.served.join("pub/set/sub one/b c.bin"), content(2000, 2)).unwrap();
    let s = serve(&d.served, false, None).await;
    let m = manager().await;
    let url = s.url("ftp", "pub/set/");

    let probe = m.probe(&url, &TaskOptions::default()).await.unwrap();
    assert_eq!(probe.file_name, "set");
    assert_eq!(probe.total, Some(3000));
    let paths: Vec<_> = probe.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.bin", "sub one/b c.bin"]);

    let files = omnidl_ftp::list_folder(&url, &omnidl_core::JobOptions::default())
        .await
        .unwrap();
    assert_eq!(files[1].url, s.url("ftp", "pub/set/sub%20one/b%20c.bin"));
    // Each file's link downloads it.
    let id = m
        .add(vec![new_task(
            &files[1].url,
            &d.downloads,
            TaskOptions::default(),
        )])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(
        std::fs::read(d.downloads.join("b c.bin")).unwrap(),
        content(2000, 2)
    );
    m.shutdown().await;
}
