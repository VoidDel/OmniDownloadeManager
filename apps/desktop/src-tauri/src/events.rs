//! Relays the task manager's events to the window (docs/development-plan.md §6.4). Progress is
//! already batched by the manager (4 per second, every running task in one event).

use omnidl_core::{ProgressItem, Stats, TaskEvent, TaskManager};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressBatch {
    items: Vec<ProgressItem>,
    stats: Stats,
}

pub fn spawn(app: AppHandle, manager: &TaskManager) {
    let mut rx = manager.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let sent = match rx.recv().await {
                Ok(TaskEvent::Added { task }) => app.emit("task:added", task),
                Ok(TaskEvent::Changed { task }) => app.emit("task:changed", task),
                Ok(TaskEvent::Deleted { ids }) => app.emit("task:deleted", ids),
                Ok(TaskEvent::Progress { items, stats }) => {
                    app.emit("task:progress", ProgressBatch { items, stats })
                }
                // The page missed events: it reloads the list.
                Err(RecvError::Lagged(n)) => {
                    tracing::debug!("event bridge lagged by {n}");
                    app.emit("task:resync", ())
                }
                Err(RecvError::Closed) => return,
            };
            if let Err(e) = sent {
                tracing::debug!("event to the window: {e}");
            }
        }
    });
}
