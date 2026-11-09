//! What the commands share.

use std::{
    path::PathBuf,
    sync::{Mutex, atomic::AtomicBool},
};

use omnidl_core::TaskManager;

use crate::settings::Settings;

pub struct AppState {
    pub manager: TaskManager,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    /// The system Downloads folder.
    pub default_dir: String,
    pub log_dir: String,
    /// Downloads are being stopped before the app exits.
    pub quitting: AtomicBool,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
