//! User settings, kept as TOML in the app's config folder. The engine part is handed to the task
//! manager as is; appearance is applied by the window.

use std::{io, path::Path};

use omnidl_core::EngineConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// UI language: `auto` (the system's) or `zh-CN`, `zh-TW`, `en-US`, `ja-JP`.
    pub language: String,
    pub general: General,
    pub appearance: Appearance,
    /// Where new downloads go; empty: the system's Downloads folder.
    pub download_dir: String,
    pub engine: EngineConfig,
    pub browser: BrowserSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            general: General::default(),
            appearance: Appearance::default(),
            download_dir: String::new(),
            engine: EngineConfig::default(),
            browser: BrowserSettings::default(),
        }
    }
}

/// Running in the background, notifications, shortcuts and updates (docs/development-plan.md
/// §6.5, §7.3). Starting with the system is the system's own setting (`autostart_set`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct General {
    /// Closing the main window hides it (the app keeps downloading, from the notification area)
    /// instead of quitting.
    pub close_to_tray: bool,
    /// A notification when a download finishes, and when one fails.
    pub notify_done: bool,
    pub notify_failed: bool,
    /// Global shortcuts as accelerators (`CommandOrControl+Alt+D`); empty: none. Showing (or
    /// hiding) the main window, and a new download.
    pub shortcut_show: String,
    pub shortcut_new: String,
    /// Look for a new version a little after starting.
    pub check_updates: bool,
    /// The first-run guide has been through (or skipped).
    pub onboarded: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            notify_done: true,
            notify_failed: true,
            shortcut_show: String::new(),
            shortcut_new: String::new(),
            check_updates: true,
            onboarded: false,
        }
    }
}

/// Browser integration (docs/development-plan.md §5, §6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowserSettings {
    /// Ask in the quick-download window before taking over a browser download.
    pub quick_dialog: bool,
    /// Sites whose downloads start without asking ("don't ask again for this site").
    pub silent_sites: Vec<String>,
    /// The local HTTP channel for browsers without Native Messaging (Snap, Flatpak).
    pub http_bridge: bool,
    /// Its port, kept so a pairing stays valid across restarts (0: not chosen yet).
    pub http_port: u16,
    /// Its pairing token (empty: not made yet).
    pub http_token: String,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            quick_dialog: true,
            silent_sites: Vec::new(),
            http_bridge: false,
            http_port: 0,
            http_token: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    /// `system`, `light` or `dark`.
    pub mode: String,
    /// omni-ui built-in theme ids.
    pub light_theme: String,
    pub dark_theme: String,
    /// `compact`, `comfortable` or `spacious`.
    pub density: String,
    /// Window material (Mica, Acrylic, vibrancy) where the system has one.
    pub material: bool,
    /// Windows 11: `mica`, `mica-alt` or `acrylic` (macOS always uses vibrancy).
    pub material_kind: String,
    /// `theme` (the theme's own), `system` (the system's accent colour) or `#RRGGBB`.
    pub accent: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            mode: "system".into(),
            light_theme: "aurora".into(),
            dark_theme: "midnight".into(),
            density: "comfortable".into(),
            material: true,
            material_kind: "mica".into(),
            accent: "theme".into(),
        }
    }
}

impl Settings {
    /// Reads the settings file; a missing or unreadable one gives the defaults (the reason is
    /// logged, the file is left alone until the next save).
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                tracing::warn!("{}: {e}", path.display());
                return Self::default();
            }
        };
        match toml::from_str::<Self>(&text) {
            Ok(mut s) => {
                // Settings from before the first-run guide existed: this is not a first run.
                let guided = toml::from_str::<toml::Table>(&text)
                    .is_ok_and(|t| t.get("general").and_then(|g| g.get("onboarded")).is_some());
                if !guided {
                    s.general.onboarded = true;
                }
                s.normalized()
            }
            Err(e) => {
                tracing::warn!("{}: {e}; using defaults", path.display());
                Self::default()
            }
        }
    }

    /// Writes the file whole (temporary file, then rename), so a crash never leaves half of it.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn normalized(mut self) -> Self {
        self.engine = self.engine.normalized();
        let a = &mut self.appearance;
        if !matches!(a.mode.as_str(), "system" | "light" | "dark") {
            a.mode = "system".into();
        }
        if !matches!(a.density.as_str(), "compact" | "comfortable" | "spacious") {
            a.density = "comfortable".into();
        }
        if !matches!(a.material_kind.as_str(), "mica" | "mica-alt" | "acrylic") {
            a.material_kind = "mica".into();
        }
        let hex = a.accent.len() == 7
            && a.accent.starts_with('#')
            && a.accent[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !(hex || matches!(a.accent.as_str(), "theme" | "system")) {
            a.accent = "theme".into();
        }
        let g = &mut self.general;
        for s in [&mut g.shortcut_show, &mut g.shortcut_new] {
            *s = s.trim().to_owned();
            if !s.is_empty() && crate::shortcuts::parse(s).is_none() {
                s.clear();
            }
        }
        if !matches!(
            self.language.as_str(),
            "auto" | "zh-CN" | "zh-TW" | "en-US" | "ja-JP"
        ) {
            self.language = "auto".into();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use omnidl_core::ProxyConfig;

    use super::*;

    #[test]
    fn round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        assert_eq!(Settings::load(&path), Settings::default());
        let mut s = Settings {
            download_dir: "D:\\下载".into(),
            ..Default::default()
        };
        s.engine.speed_limit = Some(1 << 20);
        s.engine.proxy = ProxyConfig::Custom {
            url: "socks5h://127.0.0.1:1080".into(),
        };
        s.appearance.mode = "dark".into();
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
    }

    #[test]
    fn repairs_bad_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            "[appearance]\nmode = \"neon\"\n[engine]\nconnections = 500\nmaxConcurrent = 0\n",
        )
        .unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.appearance.mode, "system");
        // Written before the first-run guide existed: not a first run.
        assert!(s.general.onboarded);
        assert_eq!(s.engine.connections, 64);
        assert_eq!(s.engine.max_concurrent, 1);
        std::fs::write(
            &path,
            "[general]\nonboarded = false\nshortcutShow = \"Ctrl+Nope+Q\"\nshortcutNew = \"CommandOrControl+Alt+N\"\n",
        )
        .unwrap();
        let s = Settings::load(&path);
        assert!(!s.general.onboarded);
        assert_eq!(s.general.shortcut_show, "");
        assert_eq!(s.general.shortcut_new, "CommandOrControl+Alt+N");
        std::fs::write(&path, "not toml [").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
