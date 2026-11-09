//! The protocol between the browser bridge and the Omni Download app (docs/development-plan.md
//! §5.5, appendix A).
//!
//! Request and response types (`add_download`, `add_magnet`, `get_status` …) exported to
//! TypeScript for the extension, and the length-prefixed JSON framing used both on the Native
//! Messaging stdio pipe and on the app's local socket / named pipe.
//!
//! Lands in Phase 2. Shared by the app and `omnidl-nm-host`.
