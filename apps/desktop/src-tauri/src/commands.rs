//! Commands the window calls (docs/development-plan.md §6.4). Failures reach the page as
//! `omnidl_core::Error` (`{ code, message, status? }`), which it words from `code`.

use std::{path::Path, sync::atomic::Ordering};

use omnidl_core::{
    ConflictPolicy, Error, ErrorCode, ImportSummary, NewTask, ProbeInfo, Result, Task, TaskDetail,
    TaskExport, TaskId, TaskKind, TaskOptions, TaskStatus,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::{i18n, settings::Settings, state::AppState};

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
    /// The new-download dialog asked for while this window was being made.
    new_task: Option<crate::browser::Prefill>,
    /// The first main window of this run, not one coming back from the notification area.
    first: bool,
    /// The app starts with the system (the system's setting, not ours).
    autostart: bool,
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
        new_task: if main {
            state
                .prefill
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        } else {
            None
        },
        first: main && !state.booted.swap(true, Ordering::SeqCst),
        autostart: window
            .app_handle()
            .autolaunch()
            .is_enabled()
            .unwrap_or(false),
    }
}

/// Every task again, after the page missed events.
#[tauri::command]
pub fn task_list(state: State<'_, AppState>) -> Vec<Task> {
    state.manager.list()
}

#[tauri::command]
pub async fn settings_set(app: AppHandle, settings: Settings) -> Result<Settings> {
    save_settings(&app, settings).await
}

/// Saves the settings and applies them: engine, window material, global shortcuts, the
/// notification-area menu. Shortcuts another app holds are not saved (the error says which).
pub async fn save_settings(app: &AppHandle, settings: Settings) -> Result<Settings> {
    let state = app.state::<AppState>();
    let mut settings = settings.normalized();
    let before = state.settings();
    // The HTTP channel's switch, port and token change through its own commands only.
    let current = before.browser;
    settings.browser.http_bridge = current.http_bridge;
    settings.browser.http_port = current.http_port;
    settings.browser.http_token = current.http_token;
    let (g, b) = (&settings.general, &before.general);
    if (&g.shortcut_show, &g.shortcut_new) != (&b.shortcut_show, &b.shortcut_new)
        && let Err(keys) = crate::shortcuts::apply(app, g)
    {
        let _ = crate::shortcuts::apply(app, b);
        let l = i18n::resolve(&settings.language);
        return Err(Error::invalid(i18n::tr(
            l,
            "settings.general.shortcutTaken",
            &[("keys", &keys)],
        )));
    }
    settings
        .save(&state.settings_path)
        .map_err(|e| Error::io(&e, state.settings_path.display()))?;
    state.manager.set_config(settings.engine.clone());
    state
        .bt
        .set_upload_limit(settings.engine.upload_limit)
        .await;
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    crate::trackers::changed(app);
    let a = (&settings.appearance, &before.appearance);
    if a.0.material != a.1.material || a.0.material_kind != a.1.material_kind {
        crate::material::apply(app, &settings);
    }
    if settings.general.float_ball != before.general.float_ball {
        crate::float::sync(app);
    }
    // The ball is a page of its own: it follows the theme, material and language too.
    let _ = app.emit_to(crate::float::LABEL, "settings:changed", &settings);
    // The menu is in the UI language and checks the speed limit.
    if settings.language != before.language
        || settings.engine.speed_limit != before.engine.speed_limit
    {
        crate::tray::sync(app);
    }
    Ok(settings)
}

/// Starts the app with the system (in the background: `--background`), or stops doing so; the
/// state the system now has.
#[tauri::command]
pub fn autostart_set(app: AppHandle, enabled: bool) -> Result<bool> {
    let launcher = app.autolaunch();
    let r = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    r.map_err(Error::internal)?;
    Ok(launcher.is_enabled().unwrap_or(enabled))
}

/// The global download limit in bytes per second; `null` is unlimited. Saved with the settings.
#[tauri::command]
pub async fn speed_limit_set(app: AppHandle, limit: Option<u64>) -> Result<Settings> {
    let mut settings = app.state::<AppState>().settings();
    settings.engine.speed_limit = limit.filter(|&l| l > 0);
    save_settings(&app, settings).await
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
    let mut expanded = Vec::new();
    for item in items {
        if omnidl_ftp::is_folder(&item.source) {
            expanded.extend(ftp_folder(&state, item).await?);
        } else {
            expanded.push(item);
        }
    }
    if expanded.is_empty() {
        return Err(Error::invalid("the folder has no files"));
    }
    state.manager.add(expanded).await
}

/// An FTP folder: a task per file (those chosen, `only_files` counting in the listing's order, as
/// the dialog showed it), in a folder of the folder's name with its subfolders.
async fn ftp_folder(state: &AppState, item: NewTask) -> Result<Vec<NewTask>> {
    let o = omnidl_core::JobOptions::resolve(&item.options, &state.manager.config());
    let files = omnidl_ftp::list_folder(&item.source, &o).await?;
    let root = Path::new(&item.save_dir).join(omnidl_ftp::folder_name(&item.source));
    let chosen = item.options.only_files.clone();
    let options = TaskOptions {
        only_files: None,
        file_name: None,
        checksum: None,
        mirrors: Vec::new(),
        ..item.options
    };
    Ok(files
        .into_iter()
        .enumerate()
        .filter(|(i, _)| chosen.as_ref().is_none_or(|c| c.contains(i)))
        .map(|(_, f)| {
            let mut dir = root.clone();
            if let Some((parent, _)) = f.path.rsplit_once('/') {
                for part in parent.split('/') {
                    dir.push(omnidl_core::filename::sanitize(part));
                }
            }
            NewTask {
                source: f.url,
                save_dir: dir.to_string_lossy().into_owned(),
                options: options.clone(),
                start: item.start,
                probe: None,
            }
        })
        .collect())
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

/// To the recycle bin; with `delete_files` their files are deleted too (the tasks stay there).
#[tauri::command]
pub async fn task_remove(
    state: State<'_, AppState>,
    ids: Vec<TaskId>,
    delete_files: Option<bool>,
) -> Result<()> {
    state
        .manager
        .remove(&ids, delete_files.unwrap_or(false))
        .await
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

/// Moves tasks in the list (and the queue): `ids` go next to `anchor`, after it or before it.
#[tauri::command]
pub async fn task_reorder(
    state: State<'_, AppState>,
    ids: Vec<TaskId>,
    anchor: TaskId,
    after: bool,
) -> Result<()> {
    state.manager.reorder(&ids, anchor, after).await
}

/// The account for a download whose server asks for one (`auth_required`), then tries again.
#[tauri::command]
pub async fn task_set_credentials(
    state: State<'_, AppState>,
    id: TaskId,
    username: Option<String>,
    password: Option<String>,
) -> Result<()> {
    state
        .manager
        .set_credentials(id, username, password)
        .await?;
    state.manager.resume(&[id]).await
}

/// The answer to "a file of this name exists" (`file_exists`): `rename` or `overwrite`, then
/// the download goes on.
#[tauri::command]
pub async fn task_resolve_conflict(
    state: State<'_, AppState>,
    id: TaskId,
    policy: ConflictPolicy,
) -> Result<()> {
    state.manager.set_conflict(id, policy).await?;
    state.manager.resume(&[id]).await
}

/// Writes the task list (or the tasks in `ids`) to a JSON file; how many tasks it holds.
/// Cookies, extra headers and passwords only with `with_secrets`.
#[tauri::command]
pub async fn tasks_export(
    state: State<'_, AppState>,
    path: String,
    ids: Option<Vec<TaskId>>,
    with_secrets: bool,
) -> Result<usize> {
    let file = state.manager.export(ids.as_deref(), with_secrets).await;
    let json = serde_json::to_vec_pretty(&file).map_err(Error::internal)?;
    std::fs::write(&path, json).map_err(|e| Error::io(&e, &path))?;
    Ok(file.tasks.len())
}

/// Adds the tasks of an exported list (see `TaskManager::import`).
#[tauri::command]
pub async fn tasks_import(state: State<'_, AppState>, path: String) -> Result<ImportSummary> {
    let l = i18n::resolve(&state.settings().language);
    let bytes = std::fs::read(&path).map_err(|e| Error::io(&e, &path))?;
    let file: TaskExport = serde_json::from_slice(&bytes)
        .ok()
        .filter(|f: &TaskExport| f.app == TaskExport::APP)
        .ok_or_else(|| Error::invalid(i18n::tr(l, "settings.general.tasksNotOurs", &[])))?;
    if file.version > TaskExport::VERSION {
        return Err(Error::invalid(i18n::tr(
            l,
            "settings.general.tasksNewer",
            &[],
        )));
    }
    state.manager.import(file).await
}

/// Common user agents to choose from in the settings (name, value).
#[tauri::command]
pub fn user_agent_presets() -> Vec<(&'static str, String)> {
    omnidl_core::user_agent_presets()
}

/// The task's connections: a number, or `None` to tune them; a running download changes at
/// once.
#[tauri::command]
pub async fn task_set_connections(
    state: State<'_, AppState>,
    id: TaskId,
    connections: Option<u32>,
) -> Result<()> {
    state.manager.set_task_connections(id, connections).await
}

/// Sets the checksum a download should meet, as the user typed it (`sha256:…`, bare hex…); empty
/// clears it. A finished download is checked at once.
#[tauri::command]
pub async fn task_set_checksum(
    state: State<'_, AppState>,
    id: TaskId,
    value: String,
) -> Result<Task> {
    let sum = if value.trim().is_empty() {
        None
    } else {
        let l = i18n::resolve(&state.settings().language);
        Some(
            omnidl_core::Checksum::parse(&value)
                .ok_or_else(|| Error::invalid(i18n::tr(l, "checksum.invalid", &[])))?,
        )
    };
    state.manager.set_checksum(id, sum).await
}

/// MD5, SHA-1 and SHA-256 of a finished download, as `[algorithm, hex]` pairs.
#[tauri::command]
pub async fn task_digests(state: State<'_, AppState>, id: TaskId) -> Result<Vec<(String, String)>> {
    use omnidl_core::HashAlgo;
    let got = state
        .manager
        .digests(id, vec![HashAlgo::Md5, HashAlgo::Sha1, HashAlgo::Sha256])
        .await?;
    Ok(got
        .into_iter()
        .map(|(a, h)| (a.name().to_owned(), h))
        .collect())
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
    reveal(&app, &find(&state, id)?)
}

/// Shows a download in the file manager (also clicking its notification).
pub fn reveal(app: &AppHandle, task: &Task) -> Result<()> {
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

/// Where to get the browser extension: its store page once it is listed there, until then how to
/// install it by hand.
#[tauri::command]
pub fn extension_open(app: AppHandle, browser: String) -> Result<()> {
    const GUIDE: &str = "https://github.com/VoidDel/OmniDownloadeManager#浏览器扩展";
    /// (browser id, store page), filled in as the stores list the extension.
    const STORES: [(&str, &str); 0] = [];
    let url = STORES
        .iter()
        .find(|(b, _)| *b == browser)
        .map_or(GUIDE, |(_, url)| url);
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(Error::internal)
}

/// Plays the completion sound as it would sound (`file`: a WAV file, empty: the built-in one).
#[tauri::command]
pub fn sound_test(app: AppHandle, file: String) {
    crate::sound::play(&app, &file);
}

/// Opens the user guide, the page for reporting a problem, the source code or OmniZip's page (the
/// same family's archiver; Settings › About), in the browser.
#[tauri::command]
pub fn help_open(app: AppHandle, page: String) -> Result<()> {
    let url = match page.as_str() {
        "guide" => "https://github.com/VoidDel/OmniDownloadeManager/blob/main/docs/user-guide.md",
        "feedback" => "https://github.com/VoidDel/OmniDownloadeManager/issues/new/choose",
        "source" => "https://github.com/VoidDel/OmniDownloadeManager",
        "omnizip" => "https://github.com/VoidDel/OmniZip",
        _ => return Err(Error::invalid(format!("no such page: {page}"))),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(Error::internal)
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
