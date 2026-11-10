//! Relays the task manager's events to the window (docs/development-plan.md §6.4). Progress is
//! already batched by the manager (4 per second, every running task in one event). Changes also
//! drive notifications, progress the notification-area icon, the taskbar button and the floating
//! ball.

use omnidl_core::{ProgressItem, Stats, TaskEvent, TaskManager};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

use crate::{notify, taskbar::Taskbar};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressBatch {
    items: Vec<ProgressItem>,
    stats: Stats,
}

/// The floating ball's figures: the same as the taskbar button's, and the speed.
fn float(app: &AppHandle, taskbar: &Taskbar, speed: Option<u64>) {
    let shown = taskbar.wanted();
    crate::float::update(app, shown.is_some(), shown.flatten(), speed);
}

pub fn spawn(app: AppHandle, manager: &TaskManager) {
    let mut rx = manager.subscribe();
    let tasks = manager.list();
    let mut watch = notify::Watch::new(&tasks);
    let mut taskbar = Taskbar::new(&tasks);
    tauri::async_runtime::spawn(async move {
        loop {
            let sent = match rx.recv().await {
                Ok(TaskEvent::Added { task }) => {
                    watch.seen(&task);
                    taskbar.status(task.id, Some(task.status));
                    taskbar.apply(&app);
                    float(&app, &taskbar, None);
                    app.emit("task:added", task)
                }
                Ok(TaskEvent::Changed { task }) => {
                    let before = watch.seen(&task);
                    notify::changed(&app, before, &task);
                    crate::intake::torrent_done(&app, before, &task);
                    taskbar.status(task.id, Some(task.status));
                    taskbar.apply(&app);
                    float(&app, &taskbar, None);
                    app.emit("task:changed", task)
                }
                Ok(TaskEvent::Deleted { ids }) => {
                    watch.forget(&ids);
                    for id in &ids {
                        taskbar.status(*id, None);
                    }
                    taskbar.apply(&app);
                    float(&app, &taskbar, None);
                    app.emit("task:deleted", ids)
                }
                Ok(TaskEvent::Progress { items, stats }) => {
                    crate::tray::activity(&app, &stats);
                    taskbar.progress(&items);
                    taskbar.apply(&app);
                    float(&app, &taskbar, Some(stats.download_speed));
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
