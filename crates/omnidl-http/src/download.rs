//! One run of an HTTP download: settle the file, run the connections, save progress, finish.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use omnidl_core::{
    ConflictPolicy, Error, ErrorCode, Job, Outcome, Result, ResumeState, SegmentTable, filename,
    segments,
};
use omnidl_storage::{FileWriter, file};
use parking_lot::Mutex;
use reqwest::{
    Client, StatusCode,
    header::{CONTENT_RANGE, RANGE},
};
use tokio::{task::JoinSet, time::Instant};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    client::{self, request_error, status_error},
    parse_url,
    probe::{self, Probe},
};

/// How often the segment map is refreshed for the UI.
const UI_TICK: Duration = Duration::from_millis(250);
/// How often progress is saved (a crash loses at most this much).
const SAVE_EVERY: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

pub(crate) async fn run(retry_delay: Duration, job: Job) -> Outcome {
    let job = Arc::new(job);
    match prepare(retry_delay, &job).await {
        Ok(download) => download.transfer().await,
        Err(_) if job.cancel.is_cancelled() => {
            Outcome::Stopped(job.resume.clone().unwrap_or_default())
        }
        Err(e) => Outcome::Failed(e, job.resume.clone()),
    }
}

/// Probes, settles name and partial file, and reports the start.
async fn prepare(retry_delay: Duration, job: &Arc<Job>) -> Result<Download> {
    let url = parse_url(&job.source)?;
    let client = client::build(&job.options)?;
    let probe = probe_retrying(retry_delay, job, &client, &url).await?;
    let dir = job.save_dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&e, dir.display()))?;

    let total = probe.total;
    let mut resumable = probe.resumable;
    let connections = |resumable: bool| {
        if resumable {
            job.options.connections
        } else {
            1
        }
    };
    let (file_name, file, plan) = match &job.resume {
        Some(r) => {
            let partial = dir.join(filename::partial(&r.file_name));
            let resuming = r.resumable && r.downloaded() > 0;
            if resuming
                && let Some(why) = probe::changed(
                    &probe,
                    r.total,
                    r.etag.as_deref(),
                    r.last_modified.as_deref(),
                )
            {
                return Err(probe::remote_changed(why));
            }
            let reopened = if resuming && resumable {
                match file::open_existing(&partial) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        tracing::warn!("{}: {e}; starting over", partial.display());
                        None
                    }
                }
            } else {
                None
            };
            match reopened {
                Some(f) => (r.file_name.clone(), f, r.segments.clone()),
                None => {
                    ensure_space(&dir, total)?;
                    let f = file::create(&partial, total)
                        .map_err(|e| Error::io(&e, partial.display()))?;
                    (
                        r.file_name.clone(),
                        f,
                        segments::plan(total, connections(resumable)),
                    )
                }
            }
        }
        None => {
            ensure_space(&dir, total)?;
            let base = job
                .file_name
                .clone()
                .unwrap_or_else(|| probe.file_name(&url));
            let (name, f) = claim_name(&dir, &base, total, job.options.conflict)?;
            (name, f, segments::plan(total, connections(resumable)))
        }
    };
    if total.is_none() {
        resumable = false;
    }
    let writer = FileWriter::spawn(file).map_err(|e| Error::io(&e, "file writer"))?;
    let download = Download {
        job: job.clone(),
        client,
        url: probe.final_url.clone(),
        dir,
        retry_delay,
        state: ResumeState {
            file_name,
            final_url: Some(probe.final_url.to_string()),
            total,
            etag: probe.etag,
            last_modified: probe.last_modified,
            resumable,
            segments: Vec::new(),
            completed_at: None,
        },
        table: Arc::new(Mutex::new(SegmentTable::new(plan))),
        writer,
        last_saved: None,
    };
    job.progress.set_total(total);
    job.progress
        .set_downloaded(download.table.lock().downloaded());
    job.checkpoint.started(&download.snapshot()).await?;
    Ok(download)
}

async fn probe_retrying(
    retry_delay: Duration,
    job: &Job,
    client: &Client,
    url: &Url,
) -> Result<Probe> {
    let mut attempt = 0;
    loop {
        let r = tokio::select! {
            r = probe::probe(client, url) => r,
            _ = job.cancel.cancelled() => return Err(Error::internal("cancelled")),
        };
        match r {
            Err(e) if e.is_transient() && attempt < job.options.max_retries => {
                attempt += 1;
                tracing::debug!("probing {url} (attempt {attempt}): {e}");
                tokio::select! {
                    _ = tokio::time::sleep(backoff(retry_delay, attempt)) => {}
                    _ = job.cancel.cancelled() => return Err(Error::internal("cancelled")),
                }
            }
            r => return r,
        }
    }
}

/// Fails early when the file cannot fit.
fn ensure_space(dir: &Path, total: Option<u64>) -> Result<()> {
    let Some(total) = total else {
        return Ok(());
    };
    match file::available_space(dir) {
        Ok(free) if free < total => Err(Error::new(
            ErrorCode::DiskFull,
            format!("{total} bytes needed, {free} free in {}", dir.display()),
        )),
        _ => Ok(()),
    }
}

/// Picks the first free name `base`, `base (1)`… and creates its partial file. A name is taken
/// when another download has its partial file, or (when renaming) the file exists.
fn claim_name(
    dir: &Path,
    base: &str,
    total: Option<u64>,
    conflict: ConflictPolicy,
) -> Result<(String, std::fs::File)> {
    for n in 0..10_000 {
        let name = if n == 0 {
            base.to_owned()
        } else {
            filename::numbered(base, n)
        };
        if conflict == ConflictPolicy::Rename && dir.join(&name).exists() {
            continue;
        }
        let partial = dir.join(filename::partial(&name));
        match file::create_new(&partial, total) {
            Ok(f) => return Ok((name, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io(&e, partial.display())),
        }
    }
    Err(Error::internal(format!("no free name for {base}")))
}

fn backoff(base: Duration, attempt: u32) -> Duration {
    base.saturating_mul(1 << attempt.saturating_sub(1).min(16))
        .min(MAX_BACKOFF)
}

/// Why the connections stopped.
enum Halt {
    Cancelled,
    /// The server ignored a range: start over on one connection.
    Fallback,
    Failed(Error),
}

/// A connection's end.
enum ConnError {
    Fallback,
    /// Stops the whole download (404, disk full…).
    Fatal(Error),
    /// This connection gave up; others may take its segment.
    GaveUp(Error),
}

/// A request's end.
enum FetchError {
    Cancelled,
    Fallback,
    Failed(Error),
}

struct Download {
    job: Arc<Job>,
    client: Client,
    url: Url,
    dir: PathBuf,
    retry_delay: Duration,
    /// Everything but the segments, which live in `table`.
    state: ResumeState,
    table: Arc<Mutex<SegmentTable>>,
    writer: FileWriter,
    last_saved: Option<ResumeState>,
}

impl Download {
    fn snapshot(&self) -> ResumeState {
        ResumeState {
            segments: self.table.lock().segments(),
            ..self.state.clone()
        }
    }

    async fn transfer(mut self) -> Outcome {
        loop {
            match self.connections().await {
                Err(Halt::Fallback) if self.state.resumable => {
                    tracing::info!("{} ignores ranges: one connection", self.url);
                    self.state.resumable = false;
                    *self.table.lock() = SegmentTable::new(segments::plan(self.state.total, 1));
                    self.job.progress.set_downloaded(0);
                    if let Err(e) = self.job.checkpoint.started(&self.snapshot()).await {
                        return self.stop(Halt::Failed(e)).await;
                    }
                }
                Err(halt) => return self.stop(halt).await,
                Ok(()) => return self.complete().await,
            }
        }
    }

    /// Runs the connections until the table is complete, the job is cancelled or a connection
    /// fails for good. Saves progress every second meanwhile.
    async fn connections(&mut self) -> Result<(), Halt> {
        let stop = self.job.cancel.child_token();
        let conn = Arc::new(Conn {
            job: self.job.clone(),
            client: self.client.clone(),
            url: self.url.clone(),
            table: self.table.clone(),
            writer: self.writer.clone(),
            ranged: self.state.resumable,
            stop: stop.clone(),
            open: AtomicU32::new(0),
            retry_delay: self.retry_delay,
        });
        let count = if conn.ranged {
            self.job.options.connections
        } else {
            1
        };
        let mut set = JoinSet::new();
        {
            let mut t = self.table.lock();
            for _ in 0..count {
                let Some(idx) = t.claim() else { break };
                set.spawn(conn.clone().work(idx));
            }
        }
        let mut tick = tokio::time::interval(UI_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_save = Instant::now();
        let mut halt: Option<Halt> = None;
        let mut gave_up: Option<Error> = None;
        loop {
            tokio::select! {
                joined = set.join_next() => match joined {
                    None => break,
                    Some(Ok(Ok(()))) => {}
                    Some(Ok(Err(ConnError::GaveUp(e)))) => gave_up = Some(e),
                    Some(Ok(Err(ConnError::Fallback))) => {
                        halt.get_or_insert(Halt::Fallback);
                        stop.cancel();
                    }
                    Some(Ok(Err(ConnError::Fatal(e)))) => {
                        halt.get_or_insert(Halt::Failed(e));
                        stop.cancel();
                    }
                    Some(Err(e)) => {
                        halt.get_or_insert(Halt::Failed(Error::internal(e)));
                        stop.cancel();
                    }
                },
                _ = tick.tick() => {
                    self.publish();
                    if last_save.elapsed() >= SAVE_EVERY {
                        last_save = Instant::now();
                        if let Err(e) = self.save().await {
                            halt.get_or_insert(Halt::Failed(e));
                            stop.cancel();
                        }
                    }
                }
            }
        }
        self.publish();
        self.job.progress.set_connections(0);
        if let Some(h) = halt {
            return Err(h);
        }
        if self.job.cancel.is_cancelled() {
            return Err(Halt::Cancelled);
        }
        if !self.table.lock().is_complete() {
            return Err(Halt::Failed(gave_up.unwrap_or_else(|| {
                Error::new(ErrorCode::Network, "every connection closed early")
            })));
        }
        Ok(())
    }

    fn publish(&self) {
        let views = self.table.lock().views();
        self.job.progress.set_segments(views);
    }

    /// Flushes the file, then saves the segments as they were before the flush.
    async fn save(&mut self) -> Result<()> {
        let state = self.snapshot();
        self.writer
            .flush(self.job.options.sync_writes)
            .await
            .map_err(|e| Error::io(&e, "writing the file"))?;
        self.job.checkpoint.save(&state).await?;
        self.last_saved = Some(state);
        Ok(())
    }

    /// Stopped or failed: what is on disk, for the manager to save.
    async fn stop(mut self, halt: Halt) -> Outcome {
        let mut state = self.snapshot();
        match self.writer.close().await {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!("closing the partial file: {e}");
                state = self.last_saved.take().unwrap_or_else(|| ResumeState {
                    segments: Vec::new(),
                    ..state
                });
            }
        }
        if !state.resumable {
            // Nothing to continue from without ranges.
            state.segments.clear();
        }
        match halt {
            Halt::Cancelled => Outcome::Stopped(state),
            Halt::Failed(e) => Outcome::Failed(e, Some(state)),
            Halt::Fallback => Outcome::Failed(
                Error::new(ErrorCode::HttpStatus, "the server ignored a range request"),
                Some(state),
            ),
        }
    }

    /// Every byte is in: close, rename into place, date it.
    async fn complete(self) -> Outcome {
        let state = self.snapshot();
        let got = state.downloaded();
        if let Some(total) = state.total
            && total != got
        {
            let e = Error::internal(format!("downloaded {got} of {total} bytes"));
            return Outcome::Failed(e, Some(state));
        }
        if let Err(e) = self.writer.close().await {
            return Outcome::Failed(Error::io(&e, "writing the file"), Some(state));
        }
        let partial = self.dir.join(filename::partial(&state.file_name));
        let mut name = state.file_name.clone();
        if self.job.options.conflict == ConflictPolicy::Rename && self.dir.join(&name).exists() {
            // Someone created the file meanwhile.
            if let Some(free) = (1..10_000)
                .map(|n| filename::numbered(&state.file_name, n))
                .find(|n| {
                    !self.dir.join(n).exists() && !self.dir.join(filename::partial(n)).exists()
                })
            {
                name = free;
            }
        }
        let target = self.dir.join(&name);
        if let Err(e) = std::fs::rename(&partial, &target) {
            return Outcome::Failed(Error::io(&e, target.display()), Some(state));
        }
        if let Some(t) = state
            .last_modified
            .as_deref()
            .and_then(|v| httpdate::parse_http_date(v).ok())
            && let Ok(f) = std::fs::OpenOptions::new().write(true).open(&target)
        {
            let _ = f.set_modified(t);
        }
        Outcome::Completed {
            file_name: name,
            total: got,
        }
    }
}

/// What every connection of a download shares.
struct Conn {
    job: Arc<Job>,
    client: Client,
    url: Url,
    table: Arc<Mutex<SegmentTable>>,
    writer: FileWriter,
    /// Requests carry `Range` (the server supports it and the length is known).
    ranged: bool,
    stop: CancellationToken,
    /// Requests with a body streaming.
    open: AtomicU32,
    retry_delay: Duration,
}

impl Conn {
    /// One connection: its segment, then whatever it can claim next.
    async fn work(self: Arc<Self>, mut idx: usize) -> Result<(), ConnError> {
        loop {
            let r = self.segment(idx).await;
            self.table.lock().release(idx);
            r?;
            if self.stop.is_cancelled() {
                return Ok(());
            }
            match self.table.lock().claim() {
                Some(next) => idx = next,
                None => return Ok(()),
            }
        }
    }

    /// Downloads segment `idx` until it ends, retrying transient failures with backoff.
    async fn segment(&self, idx: usize) -> Result<(), ConnError> {
        let mut attempt = 0;
        loop {
            if self.stop.is_cancelled() {
                return Ok(());
            }
            let before = self.position(idx);
            let e = match self.fetch(idx).await {
                Ok(()) | Err(FetchError::Cancelled) => return Ok(()),
                Err(FetchError::Fallback) => return Err(ConnError::Fallback),
                Err(FetchError::Failed(e)) => e,
            };
            if !e.is_transient() {
                return Err(ConnError::Fatal(e));
            }
            if self.position(idx) > before {
                attempt = 0;
            }
            // A server limiting connections: leave the segment to the others.
            if matches!(e.status, Some(429 | 503)) && self.table.lock().active() > 1 {
                tracing::debug!("{}: {e}; one connection fewer", self.url);
                return Err(ConnError::GaveUp(e));
            }
            attempt += 1;
            if attempt > self.job.options.max_retries {
                return Err(ConnError::GaveUp(e));
            }
            tracing::debug!("{} (retry {attempt}): {e}", self.url);
            tokio::select! {
                _ = tokio::time::sleep(backoff(self.retry_delay, attempt)) => {}
                _ = self.stop.cancelled() => return Ok(()),
            }
        }
    }

    fn position(&self, idx: usize) -> u64 {
        let t = self.table.lock();
        if idx < t.len() {
            t.segment(idx).pos()
        } else {
            0
        }
    }

    /// One request for the rest of segment `idx`.
    async fn fetch(&self, idx: usize) -> Result<(), FetchError> {
        let (start, end) = {
            let mut t = self.table.lock();
            if !self.ranged && t.segment(idx).downloaded > 0 {
                // Without ranges a broken connection means starting over.
                t.reset(idx);
                self.job.progress.set_downloaded(t.downloaded());
            }
            let s = t.segment(idx);
            if s.is_done() {
                return Ok(());
            }
            (s.pos(), s.end)
        };
        let mut req = self.client.get(self.url.clone());
        if self.ranged {
            let range = match end {
                Some(e) => format!("bytes={start}-{e}"),
                None => format!("bytes={start}-"),
            };
            req = req.header(RANGE, range);
        }
        let resp = tokio::select! {
            r = req.send() => r.map_err(|e| FetchError::Failed(request_error(&e)))?,
            _ = self.stop.cancelled() => return Err(FetchError::Cancelled),
        };
        match resp.status() {
            StatusCode::PARTIAL_CONTENT => {
                let got = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(probe::content_range_start);
                if got.is_some_and(|g| g != start) {
                    return Err(FetchError::Fallback);
                }
            }
            // The whole file: fine from the start, a fallback anywhere else.
            StatusCode::OK if self.ranged && start > 0 => return Err(FetchError::Fallback),
            StatusCode::OK => {}
            StatusCode::RANGE_NOT_SATISFIABLE => {
                return Err(FetchError::Failed(probe::remote_changed(
                    "the server no longer has these bytes".into(),
                )));
            }
            s => return Err(FetchError::Failed(status_error(s))),
        }
        let _open = Open::new(self);
        let mut offset = start;
        let mut body = resp.bytes_stream();
        loop {
            let next = tokio::select! {
                n = body.next() => n,
                _ = self.stop.cancelled() => return Err(FetchError::Cancelled),
            };
            let chunk = match next {
                Some(Ok(c)) => c,
                Some(Err(e)) => return Err(FetchError::Failed(request_error(&e))),
                None => {
                    let mut t = self.table.lock();
                    let s = t.segment(idx);
                    if s.end.is_none() {
                        t.finish_unknown(idx);
                        return Ok(());
                    }
                    if s.is_done() {
                        return Ok(());
                    }
                    return Err(FetchError::Failed(Error::new(
                        ErrorCode::Network,
                        "the server closed the connection early",
                    )));
                }
            };
            let len = chunk.len() as u64;
            tokio::select! {
                _ = self.job.throttle(len) => {}
                _ = self.stop.cancelled() => return Err(FetchError::Cancelled),
            }
            let n = self.table.lock().reserve(idx, len);
            if n > 0 {
                if let Err(e) = self.writer.write(offset, chunk.slice(..n as usize)).await {
                    return Err(FetchError::Failed(Error::io(&e, "writing the file")));
                }
                self.table.lock().commit(idx, n);
                self.job.progress.add(n);
                offset += n;
            }
            if n < len {
                // The segment ends here (it reached its end or was split).
                return Ok(());
            }
        }
    }
}

/// Counts a streaming request in the task's connection number while alive.
struct Open<'a>(&'a Conn);

impl<'a> Open<'a> {
    fn new(c: &'a Conn) -> Self {
        let n = c.open.fetch_add(1, Ordering::Relaxed) + 1;
        c.job.progress.set_connections(n);
        Self(c)
    }
}

impl Drop for Open<'_> {
    fn drop(&mut self) {
        let n = self.0.open.fetch_sub(1, Ordering::Relaxed) - 1;
        self.0.job.progress.set_connections(n);
    }
}
