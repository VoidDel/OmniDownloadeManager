//! What a protocol engine implements (docs/development-plan.md §4.1).
//!
//! The manager hands an engine a [`Job`] and awaits [`DownloadEngine::run`] until the download
//! completes, fails, or is cancelled through `job.cancel`. While it runs, the engine keeps
//! [`Progress`] current (the manager samples it for speed and the UI) and reports through
//! [`Checkpoint`] what has to survive a restart. Engines know nothing of the database or the UI.
//!
//! A BitTorrent run goes on after the download finished: it reports [`Checkpoint::seeding`],
//! uploads until the seeding policy is met or it is cancelled, and then returns `Completed`.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    ConflictPolicy, EngineConfig, Error, ErrorCode, ProxyConfig, RateLimiter, Result, Segment,
    SegmentView, Task, TaskId, TaskKind, TaskOptions,
    conns::{ConnControl, Mode as ConnMode},
};

#[async_trait]
pub trait DownloadEngine: Send + Sync + 'static {
    fn kind(&self) -> TaskKind;

    /// Size, name, range support of `source` without downloading it (the new-task dialog).
    async fn probe(&self, source: &str, options: &JobOptions) -> Result<ProbeInfo>;

    /// Downloads until done, failed or cancelled.
    async fn run(&self, job: Job) -> Outcome;

    /// Files, peers and trackers of a torrent task, running or not.
    async fn torrent_detail(&self, task: &Task) -> Option<TorrentDetail> {
        let _ = task;
        None
    }

    /// Changes which files of a running torrent are downloaded (the manager has already saved
    /// the choice in the task's options).
    async fn set_files(&self, id: TaskId, files: &[usize]) -> Result<()> {
        let _ = (id, files);
        Err(Error::new(
            ErrorCode::Unsupported,
            "this download has no files to choose",
        ))
    }

    /// The task was deleted for good: drop what the engine keeps for it.
    async fn forget(&self, id: TaskId) {
        let _ = id;
    }
}

/// What a probe learns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeInfo {
    /// After redirects.
    pub final_url: String,
    /// Cleaned, not yet made unique in the target folder.
    pub file_name: String,
    pub total: Option<u64>,
    pub resumable: bool,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// A torrent's files, in its order (their index is what `TaskOptions::only_files` holds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ProbeFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeFile {
    /// Relative, `/`-separated.
    pub path: String,
    pub size: u64,
}

/// A torrent task, for the details panel.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentDetail {
    pub info_hash: String,
    pub name: String,
    pub piece_size: u64,
    pub pieces: u32,
    pub files: Vec<TorrentFile>,
    /// Connected peers (empty while the task is not running).
    pub peers: Vec<PeerInfo>,
    /// Peers known from DHT, trackers and exchange.
    pub peers_seen: u32,
    pub trackers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFile {
    pub index: usize,
    pub path: String,
    pub size: u64,
    /// Bytes verified on disk, while the task runs.
    pub done: Option<u64>,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub addr: String,
    pub client: Option<String>,
    /// `tcp`, `utp` or `socks`.
    pub transport: String,
    /// Received from this peer.
    pub downloaded: u64,
    pub uploaded: u64,
}

/// What a download needs to continue after a pause or a restart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResumeState {
    /// Settled: the partial file is `file_name` + `.odm` in the save folder.
    pub file_name: String,
    pub final_url: Option<String>,
    pub total: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub resumable: bool,
    /// Empty when the download has to start over (no range support, or nothing saved yet).
    pub segments: Vec<Segment>,
    /// When it finished downloading (a torrent that was seeding when the app quit).
    pub completed_at: Option<i64>,
}

impl ResumeState {
    pub fn downloaded(&self) -> u64 {
        self.segments.iter().map(|s| s.downloaded).sum()
    }
}

/// Where an engine reports what must survive a restart.
#[async_trait]
pub trait Checkpoint: Send + Sync {
    /// Name, size and validators are settled and the partial file exists; the transfer starts.
    async fn started(&self, state: &ResumeState) -> Result<()>;
    /// Progress that is on disk (the engine flushed its writer first).
    async fn save(&self, state: &ResumeState) -> Result<()>;
    /// A torrent finished downloading and now uploads to others; its slot is free.
    async fn seeding(&self, state: &ResumeState) -> Result<()> {
        self.save(state).await
    }
}

/// Live numbers of a running download, written by the engine, read by the manager.
#[derive(Debug, Default)]
pub struct Progress {
    downloaded: AtomicU64,
    /// Uploaded in total (BitTorrent), earlier runs included.
    uploaded: AtomicU64,
    /// `u64::MAX` while unknown.
    total: AtomicU64,
    connections: AtomicU32,
    segments: Mutex<Vec<SegmentView>>,
}

impl Progress {
    pub fn new(downloaded: u64, total: Option<u64>) -> Self {
        Self {
            downloaded: AtomicU64::new(downloaded),
            total: AtomicU64::new(total.unwrap_or(u64::MAX)),
            ..Default::default()
        }
    }

    pub fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }

    pub fn set_downloaded(&self, n: u64) {
        self.downloaded.store(n, Ordering::Relaxed);
    }

    pub fn add(&self, n: u64) {
        self.downloaded.fetch_add(n, Ordering::Relaxed);
    }

    pub fn uploaded(&self) -> u64 {
        self.uploaded.load(Ordering::Relaxed)
    }

    pub fn set_uploaded(&self, n: u64) {
        self.uploaded.store(n, Ordering::Relaxed);
    }

    pub fn total(&self) -> Option<u64> {
        match self.total.load(Ordering::Relaxed) {
            u64::MAX => None,
            n => Some(n),
        }
    }

    pub fn set_total(&self, total: Option<u64>) {
        self.total
            .store(total.unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    pub fn connections(&self) -> u32 {
        self.connections.load(Ordering::Relaxed)
    }

    pub fn set_connections(&self, n: u32) {
        self.connections.store(n, Ordering::Relaxed);
    }

    pub fn segments(&self) -> Vec<SegmentView> {
        self.segments.lock().clone()
    }

    pub fn set_segments(&self, segments: Vec<SegmentView>) {
        *self.segments.lock() = segments;
    }
}

/// Task options merged with the global settings.
#[derive(Debug, Clone)]
pub struct JobOptions {
    /// Connections when not tuned.
    pub connections: u32,
    /// Tuned while the download runs, up to `auto_max_connections`.
    pub auto_connections: bool,
    pub auto_max_connections: u32,
    pub user_agent: String,
    /// The user agent may not be changed: the task sets its own, or carries cookies (a
    /// browser's cookies may only work with that browser's user agent).
    pub user_agent_fixed: bool,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub proxy: ProxyConfig,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub max_retries: u32,
    pub conflict: ConflictPolicy,
    pub sync_writes: bool,
    /// The task's own download limit, for engines that throttle inside a library.
    pub speed_limit: Option<u64>,
    pub only_files: Option<Vec<usize>>,
    pub seeding: bool,
    pub seed_ratio: f64,
    pub seed_minutes: u32,
    /// FTP sessions (1–8).
    pub ftp_connections: u32,
    pub ftp_active: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub insecure_tls: bool,
    /// Other addresses of the same file (HTTP).
    pub mirrors: Vec<String>,
    /// The sound to join to the picture (HTTP; see `TaskOptions::audio_source`).
    pub audio_source: Option<String>,
}

impl JobOptions {
    pub fn resolve(task: &TaskOptions, config: &EngineConfig) -> Self {
        Self {
            connections: task.connections.unwrap_or(config.connections).clamp(1, 64),
            auto_connections: task
                .auto_connections
                .unwrap_or(task.connections.is_none() && config.auto_connections),
            auto_max_connections: config.auto_max_connections.clamp(1, 64),
            user_agent: task
                .user_agent
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    if config.user_agent_auto {
                        crate::default_user_agent().into()
                    } else {
                        config.user_agent.clone()
                    }
                }),
            user_agent_fixed: !config.user_agent_auto
                || task
                    .user_agent
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty())
                || task
                    .cookies
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty()),
            referer: task.referer.clone().filter(|s| !s.trim().is_empty()),
            cookies: task.cookies.clone().filter(|s| !s.trim().is_empty()),
            headers: task.headers.clone(),
            proxy: task.proxy.clone().unwrap_or_else(|| config.proxy.clone()),
            connect_timeout: Duration::from_secs(config.connect_timeout_secs.into()),
            read_timeout: Duration::from_secs(config.read_timeout_secs.into()),
            max_retries: config.max_retries,
            conflict: config.conflict,
            sync_writes: config.sync_writes,
            speed_limit: task.speed_limit,
            only_files: task.only_files.clone(),
            seeding: config.seeding,
            seed_ratio: config.seed_ratio,
            seed_minutes: config.seed_minutes,
            ftp_connections: task
                .connections
                .unwrap_or(config.ftp_connections)
                .clamp(1, 8),
            ftp_active: task.ftp_active.unwrap_or(config.ftp_active),
            username: task.username.clone().filter(|s| !s.is_empty()),
            password: task.password.clone(),
            insecure_tls: task.insecure_tls.unwrap_or(false),
            mirrors: task
                .mirrors
                .iter()
                .map(|m| m.trim().to_owned())
                .filter(|m| !m.is_empty())
                .collect(),
            audio_source: task
                .audio_source
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        }
    }
}

impl JobOptions {
    /// How the connections of an HTTP download are chosen.
    pub fn conn_mode(&self) -> ConnMode {
        if self.auto_connections {
            ConnMode::Auto {
                max: self.auto_max_connections,
            }
        } else {
            ConnMode::Fixed(self.connections)
        }
    }

    /// The same for FTP sessions (the engine caps them at 8).
    pub fn ftp_conn_mode(&self) -> ConnMode {
        if self.auto_connections {
            ConnMode::Auto {
                max: self.auto_max_connections,
            }
        } else {
            ConnMode::Fixed(self.ftp_connections)
        }
    }
}

impl Default for JobOptions {
    fn default() -> Self {
        Self::resolve(&TaskOptions::default(), &EngineConfig::default())
    }
}

/// One run of a download.
pub struct Job {
    pub id: TaskId,
    pub source: String,
    pub save_dir: PathBuf,
    /// The name the user chose, for a download that has not started yet.
    pub file_name: Option<String>,
    /// Set once the download has started before.
    pub resume: Option<ResumeState>,
    pub options: JobOptions,
    pub cancel: CancellationToken,
    pub progress: Arc<Progress>,
    pub task_limit: RateLimiter,
    pub global_limit: RateLimiter,
    pub checkpoint: Arc<dyn Checkpoint>,
    /// How many connections to use, changed while the download runs.
    pub conns: Arc<ConnControl>,
}

impl Job {
    /// Waits until both limiters allow `n` more bytes.
    pub async fn throttle(&self, n: u64) {
        self.task_limit.acquire(n).await;
        self.global_limit.acquire(n).await;
    }

    /// The stricter speed limit in force, if any.
    pub fn speed_limit(&self) -> Option<u64> {
        match (self.task_limit.rate(), self.global_limit.rate()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// How a run ended.
#[derive(Debug)]
pub enum Outcome {
    /// The file is in place under `file_name` (which may differ from the planned one if the
    /// name was taken meanwhile).
    Completed { file_name: String, total: u64 },
    /// Cancelled; what is on disk.
    Stopped(ResumeState),
    /// What is on disk, if anything.
    Failed(crate::Error, Option<ResumeState>),
}
