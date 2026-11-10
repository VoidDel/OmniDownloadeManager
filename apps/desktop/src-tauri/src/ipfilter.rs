//! IP filtering (BT settings): the block list BitTorrent reads when it starts, made from the lists
//! the user chose (web addresses or files, in any of the usual formats) and the addresses
//! blocked by hand (`omnidl_bt::ipfilter`). A list that cannot be fetched fails the update and
//! the list in use stays: leaving it out would quietly block less. Made again when the user asks ("Update"), and on
//! starting when it is on and there is none yet. Changes apply from the next start of the app.

use std::{
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use omnidl_core::{Error, ErrorCode, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Where the list goes: beside the BitTorrent state.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("torrents").join("ipfilter.p2p")
}

fn current(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| path(&d))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpFilterStatus {
    /// Address ranges in the list in use.
    pub rules: usize,
    /// When it was made (Unix milliseconds).
    pub updated_at: Option<i64>,
}

#[tauri::command]
pub fn ip_filter_status(app: AppHandle) -> IpFilterStatus {
    let Some(p) = current(&app) else {
        return IpFilterStatus {
            rules: 0,
            updated_at: None,
        };
    };
    IpFilterStatus {
        rules: omnidl_bt::ipfilter::count(&p),
        updated_at: std::fs::metadata(&p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64),
    }
}

/// Makes the list again from the settings.
#[tauri::command]
pub async fn ip_filter_update(app: AppHandle) -> Result<IpFilterStatus> {
    let state = app.state::<AppState>();
    let e = state.settings().engine;
    let out = current(&app).ok_or_else(|| Error::internal("no data folder"))?;
    let mut lists = Vec::new();
    for source in e
        .bt_ip_filter_lists
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        lists.push(fetch(source).await?);
    }
    let n = tokio::task::spawn_blocking(move || {
        omnidl_bt::ipfilter::write(&lists, &e.bt_banned, &out)
            .map_err(|err| Error::io(&err, out.display()))
    })
    .await
    .map_err(Error::internal)??;
    tracing::info!("IP filter: {n} ranges");
    Ok(ip_filter_status(app))
}

/// The bytes of a list: downloaded from a web address, else read from a file (also the tracker
/// list's, `trackers::update`).
pub(crate) async fn fetch(source: &str) -> Result<Vec<u8>> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(Error::internal)?;
        let resp = client
            .get(source)
            .send()
            .await
            .map_err(|e| Error::new(ErrorCode::Network, format!("{source}: {e}")))?;
        if !resp.status().is_success() {
            let mut err = Error::new(
                ErrorCode::HttpStatus,
                format!("{source}: {}", resp.status()),
            );
            err.status = Some(resp.status().as_u16());
            return Err(err);
        }
        return resp
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| Error::new(ErrorCode::Network, format!("{source}: {e}")));
    }
    let p = Path::new(source);
    std::fs::read(p).map_err(|e| Error::io(&e, p.display()))
}

/// On starting: a list that is on but was never made is made now (for the next start of
/// BitTorrent; this one has already read what there was).
pub fn prepare(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let e = state.settings().engine;
    if !e.bt_ip_filter || current(app).is_some_and(|p| p.is_file()) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = ip_filter_update(app).await {
            tracing::warn!("IP filter: {e}");
        }
    });
}
