//! Magnet links and `.torrent` files handed to the app by the system: a click on a magnet link
//! (deep link), opening a `.torrent` (file association) or the command line. They open the
//! new-download dialog in the main window, like a batch from the browser.

use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

use tauri::{AppHandle, Emitter, Manager};

use crate::{browser::Prefill, state::AppState};

/// The magnet links and existing `.torrent` files among command-line arguments (relative paths
/// against `cwd`).
pub fn from_args<S: AsRef<str>>(args: impl IntoIterator<Item = S>, cwd: &Path) -> Vec<String> {
    args.into_iter()
        .filter_map(|a| source(a.as_ref(), cwd))
        .collect()
}

/// From URLs the system opened the app with (macOS): `magnet:` and `file://…torrent`.
pub fn from_urls(urls: impl IntoIterator<Item = url::Url>) -> Vec<String> {
    urls.into_iter()
        .filter_map(|u| source(u.as_str(), Path::new("/")))
        .collect()
}

fn source(arg: &str, cwd: &Path) -> Option<String> {
    let a = arg.trim();
    if a.get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("magnet:"))
    {
        return Some(a.to_owned());
    }
    let path = if a
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("file://"))
    {
        url::Url::parse(a).ok()?.to_file_path().ok()?
    } else {
        PathBuf::from(a)
    };
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("torrent"))
    {
        return None;
    }
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    path.is_file().then(|| path.to_string_lossy().into_owned())
}

/// Shows the main window with the new-download dialog for `sources`; before the page has
/// loaded they wait for it (`app_bootstrap` hands them over).
pub fn open(app: &AppHandle, sources: Vec<String>) {
    if sources.is_empty() {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    crate::focus_main(app);
    if state.page_ready.load(Ordering::SeqCst) {
        let prefill = Prefill {
            text: sources.join("\n"),
            ..Default::default()
        };
        let _ = app.emit_to("main", "app:new-task", prefill);
    } else {
        state
            .links
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(sources);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_magnets_and_torrent_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.torrent"), b"d4:infod").unwrap();
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567";
        let found = from_args(
            [
                "--background",
                magnet,
                "a.torrent",
                "missing.torrent",
                "x.zip",
            ],
            dir.path(),
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0], magnet);
        assert!(found[1].ends_with("a.torrent"));
        let file = url::Url::from_file_path(dir.path().join("a.torrent")).unwrap();
        let mag = url::Url::parse(magnet).unwrap();
        assert_eq!(from_urls([file, mag]).len(), 2);
    }
}
