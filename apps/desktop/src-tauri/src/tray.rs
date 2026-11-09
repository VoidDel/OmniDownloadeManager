//! Notification-area icon (docs/development-plan.md §6.5, §7.3): the app lives there while the
//! main window is hidden (closed to the tray, started with the system or by a browser).
//!
//! A click shows the main window. The menu shows it, starts a new download, starts or pauses
//! everything, sets the global speed limit and quits. While something downloads, the icon carries
//! a green dot and the tooltip shows the speeds.

use std::sync::Mutex;

use omnidl_core::{Stats, TaskStatus};
use tauri::{
    AppHandle, Emitter, Manager, Wry,
    image::Image,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::{browser::Prefill, i18n, state::AppState};

const ID: &str = "omnidl";
const NAME: &str = "Omni Download";

/// The global limit's presets, as in the status bar (bytes per second).
const SPEED_PRESETS: [u64; 6] = [512 * 1024, 1 << 20, 2 << 20, 5 << 20, 10 << 20, 20 << 20];

/// What the icon shows now: busy (the dot) and the tooltip, so a progress tick that changes
/// nothing visible costs nothing.
static SHOWN: Mutex<(bool, String)> = Mutex::new((false, String::new()));

/// Puts the icon in the notification area (once), or gives it a fresh menu: after the language
/// or the speed limit changed.
pub fn sync(app: &AppHandle) {
    let menu = match menu(app) {
        Ok(m) => m,
        Err(e) => return tracing::error!("tray menu: {e}"),
    };
    if let Some(tray) = app.tray_by_id(ID) {
        let _ = tray.set_menu(Some(menu));
        return;
    }
    let mut b = TrayIconBuilder::with_id(ID)
        .tooltip(NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::focus_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    match b.build(app) {
        Ok(_) => *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = (false, NAME.into()),
        // Linux without a tray (GNOME without the AppIndicator extension): the app still works
        // from its window.
        Err(e) => tracing::warn!("tray icon: {e}"),
    }
}

/// Whether the icon is there: closing the main window may then hide it.
pub fn shown(app: &AppHandle) -> bool {
    app.tray_by_id(ID).is_some()
}

/// The totals of a progress tick: the dot while something downloads, the speeds in the tooltip.
pub fn activity(app: &AppHandle, stats: &Stats) {
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    let busy = stats.running > 0;
    let text = if stats.running + stats.seeding > 0 {
        format!(
            "{NAME}\n↓ {}  ↑ {}",
            i18n::speed(stats.download_speed),
            i18n::speed(stats.upload_speed)
        )
    } else {
        NAME.to_owned()
    };
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    if shown.1 != text {
        let _ = tray.set_tooltip(Some(&text));
        shown.1 = text;
    }
    if shown.0 != busy {
        let icon = app.default_window_icon().map(|i| {
            if busy {
                with_dot(i)
            } else {
                i.clone().to_owned()
            }
        });
        let _ = tray.set_icon(icon);
        shown.0 = busy;
    }
}

/// The app icon with a green dot in its lower right corner, ringed so it stands out on any
/// taskbar.
fn with_dot(icon: &Image<'_>) -> Image<'static> {
    let (w, h) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    let size = w.min(h) as f32;
    let r = size * 0.24;
    let (cx, cy) = (w as f32 - r - size * 0.02, h as f32 - r - size * 0.02);
    let ring = (size * 0.06).max(1.0);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let px = if d <= r - ring {
                [0x22, 0xC5, 0x5E, 0xFF]
            } else if d <= r {
                [0xFF, 0xFF, 0xFF, 0xFF]
            } else {
                continue;
            };
            let i = ((y * w + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&px);
        }
    }
    Image::new_owned(rgba, w, h)
}

/// Text for a menu item: on Windows `&` marks the access key, so a literal one is doubled.
fn text(s: &str) -> String {
    if cfg!(windows) {
        s.replace('&', "&&")
    } else {
        s.to_owned()
    }
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let l = i18n::current(app);
    let limit = app.state::<AppState>().settings().engine.speed_limit;
    let item = |id: &str, key: &str| {
        MenuItem::with_id(app, id, text(&i18n::tr(l, key, &[])), true, None::<&str>)
    };
    let check = |id: String, label: String, on: bool| {
        CheckMenuItem::with_id(app, id, text(&label), true, on, None::<&str>)
    };
    let speeds = Submenu::with_id(
        app,
        "limits",
        text(&i18n::tr(l, "shell.speedLimit.title", &[])),
        true,
    )?;
    speeds.append(&check(
        "limit:0".into(),
        i18n::tr(l, "shell.speedLimit.unlimited", &[]),
        limit.is_none(),
    )?)?;
    speeds.append(&PredefinedMenuItem::separator(app)?)?;
    for p in SPEED_PRESETS {
        speeds.append(&check(
            format!("limit:{p}"),
            i18n::speed(p),
            limit == Some(p),
        )?)?;
    }
    if let Some(custom) = limit.filter(|l| !SPEED_PRESETS.contains(l)) {
        let label = i18n::tr(
            l,
            "shell.speedLimit.customValue",
            &[("speed", &i18n::speed(custom))],
        );
        speeds.append(&check(format!("limit:{custom}"), label, true)?)?;
    }
    let menu = Menu::new(app)?;
    menu.append(&item("show", "tray.show")?)?;
    menu.append(&item("new", "shell.cmd.newDownload")?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&item("start-all", "shell.cmd.startAll")?)?;
    menu.append(&item("pause-all", "shell.cmd.pauseAll")?)?;
    menu.append(&speeds)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&item("quit", "tray.quit")?)?;
    Ok(menu)
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref().to_owned();
    let app = app.clone();
    match id.as_str() {
        "show" => crate::focus_main(&app),
        "new" => new_download(&app),
        "quit" => app.exit(0),
        _ => {
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let ids = |keep: fn(TaskStatus) -> bool| {
                    state
                        .manager
                        .list()
                        .into_iter()
                        .filter(|t| keep(t.status))
                        .map(|t| t.id)
                        .collect::<Vec<_>>()
                };
                let r = match id.as_str() {
                    "start-all" => {
                        state
                            .manager
                            .resume(&ids(|s| s == TaskStatus::Paused))
                            .await
                    }
                    "pause-all" => {
                        let running = ids(|s| {
                            matches!(
                                s,
                                TaskStatus::Queued | TaskStatus::Probing | TaskStatus::Downloading
                            )
                        });
                        state.manager.pause(&running).await
                    }
                    other => match other.strip_prefix("limit:").map(str::parse::<u64>) {
                        Some(Ok(limit)) => {
                            let mut s = state.settings();
                            s.engine.speed_limit = Some(limit).filter(|&l| l > 0);
                            crate::commands::save_settings(&app, s)
                                .await
                                .map(|s| drop(app.emit("settings:changed", s)))
                        }
                        _ => Ok(()),
                    },
                };
                if let Err(e) = r {
                    tracing::warn!("tray {id}: {e}");
                }
            });
        }
    }
}

/// Shows the main window with the new-download dialog open.
pub fn new_download(app: &AppHandle) {
    crate::focus_main(app);
    let _ = app.emit_to("main", "app:new-task", Prefill::default());
}
