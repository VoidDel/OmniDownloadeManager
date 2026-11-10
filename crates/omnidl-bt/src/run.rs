//! One run of a torrent task: metadata, the add, then watching it until it is done, the seeding
//! policy is met, or the run is cancelled.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use librqbit::{
    AddTorrent, AddTorrentOptions, ManagedTorrent, Session, TorrentStatsState, limits::LimitsConfig,
};
use omnidl_core::{Error, ErrorCode, Job, JobOptions, Outcome, ResumeState, Segment, now_ms};

use crate::{BtEngine, Fetched, Source, bps, failure, meta::Meta};

/// How often the torrent's statistics are read.
const TICK: Duration = Duration::from_millis(500);
/// How often the uploaded total is saved while the task runs.
const SAVE_EVERY: Duration = Duration::from_secs(30);

pub(crate) async fn run(engine: &BtEngine, job: Job) -> Outcome {
    let session = match engine.session().await {
        Ok(s) => s,
        Err(e) => return Outcome::Failed(e, None),
    };
    let fetched = match metadata(engine, &session, &job).await {
        Ok(Some(f)) => f,
        Ok(None) => return Outcome::Stopped(ResumeState::default()),
        Err(e) => return Outcome::Failed(e, None),
    };
    let meta = match Meta::parse(&fetched.bytes) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed(e, None),
    };
    let source = Source::parse(&job.source).ok();
    let fallback = source.as_ref().map_or_else(
        || meta.info_hash.clone(),
        |s| s.fallback_name(&meta.info_hash),
    );
    let layout = meta.layout(&fallback);
    // Indices out of range (a hand-edited database) are dropped; none left means all.
    let only: Option<Vec<usize>> = job
        .options
        .only_files
        .as_ref()
        .map(|o| {
            o.iter()
                .copied()
                .filter(|&i| i < meta.files.len())
                .collect()
        })
        .filter(|o: &Vec<usize>| !o.is_empty());
    let size = meta.size(only.as_deref());
    let mut state = ResumeState {
        file_name: layout.name.clone(),
        total: Some(size),
        resumable: true,
        segments: segment(size, job.progress.downloaded().min(size)),
        completed_at: job.resume.as_ref().and_then(|r| r.completed_at),
        ..Default::default()
    };
    if let Err(e) = job.checkpoint.started(&state).await {
        return Outcome::Failed(e, None);
    }

    let busy = engine
        .running
        .lock()
        .iter()
        .any(|(id, h)| *id != job.id && h.info_hash().as_string() == meta.info_hash);
    if busy {
        let e = Error::new(
            ErrorCode::InvalidRequest,
            "another task is already downloading this torrent",
        );
        return Outcome::Failed(e, Some(state));
    }
    let folder = match &layout.subfolder {
        Some(sub) => job.save_dir.join(sub),
        None => job.save_dir.clone(),
    };
    let mut peers = fetched.peers.clone();
    match &source {
        Some(s) => peers.extend(engine.peers_for(s)),
        None => peers.extend(&engine.config.extra_peers),
    }
    let options = AddTorrentOptions {
        // Existing files are checked and kept: that is how a task resumes.
        overwrite: true,
        output_folder: Some(folder.to_string_lossy().into_owned()),
        only_files: only,
        initial_peers: Some(peers),
        // Besides the torrent's own; never for a private one (BEP 27).
        trackers: (!meta.private).then(|| engine.extra_trackers()),
        ratelimits: LimitsConfig {
            download_bps: bps(job.options.speed_limit),
            upload_bps: None,
        },
        ..Default::default()
    };
    let added = tokio::select! {
        _ = job.cancel.cancelled() => return Outcome::Stopped(state),
        r = session.add_torrent(AddTorrent::from_bytes(fetched.bytes.clone()), Some(options)) => r,
    };
    let handle = match added.map(|r| r.into_handle()) {
        Ok(Some(h)) => h,
        Ok(None) => {
            let e = Error::new(ErrorCode::Internal, "the torrent was not added");
            return Outcome::Failed(e, Some(state));
        }
        Err(e) => return Outcome::Failed(failure(e), Some(state)),
    };
    engine.running.lock().insert(job.id, handle.clone());
    let outcome = watch(&session, &handle, &job, &mut state).await;
    engine.running.lock().remove(&job.id);
    if let Err(e) = session.delete(handle.id().into(), false).await {
        tracing::warn!("removing torrent {}: {e:#}", meta.info_hash);
    }
    match outcome {
        Watched::Done => Outcome::Completed {
            file_name: state.file_name,
            total: size,
        },
        Watched::Cancelled => Outcome::Stopped(state),
        Watched::Failed(e) => Outcome::Failed(e, Some(state)),
    }
}

/// The task's saved metadata, or else fetched and saved; `None` when cancelled meanwhile.
async fn metadata(
    engine: &BtEngine,
    session: &Arc<Session>,
    job: &Job,
) -> omnidl_core::Result<Option<Fetched>> {
    let path = engine.torrent_path(job.id);
    if let Ok(bytes) = tokio::fs::read(&path).await {
        return Ok(Some(Fetched {
            bytes: bytes.into(),
            peers: Vec::new(),
        }));
    }
    let source = Source::parse(&job.source)?;
    let fetched = tokio::select! {
        _ = job.cancel.cancelled() => return Ok(None),
        r = engine.fetch(session, &source) => r?,
    };
    Meta::parse(&fetched.bytes)?;
    tokio::fs::create_dir_all(&engine.config.state_dir)
        .await
        .map_err(|e| Error::io(&e, engine.config.state_dir.display()))?;
    tokio::fs::write(&path, &fetched.bytes)
        .await
        .map_err(|e| Error::io(&e, path.display()))?;
    Ok(Some(fetched))
}

enum Watched {
    /// Downloaded, and seeded as long as the policy asks.
    Done,
    Cancelled,
    Failed(Error),
}

async fn watch(
    session: &Arc<Session>,
    handle: &Arc<ManagedTorrent>,
    job: &Job,
    state: &mut ResumeState,
) -> Watched {
    let base_uploaded = job.progress.uploaded();
    let mut seeding = false;
    let mut saved = Instant::now();
    // The session's download limit follows the global one (it applies to every torrent).
    let mut download_limit = None;
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = job.cancel.cancelled() => break,
            _ = tick.tick() => {}
        }
        let s = handle.stats();
        if matches!(s.state, TorrentStatsState::Error) {
            let message = s.error.unwrap_or_else(|| "the torrent stopped".into());
            return Watched::Failed(Error::new(ErrorCode::Io, message));
        }
        let total = s.total_bytes;
        let have = s.progress_bytes.min(total);
        let uploaded = base_uploaded + s.uploaded_bytes;
        job.progress.set_total(Some(total));
        job.progress.set_downloaded(have);
        job.progress.set_uploaded(uploaded);
        job.progress
            .set_connections(s.live.as_ref().map_or(0, |l| l.snapshot.peer_stats.live));
        state.total = Some(total);
        state.segments = segment(total, have);

        let global = job.global_limit.rate();
        if global != download_limit {
            session.ratelimits.set_download_bps(bps(global));
            download_limit = global;
        }

        if s.finished && !seeding && matches!(s.state, TorrentStatsState::Live) {
            let since = *state.completed_at.get_or_insert_with(now_ms);
            if !job.options.seeding || seed_done(&job.options, uploaded, total, since) {
                return Watched::Done;
            }
            seeding = true;
            if let Err(e) = job.checkpoint.seeding(state).await {
                tracing::warn!("task {}: {e}", job.id);
            }
        } else if seeding
            && let Some(since) = state.completed_at
            && seed_done(&job.options, uploaded, total, since)
        {
            return Watched::Done;
        }
        if saved.elapsed() >= SAVE_EVERY {
            saved = Instant::now();
            if let Err(e) = job.checkpoint.save(state).await {
                tracing::warn!("task {}: {e}", job.id);
            }
        }
    }
    if seeding {
        Watched::Done
    } else {
        Watched::Cancelled
    }
}

/// The seeding policy: enough uploaded (ratio) or seeded long enough.
fn seed_done(o: &JobOptions, uploaded: u64, size: u64, since: i64) -> bool {
    let ratio = o.seed_ratio > 0.0 && size > 0 && uploaded as f64 >= o.seed_ratio * size as f64;
    let time = o.seed_minutes > 0 && now_ms() - since >= i64::from(o.seed_minutes) * 60_000;
    ratio || time
}

/// A torrent's progress as one segment (pieces arrive in any order; the details panel shows
/// files instead).
fn segment(total: u64, have: u64) -> Vec<Segment> {
    if total == 0 {
        return Vec::new();
    }
    vec![Segment {
        start: 0,
        end: Some(total - 1),
        downloaded: have,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_the_seeding_policy() {
        let o = |ratio, minutes| JobOptions {
            seed_ratio: ratio,
            seed_minutes: minutes,
            ..Default::default()
        };
        let now = now_ms();
        assert!(!seed_done(&o(1.5, 0), 100, 100, now));
        assert!(seed_done(&o(1.5, 0), 150, 100, now));
        assert!(!seed_done(&o(0.0, 0), 1 << 40, 100, 0), "no limit");
        assert!(!seed_done(&o(0.0, 60), 0, 100, now));
        assert!(seed_done(&o(0.0, 60), 0, 100, now - 3_600_000));
        assert!(
            seed_done(&o(2.0, 60), 0, 100, now - 3_600_000),
            "either limit"
        );
    }
}
