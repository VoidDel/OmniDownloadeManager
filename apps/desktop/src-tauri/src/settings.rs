//! User settings, kept as TOML in the app's config folder. The engine part is handed to the task
//! manager as is; appearance is applied by the window.

use std::{io, path::Path};

use omnidl_core::EngineConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub appearance: Appearance,
    /// Where new downloads go; empty: the system's Downloads folder.
    pub download_dir: String,
    pub engine: EngineConfig,
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
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            mode: "system".into(),
            light_theme: "aurora".into(),
            dark_theme: "midnight".into(),
            density: "comfortable".into(),
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
            Ok(s) => s.normalized(),
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
        assert_eq!(s.engine.connections, 64);
        assert_eq!(s.engine.max_concurrent, 1);
        std::fs::write(&path, "not toml [").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }
}
