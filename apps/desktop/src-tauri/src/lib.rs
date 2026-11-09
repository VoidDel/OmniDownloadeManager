//! The Omni Download desktop app (Tauri 2): the main window, the task manager behind its
//! commands, the stream of task events to the window, and the browser bridge (local pipe for the
//! Native Messaging host, HTTP fallback, quick-download window). The tray arrives with Phase 5
//! (docs/development-plan.md §10).

mod browser;
mod commands;
mod events;
mod secrets;
mod settings;
mod state;

use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock, atomic::Ordering},
};

use omnidl_core::TaskManager;
use omnidl_http::HttpEngine;
use omnidl_storage::Db;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tracing_appender::non_blocking::WorkerGuard;

use crate::{settings::Settings, state::AppState};

/// Keeps the log writer flushing until exit.
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn run() {
    tauri::Builder::default()
        // First: a second launch hands over to this one and exits before anything else runs.
        // It shows the window, unless it is the browser bridge starting the app
        // (`--background`) while it is already running.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if !argv.iter().any(|a| a == "--background") {
                focus_main(app);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle();
            let paths = handle.path();
            let data_dir = paths.app_data_dir()?;
            let config_dir = paths.app_config_dir()?;
            let log_dir = paths.app_log_dir()?;
            init_logging(&log_dir);
            tracing::info!("Omni Download {} starting", env!("CARGO_PKG_VERSION"));
            let default_dir = paths
                .download_dir()
                .or_else(|_| paths.home_dir())?
                .to_string_lossy()
                .into_owned();
            let settings_path = config_dir.join("settings.toml");
            let settings = Settings::load(&settings_path);

            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("omni.db");
            let engine_config = settings.engine.clone();
            // Cookies and headers from the browser are stored sealed with this key.
            let sealing: Arc<dyn omnidl_core::Secrets> = match secrets::load_or_create(&data_dir) {
                Some(key) => Arc::new(omnidl_core::AesSecrets::new(&key)),
                None => Arc::new(omnidl_core::NoSecrets),
            };
            let manager = tauri::async_runtime::block_on(async move {
                let db = Db::open(&db_path).await?;
                let engines: Vec<Arc<dyn omnidl_core::DownloadEngine>> =
                    vec![Arc::new(HttpEngine::new())];
                TaskManager::open_with_secrets(db, engines, engine_config, sealing)
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
            })
            .map_err(|e| {
                tracing::error!("opening the task database: {e}");
                e
            })?;
            events::spawn(handle.clone(), &manager);
            app.manage(AppState {
                manager,
                settings: Mutex::new(settings),
                settings_path,
                default_dir,
                log_dir: log_dir.to_string_lossy().into_owned(),
                quitting: Default::default(),
                background: std::env::args().any(|a| a == "--background").into(),
                nm_dir: paths.app_local_data_dir()?.join("nm"),
                bridge_ok: Default::default(),
                http: Default::default(),
                quick: Default::default(),
                quick_seq: Default::default(),
                seen: Default::default(),
            });
            main_window(handle)?;
            browser::start(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_bootstrap,
            commands::task_list,
            commands::settings_set,
            commands::speed_limit_set,
            commands::task_probe,
            commands::task_add,
            commands::task_pause,
            commands::task_resume,
            commands::task_restart,
            commands::task_remove,
            commands::task_restore,
            commands::task_purge,
            commands::task_detail,
            commands::task_set_speed_limit,
            commands::task_open,
            commands::task_reveal,
            commands::folder_open,
            commands::disk_free,
            browser::browser_status,
            browser::browser_repair,
            browser::http_bridge_set,
            browser::http_bridge_new_token,
            browser::quick_request,
            browser::quick_resolve,
        ])
        .on_window_event(|window, event| {
            // A quick-download window closed without an answer cancels its download.
            if let tauri::WindowEvent::Destroyed = event
                && window.label().starts_with("quick-")
                && let Some(state) = window.try_state::<AppState>()
            {
                browser::quick::closed(&state, window.label());
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Omni Download")
        .run(|app, event| {
            if let RunEvent::ExitRequested { api, .. } = event {
                quit(app, &api);
            }
        });
}

/// Exiting waits for downloads to save their progress: the first request is held back while
/// the manager stops them, then the app exits for real.
fn quit(app: &AppHandle, api: &tauri::ExitRequestApi) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    api.prevent_exit();
    let manager = state.manager.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        manager.shutdown().await;
        tracing::info!("downloads stopped; exiting");
        app.exit(0);
    });
}

/// Main window: hidden until the page has drawn (it shows itself), own title bar except on
/// macOS, where the traffic lights sit over the sidebar.
fn main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Omni Download")
        .inner_size(1200.0, 760.0)
        .min_inner_size(900.0, 600.0)
        .center()
        .visible(false);
    #[cfg(target_os = "macos")]
    let b = b
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(13.0, 22.5));
    #[cfg(not(target_os = "macos"))]
    let b = b.decorations(false);
    b.build()
}

pub(crate) fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Daily log files in the app's log folder (and stderr in debug builds). `RUST_LOG` overrides
/// the level.
fn init_logging(dir: &Path) {
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(if cfg!(debug_assertions) {
            "info,omnidl_core=debug,omnidl_http=debug,omnidl_desktop_lib=debug"
        } else {
            "info"
        })
    });
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("omnidl")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir);
    let file_layer = match file {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let _ = LOG_GUARD.set(guard);
            Some(fmt::layer().with_ansi(false).with_writer(writer))
        }
        Err(e) => {
            eprintln!("log file in {}: {e}", dir.display());
            None
        }
    };
    let stderr = cfg!(debug_assertions).then(|| fmt::layer().with_writer(std::io::stderr));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr)
        .try_init();
}
