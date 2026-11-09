//! The Omni Download desktop app (Tauri 2): the main window. Commands, the event bridge, the
//! tray and the IPC server for the browser bridge arrive with their phases
//! (docs/development-plan.md §10).

use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Starts the app.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            main_window(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Omni Download");
}

/// The main window (docs/development-plan.md §7.3). It is created hidden and the page shows it
/// once its first frame is painted, so it never flashes white.
fn main_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Omni Download")
        .inner_size(1200.0, 760.0)
        .min_inner_size(900.0, 600.0)
        .center()
        .visible(false);
    // macOS keeps its traffic lights over the sidebar's top row (40 px, as in OmniZip).
    #[cfg(target_os = "macos")]
    let b = b
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(13.0, 22.5));
    // Windows and Linux draw their own title bar and window buttons (omni-ui `.winctl`); on Linux
    // the page also hands presses on its edges to the window manager to resize.
    #[cfg(not(target_os = "macos"))]
    let b = b.decorations(false);
    b.build()
}
