//! Where the app listens, per user (ADR-002): a named pipe on Windows, a Unix socket elsewhere.
//! Only the user who started the app can connect: the pipe keeps its default security (full
//! access for its owner, administrators and SYSTEM; others may only read, which is not enough to
//! talk) and refuses remote clients; the socket is 0600 in a 0700 folder.

use std::io;

/// Overrides the endpoint (tests; several app builds side by side).
pub const ENV_ENDPOINT: &str = "OMNIDL_IPC_ENDPOINT";

/// The app's endpoint for the current user: `$OMNIDL_IPC_ENDPOINT`, else the default.
pub fn endpoint() -> String {
    match std::env::var(ENV_ENDPOINT) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default_endpoint(),
    }
}

/// FNV-1a: a stable hash for a name (not a secret).
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `\\.\pipe\omnidl-<hash of DOMAIN\user>`: pipe names are machine-wide, so the user is in it.
#[cfg(windows)]
fn default_endpoint() -> String {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    format!(r"\\.\pipe\omnidl-{:016x}", fnv(&user.to_lowercase()))
}

/// Linux: `$XDG_RUNTIME_DIR/omnidl.sock` (per user, 0700), else `~/.local/share/omnidl/`.
/// macOS: `~/Library/Application Support/app.omnidownload.desktop/omnidl.sock`.
#[cfg(unix)]
fn default_endpoint() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let user = std::env::var("USER").unwrap_or_default();
    if cfg!(target_os = "macos") {
        return format!("{home}/Library/Application Support/app.omnidownload.desktop/omnidl.sock");
    }
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => format!("{dir}/omnidl.sock"),
        _ if !home.is_empty() => format!("{home}/.local/share/omnidl/omnidl.sock"),
        _ => format!("/tmp/omnidl-{:016x}.sock", fnv(&user)),
    }
}

/// A connection to the app.
#[cfg(windows)]
pub type Stream = std::fs::File;
#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;

/// Connects to the app (blocking). Fails at once when it is not listening.
#[cfg(windows)]
pub fn connect(endpoint: &str) -> io::Result<Stream> {
    // All instances busy (another host just connected): the app creates the next one at once.
    const ERROR_PIPE_BUSY: i32 = 231;
    let mut tries = 0;
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(endpoint)
        {
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && tries < 40 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            r => return r,
        }
    }
}

#[cfg(unix)]
pub fn connect(endpoint: &str) -> io::Result<Stream> {
    std::os::unix::net::UnixStream::connect(endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_per_user_endpoint() {
        let e = default_endpoint();
        if cfg!(windows) {
            assert!(e.starts_with(r"\\.\pipe\omnidl-"), "{e}");
        } else {
            assert!(e.ends_with(".sock"), "{e}");
        }
        assert_eq!(default_endpoint(), e, "stable");
        assert_ne!(fnv("a\\b"), fnv("a\\c"));
    }
}
