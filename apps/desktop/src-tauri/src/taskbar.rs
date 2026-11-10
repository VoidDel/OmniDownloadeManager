//! The overall progress of the running downloads on the main window's taskbar button (Windows),
//! Dock icon (macOS) or launcher entry (Linux, where the desktop supports it): the same figure as
//! the sidebar's summary. Nothing while nothing downloads.

use std::collections::{HashMap, HashSet};

use omnidl_core::{ProgressItem, TaskId, TaskStatus};
use tauri::{
    AppHandle, Manager,
    window::{ProgressBarState, ProgressBarStatus},
};

/// What the button shows: `None` nothing, `Some(None)` busy without a known size, `Some(Some(p))`
/// p percent.
type Shown = Option<Option<u64>>;

#[derive(Default)]
pub struct Taskbar {
    /// Tasks downloading now (from their changes), and their latest figures.
    running: HashSet<TaskId>,
    figures: HashMap<TaskId, (u64, Option<u64>)>,
    shown: Shown,
}

impl Taskbar {
    pub fn new(tasks: &[omnidl_core::Task]) -> Self {
        let mut t = Self::default();
        for task in tasks {
            t.status(task.id, Some(task.status));
        }
        t
    }

    /// A task's status changed (or it was deleted: `None`).
    pub fn status(&mut self, id: TaskId, status: Option<TaskStatus>) {
        if matches!(status, Some(TaskStatus::Downloading | TaskStatus::Probing)) {
            self.running.insert(id);
        } else {
            self.running.remove(&id);
            self.figures.remove(&id);
        }
    }

    /// A progress tick.
    pub fn progress(&mut self, items: &[ProgressItem]) {
        for i in items {
            if self.running.contains(&i.id) {
                self.figures.insert(i.id, (i.downloaded, i.total));
            }
        }
    }

    /// What shows: nothing, busy without a figure, or a percent (the floating ball shows it
    /// too).
    pub fn wanted(&self) -> Shown {
        if self.running.is_empty() {
            return None;
        }
        let (mut done, mut total) = (0u64, 0u64);
        for id in &self.running {
            match self.figures.get(id) {
                Some((d, Some(t))) => {
                    done += d;
                    total += t;
                }
                // A size not known (yet): busy, without a figure.
                _ => return Some(None),
            }
        }
        Some((total > 0).then(|| (done.min(total) * 100 / total).min(100)))
    }

    /// Shows the figure on the main window's button, when it changed (or the window is new).
    pub fn apply(&mut self, app: &AppHandle) {
        let Some(window) = app.get_webview_window("main") else {
            // Closed to the notification area: a new window starts blank.
            self.shown = None;
            return;
        };
        let wanted = self.wanted();
        if wanted == self.shown {
            return;
        }
        let state = match wanted {
            None => ProgressBarState {
                status: Some(ProgressBarStatus::None),
                progress: None,
            },
            Some(None) => ProgressBarState {
                status: Some(ProgressBarStatus::Indeterminate),
                progress: None,
            },
            Some(Some(p)) => ProgressBarState {
                status: Some(ProgressBarStatus::Normal),
                progress: Some(p),
            },
        };
        match window.set_progress_bar(state) {
            Ok(()) => self.shown = wanted,
            Err(e) => tracing::debug!("taskbar progress: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: TaskId, downloaded: u64, total: Option<u64>) -> ProgressItem {
        ProgressItem {
            id,
            downloaded,
            total,
            speed: 0,
            eta: None,
            connections: 0,
            uploaded: 0,
            upload_speed: 0,
        }
    }

    #[test]
    fn sums_the_running_downloads() {
        let (a, b, c) = (TaskId::new_v4(), TaskId::new_v4(), TaskId::new_v4());
        let mut t = Taskbar::default();
        assert_eq!(t.wanted(), None);
        t.status(a, Some(TaskStatus::Downloading));
        t.status(b, Some(TaskStatus::Downloading));
        t.status(c, Some(TaskStatus::Seeding));
        // A seeding torrent's figures do not count.
        t.progress(&[
            item(a, 25, Some(100)),
            item(b, 75, Some(100)),
            item(c, 10, Some(10)),
        ]);
        assert_eq!(t.wanted(), Some(Some(50)));
        // One of unknown size: busy, without a figure.
        t.progress(&[item(b, 80, None)]);
        assert_eq!(t.wanted(), Some(None));
        t.status(b, Some(TaskStatus::Completed));
        assert_eq!(t.wanted(), Some(Some(25)));
        t.status(a, None);
        assert_eq!(t.wanted(), None);
    }
}
