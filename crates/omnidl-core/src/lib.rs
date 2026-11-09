//! The download domain of Omni Download (docs/development-plan.md §4.1, §4.5).
//!
//! - [`Task`], [`TaskStatus`] and its state machine; [`TaskOptions`] and [`EngineConfig`].
//! - [`segments`]: the segment table shared by the multi-connection engines (HTTP now, FTP in
//!   Phase 4), with work stealing.
//! - [`filename`]: cleaning names for every file system, numbering duplicates, categories.
//! - [`RateLimiter`]: token buckets for the global and per-task limits.
//! - [`DownloadEngine`]: what a protocol crate implements; [`TaskManager`] schedules tasks onto
//!   engines, persists them through `omnidl-storage` and publishes [`TaskEvent`]s.

mod config;
mod engine;
mod error;
mod events;
pub mod filename;
mod manager;
mod options;
mod ratelimit;
mod secrets;
pub mod segments;
mod task;

pub use config::{ConflictPolicy, EngineConfig, default_user_agent};
pub use engine::{
    Checkpoint, DownloadEngine, Job, JobOptions, Outcome, PeerInfo, ProbeFile, ProbeInfo, Progress,
    ResumeState, TorrentDetail, TorrentFile,
};
pub use error::{Error, ErrorCode, Result};
pub use events::{EventBus, ProgressItem, Stats, TaskEvent};
pub use manager::{NewTask, TaskDetail, TaskManager};
pub use options::{ProxyConfig, TaskOptions};
pub use ratelimit::RateLimiter;
pub use secrets::{AesSecrets, NoSecrets, Secrets};
pub use segments::{Segment, SegmentTable, SegmentView};
pub use task::{Category, Priority, Task, TaskId, TaskKind, TaskStatus};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
