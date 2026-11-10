//! Global shortcuts (docs/development-plan.md §6.5): showing or hiding the main window, and a new
//! download, from any app. Both are off until set in the settings: any default would take keys
//! from other apps (or from the browser extension's own Alt+Shift+D).

use tauri::{AppHandle, Manager, Wry, plugin::TauriPlugin};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::{settings::General, state::AppState};

/// An accelerator (`CommandOrControl+Alt+D`), if it is one.
pub fn parse(s: &str) -> Option<Shortcut> {
    s.parse().ok()
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(pressed)
        .build()
}

fn pressed(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let general = state.settings().general;
    let is = |s: &str| parse(s).is_some_and(|x| x == *shortcut);
    if is(&general.shortcut_new) {
        crate::tray::new_download(app);
    } else if is(&general.shortcut_show) {
        // Shows the window, or sends it to the notification area when it is already in front.
        if let Some(w) = app.get_webview_window("main")
            && w.is_visible().unwrap_or(false)
            && w.is_focused().unwrap_or(false)
            && crate::tray::shown(app)
        {
            crate::to_tray(app);
        } else {
            crate::focus_main(app);
        }
    }
}

/// Registers the shortcuts of the settings in place of the previous ones. A shortcut another
/// app holds cannot be had: the error names it (the ones that could be registered stay).
pub fn apply(app: &AppHandle, general: &General) -> Result<(), String> {
    let gs = app.global_shortcut();
    if let Err(e) = gs.unregister_all() {
        tracing::warn!("global shortcuts: {e}");
    }
    let mut taken = None;
    for s in [&general.shortcut_show, &general.shortcut_new] {
        let Some(sc) = parse(s) else {
            continue;
        };
        if let Err(e) = gs.register(sc) {
            tracing::warn!("global shortcut {s}: {e}");
            taken.get_or_insert_with(|| s.clone());
        }
    }
    taken.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_accelerators() {
        assert!(parse("CommandOrControl+Alt+D").is_some());
        assert!(parse("Ctrl+Shift+F12").is_some());
        assert!(parse("Ctrl+Nope+Q").is_none());
        assert!(parse("").is_none());
    }
}
