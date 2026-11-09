//! Persistence and file writing for Omni Download (docs/development-plan.md §4.6, §4.2.4).
//!
//! - SQLite repositories for tasks, segments and torrent metadata (`sqlx`, WAL), with migrations.
//! - The writer actor: one task per download that receives `(offset, bytes)` from the
//!   connections, writes them in order, preallocates the file and renames it when complete.
//!
//! Lands in Phase 1. Depends on nothing else in the workspace; every other crate builds on it.
