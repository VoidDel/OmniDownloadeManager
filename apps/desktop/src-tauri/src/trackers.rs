//! Extra trackers (BT settings): the user's own list and lists subscribed to (web addresses or
//! files), added to every torrent that is not private. The fetched lists are kept beside the
//! BitTorrent state, each under a `# <address>` line, and fetched again when asked ("Update"),
//! when an address is added, and once a day while the app runs. A list that cannot be fetched
//! keeps what it had; the others are updated. Tasks that start after a change use the result.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use omnidl_core::{EngineConfig, Error, ErrorCode, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// How old the fetched lists may get before they are fetched again.
const MAX_AGE: Duration = Duration::from_secs(24 * 3600);

/// Where the fetched lists go.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("torrents").join("trackers.txt")
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| path(&d))
}

/// The subscribed addresses of the settings, trimmed, without empty ones or repeats.
fn sources(e: &EngineConfig) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in e.bt_trackers_urls.iter().map(|s| s.trim()) {
        if !s.is_empty() && !out.iter().any(|o| o == s) {
            out.push(s.to_owned());
        }
    }
    out
}

/// The lists kept, by the address they came from.
fn kept(path: &Path) -> Vec<(String, Vec<String>)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        match line.strip_prefix("# ") {
            Some(source) => out.push((source.to_owned(), String::new())),
            None => {
                if let Some((_, body)) = out.last_mut() {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
    }
    out.into_iter()
        .map(|(s, body)| (s, omnidl_bt::trackers::parse(&body)))
        .collect()
}

fn keep(path: &Path, lists: &[(String, Vec<String>)]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = String::new();
    for (source, list) in lists {
        text.push_str(&format!("# {source}\n"));
        for t in list {
            text.push_str(t);
            text.push('\n');
        }
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The kept lists of the addresses of the settings, in their order (those never fetched are
/// left out).
fn fetched(path: &Path, e: &EngineConfig) -> Vec<(String, Vec<String>)> {
    let mut all = kept(path);
    sources(e)
        .into_iter()
        .filter_map(|s| {
            let i = all.iter().position(|(k, _)| *k == s)?;
            Some(all.swap_remove(i))
        })
        .collect()
}

/// The trackers the settings add to torrents (none when switched off).
pub fn in_use(e: &EngineConfig, data_dir: &Path) -> Vec<String> {
    if !e.bt_extra_trackers {
        return Vec::new();
    }
    let lists = fetched(&path(data_dir), e);
    omnidl_bt::trackers::merge(
        std::iter::once(e.bt_trackers.as_slice()).chain(lists.iter().map(|(_, l)| l.as_slice())),
    )
}

/// Hands the trackers of the settings to the engine.
pub fn apply(app: &AppHandle) {
    let (Some(state), Ok(dir)) = (app.try_state::<AppState>(), app.path().app_data_dir()) else {
        return;
    };
    let list = in_use(&state.settings().engine, &dir);
    state.bt.set_extra_trackers(&list);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackersStatus {
    /// Trackers added to torrents, the user's and the fetched ones together.
    pub count: usize,
    /// Trackers in the subscribed lists (without repeats).
    pub fetched: usize,
    /// Subscribed addresses, and how many of them have a list.
    pub sources: usize,
    pub ready: usize,
    /// When the lists were last fetched (Unix milliseconds).
    pub updated_at: Option<i64>,
}

#[tauri::command]
pub fn trackers_status(app: AppHandle) -> TrackersStatus {
    let state = app.state::<AppState>();
    let mut e = state.settings().engine;
    e.bt_extra_trackers = true;
    let (Some(p), Ok(dir)) = (file(&app), app.path().app_data_dir()) else {
        return TrackersStatus {
            count: 0,
            fetched: 0,
            sources: 0,
            ready: 0,
            updated_at: None,
        };
    };
    let lists = fetched(&p, &e);
    TrackersStatus {
        count: in_use(&e, &dir).len(),
        fetched: omnidl_bt::trackers::merge(lists.iter().map(|(_, l)| l.as_slice())).len(),
        sources: sources(&e).len(),
        ready: lists.len(),
        updated_at: (!lists.is_empty())
            .then(|| modified(&p))
            .flatten()
            .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64),
    }
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Fetches the subscribed lists again. One that fails keeps what it had and is named in the
/// error, after the others were updated.
#[tauri::command]
pub async fn trackers_update(app: AppHandle) -> Result<TrackersStatus> {
    let e = app.state::<AppState>().settings().engine;
    let wanted = sources(&e);
    if wanted.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidRequest,
            "no tracker list address",
        ));
    }
    let out = file(&app).ok_or_else(|| Error::internal("no data folder"))?;
    let mut old = fetched(&out, &e);
    let mut lists = Vec::new();
    let mut failed = Vec::new();
    for source in wanted {
        let got = match crate::ipfilter::fetch(&source).await {
            Ok(bytes) => {
                let list = omnidl_bt::trackers::parse(&String::from_utf8_lossy(&bytes));
                if list.is_empty() {
                    Err(format!("{source}: no tracker addresses"))
                } else {
                    Ok(list)
                }
            }
            Err(e) => Err(e.message),
        };
        match got {
            Ok(list) => {
                tracing::info!("trackers: {} from {source}", list.len());
                lists.push((source, list));
            }
            Err(msg) => {
                tracing::warn!("trackers: {msg}");
                failed.push(msg);
                if let Some(i) = old.iter().position(|(k, _)| *k == source) {
                    lists.push(old.swap_remove(i));
                }
            }
        }
    }
    keep(&out, &lists).map_err(|e| Error::io(&e, out.display()))?;
    apply(&app);
    if !failed.is_empty() {
        return Err(Error::new(ErrorCode::Network, failed.join("; ")));
    }
    Ok(trackers_status(app))
}

/// Whether the lists of the settings should be fetched (again): on, with addresses, and one
/// of them never fetched or the lists older than a day.
fn due(app: &AppHandle) -> bool {
    let (Some(state), Some(p)) = (app.try_state::<AppState>(), file(app)) else {
        return false;
    };
    let e = state.settings().engine;
    let wanted = sources(&e).len();
    if !e.bt_extra_trackers || wanted == 0 {
        return false;
    }
    fetched(&p, &e).len() < wanted
        || modified(&p)
            .and_then(|t| t.elapsed().ok())
            .is_none_or(|age| age >= MAX_AGE)
}

fn refresh(app: &AppHandle) {
    if !due(app) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = trackers_update(app).await {
            tracing::warn!("trackers: {e}");
        }
    });
}

/// After the settings were saved: the engine gets the new list, and lists of new addresses
/// are fetched.
pub fn changed(app: &AppHandle) {
    apply(app);
    refresh(app);
}

/// On starting: the lists are fetched when due, and checked every hour from then on.
pub fn start(app: AppHandle) {
    refresh(&app);
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        tick.tick().await;
        loop {
            tick.tick().await;
            refresh(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_lists_of_each_address() {
        let dir = tempfile::tempdir().unwrap();
        let p = path(dir.path());
        let parse = omnidl_bt::trackers::parse;
        let a = parse("udp://t.example:1/announce\nhttp://u.example/announce");
        let b = parse("udp://t.example:1/announce\nudp://v.example:3/announce");
        keep(
            &p,
            &[
                ("https://a.example/list.txt".into(), a.clone()),
                ("/lists/b.txt".into(), b.clone()),
            ],
        )
        .unwrap();
        assert_eq!(kept(&p).len(), 2);

        let mut e = EngineConfig {
            bt_trackers: vec!["udp://own.example:2/announce".into()],
            bt_trackers_urls: vec![
                " /lists/b.txt ".into(),
                "https://a.example/list.txt".into(),
                "".into(),
            ],
            ..Default::default()
        };
        assert!(in_use(&e, dir.path()).is_empty(), "off");
        e.bt_extra_trackers = true;
        assert_eq!(
            fetched(&p, &e)
                .iter()
                .map(|(s, _)| s.as_str())
                .collect::<Vec<_>>(),
            ["/lists/b.txt", "https://a.example/list.txt"]
        );
        assert_eq!(
            in_use(&e, dir.path()),
            [
                "udp://own.example:2/announce",
                "udp://t.example:1/announce",
                "udp://v.example:3/announce",
                "http://u.example/announce",
            ]
        );
        // An address that is no longer subscribed to: its list is not used.
        e.bt_trackers_urls = vec![
            "https://a.example/list.txt".into(),
            "https://c.example/new.txt".into(),
        ];
        assert_eq!(fetched(&p, &e).len(), 1);
        assert_eq!(
            in_use(&e, dir.path()),
            [
                "udp://own.example:2/announce",
                "udp://t.example:1/announce",
                "http://u.example/announce"
            ]
        );
    }
}
