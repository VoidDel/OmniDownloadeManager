//! The task manager and scheduler (docs/development-plan.md §3.4, §4.5).
//!
//! Tasks live in memory (for the UI) and in the database (to survive a restart). Every change
//! goes: mutate under the state lock → write the task's current row → publish `Changed`. Queued
//! tasks start in priority, then list order while fewer than `max_concurrent` run. A running task
//! is an engine future; pausing, removing or quitting cancels it and the outcome decides the
//! status it lands in.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use omnidl_storage::{Db, SegmentRecord};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    Checkpoint, DownloadEngine, EngineConfig, Error, ErrorCode, EventBus, Job, JobOptions, Outcome,
    Priority, ProbeInfo, Progress, ProgressItem, RateLimiter, Result, ResumeState, Segment,
    SegmentView, Stats, Task, TaskEvent, TaskId, TaskKind, TaskOptions, TaskStatus, filename,
    now_ms,
};

/// How often running tasks are sampled and progress is published.
const TICK: Duration = Duration::from_millis(250);
/// Speed is averaged over this window.
const SPEED_WINDOW: Duration = Duration::from_secs(3);
/// How long quitting waits for downloads to save their progress.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

/// A download to add.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub source: String,
    pub save_dir: String,
    #[serde(default)]
    pub options: TaskOptions,
    /// `false` adds it paused.
    #[serde(default = "yes")]
    pub start: bool,
    /// What the new-task dialog already learned: name and size show at once.
    #[serde(default)]
    pub probe: Option<ProbeInfo>,
}

fn yes() -> bool {
    true
}

/// A task with its segments, for the details panel.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub task: Task,
    pub segments: Vec<SegmentView>,
    /// The finished file.
    pub path: String,
}

/// What to do with a running task once its engine has stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    None,
    Pause,
    Remove,
    Purge { delete_files: bool },
    Shutdown,
}

struct Running {
    cancel: CancellationToken,
    stop: Stop,
    progress: Arc<Progress>,
    limit: RateLimiter,
    meter: SpeedMeter,
}

#[derive(Default)]
struct State {
    tasks: HashMap<TaskId, Task>,
    running: HashMap<TaskId, Running>,
    next_order: i64,
}

struct Inner {
    db: Db,
    engines: Vec<Arc<dyn DownloadEngine>>,
    bus: EventBus,
    config: RwLock<EngineConfig>,
    global_limit: RateLimiter,
    state: Mutex<State>,
    /// Orders row writes: each writes the task as it is when the write starts, so the last write
    /// is always the latest state.
    persist: tokio::sync::Mutex<()>,
    /// A running task finished.
    stopped: Notify,
    closing: AtomicBool,
}

/// The download manager. Cheap to clone.
#[derive(Clone)]
pub struct TaskManager {
    inner: Arc<Inner>,
}

impl TaskManager {
    /// Loads the tasks from `db` and starts what is queued. Downloads that were running when the
    /// app quit are queued again (or paused, without `resume_on_startup`). Needs a Tokio runtime.
    pub async fn open(
        db: Db,
        engines: Vec<Arc<dyn DownloadEngine>>,
        config: EngineConfig,
    ) -> Result<Self> {
        let config = config.normalized();
        let mut state = State::default();
        for record in db.list_tasks().await? {
            let id = record.id.clone();
            let Some(mut t) = Task::from_record(record) else {
                tracing::warn!("skipping unreadable task {id}");
                continue;
            };
            if t.status.is_running() {
                t.status = match t.status {
                    TaskStatus::Seeding => TaskStatus::Completed,
                    _ if config.resume_on_startup => TaskStatus::Queued,
                    _ => TaskStatus::Paused,
                };
                db.update_task(&t.to_record()).await?;
            }
            state.next_order = state.next_order.max(t.sort_order + 1);
            state.tasks.insert(t.id, t);
        }
        let inner = Arc::new(Inner {
            db,
            engines,
            bus: EventBus::new(1024),
            global_limit: RateLimiter::new(config.speed_limit),
            config: RwLock::new(config),
            state: Mutex::new(state),
            persist: tokio::sync::Mutex::new(()),
            stopped: Notify::new(),
            closing: AtomicBool::new(false),
        });
        tokio::spawn(ticker(Arc::downgrade(&inner)));
        inner.schedule();
        Ok(Self { inner })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TaskEvent> {
        self.inner.bus.subscribe()
    }

    /// Every task, in list order.
    pub fn list(&self) -> Vec<Task> {
        let mut v: Vec<Task> = self.inner.state.lock().tasks.values().cloned().collect();
        v.sort_by_key(|t| (t.sort_order, t.created_at));
        v
    }

    pub fn get(&self, id: TaskId) -> Option<Task> {
        self.inner.state.lock().tasks.get(&id).cloned()
    }

    pub async fn detail(&self, id: TaskId) -> Result<TaskDetail> {
        let (task, live) = {
            let st = self.inner.state.lock();
            let task = st.tasks.get(&id).cloned().ok_or_else(|| missing(id))?;
            (task, st.running.get(&id).map(|r| r.progress.segments()))
        };
        let segments = match live {
            Some(v) if !v.is_empty() => v,
            _ => self
                .inner
                .db
                .segments(&id.to_string())
                .await?
                .into_iter()
                .map(|r| {
                    let s = segment(r);
                    SegmentView {
                        start: s.start,
                        end: s.end,
                        downloaded: s.downloaded,
                        active: false,
                    }
                })
                .collect(),
        };
        Ok(TaskDetail {
            path: task.path().to_string_lossy().into_owned(),
            task,
            segments,
        })
    }

    pub fn config(&self) -> EngineConfig {
        self.inner.config.read().clone()
    }

    /// Applies new settings: the global limit at once, the rest to downloads that start later.
    pub fn set_config(&self, config: EngineConfig) {
        let config = config.normalized();
        if config.speed_limit != self.inner.global_limit.rate() {
            self.inner.global_limit.set_rate(config.speed_limit);
        }
        *self.inner.config.write() = config;
        self.inner.schedule();
    }

    /// The global download limit in bytes per second (`None`: unlimited).
    pub fn set_speed_limit(&self, limit: Option<u64>) {
        let limit = limit.filter(|&l| l > 0);
        self.inner.config.write().speed_limit = limit;
        self.inner.global_limit.set_rate(limit);
    }

    /// Asks the server about `source` without downloading it.
    pub async fn probe(&self, source: &str, options: &TaskOptions) -> Result<ProbeInfo> {
        let source = source.trim();
        let engine = self.inner.engine_for(source)?;
        let options = JobOptions::resolve(options, &self.inner.config.read());
        engine.probe(source, &options).await
    }

    /// Adds downloads (all or, at the first invalid one, those before it).
    pub async fn add(&self, items: Vec<NewTask>) -> Result<Vec<Task>> {
        let mut added = Vec::with_capacity(items.len());
        for item in items {
            let source = item.source.trim().to_owned();
            let engine = self.inner.engine_for(&source)?;
            let save_dir = item.save_dir.trim().to_owned();
            if save_dir.is_empty() {
                return Err(Error::invalid("no download folder"));
            }
            let mut options = item.options;
            options.file_name = options
                .file_name
                .filter(|n| !n.trim().is_empty())
                .map(|n| filename::sanitize(&n));
            let probe = item.probe.as_ref();
            let file_name = options
                .file_name
                .clone()
                .or_else(|| probe.map(|p| filename::sanitize(&p.file_name)))
                .or_else(|| filename::from_url(&source))
                .unwrap_or_else(|| "download".into());
            let task = {
                let mut st = self.inner.state.lock();
                let sort_order = st.next_order;
                st.next_order += 1;
                Task {
                    id: Uuid::new_v4(),
                    kind: engine.kind(),
                    source,
                    final_url: probe.map(|p| p.final_url.clone()),
                    save_dir,
                    category: filename::category(&file_name),
                    file_name,
                    status: if item.start {
                        TaskStatus::Queued
                    } else {
                        TaskStatus::Paused
                    },
                    total_bytes: probe.and_then(|p| p.total),
                    downloaded_bytes: 0,
                    uploaded_bytes: 0,
                    speed_bps: 0,
                    eta_secs: None,
                    connections: 0,
                    priority: Priority::Normal,
                    resumable: probe.is_some_and(|p| p.resumable),
                    options,
                    error: None,
                    created_at: now_ms(),
                    started_at: None,
                    completed_at: None,
                    prev_status: None,
                    sort_order,
                    etag: None,
                    last_modified: None,
                }
            };
            self.inner.db.insert_task(&task.to_record()).await?;
            self.inner.state.lock().tasks.insert(task.id, task.clone());
            self.inner.bus.emit(TaskEvent::Added { task: task.clone() });
            added.push(task);
        }
        self.inner.schedule();
        Ok(added)
    }

    /// Pauses running and queued tasks; others are left alone.
    pub async fn pause(&self, ids: &[TaskId]) -> Result<()> {
        let mut changed = Vec::new();
        {
            let mut st = self.inner.state.lock();
            let State { tasks, running, .. } = &mut *st;
            for id in ids {
                if let Some(r) = running.get_mut(id) {
                    if r.stop == Stop::None {
                        r.stop = Stop::Pause;
                    }
                    r.cancel.cancel();
                } else if let Some(t) = tasks.get_mut(id)
                    && t.status == TaskStatus::Queued
                {
                    t.status = TaskStatus::Paused;
                    changed.push(*id);
                }
            }
        }
        for id in changed {
            self.inner.changed(id).await;
        }
        Ok(())
    }

    /// Queues paused and failed tasks again.
    pub async fn resume(&self, ids: &[TaskId]) -> Result<()> {
        let mut changed = Vec::new();
        {
            let mut st = self.inner.state.lock();
            let State { tasks, running, .. } = &mut *st;
            for id in ids {
                if running.contains_key(id) {
                    continue;
                }
                if let Some(t) = tasks.get_mut(id)
                    && matches!(t.status, TaskStatus::Paused | TaskStatus::Failed)
                {
                    t.status = TaskStatus::Queued;
                    t.error = None;
                    changed.push(*id);
                }
            }
        }
        for id in changed {
            self.inner.changed(id).await;
        }
        self.inner.schedule();
        Ok(())
    }

    /// Starts paused or failed downloads over from the first byte (after the remote file
    /// changed, say), keeping their file name.
    pub async fn restart(&self, ids: &[TaskId]) -> Result<()> {
        let mut changed = Vec::new();
        {
            let mut st = self.inner.state.lock();
            let State { tasks, running, .. } = &mut *st;
            for id in ids {
                if running.contains_key(id) {
                    continue;
                }
                if let Some(t) = tasks.get_mut(id)
                    && matches!(t.status, TaskStatus::Paused | TaskStatus::Failed)
                {
                    t.status = TaskStatus::Queued;
                    t.error = None;
                    t.downloaded_bytes = 0;
                    t.etag = None;
                    t.last_modified = None;
                    changed.push(*id);
                }
            }
        }
        for id in &changed {
            self.inner.db.clear_segments(&id.to_string()).await?;
            self.inner.changed(*id).await;
        }
        self.inner.schedule();
        Ok(())
    }

    /// Moves tasks to the recycle bin (running ones stop first). Files stay.
    pub async fn remove(&self, ids: &[TaskId]) -> Result<()> {
        let mut changed = Vec::new();
        {
            let mut st = self.inner.state.lock();
            let State { tasks, running, .. } = &mut *st;
            for id in ids {
                if let Some(r) = running.get_mut(id) {
                    if !matches!(r.stop, Stop::Purge { .. }) {
                        r.stop = Stop::Remove;
                    }
                    r.cancel.cancel();
                } else if let Some(t) = tasks.get_mut(id)
                    && t.status != TaskStatus::Removed
                {
                    t.prev_status = Some(t.status);
                    t.status = TaskStatus::Removed;
                    changed.push(*id);
                }
            }
        }
        for id in changed {
            self.inner.changed(id).await;
        }
        Ok(())
    }

    /// Takes tasks out of the recycle bin: finished and failed ones as they were, the rest
    /// paused.
    pub async fn restore(&self, ids: &[TaskId]) -> Result<()> {
        let mut changed = Vec::new();
        {
            let mut st = self.inner.state.lock();
            for id in ids {
                if let Some(t) = st.tasks.get_mut(id)
                    && t.status == TaskStatus::Removed
                {
                    t.status = match t.prev_status.take() {
                        Some(s @ (TaskStatus::Completed | TaskStatus::Failed)) => s,
                        _ => TaskStatus::Paused,
                    };
                    changed.push(*id);
                }
            }
        }
        for id in changed {
            self.inner.changed(id).await;
        }
        Ok(())
    }

    /// Deletes tasks for good, with their partial files, and with `delete_files` their finished
    /// files too.
    pub async fn purge(&self, ids: &[TaskId], delete_files: bool) -> Result<()> {
        let mut now = Vec::new();
        {
            let mut st = self.inner.state.lock();
            for id in ids {
                if let Some(r) = st.running.get_mut(id) {
                    r.stop = Stop::Purge { delete_files };
                    r.cancel.cancel();
                } else if st.tasks.contains_key(id) {
                    now.push(*id);
                }
            }
        }
        self.inner.delete(&now, delete_files).await
    }

    /// The task's own download limit (`None`: only the global one applies).
    pub async fn set_task_speed_limit(&self, id: TaskId, limit: Option<u64>) -> Result<()> {
        let limit = limit.filter(|&l| l > 0);
        {
            let mut st = self.inner.state.lock();
            let t = st.tasks.get_mut(&id).ok_or_else(|| missing(id))?;
            t.options.speed_limit = limit;
            if let Some(r) = st.running.get(&id) {
                r.limit.set_rate(limit);
            }
        }
        self.inner.changed(id).await;
        Ok(())
    }

    /// Stops every download, waits (a bounded time) for them to save their progress, and closes
    /// the database. Running downloads keep their status, so they resume on the next launch.
    pub async fn shutdown(&self) {
        self.inner.closing.store(true, Ordering::SeqCst);
        {
            let mut st = self.inner.state.lock();
            for r in st.running.values_mut() {
                if r.stop == Stop::None {
                    r.stop = Stop::Shutdown;
                }
                r.cancel.cancel();
            }
        }
        let deadline = tokio::time::Instant::now() + SHUTDOWN_GRACE;
        loop {
            let notified = self.inner.stopped.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.inner.state.lock().running.is_empty() {
                break;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                tracing::warn!("downloads did not stop in time");
                break;
            }
        }
        self.inner.db.close().await;
    }
}

impl Inner {
    fn engine_for(&self, source: &str) -> Result<Arc<dyn DownloadEngine>> {
        let kind = TaskKind::detect(source).ok_or_else(|| {
            Error::new(
                ErrorCode::InvalidUrl,
                format!("not a link Omni Download can fetch: {source}"),
            )
        })?;
        self.engine(kind).ok_or_else(|| {
            Error::new(
                ErrorCode::Unsupported,
                format!("{} downloads are not supported yet", kind.as_str()),
            )
        })
    }

    fn engine(&self, kind: TaskKind) -> Option<Arc<dyn DownloadEngine>> {
        self.engines.iter().find(|e| e.kind() == kind).cloned()
    }

    /// Writes the task's current row and publishes it.
    async fn changed(&self, id: TaskId) {
        let task = {
            let _order = self.persist.lock().await;
            let Some(task) = self.state.lock().tasks.get(&id).cloned() else {
                return;
            };
            if let Err(e) = self.db.update_task(&task.to_record()).await {
                tracing::error!("saving task {id}: {e}");
            }
            task
        };
        self.bus.emit(TaskEvent::Changed { task });
    }

    async fn save_segments(&self, id: TaskId, s: &ResumeState) -> Result<()> {
        let records: Vec<SegmentRecord> = s
            .segments
            .iter()
            .map(|s| SegmentRecord {
                start: s.start as i64,
                end: s.end.map(|e| e as i64),
                downloaded: s.downloaded as i64,
            })
            .collect();
        self.db
            .save_progress(
                &id.to_string(),
                s.total.map(|n| n as i64),
                s.downloaded() as i64,
                &records,
            )
            .await?;
        Ok(())
    }

    /// Starts queued tasks while there are free slots.
    fn schedule(self: &Arc<Self>) {
        if self.closing.load(Ordering::SeqCst) {
            return;
        }
        let max = self.config.read().max_concurrent as usize;
        let mut started = Vec::new();
        {
            let mut st = self.state.lock();
            let State { tasks, running, .. } = &mut *st;
            let free = max.saturating_sub(running.len());
            if free == 0 {
                return;
            }
            let mut queued: Vec<&Task> = tasks
                .values()
                .filter(|t| t.status == TaskStatus::Queued && !running.contains_key(&t.id))
                .collect();
            queued.sort_by_key(|t| (t.priority, t.sort_order, t.created_at));
            let ids: Vec<TaskId> = queued.iter().take(free).map(|t| t.id).collect();
            for id in ids {
                let Some(t) = tasks.get_mut(&id) else {
                    continue;
                };
                t.status = TaskStatus::Probing;
                t.error = None;
                let progress = Arc::new(Progress::new(t.downloaded_bytes, t.total_bytes));
                running.insert(
                    id,
                    Running {
                        cancel: CancellationToken::new(),
                        stop: Stop::None,
                        progress,
                        limit: RateLimiter::new(t.options.speed_limit),
                        meter: SpeedMeter::default(),
                    },
                );
                started.push(id);
            }
        }
        for id in started {
            let me = self.clone();
            tokio::spawn(async move {
                me.changed(id).await;
                me.drive(id).await;
            });
        }
    }

    async fn drive(self: Arc<Self>, id: TaskId) {
        let parts = {
            let st = self.state.lock();
            st.tasks.get(&id).cloned().zip(
                st.running
                    .get(&id)
                    .map(|r| (r.cancel.clone(), r.progress.clone(), r.limit.clone())),
            )
        };
        let Some((task, (cancel, progress, limit))) = parts else {
            return;
        };
        let Some(engine) = self.engine(task.kind) else {
            let e = Error::new(ErrorCode::Unsupported, "no engine for this task");
            self.finish(id, Outcome::Failed(e, None)).await;
            return;
        };
        let resume = if task.started_at.is_some() {
            let segments = match self.db.segments(&id.to_string()).await {
                Ok(v) => v.into_iter().map(segment).collect(),
                Err(e) => {
                    tracing::error!("loading segments of {id}: {e}");
                    Vec::new()
                }
            };
            Some(ResumeState {
                file_name: task.file_name.clone(),
                final_url: task.final_url.clone(),
                total: task.total_bytes,
                etag: task.etag.clone(),
                last_modified: task.last_modified.clone(),
                resumable: task.resumable,
                segments,
            })
        } else {
            None
        };
        let options = JobOptions::resolve(&task.options, &self.config.read());
        let job = Job {
            id,
            source: task.source.clone(),
            save_dir: PathBuf::from(&task.save_dir),
            file_name: task.options.file_name.clone(),
            resume,
            options,
            cancel,
            progress,
            task_limit: limit,
            global_limit: self.global_limit.clone(),
            checkpoint: Arc::new(Saver {
                inner: Arc::downgrade(&self),
                id,
            }),
        };
        let outcome = engine.run(job).await;
        self.finish(id, outcome).await;
    }

    async fn finish(self: &Arc<Self>, id: TaskId, outcome: Outcome) {
        let stop = self
            .state
            .lock()
            .running
            .remove(&id)
            .map_or(Stop::None, |r| r.stop);
        // A run stopped before it settled the file (while probing) has nothing on disk.
        let started = self
            .state
            .lock()
            .tasks
            .get(&id)
            .is_some_and(|t| t.started_at.is_some());
        let on_disk = match &outcome {
            Outcome::Stopped(s) if started => Some(s),
            Outcome::Failed(_, Some(s)) if started => Some(s),
            _ => None,
        };
        if let Some(s) = on_disk
            && let Err(e) = self.save_segments(id, s).await
        {
            tracing::error!("saving progress of {id}: {e}");
        }
        if let Stop::Purge { delete_files } = stop {
            if let Err(e) = self.delete(&[id], delete_files).await {
                tracing::error!("deleting {id}: {e}");
            }
        } else {
            let completed = matches!(outcome, Outcome::Completed { .. });
            {
                let mut st = self.state.lock();
                if let Some(t) = st.tasks.get_mut(&id) {
                    t.speed_bps = 0;
                    t.eta_secs = None;
                    t.connections = 0;
                    let natural = match outcome {
                        Outcome::Completed { file_name, total } => {
                            t.category = filename::category(&file_name);
                            t.file_name = file_name;
                            t.total_bytes = Some(total);
                            t.downloaded_bytes = total;
                            t.completed_at = Some(now_ms());
                            t.error = None;
                            TaskStatus::Completed
                        }
                        Outcome::Stopped(s) => {
                            if started {
                                t.downloaded_bytes = s.downloaded();
                                t.resumable = s.resumable;
                            }
                            TaskStatus::Paused
                        }
                        Outcome::Failed(e, s) => {
                            if let Some(s) = s.filter(|_| started) {
                                t.downloaded_bytes = s.downloaded();
                            }
                            tracing::warn!("task {id} failed: {e}");
                            t.error = Some(e);
                            TaskStatus::Failed
                        }
                    };
                    match stop {
                        Stop::Remove => {
                            t.prev_status = Some(natural);
                            t.status = TaskStatus::Removed;
                        }
                        // Left running in the database: it resumes on the next launch.
                        Stop::Shutdown if natural == TaskStatus::Paused => {}
                        _ => t.status = natural,
                    }
                }
            }
            if completed && let Err(e) = self.db.clear_segments(&id.to_string()).await {
                tracing::error!("clearing segments of {id}: {e}");
            }
            self.changed(id).await;
        }
        self.stopped.notify_waiters();
        self.schedule();
    }

    async fn delete(&self, ids: &[TaskId], delete_files: bool) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let removed: Vec<Task> = {
            let mut st = self.state.lock();
            ids.iter().filter_map(|id| st.tasks.remove(id)).collect()
        };
        let keys: Vec<String> = removed.iter().map(|t| t.id.to_string()).collect();
        self.db.delete_tasks(&keys).await?;
        for t in &removed {
            // A task that never started has no partial file of its own (another task may use
            // the same name).
            if t.started_at.is_some() {
                remove_file(&t.partial_path());
            }
            let finished =
                t.status == TaskStatus::Completed || t.prev_status == Some(TaskStatus::Completed);
            if delete_files && finished {
                remove_file(&t.path());
            }
        }
        self.bus.emit(TaskEvent::Deleted {
            ids: removed.iter().map(|t| t.id).collect(),
        });
        Ok(())
    }
}

fn remove_file(path: &std::path::Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("deleting {}: {e}", path.display());
    }
}

fn missing(id: TaskId) -> Error {
    Error::invalid(format!("no task {id}"))
}

fn segment(r: SegmentRecord) -> Segment {
    Segment {
        start: r.start.max(0) as u64,
        end: r.end.map(|e| e.max(0) as u64),
        downloaded: r.downloaded.max(0) as u64,
    }
}

/// The engine's channel back into the manager.
struct Saver {
    inner: Weak<Inner>,
    id: TaskId,
}

#[async_trait]
impl Checkpoint for Saver {
    async fn started(&self, s: &ResumeState) -> Result<()> {
        let Some(inner) = self.inner.upgrade() else {
            return Ok(());
        };
        {
            let mut st = inner.state.lock();
            if let Some(t) = st.tasks.get_mut(&self.id) {
                if t.file_name != s.file_name {
                    t.file_name = s.file_name.clone();
                    t.category = filename::category(&t.file_name);
                }
                t.final_url = s.final_url.clone();
                t.total_bytes = s.total;
                t.etag = s.etag.clone();
                t.last_modified = s.last_modified.clone();
                t.resumable = s.resumable;
                t.downloaded_bytes = s.downloaded();
                t.started_at.get_or_insert_with(now_ms);
                if t.status == TaskStatus::Probing {
                    t.status = TaskStatus::Downloading;
                }
            }
        }
        inner.save_segments(self.id, s).await?;
        inner.changed(self.id).await;
        Ok(())
    }

    async fn save(&self, s: &ResumeState) -> Result<()> {
        match self.inner.upgrade() {
            Some(inner) => inner.save_segments(self.id, s).await,
            None => Ok(()),
        }
    }
}

/// Samples running tasks: speed, ETA, connections; publishes one progress event per tick.
async fn ticker(inner: Weak<Inner>) {
    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut was_running = false;
    loop {
        interval.tick().await;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        let now = Instant::now();
        let (items, stats) = {
            let mut st = inner.state.lock();
            let State { tasks, running, .. } = &mut *st;
            let mut items = Vec::with_capacity(running.len());
            let mut speed_sum = 0;
            for (id, r) in running.iter_mut() {
                let Some(t) = tasks.get_mut(id) else {
                    continue;
                };
                let downloaded = r.progress.downloaded();
                let speed = r.meter.sample(now, downloaded);
                let total = r.progress.total().or(t.total_bytes);
                let eta = match total {
                    Some(total) if speed > 0 => {
                        Some(total.saturating_sub(downloaded).div_ceil(speed))
                    }
                    _ => None,
                };
                t.downloaded_bytes = downloaded;
                t.total_bytes = total;
                t.speed_bps = speed;
                t.eta_secs = eta;
                t.connections = r.progress.connections();
                speed_sum += speed;
                items.push(ProgressItem {
                    id: *id,
                    downloaded,
                    total,
                    speed,
                    eta,
                    connections: t.connections,
                });
            }
            let stats = Stats {
                download_speed: speed_sum,
                running: running.len() as u32,
            };
            (items, stats)
        };
        // One last empty batch after the last task stops zeroes the totals in the UI.
        if !items.is_empty() || was_running {
            was_running = !items.is_empty();
            inner.bus.emit(TaskEvent::Progress { items, stats });
        }
    }
}

/// Bytes per second over the last [`SPEED_WINDOW`].
#[derive(Default)]
struct SpeedMeter {
    samples: VecDeque<(Instant, u64)>,
}

impl SpeedMeter {
    fn sample(&mut self, now: Instant, bytes: u64) -> u64 {
        if self.samples.back().is_some_and(|&(_, b)| bytes < b) {
            // The download started over.
            self.samples.clear();
        }
        self.samples.push_back((now, bytes));
        while self.samples.len() > 2 && now.duration_since(self.samples[0].0) > SPEED_WINDOW {
            self.samples.pop_front();
        }
        let (t0, b0) = self.samples[0];
        let dt = now.duration_since(t0).as_secs_f64();
        if dt < 0.2 {
            return 0;
        }
        ((bytes - b0) as f64 / dt) as u64
    }
}

#[cfg(test)]
mod tests;
