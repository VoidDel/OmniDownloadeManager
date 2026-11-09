//! Native Messaging host for the Omni Download browser extensions (docs/development-plan.md §5.5,
//! ADR-002).
//!
//! The browser starts it for an extension and talks to it over stdin / stdout (4-byte length +
//! JSON). Each message is relayed to the app over its local pipe / socket and the app's reply is
//! written back. A message that needs the app (a download, "open Omni") starts it in the
//! background when it is not running; status queries never do. The host exits when the browser
//! closes stdin.

use std::{
    io,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use omnidl_ipc::{
    ErrorCode, MAX_REPLY, MAX_REQUEST, Response, Stream, connect, read_frame, write_frame,
};
use serde::Deserialize;

/// Where the app is, instead of next to the host (tests).
const ENV_APP: &str = "OMNIDL_APP_PATH";
/// How long a started app may take to listen.
const LAUNCH_WAIT: Duration = Duration::from_secs(10);

/// Enough of a request to route it; the rest is the app's business.
#[derive(Deserialize, Default)]
struct Peek {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "type")]
    kind: String,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("omnidl-nm-host: {e}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let endpoint = omnidl_ipc::endpoint();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut app: Option<Stream> = None;
    while let Some(msg) = read_frame(&mut input, MAX_REQUEST)? {
        let reply = relay(&endpoint, &mut app, &msg);
        write_frame(&mut output, &reply)?;
    }
    Ok(())
}

fn relay(endpoint: &str, app: &mut Option<Stream>, msg: &[u8]) -> Vec<u8> {
    let peek: Peek = serde_json::from_slice(msg).unwrap_or_default();
    let launch = !matches!(peek.kind.as_str(), "ping" | "get_status");
    if launch {
        allow_foreground();
    }
    let fail = |code, message: String| Response::err(peek.id.clone(), code, message).to_bytes();
    // A second try only when the message never reached the app (it had closed the connection).
    for _ in 0..2 {
        if app.is_none() {
            *app = match connect(endpoint) {
                Ok(s) => Some(s),
                Err(_) if launch => match start_app(endpoint) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        return fail(
                            ErrorCode::AppNotRunning,
                            format!("could not start Omni Download: {e}"),
                        );
                    }
                },
                Err(_) => {
                    return fail(
                        ErrorCode::AppNotRunning,
                        "Omni Download is not running".into(),
                    );
                }
            };
        }
        let Some(conn) = app.as_mut() else { break };
        if write_frame(conn, msg).is_err() {
            *app = None;
            continue;
        }
        return match read_frame(conn, MAX_REPLY) {
            Ok(Some(reply)) => reply,
            Ok(None) | Err(_) => {
                *app = None;
                fail(
                    ErrorCode::Internal,
                    "Omni Download closed the connection".into(),
                )
            }
        };
    }
    fail(
        ErrorCode::AppNotRunning,
        "lost the connection to Omni Download".into(),
    )
}

/// Windows only lets the foreground process (the browser the user just clicked in) and what it
/// started (this host) bring a window to the front. Passing that on lets the app's
/// quick-download window or main window come up over the browser instead of flashing.
#[cfg(windows)]
fn allow_foreground() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    // SAFETY: no pointers; failure only means the window may not come to the front.
    unsafe { AllowSetForegroundWindow(ASFW_ANY) };
}

#[cfg(not(windows))]
fn allow_foreground() {}

/// The app's executable: next to the host (installed together), or `$OMNIDL_APP_PATH`.
fn app_path() -> io::Result<PathBuf> {
    if let Some(p) = std::env::var_os(ENV_APP) {
        return Ok(p.into());
    }
    let name = if cfg!(windows) {
        "omnidl-desktop.exe"
    } else {
        "omnidl-desktop"
    };
    Ok(std::env::current_exe()?.with_file_name(name))
}

/// Starts the app without a window (`--background`) and waits until it listens.
fn start_app(endpoint: &str) -> io::Result<Stream> {
    let exe = app_path()?;
    if !exe.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} not found", exe.display()),
        ));
    }
    let mut cmd = Command::new(&exe);
    cmd.arg("--background")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    spawn_detached(&mut cmd)?;
    let deadline = Instant::now() + LAUNCH_WAIT;
    loop {
        match connect(endpoint) {
            Ok(s) => return Ok(s),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Windows: out of the browser's job object (closing the port must not kill the app), no
/// console, own process group. Browsers that forbid breaking away get a plain detached start.
#[cfg(windows)]
fn spawn_detached(cmd: &mut Command) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    cmd.creation_flags(base | CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
        .or_else(|_| cmd.creation_flags(base).spawn())
        .map(drop)
}

/// Unix: its own process group, so the browser ending the host's group leaves the app alone.
#[cfg(unix)]
fn spawn_detached(cmd: &mut Command) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0).spawn().map(drop)
}
