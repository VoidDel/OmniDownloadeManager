//! BitTorrent and magnet-link engine of Omni Download (docs/development-plan.md §4.4).
//!
//! An adapter over one `librqbit` session, started on first use. A task's metadata (the
//! `.torrent`, or what peers sent for a magnet link) is kept in the engine's folder under the
//! task's id, so a restart needs no second fetch; the files are written in place and checked
//! against their hashes when a task starts again, which is how it resumes. While a task runs
//! the engine maps the torrent's statistics onto [`Progress`], seeds once the chosen files are
//! complete, and stops by the seeding policy (ratio or time) of [`JobOptions`]. The rest of the
//! app never sees `librqbit` types.

pub mod ipfilter;
mod meta;
mod run;
pub mod trackers;

use std::{
    collections::{HashMap, HashSet},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerMode,
    ListenerOptions, Magnet, ManagedTorrent, Session, SessionOptions, dht::DhtPersistenceConfig,
    limits::LimitsConfig,
};
use omnidl_core::{
    DownloadEngine, Error, ErrorCode, Job, JobOptions, Outcome, PeerInfo, ProbeInfo, Result, Task,
    TaskId, TaskKind, TaskStatus, TorrentDetail, TorrentFile,
};
use parking_lot::Mutex;

use meta::Meta;

/// How long the new-task dialog waits for a magnet link's metadata.
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);

/// Session settings. The network ones apply when the session starts (the first torrent).
#[derive(Debug, Clone)]
pub struct BtConfig {
    /// Where the engine keeps each task's metadata (and the DHT routing table).
    pub state_dir: PathBuf,
    /// For incoming connections; 0 picks a free port. A taken port falls back to a free one.
    pub listen_port: u16,
    pub dht: bool,
    pub upnp: bool,
    /// Trackers and local peer discovery (tests turn them off).
    pub trackers: bool,
    pub local_discovery: bool,
    /// Peers tried for every torrent besides those found (tests).
    pub extra_peers: Vec<SocketAddr>,
    /// A block list in the P2P format (`ipfilter::write`): peers in it are not connected to.
    /// Read when the session starts.
    pub blocklist: Option<PathBuf>,
    /// Trackers announced to for every torrent that is not private, besides its own
    /// (`trackers::parse` addresses); [`BtEngine::set_extra_trackers`] changes them.
    pub extra_trackers: Vec<String>,
}

impl BtConfig {
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            state_dir: state_dir.into(),
            listen_port: 6881,
            dht: true,
            upnp: true,
            trackers: true,
            local_discovery: true,
            extra_peers: Vec::new(),
            blocklist: None,
            extra_trackers: Vec::new(),
        }
    }
}

/// Metadata and the peers that sent it.
#[derive(Clone)]
struct Fetched {
    bytes: Bytes,
    peers: Vec<SocketAddr>,
}

pub struct BtEngine {
    config: BtConfig,
    session: tokio::sync::Mutex<Option<Arc<Session>>>,
    /// The torrents of running tasks.
    running: Mutex<HashMap<TaskId, Arc<ManagedTorrent>>>,
    /// Metadata fetched by probes, by info hash: adding the task right after needs no second
    /// fetch.
    probed: Mutex<HashMap<String, Fetched>>,
    /// Bytes per second for all torrents together.
    upload_limit: Mutex<Option<u64>>,
    /// Added to each torrent that is not private (not the session's `trackers`: librqbit
    /// gives those to private torrents with a single tracker too).
    extra_trackers: Mutex<Vec<String>>,
}

impl BtEngine {
    pub fn new(config: BtConfig) -> Self {
        Self {
            extra_trackers: Mutex::new(trackers::merge([config.extra_trackers.as_slice()])),
            config,
            session: tokio::sync::Mutex::new(None),
            running: Mutex::new(HashMap::new()),
            probed: Mutex::new(HashMap::new()),
            upload_limit: Mutex::new(None),
        }
    }

    /// The upload limit of all torrents together (`None`: unlimited); applies at once.
    pub async fn set_upload_limit(&self, limit: Option<u64>) {
        let limit = limit.filter(|&n| n > 0);
        *self.upload_limit.lock() = limit;
        if let Some(s) = &*self.session.lock().await {
            s.ratelimits.set_upload_bps(bps(limit));
        }
    }

    /// The trackers added to torrents that are not private; tasks that start from now on use
    /// them (running ones keep theirs).
    pub fn set_extra_trackers(&self, list: &[String]) {
        *self.extra_trackers.lock() = trackers::merge([list]);
    }

    /// The trackers added to torrents that are not private (none when trackers are off).
    fn extra_trackers(&self) -> Vec<String> {
        if self.config.trackers {
            self.extra_trackers.lock().clone()
        } else {
            Vec::new()
        }
    }

    /// Where incoming connections arrive (starts the session).
    pub async fn listen_addr(&self) -> Result<Option<SocketAddr>> {
        Ok(self.session().await?.listen_addr())
    }

    /// Stops every torrent and the session (the app quits).
    pub async fn stop(&self) {
        if let Some(s) = self.session.lock().await.take() {
            s.stop().await;
        }
    }

    async fn session(&self) -> Result<Arc<Session>> {
        let mut slot = self.session.lock().await;
        if let Some(s) = &*slot {
            return Ok(s.clone());
        }
        let s = self.start_session().await?;
        *slot = Some(s.clone());
        Ok(s)
    }

    async fn start_session(&self) -> Result<Arc<Session>> {
        let c = &self.config;
        std::fs::create_dir_all(&c.state_dir).map_err(|e| Error::io(&e, c.state_dir.display()))?;
        let options = |port: u16, ipv4_only: bool| SessionOptions {
            dht: c.dht.then(|| DhtSessionConfig {
                bootstrap_addrs: None,
                port: None,
                persistence: Some(DhtPersistenceConfig {
                    dump_interval: None,
                    config_filename: Some(c.state_dir.join("dht.json")),
                }),
            }),
            disable_trackers: !c.trackers,
            disable_local_service_discovery: !c.local_discovery,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: if ipv4_only {
                    (Ipv4Addr::UNSPECIFIED, port).into()
                } else {
                    (Ipv6Addr::UNSPECIFIED, port).into()
                },
                enable_upnp_port_forwarding: c.upnp,
                ipv4_only,
                ..Default::default()
            }),
            ipv4_only,
            ratelimits: LimitsConfig {
                upload_bps: bps(*self.upload_limit.lock()),
                download_bps: None,
            },
            client_name_and_version: Some(format!("OmniDownload/{}", env!("CARGO_PKG_VERSION"))),
            // An empty list would stop the session from starting: only one with ranges.
            blocklist_url: c
                .blocklist
                .as_deref()
                .filter(|p| ipfilter::count(p) > 0)
                .and_then(|p| url::Url::from_file_path(p).ok())
                .map(String::from),
            ..Default::default()
        };
        let folder = c.state_dir.join("default");
        // The chosen port on IPv6 and IPv4; a taken port: any port; no IPv6: IPv4 only.
        let mut attempts = vec![(c.listen_port, false)];
        if c.listen_port != 0 {
            attempts.push((0, false));
        }
        attempts.push((0, true));
        let mut last = None;
        for (port, ipv4_only) in attempts {
            match Session::new_with_opts(folder.clone(), options(port, ipv4_only)).await {
                Ok(s) => return Ok(s),
                Err(e) => {
                    tracing::warn!(
                        "BitTorrent session on port {port} (IPv4 only: {ipv4_only}): {e:#}"
                    );
                    last = Some(e);
                }
            }
        }
        Err(failure(
            last.map_or_else(|| "no session".into(), |e| format!("{e:#}")),
        ))
    }

    /// Peers to try for `source`: those of the settings and a magnet link's `x.pe` hints.
    fn peers_for(&self, source: &Source) -> Vec<SocketAddr> {
        let mut peers = self.config.extra_peers.clone();
        if let Source::Magnet { peers: hints, .. } = source {
            peers.extend(hints);
        }
        peers
    }

    fn torrent_path(&self, id: TaskId) -> PathBuf {
        self.config.state_dir.join(format!("{id}.torrent"))
    }

    /// The metadata of `source`: a local `.torrent`, one served over HTTP, or a magnet link's
    /// (asked of peers).
    async fn fetch(&self, session: &Arc<Session>, source: &Source) -> Result<Fetched> {
        let url = match source {
            Source::File(path) => {
                let bytes = tokio::fs::read(path).await.map_err(|e| {
                    Error::new(
                        ErrorCode::FileMissing,
                        format!("cannot read {}: {e}", path.display()),
                    )
                })?;
                return Ok(Fetched {
                    bytes: bytes.into(),
                    peers: Vec::new(),
                });
            }
            Source::Magnet { url, info_hash, .. } => {
                if let Some(f) = self.probed.lock().get(info_hash).cloned() {
                    return Ok(f);
                }
                // librqbit takes no extra trackers for a magnet link but those in it. Whether
                // the torrent is private is not known yet (qBittorrent adds them too).
                with_trackers(url, &self.extra_trackers())
            }
            Source::Url(url) => url.clone(),
        };
        let options = AddTorrentOptions {
            list_only: true,
            initial_peers: Some(self.peers_for(source)),
            ..Default::default()
        };
        match session
            .add_torrent(AddTorrent::from_url(url.as_str()), Some(options))
            .await
            .map_err(failure)?
        {
            AddTorrentResponse::ListOnly(r) => Ok(Fetched {
                bytes: r.torrent_bytes,
                peers: r.seen_peers,
            }),
            _ => Err(Error::new(
                ErrorCode::Internal,
                "the torrent was added early",
            )),
        }
    }
}

#[async_trait]
impl DownloadEngine for BtEngine {
    fn kind(&self) -> TaskKind {
        TaskKind::Torrent
    }

    async fn probe(&self, source: &str, _: &JobOptions) -> Result<ProbeInfo> {
        let parsed = Source::parse(source)?;
        let session = self.session().await?;
        let fetched = tokio::time::timeout(PROBE_TIMEOUT, self.fetch(&session, &parsed))
            .await
            .map_err(|_| {
                Error::new(
                    ErrorCode::Timeout,
                    "no peer sent the torrent's details in time",
                )
            })??;
        let meta = Meta::parse(&fetched.bytes)?;
        {
            let mut probed = self.probed.lock();
            if probed.len() >= 32 {
                probed.clear();
            }
            probed.insert(meta.info_hash.clone(), fetched);
        }
        let fallback = parsed.fallback_name(&meta.info_hash);
        Ok(ProbeInfo {
            final_url: source.trim().to_owned(),
            file_name: meta.layout(&fallback).name,
            total: Some(meta.size(None)),
            resumable: true,
            content_type: Some("application/x-bittorrent".into()),
            etag: None,
            last_modified: None,
            files: meta.files,
        })
    }

    async fn run(&self, job: Job) -> Outcome {
        run::run(self, job).await
    }

    async fn torrent_detail(&self, task: &Task) -> Option<TorrentDetail> {
        let handle = self.running.lock().get(&task.id).cloned();
        let bytes = tokio::fs::read(self.torrent_path(task.id)).await.ok()?;
        let meta = Meta::parse(&bytes).ok()?;
        let only = task.options.only_files.as_deref();
        let complete = matches!(task.status, TaskStatus::Completed | TaskStatus::Seeding);
        let mut d = TorrentDetail {
            name: meta.name.clone().unwrap_or_else(|| task.file_name.clone()),
            info_hash: meta.info_hash.clone(),
            piece_size: meta.piece_size,
            pieces: meta.pieces,
            files: meta
                .files
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let selected = only.is_none_or(|o| o.contains(&i));
                    TorrentFile {
                        index: i,
                        path: f.path.clone(),
                        size: f.size,
                        done: (complete && selected).then_some(f.size),
                        selected,
                    }
                })
                .collect(),
            trackers: meta.trackers.clone(),
            ..Default::default()
        };
        if let Some(h) = handle {
            let stats = h.stats();
            for (f, done) in d.files.iter_mut().zip(&stats.file_progress) {
                f.done = Some(*done);
            }
            if let Some(live) = h.live() {
                d.peers_seen = live.stats_snapshot().peer_stats.seen;
                let snapshot = live.per_peer_stats_snapshot(Default::default());
                d.peers = snapshot
                    .peers
                    .into_iter()
                    .map(|(addr, p)| PeerInfo {
                        addr,
                        client: p.client_name,
                        transport: p
                            .conn_kind
                            .map_or("tcp".into(), |k| format!("{k:?}").to_lowercase()),
                        downloaded: p.counters.fetched_bytes,
                        uploaded: p.counters.uploaded_bytes,
                    })
                    .collect();
                d.peers
                    .sort_by_key(|p| std::cmp::Reverse(p.downloaded + p.uploaded));
            }
            for t in &h.shared().trackers {
                let t = t.to_string();
                if !d.trackers.contains(&t) {
                    d.trackers.push(t);
                }
            }
        }
        // Those of the settings are added when the task starts, except to private torrents.
        if !meta.private {
            for t in self.extra_trackers() {
                if !d.trackers.contains(&t) {
                    d.trackers.push(t);
                }
            }
        }
        Some(d)
    }

    async fn set_files(&self, id: TaskId, files: &[usize]) -> Result<()> {
        let Some(handle) = self.running.lock().get(&id).cloned() else {
            return Ok(());
        };
        let session = self.session().await?;
        let files: HashSet<usize> = files.iter().copied().collect();
        session
            .update_only_files(&handle, &files)
            .await
            .map_err(failure)
    }

    async fn forget(&self, id: TaskId) {
        let path = self.torrent_path(id);
        if let Err(e) = tokio::fs::remove_file(&path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("deleting {}: {e}", path.display());
        }
    }
}

/// What a task's source names.
enum Source {
    Magnet {
        url: String,
        info_hash: String,
        /// `x.pe` peer addresses (BEP 9).
        peers: Vec<SocketAddr>,
    },
    Url(String),
    File(PathBuf),
}

impl Source {
    fn parse(source: &str) -> Result<Self> {
        let s = source.trim();
        let lower = s.get(..8).unwrap_or(s).to_ascii_lowercase();
        if lower.starts_with("magnet:") {
            let invalid = || Error::new(ErrorCode::InvalidUrl, "not a valid magnet link");
            let magnet = Magnet::parse(s).map_err(|_| invalid())?;
            let hash = magnet.as_id20().ok_or_else(invalid)?;
            let query = s.split_once('?').map_or("", |(_, q)| q);
            let peers = query
                .split('&')
                .filter_map(|kv| kv.strip_prefix("x.pe="))
                .filter_map(|v| omnidl_core::filename::percent_decode(v).parse().ok())
                .collect();
            Ok(Self::Magnet {
                url: s.to_owned(),
                info_hash: hash.as_string(),
                peers,
            })
        } else if lower.starts_with("http://") || lower.starts_with("https://") {
            Ok(Self::Url(s.to_owned()))
        } else {
            Ok(Self::File(Path::new(s).to_path_buf()))
        }
    }

    /// A name for a torrent whose metadata has none.
    fn fallback_name(&self, info_hash: &str) -> String {
        let named = match self {
            Self::Magnet { url, .. } => omnidl_core::filename::for_torrent(url),
            Self::Url(u) => omnidl_core::filename::for_torrent(u),
            Self::File(p) => omnidl_core::filename::for_torrent(&p.to_string_lossy()),
        };
        named.unwrap_or_else(|| info_hash.to_owned())
    }
}

fn bps(limit: Option<u64>) -> Option<NonZeroU32> {
    limit.and_then(|n| NonZeroU32::new(n.min(u32::MAX.into()) as u32))
}

/// A `librqbit` error.
fn failure(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::Network, format!("{e:#}"))
}

/// `magnet` with `trackers` added as `tr` parameters (those it does not name already).
fn with_trackers(magnet: &str, trackers: &[String]) -> String {
    let have = Magnet::parse(magnet)
        .map(|m| m.trackers)
        .unwrap_or_default();
    let mut out = magnet.trim().to_owned();
    for t in trackers {
        if have
            .iter()
            .any(|h| url::Url::parse(h).is_ok_and(|h| h.as_str() == t))
        {
            continue;
        }
        out.push(if out.contains('?') { '&' } else { '?' });
        out.push_str("tr=");
        out.extend(url::form_urlencoded::byte_serialize(t.as_bytes()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::with_trackers;

    #[test]
    fn adds_trackers_to_magnet_links() {
        let m = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=udp%3A%2F%2Fa.example%3A1%2Fannounce";
        let got = with_trackers(
            m,
            &[
                "udp://a.example:1/announce".into(),
                "http://b.example/announce?k=1".into(),
            ],
        );
        assert_eq!(
            got,
            format!("{m}&tr=http%3A%2F%2Fb.example%2Fannounce%3Fk%3D1")
        );
        let parsed = librqbit::Magnet::parse(&got).unwrap();
        assert_eq!(
            parsed.trackers,
            [
                "udp://a.example:1/announce",
                "http://b.example/announce?k=1"
            ]
        );
        assert_eq!(with_trackers(m, &[]), m);
    }
}
