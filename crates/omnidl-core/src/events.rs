//! Events the manager publishes (docs/development-plan.md §4.5, §6.4). The app forwards them to
//! the window; progress is already batched (one event for every running task, 4 per second).

use serde::Serialize;
use tokio::sync::broadcast;

use crate::{Task, TaskId};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TaskEvent {
    Added {
        task: Task,
    },
    /// Status, name, size or error changed.
    Changed {
        task: Task,
    },
    /// Deleted for good (not the recycle bin, which is a status).
    Deleted {
        ids: Vec<TaskId>,
    },
    /// The user moved tasks: every task, in the new list order.
    Reordered {
        ids: Vec<TaskId>,
    },
    /// Live numbers of the running tasks.
    Progress {
        items: Vec<ProgressItem>,
        stats: Stats,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressItem {
    pub id: TaskId,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Bytes per second over the last few seconds.
    pub speed: u64,
    pub eta: Option<u64>,
    /// Peers, for a torrent.
    pub connections: u32,
    pub uploaded: u64,
    pub upload_speed: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub download_speed: u64,
    pub upload_speed: u64,
    /// Downloading (seeding torrents are counted apart).
    pub running: u32,
    pub seeding: u32,
}

/// A broadcast channel. Slow subscribers lose old events (`RecvError::Lagged`) and should reload
/// the task list.
#[derive(Debug, Clone)]
pub struct EventBus {
    tx: broadcast::Sender<TaskEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        Self {
            tx: broadcast::channel(capacity).0,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TaskEvent> {
        self.tx.subscribe()
    }

    pub fn emit(&self, event: TaskEvent) {
        // No subscriber is fine.
        let _ = self.tx.send(event);
    }
}
