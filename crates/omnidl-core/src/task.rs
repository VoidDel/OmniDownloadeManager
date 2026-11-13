//! The task model and its state machine (docs/development-plan.md §4.1).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Error, Secrets, TaskOptions, filename, secrets::SEALED_PREFIX};

pub type TaskId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Http,
    Ftp,
    Torrent,
}

impl TaskKind {
    /// The engine for a source: `http(s)://`, `ftp(s)://`, `magnet:` or a `.torrent` file.
    pub fn detect(source: &str) -> Option<Self> {
        let s = source.trim();
        let lower = s.get(..8).unwrap_or(s).to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            Some(Self::Http)
        } else if lower.starts_with("ftp://") || lower.starts_with("ftps://") {
            Some(Self::Ftp)
        } else if lower.starts_with("magnet:") || s.to_ascii_lowercase().ends_with(".torrent") {
            Some(Self::Torrent)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Ftp => "ftp",
            Self::Torrent => "torrent",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "http" => Self::Http,
            "ftp" => Self::Ftp,
            "torrent" => Self::Torrent,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Waiting for a slot.
    Queued,
    /// Asking the server for size, name and range support.
    Probing,
    Downloading,
    Paused,
    /// BitTorrent only (Phase 3).
    Seeding,
    Completed,
    Failed,
    /// In the recycle bin; can be restored.
    Removed,
}

impl TaskStatus {
    pub const ALL: [Self; 8] = [
        Self::Queued,
        Self::Probing,
        Self::Downloading,
        Self::Paused,
        Self::Seeding,
        Self::Completed,
        Self::Failed,
        Self::Removed,
    ];

    /// Running in an engine.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Probing | Self::Downloading | Self::Seeding)
    }

    /// The state machine of §4.1. Restoring from the recycle bin returns to the status the task
    /// had (paused for one that was running).
    pub fn can_become(self, to: Self) -> bool {
        use TaskStatus::*;
        match (self, to) {
            (a, b) if a == b => false,
            (_, Removed) => self != Removed,
            (Queued, Probing | Downloading | Paused) => true,
            (Probing, Downloading | Seeding | Paused | Failed | Completed | Queued) => true,
            (Downloading, Seeding | Completed | Paused | Failed | Queued) => true,
            (Paused, Queued) => true,
            (Failed, Queued) => true,
            (Completed, Seeding | Queued) => true,
            // Stopping a seeding torrent completes it; quitting queues it for the next launch.
            (Seeding, Completed | Failed | Queued) => true,
            (Removed, Paused | Completed | Failed | Queued) => true,
            _ => false,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Probing => "probing",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Seeding => "seeding",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Removed => "removed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Video,
    Audio,
    Document,
    Archive,
    Program,
    DiskImage,
    Image,
    Other,
}

impl Category {
    pub const ALL: [Self; 8] = [
        Self::Video,
        Self::Audio,
        Self::Document,
        Self::Archive,
        Self::Program,
        Self::DiskImage,
        Self::Image,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Document => "document",
            Self::Archive => "archive",
            Self::Program => "program",
            Self::DiskImage => "disk_image",
            Self::Image => "image",
            Self::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .unwrap_or(Self::Other)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    High = 0,
    Normal = 1,
    Low = 2,
}

impl Priority {
    pub fn from_i64(n: i64) -> Self {
        match n {
            0 => Self::High,
            2 => Self::Low,
            _ => Self::Normal,
        }
    }
}

/// A download as the UI sees it. The byte counts and speed are live while it runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    pub source: String,
    pub final_url: Option<String>,
    pub save_dir: String,
    /// The final name. Until the download starts, a guess from the URL (or the name the user
    /// chose, `options.file_name`).
    pub file_name: String,
    pub status: TaskStatus,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub speed_bps: u64,
    pub upload_speed_bps: u64,
    pub eta_secs: Option<u64>,
    /// Open connections while it runs.
    pub connections: u32,
    pub category: Category,
    pub priority: Priority,
    /// The server accepts ranges: pausing keeps what was downloaded.
    pub resumable: bool,
    pub options: TaskOptions,
    pub error: Option<Error>,
    pub created_at: i64,
    /// When the file name was settled and the partial file created.
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    /// Time spent downloading, pauses (and seeding) excluded, in milliseconds.
    pub active_ms: u64,
    /// The highest download speed seen (bytes per second).
    pub peak_bps: u64,
    /// Its place in the list (and in the queue, within a priority): the order tasks were
    /// added in, until the user moves them (`TaskManager::reorder`).
    #[serde(default)]
    pub sort_order: i64,
    #[serde(skip)]
    pub(crate) prev_status: Option<TaskStatus>,
    #[serde(skip)]
    pub(crate) etag: Option<String>,
    #[serde(skip)]
    pub(crate) last_modified: Option<String>,
}

impl Task {
    /// Path of the finished file.
    pub fn path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.save_dir).join(&self.file_name)
    }

    /// Path of the partial file while downloading.
    pub fn partial_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.save_dir).join(filename::partial(&self.file_name))
    }

    /// The database row. Options carrying cookies, headers or a password are sealed with
    /// `secrets`.
    pub(crate) fn to_record(&self, secrets: &dyn Secrets) -> omnidl_storage::TaskRecord {
        let json = serde_json::to_string(&self.options).unwrap_or_else(|_| "{}".into());
        let sensitive = self.options.cookies.is_some()
            || !self.options.headers.is_empty()
            || self.options.password.is_some();
        let options_json = if sensitive {
            secrets.seal(json.as_bytes()).unwrap_or(json)
        } else {
            json
        };
        omnidl_storage::TaskRecord {
            id: self.id.to_string(),
            kind: self.kind.as_str().into(),
            source: self.source.clone(),
            final_url: self.final_url.clone(),
            save_dir: self.save_dir.clone(),
            file_name: self.file_name.clone(),
            status: self.status.as_str().into(),
            prev_status: self.prev_status.map(|s| s.as_str().into()),
            total_bytes: self.total_bytes.map(|n| n as i64),
            downloaded: self.downloaded_bytes as i64,
            uploaded: self.uploaded_bytes as i64,
            category: self.category.as_str().into(),
            priority: self.priority as i64,
            sort_order: self.sort_order,
            options_json,
            etag: self.etag.clone(),
            last_modified: self.last_modified.clone(),
            resumable: self.resumable,
            error_json: self
                .error
                .as_ref()
                .and_then(|e| serde_json::to_string(e).ok()),
            created_at: self.created_at,
            started_at: self.started_at,
            completed_at: self.completed_at,
            active_ms: i64::try_from(self.active_ms).unwrap_or(i64::MAX),
            peak_bps: i64::try_from(self.peak_bps).unwrap_or(i64::MAX),
        }
    }

    pub(crate) fn from_record(
        r: omnidl_storage::TaskRecord,
        secrets: &dyn Secrets,
    ) -> Option<Self> {
        let options = if r.options_json.starts_with(SEALED_PREFIX) {
            // A lost key costs the cookies, not the task.
            secrets
                .open(&r.options_json)
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_else(|| {
                    tracing::warn!("task {}: stored request details cannot be decrypted", r.id);
                    TaskOptions::default()
                })
        } else {
            serde_json::from_str(&r.options_json).unwrap_or_default()
        };
        Some(Self {
            id: r.id.parse().ok()?,
            kind: TaskKind::parse(&r.kind)?,
            source: r.source,
            final_url: r.final_url,
            save_dir: r.save_dir,
            file_name: r.file_name,
            status: TaskStatus::parse(&r.status)?,
            total_bytes: r.total_bytes.map(|n| n as u64),
            downloaded_bytes: r.downloaded as u64,
            uploaded_bytes: r.uploaded as u64,
            speed_bps: 0,
            upload_speed_bps: 0,
            eta_secs: None,
            connections: 0,
            category: Category::parse(&r.category),
            priority: Priority::from_i64(r.priority),
            resumable: r.resumable,
            options,
            error: r.error_json.and_then(|j| serde_json::from_str(&j).ok()),
            created_at: r.created_at,
            started_at: r.started_at,
            completed_at: r.completed_at,
            active_ms: u64::try_from(r.active_ms).unwrap_or(0),
            peak_bps: u64::try_from(r.peak_bps).unwrap_or(0),
            prev_status: r.prev_status.and_then(|s| TaskStatus::parse(&s)),
            sort_order: r.sort_order,
            etag: r.etag,
            last_modified: r.last_modified,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_engine() {
        assert_eq!(TaskKind::detect("https://a/b"), Some(TaskKind::Http));
        assert_eq!(TaskKind::detect("  HTTP://a/b"), Some(TaskKind::Http));
        assert_eq!(TaskKind::detect("ftp://a/b"), Some(TaskKind::Ftp));
        assert_eq!(
            TaskKind::detect("magnet:?xt=urn:btih:abc"),
            Some(TaskKind::Torrent)
        );
        assert_eq!(
            TaskKind::detect("C:\\a\\b.TORRENT"),
            Some(TaskKind::Torrent)
        );
        assert_eq!(TaskKind::detect("file:///etc/passwd"), None);
        assert_eq!(TaskKind::detect(""), None);
    }

    #[test]
    fn follows_the_state_machine() {
        use TaskStatus::*;
        assert!(Queued.can_become(Probing));
        assert!(Probing.can_become(Downloading));
        assert!(Downloading.can_become(Paused));
        assert!(Paused.can_become(Queued));
        assert!(Failed.can_become(Queued));
        assert!(Downloading.can_become(Completed));
        assert!(Completed.can_become(Removed));
        assert!(Removed.can_become(Completed));
        assert!(!Completed.can_become(Paused));
        assert!(
            !Paused.can_become(Downloading),
            "resuming goes through the queue"
        );
        assert!(!Removed.can_become(Removed));
        assert!(!Queued.can_become(Queued));
        for s in TaskStatus::ALL {
            assert_eq!(TaskStatus::parse(s.as_str()), Some(s));
        }
    }

    #[test]
    fn round_trips_a_record() {
        let t = Task {
            id: Uuid::new_v4(),
            kind: TaskKind::Http,
            source: "https://example.com/a.iso".into(),
            final_url: Some("https://cdn.example.com/a.iso".into()),
            save_dir: "/downloads".into(),
            file_name: "a.iso".into(),
            status: TaskStatus::Failed,
            total_bytes: Some(5 << 30),
            downloaded_bytes: 1 << 30,
            uploaded_bytes: 0,
            speed_bps: 0,
            upload_speed_bps: 0,
            eta_secs: None,
            connections: 0,
            category: Category::DiskImage,
            priority: Priority::High,
            resumable: true,
            options: TaskOptions {
                connections: Some(16),
                ..Default::default()
            },
            error: Some(Error::new(crate::ErrorCode::Network, "reset")),
            created_at: 5,
            started_at: Some(6),
            completed_at: None,
            active_ms: 7,
            peak_bps: 8,
            prev_status: None,
            sort_order: 3,
            etag: Some("\"e\"".into()),
            last_modified: None,
        };
        assert_eq!(
            Task::from_record(t.to_record(&crate::NoSecrets), &crate::NoSecrets),
            Some(t.clone())
        );
        // Cookies are sealed when there is a key.
        let mut t = t;
        t.options.cookies = Some("session=abc".into());
        let key = crate::AesSecrets::new(&[1; 32]);
        let r = t.to_record(&key);
        assert!(r.options_json.starts_with(SEALED_PREFIX) && !r.options_json.contains("abc"));
        assert_eq!(Task::from_record(r.clone(), &key), Some(t.clone()));
        // Without the key the task survives without its request details.
        let lost = Task::from_record(r, &crate::NoSecrets).unwrap();
        assert_eq!(lost.options, TaskOptions::default());
    }
}
