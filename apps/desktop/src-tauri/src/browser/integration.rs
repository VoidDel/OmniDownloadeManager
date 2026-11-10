//! Native Messaging host registration (docs/development-plan.md §5.5, ADR-002): a manifest per
//! browser family telling the browser where `omnidl-nm-host` is and which extensions may use it.
//!
//! - Windows: manifests in `<local app data>/nm/`, one registry value per browser under
//!   `HKCU\Software\…\NativeMessagingHosts\app.omnidownload.host` pointing at them.
//! - macOS / Linux: the manifest in each installed browser's `NativeMessagingHosts` folder.
//!
//! At start-up only missing or broken registrations are written (another install's working one
//! is left alone); "repair" in the settings rewrites them all for this install.

use std::path::{Path, PathBuf};

use omnidl_ipc::HOST_NAME;
use serde::Serialize;

/// Chromium extension IDs allowed to use the host. Each store signs the extension with its own
/// key, so each gives it its own ID (docs/extension-publishing.md).
pub const CHROMIUM_EXTENSION_IDS: &[&str] = &[
    // The development key's (`apps/extension/wxt.config.ts`): unpacked builds and the tests.
    "nfifehjghnnmamofhbbiagiaodhfeipn",
    // Microsoft Edge Add-ons.
    "opflkejfjljlncpianfbijacendmcgmo",
    // Chrome Web Store.
    "kjlikfchbfphdjcafhclmehpgnekedhg",
];
/// The Firefox extension ID (`browser_specific_settings.gecko.id`).
pub const FIREFOX_EXTENSION_ID: &str = "omni@omnidl.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Browser {
    Chrome,
    Edge,
    Firefox,
    Brave,
    Chromium,
}

impl Browser {
    pub const ALL: [Self; 5] = [
        Self::Chrome,
        Self::Edge,
        Self::Firefox,
        Self::Brave,
        Self::Chromium,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Firefox => "Firefox",
            Self::Brave => "Brave",
            Self::Chromium => "Chromium",
        }
    }

    fn firefox(self) -> bool {
        self == Self::Firefox
    }
}

/// One browser in the settings page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    pub id: Browser,
    pub name: &'static str,
    /// The browser seems installed.
    pub detected: bool,
    /// A manifest is registered and points at an existing host.
    pub registered: bool,
    /// …and that host is this install's.
    pub ours: bool,
}

/// `omnidl-nm-host` next to the app's executable, if it is there.
pub fn host_path() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "omnidl-nm-host.exe"
    } else {
        "omnidl-nm-host"
    };
    let p = std::env::current_exe().ok()?.with_file_name(name);
    p.is_file().then_some(p)
}

fn manifest(host: &Path, firefox: bool) -> serde_json::Value {
    let mut m = serde_json::json!({
        "name": HOST_NAME,
        "description": "Omni Download browser bridge",
        "path": host,
        "type": "stdio",
    });
    if firefox {
        m["allowed_extensions"] = serde_json::json!([FIREFOX_EXTENSION_ID]);
    } else {
        m["allowed_origins"] = CHROMIUM_EXTENSION_IDS
            .iter()
            .map(|id| format!("chrome-extension://{id}/"))
            .collect();
    }
    m
}

/// The host a manifest file points at, if it is a readable manifest of ours.
fn manifest_host(file: &Path) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    (v["name"] == HOST_NAME).then(|| PathBuf::from(v["path"].as_str().unwrap_or_default()))
}

/// Whether a manifest file allows the extensions this version knows. One written by an earlier
/// version does not allow the IDs that came with the stores.
fn allows_current(file: &Path, firefox: bool) -> bool {
    let key = if firefox {
        "allowed_extensions"
    } else {
        "allowed_origins"
    };
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|v| v[key] == manifest(Path::new(""), firefox)[key])
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn write_manifest(file: &Path, host: &Path, firefox: bool) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text =
        serde_json::to_string_pretty(&manifest(host, firefox)).map_err(std::io::Error::other)?;
    std::fs::write(file, text)
}

/// The state of every browser.
pub fn status(host: Option<&Path>, manifest_dir: &Path) -> Vec<BrowserState> {
    Browser::ALL
        .iter()
        .map(|&b| {
            let current = registered_host(b, manifest_dir);
            let registered = current.as_deref().is_some_and(Path::is_file);
            BrowserState {
                id: b,
                name: b.name(),
                detected: detected(b),
                registered,
                ours: registered
                    && host.is_some_and(|h| current.as_deref().is_some_and(|c| same_file(c, h))),
            }
        })
        .collect()
}

/// Registers `host` for the browsers missing a working registration, or all of them with
/// `force`. Returns the new state.
pub fn register(host: &Path, manifest_dir: &Path, force: bool) -> Vec<BrowserState> {
    for b in Browser::ALL {
        let file = registered_file(b, manifest_dir);
        let current = file.as_deref().and_then(manifest_host);
        let working = current.as_deref().is_some_and(Path::is_file);
        // This installation's own registration, written by an earlier version: written again,
        // so that the extensions allowed since then can connect.
        let stale = working
            && current.as_deref().is_some_and(|c| same_file(c, host))
            && file
                .as_deref()
                .is_some_and(|f| !allows_current(f, b.firefox()));
        if working && !stale && !force {
            continue;
        }
        if let Err(e) = register_one(b, host, manifest_dir) {
            tracing::warn!("registering the browser bridge for {}: {e}", b.name());
        }
    }
    status(Some(host), manifest_dir)
}

// ---------------------------------------------------------------------------------- Windows

#[cfg(windows)]
fn registry_key(b: Browser) -> String {
    let vendor = match b {
        Browser::Chrome => r"Software\Google\Chrome",
        Browser::Edge => r"Software\Microsoft\Edge",
        Browser::Firefox => r"Software\Mozilla",
        Browser::Brave => r"Software\BraveSoftware\Brave-Browser",
        Browser::Chromium => r"Software\Chromium",
    };
    format!(r"{vendor}\NativeMessagingHosts\{HOST_NAME}")
}

#[cfg(windows)]
fn manifest_file(b: Browser, manifest_dir: &Path) -> PathBuf {
    manifest_dir.join(if b.firefox() {
        "firefox.json"
    } else {
        "chromium.json"
    })
}

/// The manifest file the browser is pointed at.
#[cfg(windows)]
fn registered_file(b: Browser, _manifest_dir: &Path) -> Option<PathBuf> {
    windows_registry::CURRENT_USER
        .open(registry_key(b))
        .ok()?
        .get_string("")
        .ok()
        .map(PathBuf::from)
}

#[cfg(windows)]
fn registered_host(b: Browser, manifest_dir: &Path) -> Option<PathBuf> {
    manifest_host(&registered_file(b, manifest_dir)?)
}

#[cfg(windows)]
fn register_one(b: Browser, host: &Path, manifest_dir: &Path) -> std::io::Result<()> {
    let file = manifest_file(b, manifest_dir);
    write_manifest(&file, host, b.firefox())?;
    windows_registry::CURRENT_USER
        .create(registry_key(b))
        .and_then(|k| k.set_string("", file.to_string_lossy()))
        .map_err(|e| std::io::Error::other(e.to_string()))
}

#[cfg(windows)]
fn detected(b: Browser) -> bool {
    let exe = match b {
        Browser::Chrome => "chrome.exe",
        Browser::Edge => "msedge.exe",
        Browser::Firefox => "firefox.exe",
        Browser::Brave => "brave.exe",
        // Chromium has no App Paths entry. Its installer adds an uninstall entry and puts
        // chrome.exe under Chromium\Application. Not `Software\Chromium`: registering the host
        // creates it, and programs built on Chromium write `Software\Chromium\BLBeacon` too (one
        // made Edge and Chromium look alike in the settings).
        Browser::Chromium => {
            const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Chromium";
            let dirs = ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"];
            return windows_registry::CURRENT_USER.open(UNINSTALL).is_ok()
                || windows_registry::LOCAL_MACHINE.open(UNINSTALL).is_ok()
                || dirs.iter().filter_map(std::env::var_os).any(|d| {
                    Path::new(&d)
                        .join(r"Chromium\Application\chrome.exe")
                        .is_file()
                });
        }
    };
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    windows_registry::CURRENT_USER.open(&key).is_ok()
        || windows_registry::LOCAL_MACHINE.open(&key).is_ok()
}

// ---------------------------------------------------------------------------------- Unix

/// The browser's profile root and its host folder, relative to the home folder.
#[cfg(unix)]
fn folders(b: Browser) -> (&'static str, &'static str) {
    if cfg!(target_os = "macos") {
        match b {
            Browser::Chrome => (
                "Library/Application Support/Google/Chrome",
                "NativeMessagingHosts",
            ),
            Browser::Edge => (
                "Library/Application Support/Microsoft Edge",
                "NativeMessagingHosts",
            ),
            Browser::Firefox => (
                "Library/Application Support/Mozilla",
                "NativeMessagingHosts",
            ),
            Browser::Brave => (
                "Library/Application Support/BraveSoftware/Brave-Browser",
                "NativeMessagingHosts",
            ),
            Browser::Chromium => (
                "Library/Application Support/Chromium",
                "NativeMessagingHosts",
            ),
        }
    } else {
        match b {
            Browser::Chrome => (".config/google-chrome", "NativeMessagingHosts"),
            Browser::Edge => (".config/microsoft-edge", "NativeMessagingHosts"),
            Browser::Firefox => (".mozilla", "native-messaging-hosts"),
            Browser::Brave => (
                ".config/BraveSoftware/Brave-Browser",
                "NativeMessagingHosts",
            ),
            Browser::Chromium => (".config/chromium", "NativeMessagingHosts"),
        }
    }
}

#[cfg(unix)]
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

#[cfg(unix)]
fn manifest_file(b: Browser) -> PathBuf {
    let (root, hosts) = folders(b);
    home()
        .join(root)
        .join(hosts)
        .join(format!("{HOST_NAME}.json"))
}

/// The manifest file the browser reads.
#[cfg(unix)]
fn registered_file(b: Browser, _manifest_dir: &Path) -> Option<PathBuf> {
    Some(manifest_file(b))
}

#[cfg(unix)]
fn registered_host(b: Browser, manifest_dir: &Path) -> Option<PathBuf> {
    manifest_host(&registered_file(b, manifest_dir)?)
}

#[cfg(unix)]
fn register_one(b: Browser, host: &Path, _manifest_dir: &Path) -> std::io::Result<()> {
    // Only for installed browsers: no folders for the others.
    if !detected(b) {
        return Ok(());
    }
    write_manifest(&manifest_file(b), host, b.firefox())
}

#[cfg(unix)]
fn detected(b: Browser) -> bool {
    home().join(folders(b).0).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_from_an_earlier_version_is_not_current() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chromium.json");
        // As the first release wrote it: the development ID only.
        std::fs::write(
            &file,
            format!(
                r#"{{"name":"{HOST_NAME}","path":"x","type":"stdio","allowed_origins":["chrome-extension://nfifehjghnnmamofhbbiagiaodhfeipn/"]}}"#
            ),
        )
        .unwrap();
        assert!(!allows_current(&file, false));
        write_manifest(&file, Path::new("/opt/omni/omnidl-nm-host"), false).unwrap();
        assert!(allows_current(&file, false));
        // Firefox's list has not changed, and a missing file is not current.
        let firefox = dir.path().join("firefox.json");
        assert!(!allows_current(&firefox, true));
        write_manifest(&firefox, Path::new("/opt/omni/omnidl-nm-host"), true).unwrap();
        assert!(allows_current(&firefox, true));
    }

    #[test]
    fn writes_manifests_for_each_family() {
        let chromium = manifest(Path::new("/opt/omni/omnidl-nm-host"), false);
        assert_eq!(chromium["name"], HOST_NAME);
        assert_eq!(chromium["type"], "stdio");
        assert_eq!(
            chromium["allowed_origins"][0],
            "chrome-extension://nfifehjghnnmamofhbbiagiaodhfeipn/"
        );
        // The stores' IDs too.
        for id in [
            "opflkejfjljlncpianfbijacendmcgmo",
            "kjlikfchbfphdjcafhclmehpgnekedhg",
        ] {
            assert!(
                chromium["allowed_origins"]
                    .as_array()
                    .unwrap()
                    .contains(&format!("chrome-extension://{id}/").into())
            );
        }
        assert!(chromium.get("allowed_extensions").is_none());
        let firefox = manifest(Path::new("/opt/omni/omnidl-nm-host"), true);
        assert_eq!(firefox["allowed_extensions"][0], FIREFOX_EXTENSION_ID);
        assert!(firefox.get("allowed_origins").is_none());

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("m.json");
        let host = dir.path().join("host");
        write_manifest(&file, &host, false).unwrap();
        assert_eq!(manifest_host(&file), Some(host));
        std::fs::write(&file, r#"{"name":"other","path":"x"}"#).unwrap();
        assert_eq!(manifest_host(&file), None);
    }
}
