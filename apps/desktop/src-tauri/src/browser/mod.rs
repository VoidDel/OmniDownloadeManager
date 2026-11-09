//! The browser bridge (docs/development-plan.md §5, ADR-002): the local pipe the Native
//! Messaging host relays to, host registration for the browsers, the optional HTTP channel, and
//! the quick-download window.

pub mod handler;
pub mod http;
pub mod integration;
pub mod quick;

use omnidl_core::{Error, Result};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::{settings::Settings, state::AppState};

/// Starts the pipe server, registers the host where needed, and the HTTP channel when enabled.
pub fn start(app: &AppHandle) {
    let state = app.state::<AppState>();
    // Bound inside the runtime: a Tokio pipe or socket registers with its reactor (setup runs on
    // the main thread, outside it).
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        let endpoint = omnidl_ipc::endpoint();
        let listener = match omnidl_ipc::Listener::bind(&endpoint) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("browser bridge {endpoint}: {e}");
                return;
            }
        };
        a.state::<AppState>()
            .bridge_ok
            .store(true, std::sync::atomic::Ordering::SeqCst);
        tracing::info!("browser bridge on {endpoint}");
        let handler_app = a.clone();
        let r = listener
            .run(move |frame| handler::handle_frame(handler_app.clone(), frame))
            .await;
        if let Err(e) = r {
            tracing::error!("browser bridge: {e}");
        }
    });
    // An app on another endpoint (`OMNIDL_IPC_ENDPOINT`: tests, a second build side by side) is
    // not where the registered host connects to: it leaves the registration alone.
    if std::env::var_os(omnidl_ipc::ENV_ENDPOINT).is_some() {
        tracing::info!("custom browser bridge endpoint: not registering the host");
    } else if let Some(host) = integration::host_path() {
        let dir = state.nm_dir.clone();
        std::thread::spawn(move || {
            integration::register(&host, &dir, false);
        });
    } else {
        tracing::warn!("omnidl-nm-host is not next to the app: browsers cannot reach it");
    }
    if state.settings().browser.http_bridge {
        let a = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = start_http(&a).await {
                tracing::warn!("HTTP bridge: {e}");
            }
        });
    }
}

/// Starts the HTTP channel (making a token and keeping the port on first use).
async fn start_http(app: &AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let mut b = state.settings().browser;
    if b.http_token.is_empty() {
        b.http_token = http::new_token();
    }
    let running = http::start(app.clone(), b.http_port, b.http_token.clone())
        .await
        .map_err(|e| Error::io(&e, "HTTP bridge"))?;
    let port = running.port;
    stop_http(&state);
    *state.http.lock().unwrap_or_else(|e| e.into_inner()) = Some(running);
    let token = b.http_token;
    state.update_settings(|s| {
        s.browser.http_port = port;
        s.browser.http_token = token.clone();
    });
    Ok(())
}

fn stop_http(state: &AppState) {
    if let Some(r) = state.http.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = r.stop.send(());
    }
}

/// The browser integration page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Integration {
    /// The host executable (`None`: not installed next to the app).
    host_path: Option<String>,
    /// The app's pipe / socket is serving.
    bridge_ok: bool,
    browsers: Vec<integration::BrowserState>,
    /// Extensions heard from since the app started: browser and Unix milliseconds.
    seen: Vec<(String, i64)>,
    http_running: bool,
    pairing_code: Option<String>,
    settings: Settings,
}

fn integration_of(state: &AppState) -> Integration {
    let host = integration::host_path();
    let settings = state.settings();
    let http_port = state
        .http
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|r| r.port);
    Integration {
        host_path: host.as_ref().map(|p| p.to_string_lossy().into_owned()),
        bridge_ok: state.bridge_ok.load(std::sync::atomic::Ordering::SeqCst),
        browsers: integration::status(host.as_deref(), &state.nm_dir),
        seen: state.seen_browsers(),
        http_running: http_port.is_some(),
        pairing_code: http_port.map(|p| http::pairing_code(p, &settings.browser.http_token)),
        settings,
    }
}

#[tauri::command]
pub async fn browser_status(state: State<'_, AppState>) -> Result<Integration> {
    Ok(integration_of(&state))
}

/// Rewrites this install's host registration for every browser.
#[tauri::command]
pub async fn browser_repair(state: State<'_, AppState>) -> Result<Integration> {
    let host = integration::host_path().ok_or_else(|| {
        Error::invalid("omnidl-nm-host was not found next to Omni Download; reinstall it")
    })?;
    let dir = state.nm_dir.clone();
    tauri::async_runtime::spawn_blocking(move || integration::register(&host, &dir, true))
        .await
        .map_err(Error::internal)?;
    Ok(integration_of(&state))
}

/// Turns the HTTP channel on or off.
#[tauri::command]
pub async fn http_bridge_set(app: AppHandle, enabled: bool) -> Result<Integration> {
    let state = app.state::<AppState>();
    state.update_settings(|s| s.browser.http_bridge = enabled);
    if enabled {
        start_http(&app).await?;
    } else {
        stop_http(&state);
    }
    Ok(integration_of(&state))
}

/// A new pairing code (the old one stops working).
#[tauri::command]
pub async fn http_bridge_new_token(app: AppHandle) -> Result<Integration> {
    let state = app.state::<AppState>();
    state.update_settings(|s| s.browser.http_token = http::new_token());
    if state.settings().browser.http_bridge {
        start_http(&app).await?;
    }
    Ok(integration_of(&state))
}

/// What the quick-download window asking is for.
#[tauri::command]
pub fn quick_request(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Option<quick::QuickRequest> {
    state
        .quick
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(window.label())
        .map(|p| p.request.clone())
}

/// The quick-download window's answer; it closes.
#[tauri::command]
pub fn quick_resolve(window: WebviewWindow, state: State<'_, AppState>, decision: quick::Decision) {
    quick::resolve(&state, window.label(), decision);
    let _ = window.close();
}
