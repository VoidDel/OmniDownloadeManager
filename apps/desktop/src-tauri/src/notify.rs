//! Notifications when a download finishes or fails (docs/development-plan.md §6.5), as the
//! settings say, and not while the main window is in front (its list already shows it). On
//! Windows clicking one shows the file in its folder.

use std::collections::HashMap;

use omnidl_core::{Error, Task, TaskId, TaskStatus};
use tauri::{AppHandle, Manager};

use crate::{i18n, state::AppState};

/// The last status seen of each task: a notification is for a change, not for a state.
#[derive(Default)]
pub struct Watch {
    last: HashMap<TaskId, TaskStatus>,
}

impl Watch {
    pub fn new(tasks: &[Task]) -> Self {
        Self {
            last: tasks.iter().map(|t| (t.id, t.status)).collect(),
        }
    }

    pub fn seen(&mut self, task: &Task) -> Option<TaskStatus> {
        self.last.insert(task.id, task.status)
    }

    pub fn forget(&mut self, ids: &[TaskId]) {
        for id in ids {
            self.last.remove(id);
        }
    }
}

/// What a change of status is worth telling.
#[derive(Debug, PartialEq, Eq)]
enum News {
    Done,
    Failed,
}

fn news(before: Option<TaskStatus>, now: TaskStatus) -> Option<News> {
    let was_running = matches!(
        before,
        Some(TaskStatus::Queued | TaskStatus::Probing | TaskStatus::Downloading)
    );
    match now {
        // A torrent has finished when it begins to seed.
        TaskStatus::Completed | TaskStatus::Seeding if was_running => Some(News::Done),
        TaskStatus::Failed if was_running => Some(News::Failed),
        _ => None,
    }
}

/// A task changed (its status was `before`): a notification if it just finished or failed.
pub fn changed(app: &AppHandle, before: Option<TaskStatus>, task: &Task) {
    let Some(news) = news(before, task.status) else {
        return;
    };
    // The sound plays whether or not the window is in front (it says the download is done, which
    // the list does not unless one looks).
    // Finished, but not with the checksum the user gave: told as a failure, without the chime.
    let mismatch = news == News::Done
        && task
            .error
            .as_ref()
            .is_some_and(|e| e.code == omnidl_core::ErrorCode::ChecksumMismatch);
    if news == News::Done && !mismatch {
        crate::sound::done(app);
    }
    let general = app.state::<AppState>().settings().general;
    let wanted = match news {
        News::Done if !mismatch => general.notify_done,
        _ => general.notify_failed,
    };
    let in_front = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    if !wanted || in_front {
        return;
    }
    let l = i18n::current(app);
    let (title, body) = match news {
        News::Done if mismatch => (
            i18n::tr(l, "notify.checksum", &[]),
            format!(
                "{}\n{}",
                task.file_name,
                i18n::tr(l, "errors.checksum_mismatch", &[])
            ),
        ),
        News::Done => (i18n::tr(l, "notify.done", &[]), task.file_name.clone()),
        News::Failed => (
            i18n::tr(l, "notify.failed", &[]),
            match &task.error {
                Some(e) => format!("{}\n{}", task.file_name, reason(l, e)),
                None => task.file_name.clone(),
            },
        ),
    };
    show(app, &title, &body, task.id);
}

/// Why it failed, in the UI language (the window's `errorText`).
fn reason(lang: &str, e: &Error) -> String {
    let code = serde_json::to_value(e.code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "internal".into());
    match (code.as_str(), e.status) {
        ("invalid_request", _) => e.message.clone(),
        ("http_status" | "ftp_reply", Some(s)) => i18n::tr(
            lang,
            &format!("errors.{code}"),
            &[("status", &s.to_string())],
        ),
        ("http_status" | "ftp_reply", None) => {
            i18n::tr(lang, &format!("errors.{code}NoStatus"), &[])
        }
        _ => i18n::tr(lang, &format!("errors.{code}"), &[]),
    }
}

#[cfg(windows)]
fn show(app: &AppHandle, title: &str, body: &str, task: TaskId) {
    use tauri_winrt_notification::Toast;

    // Toasts belong to an app the system knows: the installer gives the Start menu shortcut this
    // identifier. A build that is not installed (development) borrows PowerShell's.
    let app_id = if cfg!(debug_assertions) {
        Toast::POWERSHELL_APP_ID.to_owned()
    } else {
        app.config().identifier.clone()
    };
    let (title, body, app) = (title.to_owned(), body.to_owned(), app.clone());
    std::thread::spawn(move || {
        let shown = Toast::new(&app_id)
            .title(&title)
            .text1(&body)
            .on_activated(move |_| {
                if let Some(t) = app.state::<AppState>().manager.get(task) {
                    let _ = crate::commands::reveal(&app, &t);
                }
                Ok(())
            })
            .show();
        if let Err(e) = shown {
            tracing::warn!("notification: {e}");
        }
    });
}

#[cfg(not(windows))]
fn show(app: &AppHandle, title: &str, body: &str, _task: TaskId) {
    use tauri_plugin_notification::NotificationExt;

    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("notification: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_of_changes_not_states() {
        use TaskStatus::*;
        assert_eq!(news(Some(Downloading), Completed), Some(News::Done));
        assert_eq!(news(Some(Downloading), Seeding), Some(News::Done));
        assert_eq!(news(Some(Probing), Failed), Some(News::Failed));
        // Already finished at start, a torrent that stops seeding, a task restored or paused.
        assert_eq!(news(None, Completed), None);
        assert_eq!(news(Some(Seeding), Completed), None);
        assert_eq!(news(Some(Removed), Completed), None);
        assert_eq!(news(Some(Downloading), Paused), None);
    }
}
