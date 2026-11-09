//! The download domain of Omni Download (docs/development-plan.md §4.1, §4.5).
//!
//! - `Task`, `TaskStatus` and the state machine (queued, probing, downloading, paused, seeding,
//!   completed, failed, removed).
//! - The `DownloadEngine` / `TaskHandle` traits the protocol crates implement.
//! - The scheduler (priority queue, concurrency slots, retries), token-bucket rate limiters and
//!   the event bus the app relays to the window.
//!
//! Lands in Phase 1. Builds on `omnidl-storage`; the engines build on this crate.
