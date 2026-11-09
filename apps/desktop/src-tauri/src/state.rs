//! What the commands share.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32},
    },
};

use omnidl_core::TaskManager;

use crate::{
    browser::{http, quick},
    settings::Settings,
};

pub struct AppState {
    pub manager: TaskManager,
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
    /// Browsers whose extension has been in touch: name → Unix milliseconds.
    pub seen: Mutex<BTreeMap<String, i64>>,
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

    /// Notes contact from `browser`'s extension; `true` the first time.
    pub fn saw_browser(&self, browser: &str) -> bool {
        let name = browser.trim().to_ascii_lowercase();
        if name.is_empty() || name.len() > 32 {
            return false;
        }
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name, omnidl_core::now_ms())
            .is_none()
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
