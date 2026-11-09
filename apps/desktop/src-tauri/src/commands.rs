//! Commands the window calls (docs/development-plan.md §6.4). Failures reach the page as
//! `omnidl_core::Error` (`{ code, message, status? }`), which it words from `code`.

use std::path::Path;

use omnidl_core::{
    Error, NewTask, ProbeInfo, Result, Task, TaskDetail, TaskId, TaskOptions, TaskStatus,
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
}

#[tauri::command]
pub fn app_bootstrap(state: State<'_, AppState>) -> Bootstrap {
    Bootstrap {
        settings: state.settings(),
        tasks: state.manager.list(),
        version: env!("CARGO_PKG_VERSION"),
        default_dir: state.default_dir.clone(),
        log_dir: state.log_dir.clone(),
    }
}

/// Every task again, after the page missed events.
#[tauri::command]
pub fn task_list(state: State<'_, AppState>) -> Vec<Task> {
    state.manager.list()
}

#[tauri::command]
pub fn settings_set(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    let settings = settings.normalized();
    settings
        .save(&state.settings_path)
        .map_err(|e| Error::io(&e, state.settings_path.display()))?;
    state.manager.set_config(settings.engine.clone());
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    Ok(settings)
}

/// The global download limit in bytes per second; `null` is unlimited. Saved with the settings.
#[tauri::command]
pub fn speed_limit_set(state: State<'_, AppState>, limit: Option<u64>) -> Result<Settings> {
    let mut settings = state.settings();
    settings.engine.speed_limit = limit.filter(|&l| l > 0);
    settings_set(state, settings)
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
    if task.status != TaskStatus::Completed {
        return Err(Error::invalid("the download has not finished"));
    }
    let path = task.path();
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| Error::internal(format!("{}: {e}", path.display())))
}

/// Shows a download in the file manager: the file once it exists, else its folder.
#[tauri::command]
pub fn task_reveal(app: AppHandle, state: State<'_, AppState>, id: TaskId) -> Result<()> {
    let task = find(&state, id)?;
    let file = if task.status == TaskStatus::Completed {
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

/// Bytes free on the volume of `path` (its nearest existing ancestor); `None` if unknown.
#[tauri::command]
pub fn disk_free(path: String) -> Option<u64> {
    let mut p = Path::new(&path);
    loop {
        if p.exists() {
            return omnidl_storage::file::available_space(p).ok();
        }
        p = p.parent()?;
    }
}

fn find(state: &AppState, id: TaskId) -> Result<Task> {
    state
        .manager
        .get(id)
        .ok_or_else(|| Error::invalid(format!("no task {id}")))
}
