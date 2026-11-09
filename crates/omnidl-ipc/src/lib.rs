//! The protocol between the browser bridge and the Omni Download app (docs/development-plan.md
//! §5.5, appendix A; ADR-002).
//!
//! ```text
//! extension ──stdio──► omnidl-nm-host ──named pipe / Unix socket──► app
//! extension ──HTTP 127.0.0.1 + pairing token (fallback)───────────► app
//! ```
//!
//! - [`Request`] / [`Response`]: JSON messages, the same on every leg. The extension's TypeScript
//!   copy is `apps/extension/lib/protocol.ts`.
//! - [`read_frame`] / [`write_frame`]: a 4-byte little-endian length, then the JSON — Native
//!   Messaging's framing, reused on the local pipe.
//! - [`endpoint`] / [`connect`]: the per-user pipe or socket the app listens on, and the blocking
//!   client the host uses. With the `tokio` feature, [`Listener`] is the app's side.

mod endpoint;
mod frame;
mod protocol;
#[cfg(feature = "tokio")]
mod server;

pub use endpoint::{ENV_ENDPOINT, Stream, connect, endpoint};
pub use frame::{MAX_REPLY, MAX_REQUEST, read_frame, write_frame};
pub use protocol::{
    AddAction, AddBatch, AddDownload, AddMagnet, AddResult, Body, BridgeError, ErrorCode,
    HOST_NAME, Pong, RecentTask, Request, Response, Status,
};
#[cfg(feature = "tokio")]
pub use server::{Listener, read_frame_async, write_frame_async};
