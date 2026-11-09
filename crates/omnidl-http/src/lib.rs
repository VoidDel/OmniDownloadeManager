//! HTTP and HTTPS engine of Omni Download (docs/development-plan.md §4.2).
//!
//! Probes size, range support and file name, splits the file into segments downloaded over
//! several connections, hands the rest of a slow segment to idle connections (work stealing),
//! and resumes after a pause or a restart when the server's ETag / Last-Modified / length are
//! unchanged. Servers that ignore ranges get one connection and start over after a pause.

mod client;
mod download;
mod probe;

use std::time::Duration;

use async_trait::async_trait;
use omnidl_core::{
    DownloadEngine, Error, ErrorCode, Job, JobOptions, Outcome, ProbeInfo, Result, TaskKind,
};
use url::Url;

/// The HTTP engine. Stateless: each download builds its own client.
#[derive(Debug, Clone)]
pub struct HttpEngine {
    /// First delay before retrying a failed connection; doubles up to a minute.
    retry_delay: Duration,
}

impl Default for HttpEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpEngine {
    pub fn new() -> Self {
        Self {
            retry_delay: Duration::from_secs(1),
        }
    }

    /// Shorter retries (tests).
    pub fn with_retry_delay(retry_delay: Duration) -> Self {
        Self { retry_delay }
    }
}

#[async_trait]
impl DownloadEngine for HttpEngine {
    fn kind(&self) -> TaskKind {
        TaskKind::Http
    }

    async fn probe(&self, source: &str, options: &JobOptions) -> Result<ProbeInfo> {
        let url = parse_url(source)?;
        let client = client::build(options)?;
        let p = probe::probe(&client, &url).await?;
        Ok(ProbeInfo {
            file_name: p.file_name(&url),
            final_url: p.final_url.to_string(),
            total: p.total,
            resumable: p.resumable,
            content_type: p.content_type,
            etag: p.etag,
            last_modified: p.last_modified,
            files: Vec::new(),
        })
    }

    async fn run(&self, job: Job) -> Outcome {
        download::run(self.retry_delay, job).await
    }
}

fn parse_url(source: &str) -> Result<Url> {
    let url = Url::parse(source.trim())
        .map_err(|e| Error::new(ErrorCode::InvalidUrl, format!("{source}: {e}")))?;
    match url.scheme() {
        "http" | "https" if url.host().is_some() => Ok(url),
        _ => Err(Error::new(
            ErrorCode::InvalidUrl,
            format!("not an HTTP link: {source}"),
        )),
    }
}
