//! The quick-download window (docs/development-plan.md §6.3): a small always-on-top window that
//! asks what to do with a download taken over from the browser. The request waits for the
//! answer, which goes back to the extension (start, later, let the browser do it, or drop it).

use std::sync::atomic::Ordering;

use omnidl_core::{ProbeInfo, TaskOptions};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

use crate::state::AppState;

/// What the window shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickRequest {
    pub url: String,
    /// The browser's name for the file.
    pub file_name: Option<String>,
    pub size: Option<u64>,
    pub mime: Option<String>,
    /// The page's site ("this site" in "don't ask again").
    pub site: String,
    /// `chrome`, `edge`, `firefox`…
    pub browser: Option<String>,
    pub has_cookies: bool,
    pub has_referer: bool,
    pub save_dir: String,
    pub connections: u32,
    /// The request details, for probing the server as the browser would.
    pub options: TaskOptions,
    /// Where the link came from: the browser, the clipboard or the floating ball (the window
    /// words its title by it, and offers "let the browser download it" for the browser only).
    pub origin: Origin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Browser,
    Clipboard,
    Float,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickAction {
    Start,
    Later,
    Browser,
    Cancel,
}

/// The user's answer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub action: QuickAction,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub save_dir: Option<String>,
    #[serde(default)]
    pub connections: Option<u32>,
    #[serde(default)]
    pub speed_limit: Option<u64>,
    /// Don't ask again for this site.
    #[serde(default)]
    pub remember: bool,
    /// What the window learned from the server (name and size show at once in the list).
    #[serde(default)]
    pub probe: Option<ProbeInfo>,
}

impl Decision {
    fn just(action: QuickAction) -> Self {
        Self {
            action,
            file_name: None,
            save_dir: None,
            connections: None,
            speed_limit: None,
            remember: false,
            probe: None,
        }
    }
}

impl Decision {
    /// Puts what the user chose (start or later) into `task`. `connections` is the global
    /// default: the same number is not stored on the task.
    pub fn apply(self, task: &mut omnidl_core::NewTask, connections: u32) {
        task.start = self.action == QuickAction::Start;
        if let Some(n) = self.file_name.filter(|n| !n.trim().is_empty()) {
            task.options.file_name = Some(omnidl_core::filename::sanitize(&n));
        }
        if let Some(dir) = self.save_dir.filter(|d| !d.trim().is_empty()) {
            task.save_dir = dir;
        }
        task.options.connections = self.connections.filter(|&c| c != connections);
        task.options.speed_limit = self.speed_limit.filter(|&l| l > 0);
        task.probe = self.probe;
    }
}

pub struct Pending {
    pub request: QuickRequest,
    reply: Option<oneshot::Sender<Decision>>,
}

/// Opens a window for `request` and waits for the answer. Closing the window cancels; a window
/// that cannot be opened starts the download (better than losing it).
pub async fn ask(app: &AppHandle, request: QuickRequest) -> Decision {
    let state = app.state::<AppState>();
    let label = format!("quick-{}", state.quick_seq.fetch_add(1, Ordering::SeqCst));
    let (tx, rx) = oneshot::channel();
    state
        .quick
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            label.clone(),
            Pending {
                request,
                reply: Some(tx),
            },
        );
    if let Err(e) = open_window(app, &label) {
        tracing::error!("quick-download window: {e}");
        state
            .quick
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&label);
        return Decision::just(QuickAction::Start);
    }
    rx.await
        .unwrap_or_else(|_| Decision::just(QuickAction::Cancel))
}

/// Delivers the answer for the window `label` (once).
pub fn resolve(state: &AppState, label: &str, decision: Decision) {
    let pending = state
        .quick
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(label);
    if let Some(mut p) = pending
        && let Some(tx) = p.reply.take()
    {
        let _ = tx.send(decision);
    }
}

/// The window closed without an answer.
pub fn closed(state: &AppState, label: &str) {
    resolve(state, label, Decision::just(QuickAction::Cancel));
}

/// Hidden until the page has drawn (it shows and focuses itself), its own title bar. The page
/// sets its height to what it shows ("Advanced" open or not, the language); this is the usual one.
fn open_window(app: &AppHandle, label: &str) -> tauri::Result<()> {
    let settings = app.state::<AppState>().settings();
    let b = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Omni Download")
        .inner_size(560.0, 244.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .decorations(false)
        .center()
        .visible(false);
    let w = crate::material::prepare(crate::with_devtools(b), &settings).build()?;
    crate::window_icons(&w.as_ref().window());
    Ok(())
}
