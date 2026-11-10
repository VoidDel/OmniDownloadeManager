//! Watching the clipboard (General settings, off by default): a download link copied anywhere
//! is offered for download (`intake`). What was on the clipboard when watching began is not
//! offered, nor links already in the list (copying a task's link from the app among them).

use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::{browser::quick::Origin, intake, state::AppState};

/// How often the clipboard is looked at. On Windows a look costs nothing until it changed.
const EVERY: Duration = Duration::from_millis(800);

pub fn start(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || watch(&app));
    if let Err(e) = spawned {
        tracing::warn!("clipboard watcher: {e}");
    }
}

fn watch(app: &AppHandle) {
    let mut clipboard: Option<arboard::Clipboard> = None;
    // The text seen last; `None` until watching starts (the first text is only noted).
    let mut last: Option<String> = None;
    #[cfg(windows)]
    let mut last_seq = 0u32;
    loop {
        std::thread::sleep(EVERY);
        let on = app
            .try_state::<AppState>()
            .is_some_and(|s| s.settings().general.watch_clipboard);
        if !on {
            last = None;
            clipboard = None;
            continue;
        }
        #[cfg(windows)]
        {
            // SAFETY: no arguments; reads a counter the system keeps.
            let seq =
                unsafe { windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber() };
            if last.is_some() && seq == last_seq {
                continue;
            }
            last_seq = seq;
        }
        if clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => clipboard = Some(c),
                Err(e) => {
                    tracing::debug!("clipboard: {e}");
                    continue;
                }
            }
        }
        // Not text (a picture, files): nothing to offer, but it is the latest content.
        let text = clipboard
            .as_mut()
            .and_then(|c| c.get_text().ok())
            .unwrap_or_default();
        let Some(prev) = last.replace(text.clone()) else {
            continue;
        };
        if prev == text {
            continue;
        }
        let links = intake::clipboard_links(app, &text);
        if !links.is_empty() {
            tracing::debug!("clipboard: offering {} link(s)", links.len());
            intake::offer(app, links, Origin::Clipboard);
        }
    }
}
