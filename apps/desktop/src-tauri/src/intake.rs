//! Links that arrive outside the browser bridge: copied to the clipboard (`clipboard`) or dropped
//! on the floating ball (`float`). One plain download link opens the quick-download window, as a
//! download from the browser does; several links, a magnet link or a torrent open the
//! new-download dialog. A link to a `.torrent` file downloads that file like any other; once it
//! is there it opens the dialog (`torrent_done`).

use omnidl_core::{Category, NewTask, Task, TaskKind, TaskOptions, TaskStatus, filename};
use tauri::{AppHandle, Manager};

use crate::{
    browser::quick::{self, Origin, QuickAction, QuickRequest},
    state::AppState,
};

/// The links in `text` when it is nothing but links (one per line or space-separated):
/// `http(s)`, `ftp(s)`, magnet links and `.torrent` URLs. Text with anything else in it (a copied
/// paragraph that mentions a link) gives none.
pub fn links_in(text: &str) -> Vec<String> {
    let text = text.trim();
    // Not a list of links worth reading.
    if text.is_empty() || text.len() > 64 * 1024 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for word in text.split_whitespace() {
        if TaskKind::detect(word).is_none() || word.ends_with(':') {
            return Vec::new();
        }
        if !out.iter().any(|l: &String| l == word) {
            out.push(word.to_owned());
        }
    }
    out
}

/// Whether a copied link is a file to download: a torrent, or a file of a downloadable category
/// (not a picture, a page or something unknown). Some extensions are left out though their
/// category says otherwise: they are as often source or text (`.ts` is TypeScript as often as a
/// video).
fn worth_offering(url: &str) -> bool {
    match TaskKind::detect(url) {
        Some(TaskKind::Torrent) => true,
        Some(TaskKind::Http | TaskKind::Ftp) => {
            let Some(name) = filename::from_url(url) else {
                return false;
            };
            let ext = name
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if name == ext || ["txt", "md", "csv", "ts", "json", "xml"].contains(&ext.as_str()) {
                return false;
            }
            // A torrent, or an HLS stream's playlist (downloaded into one video file).
            matches!(ext.as_str(), "torrent" | "m3u8")
                || !matches!(filename::category(&name), Category::Image | Category::Other)
        }
        None => false,
    }
}

/// The links of a copied text that look like downloads: a file of a downloadable kind (not a web
/// page or a picture), not already in the list.
pub fn clipboard_links(app: &AppHandle, text: &str) -> Vec<String> {
    let links: Vec<String> = links_in(text)
        .into_iter()
        .filter(|l| worth_offering(l))
        .collect();
    if links.is_empty() {
        return links;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return Vec::new();
    };
    let tasks = state.manager.list();
    links
        .into_iter()
        .filter(|l| {
            !tasks
                .iter()
                .any(|t| &t.source == l || t.final_url.as_deref() == Some(l.as_str()))
        })
        .collect()
}

/// Offers `links` for download (see the module's description).
pub fn offer(app: &AppHandle, links: Vec<String>, origin: Origin) {
    match links.as_slice() {
        [] => {}
        // An FTP folder: its files are chosen in the new-download dialog.
        [one]
            if matches!(TaskKind::detect(one), Some(TaskKind::Http | TaskKind::Ftp))
                && !omnidl_ftp::is_folder(one) =>
        {
            let app = app.clone();
            let url = one.clone();
            tauri::async_runtime::spawn(async move { ask(&app, url, origin).await });
        }
        _ => crate::links::open(app, links),
    }
}

/// A download that just finished as a `.torrent` file (from the browser, the clipboard, the
/// ball or the new-download dialog, which all fetch it as a file): the new-download dialog opens
/// with it, as when the file is opened from the
/// system (BT settings, on by default). Not for one restored from the recycle bin, or one that
/// failed its checksum.
pub fn torrent_done(app: &AppHandle, before: Option<TaskStatus>, task: &Task) {
    let finished = task.status == TaskStatus::Completed
        && matches!(before, Some(TaskStatus::Probing | TaskStatus::Downloading))
        && task.error.is_none();
    if !finished
        || task.kind == TaskKind::Torrent
        || !task.file_name.to_ascii_lowercase().ends_with(".torrent")
    {
        return;
    }
    let on = app
        .try_state::<AppState>()
        .is_some_and(|s| s.settings().general.open_torrents);
    let path = task.path();
    if on && path.is_file() {
        crate::links::open(app, vec![path.to_string_lossy().into_owned()]);
    }
}

/// The quick-download window for one link, then the download as the user chose.
async fn ask(app: &AppHandle, url: String, origin: Origin) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let settings = state.settings();
    let named = filename::from_url(&url).unwrap_or_default();
    let mut task = NewTask {
        source: url.clone(),
        save_dir: state.dir_for(&named),
        options: TaskOptions::default(),
        start: true,
        probe: None,
    };
    let site = url::Url::parse(&url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_default();
    let request = QuickRequest {
        url,
        file_name: None,
        size: None,
        mime: None,
        site,
        browser: None,
        has_cookies: false,
        has_referer: false,
        save_dir: task.save_dir.clone(),
        connections: settings.engine.connections,
        options: TaskOptions::default(),
        origin,
    };
    let decision = quick::ask(app, request).await;
    if !matches!(decision.action, QuickAction::Start | QuickAction::Later) {
        return;
    }
    decision.apply(&mut task, settings.engine.connections);
    if let Err(e) = state.manager.add(vec![task]).await {
        tracing::warn!("adding a {origin:?} link: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_only_text_that_is_links() {
        assert_eq!(
            links_in(" https://a.com/x.zip \n"),
            vec!["https://a.com/x.zip"]
        );
        assert_eq!(
            links_in("https://a.com/x.zip\nftp://b.org/y.iso https://a.com/x.zip"),
            vec!["https://a.com/x.zip", "ftp://b.org/y.iso"]
        );
        assert_eq!(links_in("magnet:?xt=urn:btih:abc").len(), 1);
        // Prose that mentions a link, or not a link at all.
        assert!(links_in("see https://a.com/x.zip for it").is_empty());
        assert!(links_in("hello").is_empty());
        assert!(links_in("").is_empty());
    }

    #[test]
    fn offers_files_not_pages_or_pictures() {
        for yes in [
            "https://a.com/setup.exe",
            "https://a.com/movie.mkv?token=1",
            "ftp://b.org/pub/distro.iso",
            "https://a.com/files/report.pdf",
            "https://a.com/x.torrent",
            "https://cdn.a.com/v/1080p/index.m3u8?token=1",
            "magnet:?xt=urn:btih:abc",
        ] {
            assert!(worth_offering(yes), "{yes}");
        }
        for no in [
            "https://a.com/",
            "https://a.com/page",
            "https://a.com/index.html",
            "https://a.com/photo.jpg",
            "https://github.com/o/r/blob/main/src/app.ts",
            "https://a.com/README.md",
            "https://a.com/notes.txt",
        ] {
            assert!(!worth_offering(no), "{no}");
        }
    }
}
