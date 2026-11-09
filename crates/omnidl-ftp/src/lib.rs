//! FTP and FTPS engine of Omni Download (docs/development-plan.md §4.3).
//!
//! Logs in (anonymous or with credentials), reads SIZE / MDTM, downloads segments over parallel
//! sessions with REST, and resumes like the HTTP engine, sharing its segment table and writer.
//!
//! Lands in Phase 4 (`suppaftp` with rustls).
