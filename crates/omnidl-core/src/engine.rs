//! What a protocol engine implements (docs/development-plan.md §4.1).
//!
//! The manager hands an engine a [`Job`] and awaits [`DownloadEngine::run`] until the download
//! completes, fails, or is cancelled through `job.cancel`. While it runs, the engine keeps
//! [`Progress`] current (the manager samples it for speed and the UI) and reports through
//! [`Checkpoint`] what has to survive a restart. Engines know nothing of the database or the UI.

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
    ConflictPolicy, EngineConfig, ProxyConfig, RateLimiter, Result, Segment, SegmentView, TaskId,
    TaskKind, TaskOptions,
};

#[async_trait]
pub trait DownloadEngine: Send + Sync + 'static {
    fn kind(&self) -> TaskKind;

    /// Size, name, range support of `source` without downloading it (the new-task dialog).
    async fn probe(&self, source: &str, options: &JobOptions) -> Result<ProbeInfo>;

    /// Downloads until done, failed or cancelled.
    async fn run(&self, job: Job) -> Outcome;
}

/// What a probe learns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// Live numbers of a running download, written by the engine, read by the manager.
#[derive(Debug, Default)]
pub struct Progress {
    downloaded: AtomicU64,
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
    pub connections: u32,
    pub user_agent: String,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub proxy: ProxyConfig,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub max_retries: u32,
    pub conflict: ConflictPolicy,
    pub sync_writes: bool,
}

impl JobOptions {
    pub fn resolve(task: &TaskOptions, config: &EngineConfig) -> Self {
        Self {
            connections: task.connections.unwrap_or(config.connections).clamp(1, 64),
            user_agent: task
                .user_agent
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| config.user_agent.clone()),
            referer: task.referer.clone().filter(|s| !s.trim().is_empty()),
            cookies: task.cookies.clone().filter(|s| !s.trim().is_empty()),
            headers: task.headers.clone(),
            proxy: task.proxy.clone().unwrap_or_else(|| config.proxy.clone()),
            connect_timeout: Duration::from_secs(config.connect_timeout_secs.into()),
            read_timeout: Duration::from_secs(config.read_timeout_secs.into()),
            max_retries: config.max_retries,
            conflict: config.conflict,
            sync_writes: config.sync_writes,
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
}

impl Job {
    /// Waits until both limiters allow `n` more bytes.
    pub async fn throttle(&self, n: u64) {
        self.task_limit.acquire(n).await;
        self.global_limit.acquire(n).await;
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
