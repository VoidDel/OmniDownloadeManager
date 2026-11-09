//! Updates (docs/development-plan.md §7.4): `latest.json` of the newest GitHub release, whose
//! packages are signed with the project's minisign key (the public half is in
//! `tauri.conf.json`; the private half signs releases in CI and never enters the repository).
//!
//! The window asks (`update_check`), shows what is new, and installs when told
//! (`update_install`): the package downloads first (with progress), then downloads in progress
//! stop and save their place, then the new version installs and starts.

use std::sync::{Mutex, atomic::Ordering};

use omnidl_core::{Error, ErrorCode, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::state::AppState;

/// The update found by the last check, until it is installed.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    /// Release notes (Markdown), possibly empty.
    notes: String,
    /// RFC 3339.
    date: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

fn failed(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::Network, e.to_string())
}

/// A newer version, if there is one.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>> {
    let found = app
        .updater()
        .map_err(failed)?
        .check()
        .await
        .map_err(failed)?;
    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone().unwrap_or_default(),
        date: u.date.and_then(|d| {
            d.format(&time::format_description::well_known::Rfc3339)
                .ok()
        }),
    });
    *app.state::<Pending>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = found;
    Ok(info)
}

/// Downloads and installs the update found by the last check, then starts the new version.
/// Progress arrives as `update:progress` events.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<()> {
    let update = app
        .state::<Pending>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| Error::invalid("no update to install"))?;
    let mut downloaded = 0u64;
    let mut last = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                // About a hundred events for the whole package.
                let step = total.map_or(1 << 20, |t| (t / 100).max(1 << 16));
                if downloaded - last >= step || total == Some(downloaded) {
                    last = downloaded;
                    let _ = app.emit("update:progress", UpdateProgress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(failed)?;
    // Downloads save their place before the new version takes over: on Windows the installer
    // ends this process as soon as it starts.
    let state = app.state::<AppState>();
    state.quitting.store(true, Ordering::SeqCst);
    state.manager.shutdown().await;
    state.bt.stop().await;
    if let Err(e) = update.install(bytes) {
        // Not installed (a bad signature, say): this version starts again, downloads resume.
        tracing::error!("installing the update: {e}");
    }
    app.restart();
}
