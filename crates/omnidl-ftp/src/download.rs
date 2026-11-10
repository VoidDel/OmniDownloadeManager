//! One run of an FTP download: probe, settle the file, run the sessions, save progress, finish.
//! The structure follows the HTTP engine's: a segment table with work stealing, a writer task,
//! a checkpoint every second.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};

use bytes::Bytes;
use omnidl_core::{
    Crew, Error, ErrorCode, Job, Outcome, Result, ResumeState, SegmentTable, Shift, Worker,
    disk::{backoff, claim_name, ensure_space, place},
    filename, segments,
};
use omnidl_storage::{FileWriter, file};
use parking_lot::Mutex;
use tokio::{io::AsyncReadExt, task::JoinSet, time::Instant};

use crate::{
    Probe, probe,
    session::{self, Ftp, Target, timed},
};

const UI_TICK: Duration = Duration::from_millis(250);
const SAVE_EVERY: Duration = Duration::from_secs(1);
const READ_BUF: usize = 64 * 1024;
/// Servers often refuse more sessions than this from one address.
const MAX_SESSIONS: u32 = 8;

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

/// Logs in, probes (retrying temporary failures), settles name and partial file, reports the
/// start. The probing session is handed to the first worker.
async fn prepare(retry_delay: Duration, job: &Arc<Job>) -> Result<Download> {
    let target = Target::parse(&job.source, &job.options)?;
    let (probe, ftp) = probe_retrying(retry_delay, job, &target).await?;
    let dir = job.save_dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&e, dir.display()))?;
    let total = probe.total;
    let resumable = probe.resumable;
    // Tuning starts from the sessions of the settings (servers are strict about them).
    let first = job.conns.begin(
        job.options.ftp_conn_mode(),
        MAX_SESSIONS,
        job.options.ftp_connections,
    );
    let sessions = if resumable { first } else { 1 };
    let (file_name, file, plan) = match &job.resume {
        Some(r) => {
            let partial = dir.join(filename::partial(&r.file_name));
            let resuming = r.resumable && resumable && r.downloaded() > 0;
            if resuming && let Some(why) = changed(&probe, r) {
                return Err(Error::new(ErrorCode::RemoteChanged, why));
            }
            let reopened = if resuming {
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
                    (r.file_name.clone(), f, segments::plan(total, sessions))
                }
            }
        }
        None => {
            ensure_space(&dir, total)?;
            let base = job.file_name.clone().unwrap_or_else(|| target.file_name());
            let (name, f) = claim_name(&dir, &base, total, job.options.conflict)?;
            (name, f, segments::plan(total, sessions))
        }
    };
    let writer = FileWriter::spawn(file).map_err(|e| Error::io(&e, "file writer"))?;
    let download = Download {
        job: job.clone(),
        target: target.clone(),
        dir,
        retry_delay,
        state: ResumeState {
            file_name,
            final_url: Some(target.url.clone()),
            total,
            etag: None,
            last_modified: probe.modified,
            resumable,
            segments: Vec::new(),
            completed_at: None,
        },
        table: Arc::new(Mutex::new(SegmentTable::new(plan))),
        writer,
        spare: Arc::new(Mutex::new(Some(ftp))),
        last_saved: None,
    };
    job.progress.set_total(total);
    job.progress
        .set_downloaded(download.table.lock().downloaded());
    job.checkpoint.started(&download.snapshot()).await?;
    Ok(download)
}

/// Why resuming would mix two versions of the file.
fn changed(p: &Probe, r: &ResumeState) -> Option<String> {
    if p.total != r.total {
        return Some(format!("size {:?} → {:?}", r.total, p.total));
    }
    match (&r.last_modified, &p.modified) {
        (Some(a), Some(b)) if a != b => Some(format!("modified {a} → {b}")),
        _ => None,
    }
}

async fn probe_retrying(retry_delay: Duration, job: &Job, target: &Target) -> Result<(Probe, Ftp)> {
    let mut attempt = 0;
    loop {
        let r = tokio::select! {
            r = async {
                let mut ftp = session::open(target, &job.options).await?;
                let p = probe(&mut ftp, target, &job.options).await?;
                Ok::<_, Error>((p, ftp))
            } => r,
            _ = job.cancel.cancelled() => return Err(Error::internal("cancelled")),
        };
        match r {
            Err(e) if e.is_transient() && attempt < job.options.max_retries => {
                attempt += 1;
                tracing::debug!("probing {} (attempt {attempt}): {e}", target.url);
                tokio::select! {
                    _ = tokio::time::sleep(backoff(retry_delay, attempt)) => {}
                    _ = job.cancel.cancelled() => return Err(Error::internal("cancelled")),
                }
            }
            r => return r,
        }
    }
}

enum Halt {
    Cancelled,
    /// The server refused `REST`: start over on one session.
    Fallback,
    Failed(Error),
}

enum ConnError {
    Fallback,
    Fatal(Error),
    /// This session gave up; others may take its segment.
    GaveUp(Error),
    /// A connection started in place of a replaced one gave up (refused, most likely because
    /// the server still counts the old one): not a reason for fewer connections.
    ReplacementGaveUp(Error),
}

enum FetchError {
    Cancelled,
    Fallback,
    Failed(Error),
}

struct Download {
    job: Arc<Job>,
    target: Target,
    dir: PathBuf,
    retry_delay: Duration,
    state: ResumeState,
    table: Arc<Mutex<SegmentTable>>,
    writer: FileWriter,
    /// The probing session, for the first worker.
    spare: Arc<Mutex<Option<Ftp>>>,
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
            match self.sessions().await {
                Err(Halt::Fallback) if self.state.resumable => {
                    tracing::info!("{} refuses REST: one session", self.target.url);
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

    /// Runs the sessions until the table is complete, the job is cancelled or a session fails
    /// for good. Saves progress every second meanwhile.
    async fn sessions(&mut self) -> Result<(), Halt> {
        let stop = self.job.cancel.child_token();
        let conn = Arc::new(Conn {
            job: self.job.clone(),
            target: self.target.clone(),
            table: self.table.clone(),
            writer: self.writer.clone(),
            ranged: self.state.resumable,
            crew: Crew::new(stop.clone(), self.job.conns.crew_window()),
            open: AtomicU32::new(0),
            retry_delay: self.retry_delay,
            spare: self.spare.clone(),
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
                        // "421 Too many connections", or it kept failing: no more sessions than
                        // the others, for a while.
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
                Error::new(ErrorCode::Network, "every session closed early")
            })));
        }
        Ok(())
    }

    fn publish(&self) {
        let views = self.table.lock().views();
        self.job.progress.set_segments(views);
    }

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

    async fn stop(mut self, halt: Halt) -> Outcome {
        let mut state = self.snapshot();
        if let Err(e) = self.writer.close().await {
            tracing::warn!("closing the partial file: {e}");
            state = self.last_saved.take().unwrap_or_else(|| ResumeState {
                segments: Vec::new(),
                ..state
            });
        }
        if !state.resumable {
            state.segments.clear();
        }
        match halt {
            Halt::Cancelled => Outcome::Stopped(state),
            Halt::Failed(e) => Outcome::Failed(e, Some(state)),
            Halt::Fallback => Outcome::Failed(
                Error::new(ErrorCode::FtpReply, "the server refused to resume (REST)"),
                Some(state),
            ),
        }
    }

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
        // The server's modification time.
        if let Some(secs) = state
            .last_modified
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            && let Ok(f) = std::fs::OpenOptions::new()
                .write(true)
                .open(self.dir.join(&name))
        {
            let _ = f.set_modified(UNIX_EPOCH + Duration::from_secs(secs));
        }
        Outcome::Completed {
            file_name: name,
            total: got,
        }
    }
}

/// What every session of a download shares.
struct Conn {
    job: Arc<Job>,
    target: Target,
    table: Arc<Mutex<SegmentTable>>,
    writer: FileWriter,
    /// Transfers start where their segment does (`REST`).
    ranged: bool,
    /// The sessions, each stopped with the download or on its own (retired, replaced).
    crew: Arc<Crew>,
    open: AtomicU32,
    retry_delay: Duration,
    spare: Arc<Mutex<Option<Ftp>>>,
}

impl Conn {
    /// Starts up to `n` sessions, each on a segment it claims.
    fn spawn(self: &Arc<Self>, set: &mut JoinSet<Result<(), ConnError>>, n: u32) {
        let mut t = self.table.lock();
        for _ in 0..n {
            let Some(idx) = t.claim() else { break };
            set.spawn(self.clone().work(self.crew.join(), idx));
        }
    }

    /// One session: its segment, then whatever it can claim next. A transfer that ran to the
    /// end of the file leaves the session reusable; one stopped early (its segment ended or
    /// was split) closes it, since aborting a transfer is unreliable across servers.
    async fn work(self: Arc<Self>, shift: Shift, mut idx: usize) -> Result<(), ConnError> {
        let w = shift.worker();
        let mut ftp = self.spare.lock().take();
        let r = loop {
            let r = self.segment(w, idx, &mut ftp).await;
            self.table.lock().release(idx);
            match r {
                Err(ConnError::GaveUp(e)) if w.is_replacement() => {
                    break Err(ConnError::ReplacementGaveUp(e));
                }
                Err(e) => break Err(e),
                Ok(()) => {}
            }
            // The download stopped, or this session was retired or replaced.
            if w.is_cancelled() {
                break Ok(());
            }
            match self.table.lock().claim() {
                Some(next) => idx = next,
                None => break Ok(()),
            }
        };
        if let Some(mut f) = ftp {
            let _ = tokio::time::timeout(Duration::from_secs(2), f.quit()).await;
        }
        r
    }

    async fn segment(
        &self,
        w: &Worker,
        idx: usize,
        ftp: &mut Option<Ftp>,
    ) -> Result<(), ConnError> {
        let mut attempt = 0;
        loop {
            if w.is_cancelled() {
                return Ok(());
            }
            let before = self.position(idx);
            let e = match self.fetch(w, idx, ftp).await {
                Ok(()) | Err(FetchError::Cancelled) => return Ok(()),
                Err(FetchError::Fallback) => return Err(ConnError::Fallback),
                Err(FetchError::Failed(e)) => e,
            };
            *ftp = None;
            if !e.is_transient() {
                return Err(ConnError::Fatal(e));
            }
            if self.position(idx) > before {
                attempt = 0;
            }
            // "421 Too many connections": leave the segment to the other sessions.
            if e.status == Some(421) && self.table.lock().active() > 1 {
                tracing::debug!("{}: {e}; one session fewer", self.target.url);
                return Err(ConnError::GaveUp(e));
            }
            attempt += 1;
            if attempt > self.job.options.max_retries {
                return Err(ConnError::GaveUp(e));
            }
            tracing::debug!("{} (retry {attempt}): {e}", self.target.url);
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

    /// One `RETR` for the rest of segment `idx`.
    async fn fetch(
        &self,
        w: &Worker,
        idx: usize,
        slot: &mut Option<Ftp>,
    ) -> Result<(), FetchError> {
        let start = {
            let mut t = self.table.lock();
            if !self.ranged && t.segment(idx).downloaded > 0 {
                t.reset(idx);
                self.job.progress.set_downloaded(t.downloaded());
            }
            let s = t.segment(idx);
            if s.is_done() {
                return Ok(());
            }
            w.set_left(s.remaining());
            s.pos()
        };
        let o = &self.job.options;
        if slot.is_none() {
            let opened = tokio::select! {
                r = session::open(&self.target, o) => r,
                _ = w.cancelled() => return Err(FetchError::Cancelled),
            };
            *slot = Some(opened.map_err(FetchError::Failed)?);
        }
        let Some(ftp) = slot.as_mut() else {
            return Err(FetchError::Cancelled);
        };
        if self.ranged && start > 0 {
            let _ = ftp.resume_transfer(start as usize).await;
        }
        let opened = tokio::select! {
            r = timed(o.read_timeout, ftp.retr_as_stream(self.target.path.as_str())) => r,
            _ = w.cancelled() => {
                *slot = None;
                return Err(FetchError::Cancelled);
            }
        };
        let mut stream = match opened {
            Ok(s) => s,
            Err(e) => {
                *slot = None;
                // REST (or RETR from an offset) not implemented.
                if start > 0 && e.code == ErrorCode::FtpReply && matches!(e.status, Some(500..=504))
                {
                    return Err(FetchError::Fallback);
                }
                return Err(FetchError::Failed(e));
            }
        };
        let _open = Open::new(self);
        let mut offset = start;
        let mut buf = vec![0u8; READ_BUF];
        loop {
            let read = tokio::select! {
                r = tokio::time::timeout(o.read_timeout, stream.read(&mut buf)) => r,
                _ = w.cancelled() => {
                    drop(stream);
                    *slot = None;
                    return Err(FetchError::Cancelled);
                }
            };
            let n = match read {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => {
                    drop(stream);
                    *slot = None;
                    return Err(FetchError::Failed(Error::new(
                        ErrorCode::Network,
                        e.to_string(),
                    )));
                }
                Err(_) => {
                    drop(stream);
                    *slot = None;
                    return Err(FetchError::Failed(Error::new(
                        ErrorCode::Timeout,
                        "no data from the FTP server",
                    )));
                }
            };
            if n == 0 {
                let done = {
                    let mut t = self.table.lock();
                    if t.segment(idx).end.is_none() {
                        t.finish_unknown(idx);
                    }
                    t.segment(idx).is_done()
                };
                if !done {
                    drop(stream);
                    *slot = None;
                    return Err(FetchError::Failed(Error::new(
                        ErrorCode::Network,
                        "the server closed the transfer early",
                    )));
                }
                // The "226 transfer complete" reply; without it the session is not reused.
                if timed(o.read_timeout, stream.finish()).await.is_err() {
                    *slot = None;
                }
                return Ok(());
            }
            let len = n as u64;
            tokio::select! {
                _ = self.job.throttle(len) => {}
                _ = w.cancelled() => {
                    drop(stream);
                    *slot = None;
                    return Err(FetchError::Cancelled);
                }
            }
            let take = self.table.lock().reserve(idx, len);
            if take > 0 {
                let chunk = Bytes::copy_from_slice(&buf[..take as usize]);
                if let Err(e) = self.writer.write(offset, chunk).await {
                    return Err(FetchError::Failed(Error::io(&e, "writing the file")));
                }
                let left = {
                    let mut t = self.table.lock();
                    t.commit(idx, take);
                    t.segment(idx).remaining()
                };
                w.add(take);
                w.set_left(left);
                self.job.progress.add(take);
                offset += take;
            }
            if take < len {
                // The segment ends here; the rest of the file is someone else's.
                drop(stream);
                *slot = None;
                return Ok(());
            }
        }
    }
}

/// Counts a running transfer in the task's connection number while alive.
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
