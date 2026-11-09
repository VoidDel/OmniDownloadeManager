//! BitTorrent and magnet-link engine of Omni Download (docs/development-plan.md §4.4).
//!
//! An adapter over one `librqbit` session: fetches magnet metadata, lets the user pick files,
//! maps torrent statistics onto tasks, and applies the seeding policy (ratio or time). The rest
//! of the app never sees `librqbit` types, so the engine can be replaced.
//!
//! Lands in Phase 3.
