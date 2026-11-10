//! The floating ball (docs/development-plan.md §6.3, "悬浮窗"; General settings, off by
//! default): a small round window above the others with the overall progress and speed. Links
//! dragged onto it are offered for download (`intake`), a `.torrent` file opens the
//! new-download dialog; a double click opens the main window, the right button a menu. It stays
//! where it was put (window-state) and does not take the focus from the app in use.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

use crate::{browser::quick::Origin, i18n, intake, state::AppState};

pub const LABEL: &str = "float";
/// The window (the ball is drawn inside it, with room for its ring and glow).
const SIZE: f64 = 72.0;

/// What the ball shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatState {
    /// Something downloads.
    pub busy: bool,
    /// Overall percent of the running downloads, when all their sizes are known.
    pub percent: Option<u64>,
    /// Download speed, bytes per second.
    pub speed: u64,
}

static STATE: Mutex<FloatState> = Mutex::new(FloatState {
    busy: false,
    percent: None,
    speed: 0,
});

/// Shows or closes the ball as the settings say.
pub fn sync(app: &AppHandle) {
    let on = app
        .try_state::<AppState>()
        .is_some_and(|s| s.settings().general.float_ball);
    match (on, app.get_webview_window(LABEL)) {
        (true, None) => open(app),
        (false, Some(w)) => {
            let _ = w.close();
        }
        _ => {}
    }
}

/// Made on a thread of its own: building a window on the main thread outside the event handler
/// can hang the app (see `focus_main`).
fn open(app: &AppHandle) {
    static OPENING: AtomicBool = AtomicBool::new(false);
    if OPENING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if app.get_webview_window(LABEL).is_none()
            && let Err(e) = build(&app)
        {
            tracing::error!("floating ball: {e}");
        }
        OPENING.store(false, Ordering::SeqCst);
    });
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let mut b = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Omni Download")
        .inner_size(SIZE, SIZE)
        // Windows otherwise keeps a window at least ~136 px wide: a clear band either side of
        // the ball that would still catch the clicks meant for what is under it.
        .min_inner_size(SIZE, SIZE)
        .max_inner_size(SIZE, SIZE)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focusable(false)
        .focused(false)
        // Links dragged from a browser arrive as the page's drop events.
        .disable_drag_drop_handler()
        .visible(false);
    // First time: the lower right of the main screen (afterwards where it was left).
    if let Ok(Some(m)) = app.primary_monitor() {
        let scale = m.scale_factor();
        let area = m.work_area();
        let x = (area.position.x as f64 + area.size.width as f64) / scale - SIZE - 48.0;
        let y = (area.position.y as f64 + area.size.height as f64) / scale - SIZE - 160.0;
        b = b.position(x.max(0.0), y.max(0.0));
    }
    let w = b.build()?;
    // window-state brings back the position, and a size saved before the size was fixed.
    w.set_size(tauri::LogicalSize::new(SIZE, SIZE))?;
    clip(&w.as_ref().window());
    w.on_menu_event(|w, event| on_menu(w.app_handle(), event));
    Ok(())
}

/// Windows: the window cut to a circle, so that its clear corners let clicks through to what is
/// under them. Again when the window moves to a screen of another scale.
///
/// No window material on the ball: Windows 11 does not draw Mica or Acrylic in a window cut to a
/// shape (and Mica shows a flat colour in a window that is never active, as the ball is); the
/// old blur-behind draws a grey disc. The page draws it in the theme's colours instead.
pub fn clip(window: &tauri::Window) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Graphics::Gdi::{CreateEllipticRgn, SetWindowRgn},
            UI::WindowsAndMessaging::{
                GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_CAPTION,
                WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
            },
        };
        let (Ok(h), Ok(scale)) = (window.hwnd(), window.scale_factor()) else {
            return;
        };
        let px = (SIZE * scale).round() as i32;
        // SAFETY: a live window handle; the region belongs to the system once it is set.
        unsafe {
            // A borderless window keeps its caption style, hidden by the system's frame drawing
            // until a region is set: then the title bar shows through the clear page. A plain
            // popup has none.
            let hwnd = h.0;
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let plain = (style
                & !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX))
                | WS_POPUP;
            if plain != style {
                SetWindowLongPtrW(hwnd, GWL_STYLE, plain as isize);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let rgn = CreateEllipticRgn(0, 0, px + 1, px + 1);
            if !rgn.is_null() {
                SetWindowRgn(h.0, rgn, 1);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// The latest figures, to the ball when it is there.
pub fn update(app: &AppHandle, busy: bool, percent: Option<u64>, speed: Option<u64>) {
    let next = {
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let next = FloatState {
            busy,
            percent: percent.filter(|_| busy),
            speed: if busy { speed.unwrap_or(s.speed) } else { 0 },
        };
        if next == *s {
            return;
        }
        *s = next;
        next
    };
    if app.get_webview_window(LABEL).is_some() {
        let _ = app.emit_to(LABEL, "float:state", next);
    }
}

#[tauri::command]
pub fn float_state() -> FloatState {
    *STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Text dropped on the ball: the links in it.
#[tauri::command]
pub fn float_drop(app: AppHandle, text: String) {
    let mut links = intake::links_in(&text);
    if links.is_empty() {
        // Some text with links in it (dragged from a page): its links.
        links = text
            .split_whitespace()
            .filter(|w| omnidl_core::TaskKind::detect(w).is_some())
            .map(str::to_owned)
            .collect();
    }
    intake::offer(&app, links, Origin::Float);
}

/// A `.torrent` file dropped on the ball. The page has its bytes, not its path: it is written to
/// the app's cache and opened from there.
#[tauri::command]
pub fn float_drop_torrent(app: AppHandle, name: String, data: Vec<u8>) -> Result<(), String> {
    if data.len() > 32 << 20 {
        return Err("too large for a torrent".into());
    }
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("dropped");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut name = omnidl_core::filename::sanitize(&name);
    if !name.to_ascii_lowercase().ends_with(".torrent") {
        name.push_str(".torrent");
    }
    let path = dir.join(name);
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    crate::links::open(&app, vec![path.to_string_lossy().into_owned()]);
    Ok(())
}

#[tauri::command]
pub fn float_open(app: AppHandle) {
    crate::focus_main(&app);
}

/// The ball's menu, at the pointer.
#[tauri::command]
pub fn float_menu(window: tauri::WebviewWindow) -> Result<(), String> {
    let app = window.app_handle();
    let l = i18n::current(app);
    let item =
        |id: &str, key: &str| MenuItem::with_id(app, id, i18n::tr(l, key, &[]), true, None::<&str>);
    let build = || -> tauri::Result<Menu<tauri::Wry>> {
        let menu = Menu::new(app)?;
        menu.append(&item("float:show", "tray.show")?)?;
        menu.append(&item("float:new", "shell.cmd.newDownload")?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&item("float:start-all", "shell.cmd.startAll")?)?;
        menu.append(&item("float:pause-all", "shell.cmd.pauseAll")?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&item("float:hide", "float.hide")?)?;
        Ok(menu)
    };
    let menu = build().map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let Some(id) = event.id().as_ref().strip_prefix("float:") else {
        return;
    };
    match id {
        "show" => crate::focus_main(app),
        "new" => crate::tray::new_download(app),
        "start-all" | "pause-all" => crate::tray::run_all(app, id == "start-all"),
        "hide" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let Some(state) = app.try_state::<AppState>() else {
                    return;
                };
                let mut s = state.settings();
                s.general.float_ball = false;
                match crate::commands::save_settings(&app, s).await {
                    Ok(s) => drop(app.emit("settings:changed", s)),
                    Err(e) => tracing::warn!("hiding the floating ball: {e}"),
                }
            });
        }
        _ => {}
    }
}
