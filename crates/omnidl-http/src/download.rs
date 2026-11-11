//! One run of an HTTP download: settle the file, run the connections, save progress, finish.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use omnidl_core::{
    Crew, Error, ErrorCode, Job, Outcome, Result, ResumeState, SegmentTable, Shift, Worker,
    conns::{AUTO_START, MAX_CONNECTIONS},
    disk::{backoff, claim_name, ensure_space, place},
    filename, segments,
};
use omnidl_storage::{FileWriter, file};
use parking_lot::Mutex;
use reqwest::{
    Client, StatusCode,
    header::{CONTENT_RANGE, RANGE},
};
use tokio::{task::JoinSet, time::Instant};
use url::Url;

use crate::{
    client::{request_error, status_error},
    hls, parse_url,
    probe::{self, Probe},
};

/// How often the segment map is refreshed for the UI.
const UI_TICK: Duration = Duration::from_millis(250);
/// How often progress is saved (a crash loses at most this much).
const SAVE_EVERY: Duration = Duration::from_secs(1);

pub(crate) async fn run(retry_delay: Duration, job: Job) -> Outcome {
    let job = Arc::new(job);
    // Playlists, and a picture with its sound apart (fetched in parts the same way, joined).
    let playlist = job.options.audio_source.is_some()
        || parse_url(&job.source).is_ok_and(|u| hls::is_playlist_url(&u))
        || job.resume.as_ref().is_some_and(hls::is_hls_state);
    if playlist {
        return hls::run(retry_delay, job).await;
    }
    match prepare(retry_delay, &job).await {
        Ok(Prepared::File(download)) => (*download).transfer().await,
        // A playlist behind a link without `.m3u8` (its Content-Type says so).
        Ok(Prepared::Playlist) => hls::run(retry_delay, job).await,
        Err(_) if job.cancel.is_cancelled() => {
            Outcome::Stopped(job.resume.clone().unwrap_or_default())
        }
        Err(e) => Outcome::Failed(e, job.resume.clone()),
    }
}

enum Prepared {
    File(Box<Download>),
    Playlist,
}

/// Probes, settles name and partial file, and reports the start.
async fn prepare(retry_delay: Duration, job: &Arc<Job>) -> Result<Prepared> {
    let url = parse_url(&job.source)?;
    // Another user agent when the server refuses this one (`ua`).
    let (client, probe) = crate::ua::probe_with(&job.options, &url, |c| {
        let (job, url) = (job.clone(), url.clone());
        async move { probe_retrying(retry_delay, &job, &c, &url).await }
    })
    .await?;
    if hls::is_playlist_type(probe.content_type.as_deref()) {
        return Ok(Prepared::Playlist);
    }
    let dir = job.save_dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&e, dir.display()))?;

    let total = probe.total;
    let mut resumable = probe.resumable;
    // The first segments, one per connection; the user or the tuner change the number later.
    let first = job
        .conns
        .begin(job.options.conn_mode(), MAX_CONNECTIONS, AUTO_START);
    let connections = |resumable: bool| if resumable { first } else { 1 };
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
    // Mirrors serve ranges of the same file: only for a download in segments.
    let mirrors = if resumable && !job.options.mirrors.is_empty() {
        check_mirrors(job, &client, &probe).await
    } else {
        Vec::new()
    };
    let writer = FileWriter::spawn(file).map_err(|e| Error::io(&e, "file writer"))?;
    let download = Download {
        job: job.clone(),
        client,
        url: probe.final_url.clone(),
        mirrors,
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
    Ok(Prepared::File(Box::new(download)))
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

/// The mirrors that serve the same file as `probe` (same length, ranges accepted), asked at
/// once and only once each; the others are left out (and said why in the log).
async fn check_mirrors(job: &Job, client: &Client, probe: &Probe) -> Vec<Url> {
    let asks = job
        .options
        .mirrors
        .iter()
        .filter_map(|m| match parse_url(m) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => Some(u),
            _ => {
                tracing::info!("mirror {m}: not an HTTP link; left out");
                None
            }
        });
    let checks = asks.map(|u| async move {
        let r = tokio::time::timeout(Duration::from_secs(20), probe::probe(client, &u)).await;
        (u, r)
    });
    let mut good = Vec::new();
    for (u, r) in futures_util::future::join_all(checks).await {
        match r {
            Ok(Ok(p)) if p.resumable && p.total == probe.total => good.push(p.final_url),
            Ok(Ok(p)) => tracing::info!(
                "mirror {u}: {} bytes, ranges {}; left out",
                p.total.map_or("?".into(), |t| t.to_string()),
                p.resumable
            ),
            Ok(Err(e)) => tracing::info!("mirror {u}: {e}; left out"),
            Err(_) => tracing::info!("mirror {u}: no answer; left out"),
        }
    }
    if !good.is_empty() {
        tracing::debug!("{}: {} mirror(s) in use", probe.final_url, good.len());
    }
    good
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
    /// A connection started in place of a replaced one gave up (refused, most likely because
    /// the server still counts the old one): not a reason for fewer connections.
    ReplacementGaveUp(Error),
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
    /// Other addresses of the file, checked (see `check_mirrors`).
    mirrors: Vec<Url>,
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
        let mut sources = vec![self.url.clone()];
        if self.state.resumable {
            sources.extend(self.mirrors.iter().cloned());
        }
        let conn = Arc::new(Conn {
            job: self.job.clone(),
            client: self.client.clone(),
            dropped: Mutex::new(vec![false; sources.len()]),
            sources,
            table: self.table.clone(),
            writer: self.writer.clone(),
            ranged: self.state.resumable,
            crew: Crew::new(stop.clone(), self.job.conns.crew_window()),
            next_source: AtomicUsize::new(0),
            open: AtomicU32::new(0),
            retry_delay: self.retry_delay,
        });
        let count = if conn.ranged {
            self.job.conns.target()
        } else {
            1
        };
        let mut set = JoinSet::new();
        conn.spawn(&mut set, count);
        let mut tick = tokio::time::interval(UI_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_save = Instant::now();
        let mut halt: Option<Halt> = None;
        let mut gave_up: Option<Error> = None;
        loop {
            tokio::select! {
                joined = set.join_next() => match joined {
                    None => break,
                    // One ended (done, retired or replaced): its place may need filling.
                    Some(Ok(Ok(()))) => {
                        if conn.ranged && !stop.is_cancelled() {
                            conn.spawn(&mut set, conn.crew.vacancies(self.job.conns.target()));
                        }
                    }
                    Some(Ok(Err(ConnError::ReplacementGaveUp(e)))) => {
                        conn.crew.replacement_refused();
                        gave_up = Some(e);
                        // Its place is taken again at the next tick; at once if it was alone.
                        if set.is_empty() && !stop.is_cancelled() {
                            conn.spawn(&mut set, conn.crew.vacancies(self.job.conns.target()));
                            if set.is_empty() {
                                break;
                            }
                        }
                    }
                    Some(Ok(Err(ConnError::GaveUp(e)))) => {
                        // The server refused one more, or it kept failing: no more than the
                        // others, for a while.
                        // Those still trying count too: on a slow line they may not have an answer yet.
                        self.job
                            .conns
                            .overloaded(conn.crew.running().max(conn.open.load(Ordering::Relaxed)));
                        conn.crew.pause_replacing();
                        gave_up = Some(e);
                        if set.is_empty() {
                            break;
                        }
                    }
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
                    if conn.ranged && !stop.is_cancelled() {
                        // More or fewer connections, and a new one for a slow one.
                        let job = &self.job;
                        job.conns.observe(job.progress.downloaded(), job.speed_limit());
                        let more = conn.crew.rebalance(job.conns.target(), job.conns.limited());
                        conn.spawn(&mut set, more);
                    }
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
        let name = match place(&self.dir, &state.file_name, self.job.options.conflict) {
            Ok(n) => n,
            Err(e) => return Outcome::Failed(e, Some(state)),
        };
        let target = self.dir.join(&name);
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
    /// The link, then the mirrors in use.
    sources: Vec<Url>,
    /// Mirrors left out after they failed (the link never is).
    dropped: Mutex<Vec<bool>>,
    table: Arc<Mutex<SegmentTable>>,
    writer: FileWriter,
    /// Requests carry `Range` (the server supports it and the length is known).
    ranged: bool,
    /// The connections, each stopped with the download or on its own (retired, replaced).
    crew: Arc<Crew>,
    /// Connections take the link and the mirrors in turn.
    next_source: AtomicUsize,
    /// Requests with a body streaming.
    open: AtomicU32,
    retry_delay: Duration,
}

impl Conn {
    fn url(&self) -> &Url {
        &self.sources[0]
    }

    /// Leaves mirror `src` out and gives the source to use instead.
    fn drop_source(&self, src: usize, why: &dyn std::fmt::Display) -> usize {
        let mut dropped = self.dropped.lock();
        if !dropped[src] {
            dropped[src] = true;
            tracing::info!("mirror {}: {why}; left out", self.sources[src]);
        }
        (1..self.sources.len())
            .map(|i| (src + i) % self.sources.len())
            .find(|&i| !dropped[i])
            .unwrap_or(0)
    }

    /// Starts up to `n` connections, each on a segment it claims.
    fn spawn(self: &Arc<Self>, set: &mut JoinSet<Result<(), ConnError>>, n: u32) {
        let mut t = self.table.lock();
        for _ in 0..n {
            let Some(idx) = t.claim() else { break };
            let src = self.next_source.fetch_add(1, Ordering::Relaxed) % self.sources.len();
            set.spawn(self.clone().work(self.crew.join(), idx, src));
        }
    }

    /// One connection: its segment, then whatever it can claim next, from source `src` (the
    /// link or a mirror). A retired connection leaves its segment to the others.
    async fn work(
        self: Arc<Self>,
        shift: Shift,
        mut idx: usize,
        mut src: usize,
    ) -> Result<(), ConnError> {
        let w = shift.worker();
        loop {
            let r = self.segment(w, idx, &mut src).await;
            self.table.lock().release(idx);
            match r {
                Err(ConnError::GaveUp(e)) if w.is_replacement() => {
                    return Err(ConnError::ReplacementGaveUp(e));
                }
                r => r?,
            }
            if w.is_cancelled() {
                return Ok(());
            }
            match self.table.lock().claim() {
                Some(next) => idx = next,
                None => return Ok(()),
            }
        }
    }

    /// Downloads segment `idx` until it ends, retrying transient failures with backoff. A mirror
    /// that fails in any way is left out and the segment goes on from another source.
    async fn segment(&self, w: &Worker, idx: usize, src: &mut usize) -> Result<(), ConnError> {
        let mut attempt = 0;
        loop {
            if w.is_cancelled() {
                return Ok(());
            }
            if self.dropped.lock()[*src] {
                *src = 0;
            }
            let before = self.position(idx);
            let e = match self.fetch(w, idx, *src).await {
                Ok(()) | Err(FetchError::Cancelled) => return Ok(()),
                Err(FetchError::Fallback) if *src != 0 => {
                    *src = self.drop_source(*src, &"ignores ranges");
                    continue;
                }
                Err(FetchError::Fallback) => return Err(ConnError::Fallback),
                Err(FetchError::Failed(e)) if *src != 0 => {
                    *src = self.drop_source(*src, &e);
                    continue;
                }
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
                tracing::debug!("{}: {e}; one connection fewer", self.url());
                return Err(ConnError::GaveUp(e));
            }
            attempt += 1;
            if attempt > self.job.options.max_retries {
                return Err(ConnError::GaveUp(e));
            }
            tracing::debug!("{} (retry {attempt}): {e}", self.url());
            tokio::select! {
                _ = tokio::time::sleep(backoff(self.retry_delay, attempt)) => {}
                _ = w.cancelled() => return Ok(()),
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

    /// One request for the rest of segment `idx`, to source `src`.
    async fn fetch(&self, w: &Worker, idx: usize, src: usize) -> Result<(), FetchError> {
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
        w.set_left(end.map(|e| e + 1 - start));
        let mut req = self.client.get(self.sources[src].clone());
        if self.ranged {
            let range = match end {
                Some(e) => format!("bytes={start}-{e}"),
                None => format!("bytes={start}-"),
            };
            req = req.header(RANGE, range);
        }
        let resp = tokio::select! {
            r = req.send() => r.map_err(|e| FetchError::Failed(request_error(&e)))?,
            _ = w.cancelled() => return Err(FetchError::Cancelled),
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
                _ = w.cancelled() => return Err(FetchError::Cancelled),
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
                _ = w.cancelled() => return Err(FetchError::Cancelled),
            }
            let n = self.table.lock().reserve(idx, len);
            if n > 0 {
                if let Err(e) = self.writer.write(offset, chunk.slice(..n as usize)).await {
                    return Err(FetchError::Failed(Error::io(&e, "writing the file")));
                }
                let left = {
                    let mut t = self.table.lock();
                    t.commit(idx, n);
                    t.segment(idx).remaining()
                };
                w.add(n);
                w.set_left(left);
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
