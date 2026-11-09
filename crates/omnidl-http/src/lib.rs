//! HTTP and HTTPS engine of Omni Download (docs/development-plan.md §4.2).
//!
//! Probes size, range support and file name, splits the file into segments downloaded over
//! several connections, hands finished ranges to idle connections (work stealing), and resumes
//! after a restart when the server's ETag / Last-Modified / length are unchanged.
//!
//! Lands in Phase 1 (`reqwest` with rustls).
