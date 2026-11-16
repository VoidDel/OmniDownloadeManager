//! User settings, kept as TOML in the app's config folder. The engine part is handed to the task
//! manager as is; appearance is applied by the window.

use std::{collections::BTreeMap, io, path::Path};

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
    /// Whether downloads are sorted into folders by their category (`folder_for`).
    pub category_folders: CategoryFolders,
    /// With `CategoryFolders::Manual`, a folder of its own for a category (`video`,
    /// `disk_image`… as the tasks name them); one not listed goes to `download_dir`. Kept when
    /// another way is chosen.
    pub category_dirs: BTreeMap<String, String>,
    pub engine: EngineConfig,
    pub browser: BrowserSettings,
    /// Starting and stopping at set times, and what to do once everything is downloaded.
    pub schedule: crate::schedule::Schedule,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            general: General::default(),
            appearance: Appearance::default(),
            download_dir: String::new(),
            category_folders: CategoryFolders::None,
            category_dirs: BTreeMap::new(),
            engine: EngineConfig::default(),
            browser: BrowserSettings::default(),
            schedule: crate::schedule::Schedule::default(),
        }
    }
}

/// How downloads are sorted into folders by their category. A file of no category (`other`)
/// goes to the download folder whichever way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategoryFolders {
    /// Everything in the download folder.
    #[default]
    None,
    /// A folder per category inside the download folder, named as the UI names the category.
    Auto,
    /// The folder set for each category (`category_dirs`); the download folder for the others.
    Manual,
}

/// The floating ball's looks. One this version does not know (a file from a newer one) is the
/// classic one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", from = "String")]
pub enum FloatStyle {
    #[default]
    Classic,
    Liquid,
    Comet,
    Aurora,
}

impl From<String> for FloatStyle {
    fn from(name: String) -> Self {
        match name.as_str() {
            "liquid" => Self::Liquid,
            "comet" => Self::Comet,
            "aurora" => Self::Aurora,
            _ => Self::Classic,
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
    /// A sound when a download finishes: the built-in chime, or `sound_file` (a WAV file) when
    /// it is set and still there.
    pub sound_done: bool,
    pub sound_file: String,
    /// Global shortcuts as accelerators (`CommandOrControl+Alt+D`); empty: none. Showing (or
    /// hiding) the main window, and a new download.
    pub shortcut_show: String,
    pub shortcut_new: String,
    /// Look for a new version a little after starting.
    pub check_updates: bool,
    /// The floating ball: a small always-on-top circle with the overall progress; links dropped
    /// on it are downloaded (see `float`).
    pub float_ball: bool,
    /// How the ball is drawn (the page's business: features/float/Ball.tsx).
    pub float_style: FloatStyle,
    /// Download links copied anywhere are offered for download (see `clipboard`).
    pub watch_clipboard: bool,
    /// A `.torrent` file downloaded over HTTP or FTP opens the new-download dialog with it (its
    /// files to choose), as one opened from the system does. On the BT page.
    pub open_torrents: bool,
    /// Deleting a task (to the recycle bin): `ask` whether its files go too, or the answer
    /// remembered from that question, `keep` or `delete`. On the Download page.
    pub remove_files: String,
    /// The first-run guide has been through (or skipped).
    pub onboarded: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            notify_done: true,
            notify_failed: true,
            sound_done: true,
            sound_file: String::new(),
            shortcut_show: String::new(),
            shortcut_new: String::new(),
            check_updates: true,
            float_ball: false,
            float_style: FloatStyle::default(),
            watch_clipboard: false,
            open_torrents: true,
            remove_files: "ask".into(),
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
                let table = toml::from_str::<toml::Table>(&text).ok();
                let has = |group: &str, key: &str| {
                    table
                        .as_ref()
                        .and_then(|t| t.get(group))
                        .and_then(|g| g.get(key))
                        .is_some()
                };
                if !has("general", "onboarded") {
                    s.general.onboarded = true;
                }
                // Settings from before the choice of how to sort: folders set for categories
                // were used, nothing else was sorted.
                if table
                    .as_ref()
                    .is_some_and(|t| !t.contains_key("categoryFolders"))
                {
                    s.category_folders = if s.category_dirs.values().any(|d| !d.trim().is_empty()) {
                        CategoryFolders::Manual
                    } else {
                        CategoryFolders::None
                    };
                }
                // Settings from before the automatic user agent: one the user changed is kept.
                if !has("engine", "userAgentAuto") {
                    let ua = s.engine.user_agent.trim();
                    s.engine.user_agent_auto =
                        ua.is_empty() || ua == omnidl_core::default_user_agent();
                }
                s.normalized()
            }
            Err(e) => {
                tracing::warn!("{}: {e}; using defaults", path.display());
                Self::default()
            }
        }
    }

    /// The folder a new download of `category` (`video`, `disk_image`… as the tasks name them)
    /// goes to: `download_dir` (the folder new downloads go to, resolved), or a folder for its
    /// category in it named in `lang` (`Auto`), or the one set for it (`Manual`).
    pub fn folder_for(&self, category: &str, download_dir: &str, lang: &str) -> String {
        if category == "other" || category.is_empty() {
            return download_dir.to_owned();
        }
        match self.category_folders {
            CategoryFolders::None => download_dir.to_owned(),
            CategoryFolders::Auto => {
                let name = crate::i18n::tr(lang, &format!("category.{category}"), &[]);
                Path::new(download_dir)
                    .join(omnidl_core::filename::sanitize(&name))
                    .to_string_lossy()
                    .into_owned()
            }
            CategoryFolders::Manual => match self.category_dirs.get(category) {
                Some(dir) if !dir.trim().is_empty() => dir.trim().to_owned(),
                _ => download_dir.to_owned(),
            },
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
        self.schedule = self.schedule.normalized();
        let g = &mut self.general;
        if !matches!(g.remove_files.as_str(), "ask" | "keep" | "delete") {
            g.remove_files = "ask".into();
        }
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
    fn sorts_downloads_by_category_as_chosen() {
        let dl = if cfg!(windows) {
            "D:\\下载"
        } else {
            "/home/u/下载"
        };
        let mut s = Settings::default();
        s.category_dirs.insert("video".into(), "E:\\影片".into());
        // Not sorted: everything in the download folder.
        assert_eq!(s.folder_for("video", dl, "zh-CN"), dl);
        // A folder per category inside it, named in the UI's language; none for "other".
        s.category_folders = CategoryFolders::Auto;
        let sub = |name: &str| Path::new(dl).join(name).to_string_lossy().into_owned();
        assert_eq!(s.folder_for("video", dl, "zh-CN"), sub("视频"));
        assert_eq!(s.folder_for("disk_image", dl, "en-US"), sub("Disk images"));
        assert_eq!(s.folder_for("other", dl, "zh-CN"), dl);
        // The folders set; the download folder for a category without one.
        s.category_folders = CategoryFolders::Manual;
        assert_eq!(s.folder_for("video", dl, "zh-CN"), "E:\\影片");
        assert_eq!(s.folder_for("audio", dl, "zh-CN"), dl);
        assert_eq!(s.folder_for("other", dl, "zh-CN"), dl);
    }

    #[test]
    fn sorts_as_before_in_older_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        // Folders set for categories: they were used.
        std::fs::write(&path, "[categoryDirs]\nvideo = 'E:\\影片'\n").unwrap();
        assert_eq!(
            Settings::load(&path).category_folders,
            CategoryFolders::Manual
        );
        // None set: nothing was sorted.
        std::fs::write(&path, "downloadDir = 'D:\\下载'\n").unwrap();
        assert_eq!(
            Settings::load(&path).category_folders,
            CategoryFolders::None
        );
        // A choice saved is kept.
        let mut s = Settings::load(&path);
        s.category_folders = CategoryFolders::Auto;
        s.save(&path).unwrap();
        assert_eq!(
            Settings::load(&path).category_folders,
            CategoryFolders::Auto
        );
    }

    #[test]
    fn keeps_the_floating_balls_look() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut s = Settings::default();
        assert_eq!(s.general.float_style, FloatStyle::Classic);
        s.general.float_style = FloatStyle::Aurora;
        s.save(&path).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("floatStyle = \"aurora\"")
        );
        assert_eq!(
            Settings::load(&path).general.float_style,
            FloatStyle::Aurora
        );
        // A look of a newer version: the classic one, and the rest of the file still read.
        std::fs::write(
            &path,
            "[general]\nfloatStyle = \"nebula\"\nfloatBall = true\n",
        )
        .unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.general.float_style, FloatStyle::Classic);
        assert!(s.general.float_ball);
    }

    #[test]
    fn keeps_a_user_agent_changed_before_the_automatic_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "[engine]\nuserAgent = \"Wget/1.21\"\n").unwrap();
        let s = Settings::load(&path);
        assert!(!s.engine.user_agent_auto);
        assert_eq!(s.engine.user_agent, "Wget/1.21");
        std::fs::write(&path, "[engine]\nconnections = 4\n").unwrap();
        assert!(
            Settings::load(&path).engine.user_agent_auto,
            "the default one: automatic"
        );
        let mut s = Settings::load(&path);
        s.engine.user_agent_auto = false;
        s.save(&path).unwrap();
        assert!(
            !Settings::load(&path).engine.user_agent_auto,
            "a choice saved is kept"
        );
    }

    #[test]
    fn reads_the_single_ip_filter_list_of_older_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            "[engine]\nbtIpFilter = true\nbtIpFilterList = \"https://a.example/level1.gz\"\n",
        )
        .unwrap();
        let mut s = Settings::load(&path);
        assert!(s.engine.bt_ip_filter);
        assert_eq!(s.engine.bt_ip_filter_lists, ["https://a.example/level1.gz"]);
        s.engine.bt_ip_filter_lists.push("/lists/more.p2p".into());
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
