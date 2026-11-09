//! Persistence and file writing for Omni Download (docs/development-plan.md §4.6, §4.2.4).
//!
//! - [`Db`]: the SQLite database (`sqlx`, WAL) with its embedded migrations, and the task and
//!   segment repositories. Rows are plain records ([`TaskRecord`], [`SegmentRecord`]); the domain
//!   types live in `omnidl-core`, which builds on this crate and converts.
//! - [`file`]: creates the partial file of a download with its space reserved up front.
//! - [`FileWriter`]: the writer actor. Every connection of a download sends `(offset, bytes)` to
//!   it; one thread writes them with positioned writes, merging adjacent blocks, and syncs to
//!   disk when asked so that saved progress never runs ahead of the data.

mod db;
pub mod file;
mod writer;

pub use db::{Db, SegmentRecord, TaskRecord};
pub use writer::FileWriter;

/// Storage failures.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("database migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
