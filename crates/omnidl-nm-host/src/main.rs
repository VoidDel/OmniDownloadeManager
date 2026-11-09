//! Native Messaging host for the Omni Download browser extensions (docs/development-plan.md §5.5).
//!
//! The browser starts it for an extension and talks to it over stdin / stdout (4-byte length +
//! JSON). It relays each message to the running app over the local socket / named pipe, starting
//! the app in the background when it is not running, and exits when the browser disconnects.
//!
//! Lands in Phase 2; until then it only reports that it is not available.

fn main() {
    eprintln!(
        "omnidl-nm-host {}: the browser bridge arrives in Phase 2",
        env!("CARGO_PKG_VERSION")
    );
    std::process::exit(2);
}
