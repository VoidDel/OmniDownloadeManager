//! What the commands share.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32},
    },
};

use omnidl_bt::BtEngine;
use omnidl_core::TaskManager;

use crate::{
    browser::{http, quick},
    settings::Settings,
};

pub struct AppState {
    pub manager: TaskManager,
    /// Also in the manager's engines: the app sets its upload limit and stops it on exit.
    pub bt: Arc<BtEngine>,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    /// The system Downloads folder.
    pub default_dir: String,
    pub log_dir: String,
    /// Downloads are being stopped before the app exits.
    pub quitting: AtomicBool,
    /// Started by the browser bridge (`--background`): the main window stays hidden. Read once
    /// by the window.
    pub background: AtomicBool,
    /// Where the Native Messaging manifests go on Windows.
    pub nm_dir: PathBuf,
    /// The pipe / socket for the host is serving.
    pub bridge_ok: AtomicBool,
    /// The HTTP channel, when running.
    pub http: Mutex<Option<http::Running>>,
    /// Quick-download windows waiting for an answer, by window label.
    pub quick: Mutex<HashMap<String, quick::Pending>>,
    pub quick_seq: AtomicU32,
    /// Browsers whose extension has been in touch in this run: name → Unix milliseconds.
    pub seen: Mutex<BTreeMap<String, i64>>,
    /// The same across runs (`browsers-seen.json`): when each browser's extension was first
    /// heard from in the latest run it was.
    pub seen_before: Mutex<BTreeMap<String, i64>>,
    pub seen_path: PathBuf,
    /// The main window's page has loaded (asked for its first data).
    pub page_ready: AtomicBool,
    /// Magnet links and `.torrent` files that arrived before it had.
    pub links: Mutex<Vec<String>>,
    /// The new-download dialog to open once it has (asked for while the window was being made).
    pub prefill: Mutex<Option<crate::browser::Prefill>>,
    /// Running without a main window on purpose: started in the background, or closed to the
    /// notification area. The window (and its WebView's memory) comes back when it is shown.
    pub in_tray: AtomicBool,
    /// The main window has loaded once in this run; later ones are it coming back.
    pub booted: AtomicBool,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Changes the settings and saves them (a failed save is logged; the change still applies).
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> Settings {
        let s = {
            let mut s = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            change(&mut s);
            s.clone()
        };
        if let Err(e) = s.save(&self.settings_path) {
            tracing::error!("{}: {e}", self.settings_path.display());
        }
        s
    }

    /// The folder new downloads go to.
    pub fn download_dir(&self) -> String {
        let s = self.settings();
        if s.download_dir.trim().is_empty() {
            self.default_dir.clone()
        } else {
            s.download_dir
        }
    }

    /// Notes contact from `browser`'s extension; `true` the first time in this run (and then
    /// it is remembered for later runs too).
    pub fn saw_browser(&self, browser: &str) -> bool {
        let name = browser.trim().to_ascii_lowercase();
        if name.is_empty() || name.len() > 32 || !name.bytes().all(|b| b.is_ascii_lowercase()) {
            return false;
        }
        let now = omnidl_core::now_ms();
        let first = self
            .seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.clone(), now)
            .is_none();
        if first {
            let all = {
                let mut before = self.seen_before.lock().unwrap_or_else(|e| e.into_inner());
                before.insert(name, now);
                before.clone()
            };
            if let Err(e) = serde_json::to_vec(&all)
                .map_err(std::io::Error::other)
                .and_then(|bytes| std::fs::write(&self.seen_path, bytes))
            {
                tracing::warn!("{}: {e}", self.seen_path.display());
            }
        }
        first
    }

    /// Contacts remembered from earlier runs and this one.
    pub fn seen_before(&self) -> Vec<(String, i64)> {
        self.seen_before
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// Reads what [`saw_browser`](Self::saw_browser) remembered (nothing when the file is
    /// missing or unreadable).
    pub fn load_seen(path: &std::path::Path) -> BTreeMap<String, i64> {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn seen_browsers(&self) -> Vec<(String, i64)> {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }
}
