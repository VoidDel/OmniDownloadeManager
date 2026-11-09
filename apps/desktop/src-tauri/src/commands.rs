//! Commands the window calls (docs/development-plan.md §6.4). Failures reach the page as
//! `omnidl_core::Error` (`{ code, message, status? }`), which it words from `code`.

use std::{path::Path, sync::atomic::Ordering};

use omnidl_core::{
    Error, ErrorCode, NewTask, ProbeInfo, Result, Task, TaskDetail, TaskId, TaskKind, TaskOptions,
    TaskStatus,
};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{settings::Settings, state::AppState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    settings: Settings,
    tasks: Vec<Task>,
    version: &'static str,
    /// The system Downloads folder (used while `settings.downloadDir` is empty).
    default_dir: String,
    log_dir: String,
    /// Started by the browser bridge: the main window stays hidden (asked once).
    background: bool,
    /// Browsers whose extension has been in touch: name and Unix milliseconds.
    seen_browsers: Vec<(String, i64)>,
    /// Window materials exist on this system (Windows 11, macOS).
    material_supported: bool,
    /// The system's accent colour (`#RRGGBB`), for the "follow the system" accent.
    system_accent: Option<String>,
    /// Magnet links and `.torrent` files the app was started with: the new-download dialog
    /// opens with them.
    open_links: Vec<String>,
}

#[tauri::command]
pub fn app_bootstrap(window: tauri::WebviewWindow, state: State<'_, AppState>) -> Bootstrap {
    let main = window.label() == "main";
    let open_links = if main {
        state.page_ready.store(true, Ordering::SeqCst);
        std::mem::take(&mut *state.links.lock().unwrap_or_else(|e| e.into_inner()))
    } else {
        Vec::new()
    };
    Bootstrap {
        settings: state.settings(),
        tasks: if main {
            state.manager.list()
        } else {
            Vec::new()
        },
        version: env!("CARGO_PKG_VERSION"),
        default_dir: state.default_dir.clone(),
        log_dir: state.log_dir.clone(),
        background: main && state.background.swap(false, Ordering::SeqCst),
        seen_browsers: state.seen_browsers(),
        material_supported: crate::material::supported(),
        system_accent: crate::material::system_accent(),
        open_links,
    }
}

/// Every task again, after the page missed events.
#[tauri::command]
pub fn task_list(state: State<'_, AppState>) -> Vec<Task> {
    state.manager.list()
}

#[tauri::command]
pub async fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings> {
    let mut settings = settings.normalized();
    let before = state.settings();
    // The HTTP channel's switch, port and token change through its own commands only.
    let current = before.browser;
    settings.browser.http_bridge = current.http_bridge;
    settings.browser.http_port = current.http_port;
    settings.browser.http_token = current.http_token;
    settings
        .save(&state.settings_path)
        .map_err(|e| Error::io(&e, state.settings_path.display()))?;
    state.manager.set_config(settings.engine.clone());
    state
        .bt
        .set_upload_limit(settings.engine.upload_limit)
        .await;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    let a = (&settings.appearance, &before.appearance);
    if a.0.material != a.1.material || a.0.material_kind != a.1.material_kind {
        crate::material::apply(&app, &settings);
    }
    Ok(settings)
}

/// The global download limit in bytes per second; `null` is unlimited. Saved with the settings.
#[tauri::command]
pub async fn speed_limit_set(
    app: AppHandle,
    state: State<'_, AppState>,
    limit: Option<u64>,
) -> Result<Settings> {
    let mut settings = state.settings();
    settings.engine.speed_limit = limit.filter(|&l| l > 0);
    settings_set(app, state, settings).await
}

#[tauri::command]
pub async fn task_probe(
    state: State<'_, AppState>,
    url: String,
    options: TaskOptions,
) -> Result<ProbeInfo> {
    state.manager.probe(&url, &options).await
}

#[tauri::command]
pub async fn task_add(state: State<'_, AppState>, items: Vec<NewTask>) -> Result<Vec<Task>> {
    state.manager.add(items).await
}

#[tauri::command]
pub async fn task_pause(state: State<'_, AppState>, ids: Vec<TaskId>) -> Result<()> {
    state.manager.pause(&ids).await
}

#[tauri::command]
pub async fn task_resume(state: State<'_, AppState>, ids: Vec<TaskId>) -> Result<()> {
    state.manager.resume(&ids).await
}

#[tauri::command]
pub async fn task_restart(state: State<'_, AppState>, ids: Vec<TaskId>) -> Result<()> {
    state.manager.restart(&ids).await
}

#[tauri::command]
pub async fn task_remove(state: State<'_, AppState>, ids: Vec<TaskId>) -> Result<()> {
    state.manager.remove(&ids).await
}

#[tauri::command]
pub async fn task_restore(state: State<'_, AppState>, ids: Vec<TaskId>) -> Result<()> {
    state.manager.restore(&ids).await
}

#[tauri::command]
pub async fn task_purge(
    state: State<'_, AppState>,
    ids: Vec<TaskId>,
    delete_files: bool,
) -> Result<()> {
    state.manager.purge(&ids, delete_files).await
}

#[tauri::command]
pub async fn task_detail(state: State<'_, AppState>, id: TaskId) -> Result<TaskDetail> {
    state.manager.detail(id).await
}

/// Which files of a torrent task are downloaded (indices in the torrent's order).
#[tauri::command]
pub async fn task_files_set(
    state: State<'_, AppState>,
    id: TaskId,
    files: Vec<usize>,
) -> Result<()> {
    state.manager.set_torrent_files(id, files).await
}

#[tauri::command]
pub async fn task_set_speed_limit(
    state: State<'_, AppState>,
    id: TaskId,
    limit: Option<u64>,
) -> Result<()> {
    state.manager.set_task_speed_limit(id, limit).await
}

/// Opens a finished download with its default app.
#[tauri::command]
pub fn task_open(app: AppHandle, state: State<'_, AppState>, id: TaskId) -> Result<()> {
    let task = find(&state, id)?;
    if !matches!(task.status, TaskStatus::Completed | TaskStatus::Seeding) {
        return Err(Error::invalid("the download has not finished"));
    }
    let path = task.path();
    if !path.exists() {
        return Err(Error::new(
            ErrorCode::FileMissing,
            format!("{} no longer exists", path.display()),
        ));
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::internal(format!("{}: {e}", path.display())))
}

/// Finished downloads whose file (or torrent folder) is no longer where it was saved.
#[tauri::command]
pub fn task_missing(state: State<'_, AppState>) -> Vec<TaskId> {
    state
        .manager
        .list()
        .into_iter()
        .filter(|t| t.status == TaskStatus::Completed && !t.path().exists())
        .map(|t| t.id)
        .collect()
}

/// Shows a download in the file manager: the file once it exists, else its folder.
#[tauri::command]
pub fn task_reveal(app: AppHandle, state: State<'_, AppState>, id: TaskId) -> Result<()> {
    let task = find(&state, id)?;
    // A torrent writes its files in place, without a partial file.
    let done = matches!(task.status, TaskStatus::Completed | TaskStatus::Seeding);
    let file = if done || task.kind == TaskKind::Torrent {
        task.path()
    } else {
        task.partial_path()
    };
    let opener = app.opener();
    let r = if file.exists() {
        opener.reveal_item_in_dir(&file)
    } else {
        opener.open_path(task.save_dir.clone(), None::<&str>)
    };
    r.map_err(|e| Error::internal(format!("{}: {e}", file.display())))
}

/// Opens a folder (the download folder from the settings page).
#[tauri::command]
pub fn folder_open(app: AppHandle, path: String) -> Result<()> {
    if !Path::new(&path).is_dir() {
        return Err(Error::invalid(format!("not a folder: {path}")));
    }
    app.opener()
        .open_path(path.clone(), None::<&str>)
        .map_err(|e| Error::internal(format!("{path}: {e}")))
}

fn find(state: &AppState, id: TaskId) -> Result<Task> {
    state
        .manager
        .get(id)
        .ok_or_else(|| Error::invalid(format!("no task {id}")))
}

/// Whether this app opens magnet links. Windows and Linux can register at run time; on macOS
/// the installed app is registered by its bundle.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MagnetHandler {
    can_register: bool,
    registered: bool,
}

#[tauri::command]
pub fn magnet_status(app: AppHandle) -> MagnetHandler {
    use tauri_plugin_deep_link::DeepLinkExt;
    MagnetHandler {
        can_register: cfg!(any(windows, target_os = "linux")),
        registered: app.deep_link().is_registered("magnet").unwrap_or(false),
    }
}

/// Makes this app the one that opens magnet links.
#[tauri::command]
pub fn magnet_register(app: AppHandle) -> Result<MagnetHandler> {
    use tauri_plugin_deep_link::DeepLinkExt;
    app.deep_link()
        .register("magnet")
        .map_err(|e| Error::internal(format!("registering for magnet links: {e}")))?;
    Ok(magnet_status(app))
}
