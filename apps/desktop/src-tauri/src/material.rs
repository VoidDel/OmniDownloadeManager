//! Window material (as in OmniZip): Windows 11 Mica, Mica Alt or Acrylic, macOS vibrancy. The
//! windows are transparent where a material is available; the page paints the theme canvas
//! itself while the material is off (omni-ui's `data-material`), and only the sidebar (and the
//! quick-download window, under the sidebar's veil) lets the material show.

use tauri::{
    AppHandle, Manager, WebviewWindowBuilder,
    utils::config::WindowEffectsConfig,
    window::{Color, Effect, EffectsBuilder},
};

use crate::settings::Settings;

/// Window materials exist here: Windows 11 (build 22000 on) and macOS.
pub fn supported() -> bool {
    #[cfg(windows)]
    {
        windows_build() >= 22000
    }
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        false
    }
}

#[cfg(windows)]
fn windows_build() -> u32 {
    windows_registry::LOCAL_MACHINE
        .open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|k| k.get_string("CurrentBuildNumber"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// The material for `settings` (`None` when off or unavailable). Windows 11 offers Mica (the
/// desktop wallpaper, heavily blurred), Mica Alt (the same with a stronger tint) and Acrylic (a
/// live blur of what is behind the window); macOS always uses vibrancy.
pub fn effects(settings: &Settings) -> Option<WindowEffectsConfig> {
    if !settings.appearance.material || !supported() {
        return None;
    }
    let effect = if cfg!(target_os = "macos") {
        Effect::Sidebar
    } else {
        match settings.appearance.material_kind.as_str() {
            "mica-alt" => Effect::Tabbed,
            "acrylic" => Effect::Acrylic,
            _ => Effect::Mica,
        }
    };
    Some(EffectsBuilder::new().effect(effect).build())
}

/// Prepares a window being built: transparent where materials exist (whether or not one is on:
/// an opaque background would be painted under the page and cover the material, and it cannot be
/// cleared reliably later), with the current material.
pub fn prepare<'a>(
    b: WebviewWindowBuilder<'a, tauri::Wry, AppHandle>,
    settings: &Settings,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    if !supported() {
        return b;
    }
    #[cfg(windows)]
    let b = b.transparent(true);
    let b = b.background_color(Color(0, 0, 0, 0));
    match effects(settings) {
        Some(fx) => b.effects(fx),
        None => b,
    }
}

/// Applies the material settings to every open window but the floating ball (see `float::clip`).
pub fn apply(app: &AppHandle, settings: &Settings) {
    let fx = effects(settings);
    for (label, w) in app.webview_windows() {
        if label == crate::float::LABEL {
            continue;
        }
        let _ = w.set_effects(fx.clone());
    }
}

/// The system's accent colour as `#RRGGBB` (Windows; elsewhere unknown).
pub fn system_accent() -> Option<String> {
    #[cfg(windows)]
    {
        // DWM stores the accent as 0xAABBGGRR.
        let v = windows_registry::CURRENT_USER
            .open(r"Software\Microsoft\Windows\DWM")
            .and_then(|k| k.get_u32("AccentColor"))
            .ok()?;
        Some(format!(
            "#{:02X}{:02X}{:02X}",
            v & 0xFF,
            (v >> 8) & 0xFF,
            (v >> 16) & 0xFF
        ))
    }
    #[cfg(not(windows))]
    {
        None
    }
}
