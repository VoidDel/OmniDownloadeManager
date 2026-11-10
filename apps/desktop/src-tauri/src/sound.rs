//! The sound when a download finishes (General settings): the built-in chime or a WAV file of the
//! user's. Played by the system (Windows' PlaySound, macOS' afplay, PulseAudio / ALSA on Linux),
//! so it works while the app has no window.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager};

/// The built-in chime (a soft two-note bell, generated for the app).
const CHIME: &[u8] = include_bytes!("../sounds/done.wav");

/// Several downloads finishing together make one sound.
const QUIET_FOR: Duration = Duration::from_millis(1500);

static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// The sound for a finished download, as the settings say.
pub fn done(app: &AppHandle) {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let general = state.settings().general;
    if !general.sound_done {
        return;
    }
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed() < QUIET_FOR) {
            return;
        }
        *last = Some(Instant::now());
    }
    play(app, &general.sound_file);
}

/// Plays `file`, or the built-in chime when it is empty or missing. Returns at once.
pub fn play(app: &AppHandle, file: &str) {
    let path = match file.trim() {
        f if !f.is_empty() && Path::new(f).is_file() => PathBuf::from(f),
        _ => match chime(app) {
            Some(p) => p,
            None => return,
        },
    };
    std::thread::spawn(move || {
        if let Err(e) = system_play(&path) {
            tracing::debug!("playing {}: {e}", path.display());
        }
    });
}

/// The built-in chime as a file (the system players take a path), written once.
fn chime(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_local_data_dir().ok()?;
    let path = dir.join("done.wav");
    let current = std::fs::metadata(&path).is_ok_and(|m| m.len() == CHIME.len() as u64);
    if !current {
        std::fs::create_dir_all(&dir).ok()?;
        if let Err(e) = std::fs::write(&path, CHIME) {
            tracing::warn!("{}: {e}", path.display());
            return None;
        }
    }
    Some(path)
}

#[cfg(windows)]
fn system_play(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_FILENAME, SND_NODEFAULT, SND_SYNC};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // On this thread of its own: synchronous, so the sound is not cut off when the call returns.
    let ok = unsafe {
        PlaySoundW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            SND_FILENAME | SND_NODEFAULT | SND_SYNC,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::other("PlaySound failed"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn system_play(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("afplay")
        .arg(path)
        .status()
        .map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_play(path: &Path) -> std::io::Result<()> {
    // PulseAudio / PipeWire first, then ALSA, then libcanberra.
    let tries: [(&str, &[&str]); 3] = [
        ("paplay", &[]),
        ("aplay", &["-q"]),
        ("canberra-gtk-play", &["-f"]),
    ];
    let mut last = std::io::Error::other("no sound player found");
    for (program, args) in tries {
        match std::process::Command::new(program)
            .args(args)
            .arg(path)
            .status()
        {
            Ok(s) if s.success() => return Ok(()),
            Ok(s) => last = std::io::Error::other(format!("{program}: {s}")),
            Err(e) => last = e,
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chime_is_a_wav_file() {
        assert_eq!(&CHIME[..4], b"RIFF");
        assert_eq!(&CHIME[8..12], b"WAVE");
    }
}
