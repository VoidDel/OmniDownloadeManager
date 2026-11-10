//! The engine against a second `librqbit` session on loopback: no DHT, trackers or local
//! discovery, the peers know each other's address.

use std::{
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
    create_torrent, limits::LimitsConfig, spawn_utils::BlockingSpawner,
};
use omnidl_bt::{BtConfig, BtEngine};
use omnidl_core::{
    DownloadEngine, EngineConfig, NewTask, Task, TaskId, TaskManager, TaskOptions, TaskStatus,
};
use omnidl_storage::Db;

const MIB: usize = 1 << 20;

/// Bytes that differ at every offset and between files.
fn content(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u32).wrapping_mul(2_654_435_761).to_le_bytes()[i % 4] ^ seed)
        .collect()
}

/// A session that only talks to peers it is told about.
async fn session(dir: &Path, upload_bps: Option<u32>) -> Arc<Session> {
    let options = SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        listen: Some(ListenerOptions {
            listen_addr: (Ipv4Addr::LOCALHOST, 0).into(),
            ..Default::default()
        }),
        ratelimits: LimitsConfig {
            upload_bps: upload_bps.and_then(std::num::NonZeroU32::new),
            download_bps: None,
        },
        ..Default::default()
    };
    Session::new_with_opts(dir.to_path_buf(), options)
        .await
        .unwrap()
}

fn local(addr: SocketAddr) -> SocketAddr {
    (Ipv4Addr::LOCALHOST, addr.port()).into()
}

/// Makes a torrent of `path` (a file or a folder) and seeds it from where it is.
struct Seeder {
    session: Arc<Session>,
    torrent: Vec<u8>,
    info_hash: String,
}

impl Seeder {
    async fn start(path: &Path, upload_bps: Option<u32>) -> Self {
        let made = create_torrent(
            path,
            CreateTorrentOptions {
                name: None,
                trackers: Vec::new(),
                piece_length: Some(64 * 1024),
            },
            &BlockingSpawner::new(2),
        )
        .await
        .unwrap();
        let torrent = made.as_bytes().unwrap().to_vec();
        let folder = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap().to_path_buf()
        };
        let session = session(&folder, upload_bps).await;
        let handle = session
            .add_torrent(
                AddTorrent::from_bytes(torrent.clone()),
                Some(AddTorrentOptions {
                    overwrite: true,
                    output_folder: Some(folder.to_string_lossy().into_owned()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        handle.wait_until_initialized().await.unwrap();
        assert!(handle.stats().finished, "the seeder has every piece");
        Self {
            session,
            torrent,
            info_hash: made.info_hash().as_string(),
        }
    }

    fn addr(&self) -> SocketAddr {
        local(self.session.listen_addr().unwrap())
    }
}

fn engine(state: &Path, peers: Vec<SocketAddr>) -> Arc<BtEngine> {
    let mut c = BtConfig::new(state);
    c.listen_port = 0;
    c.dht = false;
    c.upnp = false;
    c.trackers = false;
    c.local_discovery = false;
    c.extra_peers = peers;
    Arc::new(BtEngine::new(c))
}

async fn manager(db: Db, engine: Arc<BtEngine>, config: EngineConfig) -> TaskManager {
    let engines: Vec<Arc<dyn DownloadEngine>> = vec![engine];
    TaskManager::open(db, engines, config).await.unwrap()
}

fn no_seeding() -> EngineConfig {
    EngineConfig {
        seeding: false,
        ..Default::default()
    }
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
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let t = m.get(id).unwrap();
        if what(&t) {
            return t;
        }
        assert!(
            t.status != TaskStatus::Failed,
            "the task failed: {:?}",
            t.error
        );
        assert!(Instant::now() < deadline, "timed out; the task is {t:#?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn same(a: &Path, b: &Path) {
    let (x, y) = (std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
    assert!(x == y, "{} differs from {}", b.display(), a.display());
}

struct Dirs {
    _root: tempfile::TempDir,
    seed: PathBuf,
    state: PathBuf,
    downloads: PathBuf,
}

fn dirs() -> Dirs {
    let root = tempfile::tempdir().unwrap();
    let d = Dirs {
        seed: root.path().join("seed"),
        state: root.path().join("state"),
        downloads: root.path().join("downloads"),
        _root: root,
    };
    std::fs::create_dir_all(&d.seed).unwrap();
    std::fs::create_dir_all(&d.downloads).unwrap();
    d
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_a_torrent_file_from_a_peer() {
    let d = dirs();
    let file = d.seed.join("movie.mkv");
    write(&file, &content(3 * MIB + 12_345, 1));
    let seeder = Seeder::start(&file, None).await;
    let torrent = d.downloads.join("movie.torrent");
    write(&torrent, &seeder.torrent);

    let bt = engine(&d.state, vec![seeder.addr()]);
    let m = manager(Db::memory().await.unwrap(), bt.clone(), no_seeding()).await;
    let source = torrent.to_string_lossy().into_owned();
    let probe = m.probe(&source, &TaskOptions::default()).await.unwrap();
    assert_eq!(probe.file_name, "movie.mkv");
    assert_eq!(probe.total, Some(3 * MIB as u64 + 12_345));
    assert_eq!(probe.files.len(), 1);

    let added = m
        .add(vec![NewTask {
            probe: Some(probe),
            ..new_task(&source, &d.downloads, TaskOptions::default())
        }])
        .await
        .unwrap();
    assert_eq!(added[0].file_name, "movie.mkv");
    let t = wait_for(&m, added[0].id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(t.downloaded_bytes, 3 * MIB as u64 + 12_345);
    same(&file, &d.downloads.join("movie.mkv"));

    let detail = m.detail(t.id).await.unwrap();
    let torrent = detail.torrent.expect("torrent details");
    assert_eq!(torrent.info_hash, seeder.info_hash);
    assert_eq!(torrent.files[0].done, Some(3 * MIB as u64 + 12_345));
    m.shutdown().await;
    bt.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fetches_a_magnet_link_and_the_chosen_files() {
    let d = dirs();
    let pack = d.seed.join("pack");
    write(&pack.join("a.bin"), &content(MIB + 1, 2));
    write(&pack.join("sub/b.bin"), &content(2 * MIB, 3));
    write(&pack.join("c.bin"), &content(MIB / 2, 4));
    let seeder = Seeder::start(&pack, None).await;

    // The peer comes from the link's x.pe hint.
    let bt = engine(&d.state, Vec::new());
    let m = manager(Db::memory().await.unwrap(), bt.clone(), no_seeding()).await;
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&dn=pack&x.pe={}",
        seeder.info_hash,
        seeder.addr()
    );
    let started = Instant::now();
    let probe = m.probe(&magnet, &TaskOptions::default()).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "metadata in time"
    );
    assert_eq!(probe.file_name, "pack");
    let paths: Vec<&str> = probe.files.iter().map(|f| f.path.as_str()).collect();
    let a = paths.iter().position(|p| *p == "a.bin").unwrap();
    let b = paths.iter().position(|p| *p == "sub/b.bin").unwrap();
    let c = paths.iter().position(|p| *p == "c.bin").unwrap();

    let options = TaskOptions {
        only_files: Some(vec![a, c]),
        ..Default::default()
    };
    let added = m
        .add(vec![NewTask {
            probe: Some(probe),
            ..new_task(&magnet, &d.downloads, options)
        }])
        .await
        .unwrap();
    assert_eq!(added[0].total_bytes, Some((MIB + 1 + MIB / 2) as u64));
    let t = wait_for(&m, added[0].id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(t.file_name, "pack");
    let out = d.downloads.join("pack");
    same(&pack.join("a.bin"), &out.join("a.bin"));
    same(&pack.join("c.bin"), &out.join("c.bin"));
    let detail = m.detail(t.id).await.unwrap().torrent.unwrap();
    assert!(!detail.files[b].selected);
    m.shutdown().await;
    bt.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn seeds_until_the_ratio_is_reached() {
    let d = dirs();
    // The engine has the data already: checking it completes the task, then it seeds.
    let file = d.downloads.join("album.flac");
    let size = 2 * MIB;
    write(&file, &content(size, 5));
    let made = Seeder::start(&file, None).await;
    let torrent = d.seed.join("album.torrent");
    write(&torrent, &made.torrent);
    made.session.stop().await;

    let bt = engine(&d.state, Vec::new());
    let config = EngineConfig {
        seed_ratio: 1.0,
        seed_minutes: 0,
        ..Default::default()
    };
    let m = manager(Db::memory().await.unwrap(), bt.clone(), config).await;
    let id = m
        .add(vec![new_task(
            &torrent.to_string_lossy(),
            &d.downloads,
            TaskOptions::default(),
        )])
        .await
        .unwrap()[0]
        .id;
    let seeding = wait_for(&m, id, |t| t.status == TaskStatus::Seeding).await;
    assert!(seeding.completed_at.is_some());

    // A peer downloads it all from the engine: ratio 1.0.
    let leech_dir = d.seed.join("leech");
    std::fs::create_dir_all(&leech_dir).unwrap();
    let leecher = session(&leech_dir, None).await;
    let engine_addr = local(bt.listen_addr().await.unwrap().unwrap());
    leecher
        .add_torrent(
            AddTorrent::from_bytes(made.torrent.clone()),
            Some(AddTorrentOptions {
                output_folder: Some(leech_dir.to_string_lossy().into_owned()),
                initial_peers: Some(vec![engine_addr]),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert!(
        t.uploaded_bytes >= size as u64,
        "uploaded {}",
        t.uploaded_bytes
    );
    assert_eq!(t.completed_at, seeding.completed_at);
    m.shutdown().await;
    bt.stop().await;
    leecher.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resumes_after_a_restart() {
    let d = dirs();
    let file = d.seed.join("big.iso");
    let size = 6 * MIB;
    write(&file, &content(size, 6));
    // 1 MiB/s: the first run stops part way.
    let seeder = Seeder::start(&file, Some(MIB as u32)).await;
    let torrent = d.seed.join("big.torrent");
    write(&torrent, &seeder.torrent);
    let db_path = d.state.join("omni.db");
    std::fs::create_dir_all(&d.state).unwrap();

    let bt = engine(&d.state, vec![seeder.addr()]);
    let m = manager(Db::open(&db_path).await.unwrap(), bt.clone(), no_seeding()).await;
    let id = m
        .add(vec![new_task(
            &torrent.to_string_lossy(),
            &d.downloads,
            TaskOptions::default(),
        )])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.downloaded_bytes >= MIB as u64).await;
    m.shutdown().await;
    bt.stop().await;
    let partial = std::fs::metadata(d.downloads.join("big.iso")).unwrap();
    assert!(partial.len() > 0);

    // The metadata was kept: the source is not read again.
    std::fs::remove_file(&torrent).unwrap();
    let bt = engine(&d.state, vec![seeder.addr()]);
    let m = manager(Db::open(&db_path).await.unwrap(), bt.clone(), no_seeding()).await;
    let before = m.get(id).unwrap();
    assert!(before.downloaded_bytes >= MIB as u64);
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert_eq!(t.downloaded_bytes, size as u64);
    same(&file, &d.downloads.join("big.iso"));
    m.shutdown().await;
    bt.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_away_from_blocked_addresses() {
    let d = dirs();
    let file = d.seed.join("blocked.bin");
    write(&file, &content(2 * MIB, 3));
    let seeder = Seeder::start(&file, None).await;
    let torrent = d.downloads.join("blocked.torrent");
    write(&torrent, &seeder.torrent);

    // The only peer is on 127.0.0.1, which the list blocks (CIDR, as users write it).
    let list = d.state.join("ipfilter.p2p");
    let n = omnidl_bt::ipfilter::write(&[], &["127.0.0.0/8".into(), "::1".into()], &list).unwrap();
    assert_eq!(n, 2);
    let mut c = BtConfig::new(&d.state);
    c.listen_port = 0;
    c.dht = false;
    c.upnp = false;
    c.trackers = false;
    c.local_discovery = false;
    c.extra_peers = vec![seeder.addr()];
    c.blocklist = Some(list);
    let bt = Arc::new(BtEngine::new(c));
    let m = manager(Db::memory().await.unwrap(), bt.clone(), no_seeding()).await;
    let source = torrent.to_string_lossy().into_owned();
    let id = m
        .add(vec![new_task(
            &source,
            &d.downloads,
            TaskOptions::default(),
        )])
        .await
        .unwrap()[0]
        .id;
    // The session starts with the list, and nothing comes from the blocked peer.
    wait_for(&m, id, |t| t.status == TaskStatus::Downloading).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let t = m.get(id).unwrap();
    assert_eq!(t.downloaded_bytes, 0, "nothing from a blocked address");
    m.shutdown().await;
    bt.stop().await;
}

/// A tracker on loopback that answers every announce with `peer` and counts them.
struct Tracker {
    url: String,
    announces: Arc<std::sync::atomic::AtomicUsize>,
}

impl Tracker {
    async fn start(peer: Option<SocketAddr>) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}/announce", listener.local_addr().unwrap());
        let announces = Arc::new(AtomicUsize::new(0));
        let count = announces.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut conn, _)) = listener.accept().await else {
                    return;
                };
                let count = count.clone();
                tokio::spawn(async move {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 1024];
                    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                        match conn.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => req.extend_from_slice(&buf[..n]),
                        }
                    }
                    if req.starts_with(b"GET /announce?") {
                        count.fetch_add(1, Ordering::SeqCst);
                    }
                    // Compact peers: four address bytes and the port, big-endian.
                    let mut peers = Vec::new();
                    if let Some(SocketAddr::V4(a)) = peer {
                        peers.extend_from_slice(&a.ip().octets());
                        peers.extend_from_slice(&a.port().to_be_bytes());
                    }
                    let mut body = format!("d8:intervali60e5:peers{}:", peers.len()).into_bytes();
                    body.extend_from_slice(&peers);
                    body.push(b'e');
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = conn.write_all(head.as_bytes()).await;
                    let _ = conn.write_all(&body).await;
                    let _ = conn.shutdown().await;
                });
            }
        });
        Self { url, announces }
    }

    fn announces(&self) -> usize {
        self.announces.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// An engine that finds peers only through the trackers of the settings.
fn engine_with_trackers(state: &Path, trackers: Vec<String>) -> Arc<BtEngine> {
    let mut c = BtConfig::new(state);
    c.listen_port = 0;
    c.dht = false;
    c.upnp = false;
    c.trackers = true;
    c.local_discovery = false;
    c.extra_trackers = trackers;
    Arc::new(BtEngine::new(c))
}

#[tokio::test(flavor = "multi_thread")]
async fn finds_peers_through_the_trackers_of_the_settings() {
    let d = dirs();
    let file = d.seed.join("public.bin");
    write(&file, &content(2 * MIB, 4));
    let seeder = Seeder::start(&file, None).await;
    // The torrent names no tracker: the seeder is only known to the one of the settings.
    let torrent = d.downloads.join("public.torrent");
    write(&torrent, &seeder.torrent);
    let tracker = Tracker::start(Some(seeder.addr())).await;

    let bt = engine_with_trackers(&d.state, vec![tracker.url.clone()]);
    let m = manager(Db::memory().await.unwrap(), bt.clone(), no_seeding()).await;
    let source = torrent.to_string_lossy().into_owned();
    let id = m
        .add(vec![new_task(
            &source,
            &d.downloads,
            TaskOptions::default(),
        )])
        .await
        .unwrap()[0]
        .id;
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    assert!(tracker.announces() > 0);
    same(&file, &d.downloads.join(&t.file_name));
    let detail = m.detail(id).await.unwrap().torrent.unwrap();
    assert_eq!(detail.trackers, std::slice::from_ref(&tracker.url));
    m.shutdown().await;
    bt.stop().await;
}

/// `torrent` marked private (BEP 27): `private` is the last key of the info dictionary, which
/// is the last key of a torrent without trackers.
fn private(torrent: &[u8]) -> Vec<u8> {
    let mut t = torrent.to_vec();
    let at = t.len() - 2;
    t.splice(at..at, b"7:privatei1e".iter().copied());
    let parsed = librqbit::torrent_from_bytes(&t).expect("still a torrent");
    assert!(parsed.info.data.private, "marked private");
    t
}

#[tokio::test(flavor = "multi_thread")]
async fn private_torrents_keep_to_their_own_trackers() {
    let d = dirs();
    let file = d.seed.join("private.bin");
    write(&file, &content(MIB, 5));
    let seeder = Seeder::start(&file, None).await;
    let torrent = d.downloads.join("private.torrent");
    write(&torrent, &private(&seeder.torrent));
    let tracker = Tracker::start(Some(seeder.addr())).await;

    let bt = engine_with_trackers(&d.state, vec![tracker.url.clone()]);
    let m = manager(Db::memory().await.unwrap(), bt.clone(), no_seeding()).await;
    let source = torrent.to_string_lossy().into_owned();
    let id = m
        .add(vec![new_task(
            &source,
            &d.downloads,
            TaskOptions::default(),
        )])
        .await
        .unwrap()[0]
        .id;
    wait_for(&m, id, |t| t.status == TaskStatus::Downloading).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(
        tracker.announces(),
        0,
        "the info hash goes to no other tracker"
    );
    let detail = m.detail(id).await.unwrap().torrent.unwrap();
    assert!(detail.trackers.is_empty(), "{:?}", detail.trackers);
    m.shutdown().await;
    bt.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn magnet_links_use_the_trackers_of_the_settings() {
    let d = dirs();
    let file = d.seed.join("magnet.bin");
    write(&file, &content(MIB + 777, 6));
    let seeder = Seeder::start(&file, None).await;
    let tracker = Tracker::start(Some(seeder.addr())).await;
    // No `tr` and no `x.pe`: the metadata can only come through the tracker of the settings.
    let magnet = format!("magnet:?xt=urn:btih:{}", seeder.info_hash);

    let bt = engine_with_trackers(&d.state, vec![tracker.url.clone()]);
    let m = manager(Db::memory().await.unwrap(), bt.clone(), no_seeding()).await;
    let probe = m.probe(&magnet, &TaskOptions::default()).await.unwrap();
    assert_eq!(probe.total, Some(MIB as u64 + 777));
    let id = m
        .add(vec![new_task(
            &magnet,
            &d.downloads,
            TaskOptions::default(),
        )])
        .await
        .unwrap()[0]
        .id;
    let t = wait_for(&m, id, |t| t.status == TaskStatus::Completed).await;
    same(&file, &d.downloads.join(&t.file_name));
    m.shutdown().await;
    bt.stop().await;
}
