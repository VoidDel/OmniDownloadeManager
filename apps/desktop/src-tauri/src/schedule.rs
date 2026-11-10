//! Scheduled downloads (Download settings): the paused downloads start at one time and the
//! running ones pause at another, on the days chosen; and once everything is downloaded the app
//! quits, or the computer sleeps or shuts down after a minute's countdown that can be cancelled
//! (the main window shows it).

use std::{
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use chrono::{Datelike, Local};
use omnidl_core::{Task, TaskStatus};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Schedule {
    /// Start the paused downloads at `start_at` (`HH:MM`, local time).
    pub start: bool,
    pub start_at: String,
    /// Pause the running and waiting downloads at `stop_at`.
    pub stop: bool,
    pub stop_at: String,
    /// The days both apply on, 0 = Sunday … 6 = Saturday.
    pub days: Vec<u8>,
    /// Once the last download finishes.
    pub when_done: WhenDone,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            start: false,
            start_at: "02:00".into(),
            stop: false,
            stop_at: "08:00".into(),
            days: (0..7).collect(),
            when_done: WhenDone::Nothing,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhenDone {
    #[default]
    Nothing,
    Quit,
    Sleep,
    Shutdown,
}

impl Schedule {
    /// Times as `HH:MM` (a bad one back to its default), days in range and in order; no day at
    /// all means every day (the times would never apply otherwise).
    pub fn normalized(mut self) -> Self {
        let d = Self::default();
        if parse_time(&self.start_at).is_none() {
            self.start_at = d.start_at;
        }
        if parse_time(&self.stop_at).is_none() {
            self.stop_at = d.stop_at;
        }
        self.start_at = tidy(&self.start_at);
        self.stop_at = tidy(&self.stop_at);
        self.days.retain(|&n| n < 7);
        self.days.sort_unstable();
        self.days.dedup();
        if self.days.is_empty() {
            self.days = d.days;
        }
        self
    }
}

/// Minutes after midnight of `HH:MM` (or `H:MM`).
fn parse_time(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn tidy(s: &str) -> String {
    parse_time(s).map_or_else(|| s.to_owned(), |t| format!("{:02}:{:02}", t / 60, t % 60))
}

/// Looks at the clock every few seconds and acts once in the minute set.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let now = Local::now();
            let minute = now.format("%Y-%m-%d %H:%M").to_string();
            if minute == last {
                continue;
            }
            last = minute;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let s = state.settings().schedule;
            let day = now.weekday().num_days_from_sunday() as u8;
            if !s.days.contains(&day) {
                continue;
            }
            let hm = now.format("%H:%M").to_string();
            if s.start && s.start_at == hm {
                tracing::info!("schedule: starting the paused downloads");
                crate::tray::run_all(&app, true);
            }
            if s.stop && s.stop_at == hm {
                tracing::info!("schedule: pausing the downloads");
                crate::tray::run_all(&app, false);
            }
        }
    });
}

/// A countdown under way: what, and when it acts.
static PENDING: Mutex<Option<(WhenDone, Instant)>> = Mutex::new(None);
/// Bumped by each countdown and each cancel: a countdown acts only if it is still the latest.
static ROUND: AtomicU64 = AtomicU64::new(0);
pub const COUNTDOWN: Duration = Duration::from_secs(60);

/// A task changed. When a download just finished and nothing downloads or waits any more, the
/// action of `when_done`.
pub fn changed(app: &AppHandle, before: Option<TaskStatus>, task: &Task) {
    let finished = matches!(task.status, TaskStatus::Completed | TaskStatus::Seeding)
        && matches!(before, Some(TaskStatus::Probing | TaskStatus::Downloading));
    if !finished {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let action = state.settings().schedule.when_done;
    if action == WhenDone::Nothing {
        return;
    }
    let busy = state.manager.list().iter().any(|t| {
        matches!(
            t.status,
            TaskStatus::Queued | TaskStatus::Probing | TaskStatus::Downloading
        )
    });
    if busy {
        return;
    }
    if action == WhenDone::Quit {
        tracing::info!("schedule: everything downloaded, quitting");
        app.exit(0);
        return;
    }
    tracing::info!("schedule: everything downloaded, {action:?} in a minute");
    let round = ROUND.fetch_add(1, Ordering::SeqCst) + 1;
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((action, Instant::now() + COUNTDOWN));
    crate::focus_main(app);
    let _ = app.emit_to("main", "power:countdown", pending());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(COUNTDOWN).await;
        if ROUND.load(Ordering::SeqCst) == round {
            act(&app).await;
        }
    });
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub action: WhenDone,
    pub seconds: u64,
}

fn pending() -> Option<Pending> {
    let p = *PENDING.lock().unwrap_or_else(|e| e.into_inner());
    p.map(|(action, at)| Pending {
        action,
        seconds: at.saturating_duration_since(Instant::now()).as_secs(),
    })
}

/// The countdown under way, for a window that opened during it.
#[tauri::command]
pub fn power_pending() -> Option<Pending> {
    pending()
}

#[tauri::command]
pub fn power_cancel() {
    ROUND.fetch_add(1, Ordering::SeqCst);
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = None;
    tracing::info!("schedule: countdown cancelled");
}

/// Now rather than at the end of the countdown.
#[tauri::command]
pub async fn power_now(app: AppHandle) {
    ROUND.fetch_add(1, Ordering::SeqCst);
    act(&app).await;
}

async fn act(app: &AppHandle) {
    let Some((action, _)) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    if action == WhenDone::Shutdown
        && let Some(state) = app.try_state::<AppState>()
    {
        // Progress saved before the system goes down.
        state.manager.shutdown().await;
    }
    if let Err(e) = power(action) {
        tracing::error!("{action:?}: {e}");
    }
}

#[cfg(windows)]
fn power(action: WhenDone) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    match action {
        WhenDone::Sleep => {
            // SAFETY: plain call; false, false, false: sleep (not hibernate), wake events kept.
            let ok =
                unsafe { windows_sys::Win32::System::Power::SetSuspendState(false, false, false) };
            if !ok {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
        WhenDone::Shutdown => std::process::Command::new("shutdown")
            .args(["/s", "/t", "0"])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map(|_| ()),
        _ => Ok(()),
    }
}

#[cfg(target_os = "macos")]
fn power(action: WhenDone) -> std::io::Result<()> {
    let (program, args): (&str, &[&str]) = match action {
        WhenDone::Sleep => ("pmset", &["sleepnow"]),
        WhenDone::Shutdown => (
            "osascript",
            &["-e", "tell application \"System Events\" to shut down"],
        ),
        _ => return Ok(()),
    };
    std::process::Command::new(program)
        .args(args)
        .status()
        .map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn power(action: WhenDone) -> std::io::Result<()> {
    let verb = match action {
        WhenDone::Sleep => "suspend",
        WhenDone::Shutdown => "poweroff",
        _ => return Ok(()),
    };
    std::process::Command::new("systemctl")
        .arg(verb)
        .status()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_times_and_days_sensible() {
        let s = Schedule {
            start_at: "7:5".into(),
            stop_at: "25:00".into(),
            days: vec![6, 1, 1, 9],
            ..Default::default()
        }
        .normalized();
        assert_eq!(s.start_at, "07:05");
        assert_eq!(s.stop_at, "08:00");
        assert_eq!(s.days, vec![1, 6]);
        let none = Schedule {
            days: vec![],
            ..Default::default()
        }
        .normalized();
        assert_eq!(none.days.len(), 7);
    }

    #[test]
    fn reads_partial_settings() {
        let s: Schedule = toml::from_str("whenDone = \"shutdown\"\nstart = true").unwrap();
        assert_eq!(s.when_done, WhenDone::Shutdown);
        assert!(s.start);
        assert_eq!(s.start_at, "02:00");
    }
}
