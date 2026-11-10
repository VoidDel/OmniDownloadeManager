//! FTP and FTPS engine of Omni Download (docs/development-plan.md §4.3).
//!
//! Logs in (anonymous, or with the task's account), reads `SIZE` / `MDTM`, and downloads with
//! several sessions at once, each fetching its part with `REST` + `RETR`, sharing the segment
//! table, writer and checkpoints of the HTTP engine's design (ADR-003). `ftps://` secures the
//! control and data connections with `AUTH TLS` (port 990: implicit TLS). Resuming checks that
//! the size and modification time are unchanged. A link ending with `/` is a folder: probing it
//! lists its files (`folder`), and the app adds a task for each.

mod download;
mod folder;
mod session;

pub use folder::{FolderFile, MAX_FILES, folder_name, is_folder, list as list_folder};

use std::time::Duration;

use async_trait::async_trait;
use omnidl_core::{
    DownloadEngine, ErrorCode, Job, JobOptions, Outcome, ProbeFile, ProbeInfo, Result, TaskKind,
};

use session::{Ftp, Target, timed};

pub struct FtpEngine {
    retry_delay: Duration,
}

impl FtpEngine {
    pub fn new() -> Self {
        Self::with_retry_delay(Duration::from_secs(1))
    }

    /// The first retry waits `delay`, then twice as long each time (tests use a short one).
    pub fn with_retry_delay(delay: Duration) -> Self {
        Self { retry_delay: delay }
    }
}

impl Default for FtpEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DownloadEngine for FtpEngine {
    fn kind(&self) -> TaskKind {
        TaskKind::Ftp
    }

    async fn probe(&self, source: &str, options: &JobOptions) -> Result<ProbeInfo> {
        // A folder: its files, to choose from (adding it makes a task per file).
        if is_folder(source) {
            let files = list_folder(source, options).await?;
            return Ok(ProbeInfo {
                final_url: source.trim().to_owned(),
                file_name: folder_name(source),
                total: Some(files.iter().map(|f| f.size).sum()),
                resumable: true,
                content_type: None,
                etag: None,
                last_modified: None,
                files: files
                    .into_iter()
                    .map(|f| ProbeFile {
                        path: f.path,
                        size: f.size,
                    })
                    .collect(),
            });
        }
        let target = Target::parse(source, options)?;
        let mut ftp = session::open(&target, options).await?;
        let p = probe(&mut ftp, &target, options).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), ftp.quit()).await;
        let p = p?;
        Ok(ProbeInfo {
            final_url: target.url.clone(),
            file_name: target.file_name(),
            total: p.total,
            resumable: p.resumable,
            content_type: None,
            etag: None,
            last_modified: p.modified,
            files: Vec::new(),
        })
    }

    async fn run(&self, job: Job) -> Outcome {
        download::run(self.retry_delay, job).await
    }
}

/// What the server says about the file.
pub(crate) struct Probe {
    pub total: Option<u64>,
    /// `MDTM` as Unix seconds.
    pub modified: Option<String>,
    /// `REST` can continue it (the size is known; a server refusing `REST` makes the download
    /// start over on one session).
    pub resumable: bool,
}

pub(crate) async fn probe(ftp: &mut Ftp, t: &Target, o: &JobOptions) -> Result<Probe> {
    let total = match timed(o.read_timeout, ftp.size(t.path.as_str())).await {
        Ok(n) => Some(n as u64),
        // SIZE not implemented: the length is learned at the end.
        Err(e) if e.code == ErrorCode::FtpReply => None,
        Err(e) => return Err(e),
    };
    let modified = timed(o.read_timeout, ftp.mdtm(t.path.as_str()))
        .await
        .ok()
        .map(|t| t.and_utc().timestamp().to_string());
    Ok(Probe {
        total,
        modified,
        resumable: total.is_some(),
    })
}
