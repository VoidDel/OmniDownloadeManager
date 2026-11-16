//! HTTP and HTTPS engine of Omni Download (docs/development-plan.md §4.2).
//!
//! Probes size, range support and file name, splits the file into segments downloaded over
//! several connections, hands the rest of a slow segment to idle connections (work stealing),
//! and resumes after a pause or a restart when the server's ETag / Last-Modified / length are
//! unchanged. Servers that ignore ranges get one connection and start over after a pause.
//! An HLS playlist (`.m3u8`) is downloaded segment by segment into one video file (`hls`).

mod auth;
mod client;
mod download;
mod form;
mod hls;
mod mp4;
mod probe;
mod ua;

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
        let mut url = parse_url(source)?;
        // Playlists and their parts log in with the account in their link.
        let mut with_account = url.clone();
        auth::embed(options, &mut with_account);
        // Picture and sound apart (a site's DASH streams): one MP4 of both.
        if let Some(audio) = options.audio_source.as_deref() {
            let client = client::build_as(options, &ua::preferred(options, &url))?;
            return hls::probe_pair(&client, &with_account, audio).await;
        }
        if hls::is_playlist_url(&url) {
            let client = client::build_as(options, &ua::preferred(options, &url))?;
            return playlist(&client, &with_account, options).await;
        }
        let auth = std::sync::Arc::new(auth::Auth::new(options, &mut url));
        let form = form::Form::of(options)?.map(std::sync::Arc::new);
        // Another user agent when the server refuses this one.
        let (client, p) = ua::probe_with(options, &url, |c| {
            let (url, auth, form) = (url.clone(), auth.clone(), form.clone());
            async move {
                match form {
                    // The form is sent; its answer (the file) waits for the download.
                    Some(f) => {
                        let a = f.submit(&c, &auth, &url).await?;
                        let p = a.probe.clone();
                        form::keep(&url, &f, a);
                        Ok(p)
                    }
                    None => probe::probe(&c, &auth, &url).await,
                }
            }
        })
        .await?;
        if hls::is_playlist_type(p.content_type.as_deref()) {
            return playlist(&client, &with_account, options).await;
        }
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

/// A playlist: its video file's name; the size is learned while downloading.
async fn playlist(client: &reqwest::Client, url: &Url, o: &JobOptions) -> Result<ProbeInfo> {
    let (_, file_name) = hls::probe(client, url, None, o.hls_variant.as_deref()).await?;
    Ok(ProbeInfo {
        file_name,
        final_url: auth::without_account(url.clone()).to_string(),
        total: None,
        resumable: true,
        content_type: Some("application/vnd.apple.mpegurl".into()),
        etag: None,
        last_modified: None,
        files: Vec::new(),
    })
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
