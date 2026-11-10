//! How many connections a download uses, changed while it runs.
//!
//! - [`ConnControl`]: one per running task. The user sets a fixed number or automatic tuning
//!   ([`Mode`]); the engine reads [`ConnControl::target`] and starts or retires connections.
//! - [`Tuner`]: automatic tuning by hill climbing on measured speed. It measures a number of
//!   connections, tries more (doubling at first, then by quarters); more that bring at least
//!   10 % are kept, else it goes back and waits before trying again, waiting longer after each
//!   miss. It also tries fewer: fewer that keep 95 % of the speed are kept (the same speed with
//!   fewer connections is easier on the server). A server that refuses connections (429, 503,
//!   FTP 421) sets a ceiling at the connections that were working, tried again after a while
//!   (the refusal may have been passing); a speed limit that holds the speed back stops growth.
//! - [`Crew`]: the engine's connections and the speed of each. Retiring picks the slowest. A
//!   connection far slower than the others for a while, or that fell far below its own best
//!   (servers that limit each connection often let a new one run fast at first), is replaced by
//!   a new one that takes over its segment. Whether replacements pay is checked: when new
//!   connections are no faster than the ones they replaced, replacing pauses, longer each time.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
};

use parking_lot::Mutex;
// Tokio's clock: tests can pause it.
use tokio::time::{Duration, Instant};
use tokio_util::sync::{CancellationToken, WaitForCancellationFuture};

/// The most connections a download may have.
pub const MAX_CONNECTIONS: u32 = 64;
/// Automatic tuning starts here (or lower, as the engine asks).
pub const AUTO_START: u32 = 4;
/// Speed is measured over windows this long.
pub const WINDOW: Duration = Duration::from_secs(2);
/// Windows skipped after a change (new connections ramp up), then windows averaged.
const SETTLE: u32 = 1;
const MEASURE: usize = 2;
/// More connections must bring this much more speed; fewer must keep this much.
const GAIN_UP: f64 = 1.10;
const KEEP_DOWN: f64 = 0.95;
/// Windows to wait after a miss (doubling up to the most).
const COOLDOWN_MIN: u32 = 10;
const COOLDOWN_MAX: u32 = 60;
/// Windows to keep under the ceiling a refusal set before trying more again (the estimate may
/// be low, or the refusal passing); doubling after each refusal, back to the least once more
/// connections were taken.
const OVERLOAD_HOLD_MIN: u32 = 10;
const OVERLOAD_HOLD_MAX: u32 = 60;
/// A speed at or above this share of the limit counts as held back by it.
const LIMITED: f64 = 0.9;

/// What the user chose for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Fixed(u32),
    /// Tuned between 1 and `max`.
    Auto {
        max: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Down,
}

/// Hill climbing on the speed of each window (see the module).
#[derive(Debug, Clone)]
pub struct Tuner {
    cur: u32,
    max: u32,
    /// Lowered when the server refuses connections.
    ceiling: u32,
    /// The number before the probe in progress.
    prev: u32,
    probe: Option<Dir>,
    /// Speed measured at `prev` (during a probe) or `cur`.
    base: Option<f64>,
    settle: u32,
    samples: Vec<f64>,
    /// Measurements to wait before the next probe.
    hold: u32,
    cooldown: u32,
    /// Growing by doubling until the first miss.
    doubling: bool,
    next: Dir,
    /// The ceiling is lifted once the hold after a refusal is over.
    lift: bool,
    overload_hold: u32,
}

impl Tuner {
    pub fn new(start: u32, max: u32) -> Self {
        let max = max.clamp(1, MAX_CONNECTIONS);
        let cur = start.clamp(1, max);
        Self {
            cur,
            max,
            ceiling: max,
            prev: cur,
            probe: None,
            base: None,
            settle: SETTLE,
            samples: Vec::new(),
            hold: 0,
            cooldown: COOLDOWN_MIN,
            doubling: true,
            next: Dir::Up,
            lift: false,
            overload_hold: OVERLOAD_HOLD_MIN,
        }
    }

    pub fn current(&self) -> u32 {
        self.cur
    }

    fn limit(&self) -> u32 {
        self.max.min(self.ceiling)
    }

    /// A new upper bound; returns the number to use.
    pub fn set_max(&mut self, max: u32) -> u32 {
        self.max = max.clamp(1, MAX_CONNECTIONS);
        self.ceiling = self.ceiling.max(1);
        if self.cur > self.limit() {
            self.cur = self.limit();
            self.remeasure();
        }
        self.cur
    }

    fn remeasure(&mut self) {
        self.probe = None;
        self.base = None;
        self.settle = SETTLE;
        self.samples.clear();
    }

    /// The speed of one window (bytes per second); `limited`: a speed limit holds it back.
    /// Returns the new number of connections when it changes.
    pub fn window(&mut self, speed: f64, limited: bool) -> Option<u32> {
        if self.settle > 0 {
            self.settle -= 1;
            return None;
        }
        self.samples.push(speed);
        if self.samples.len() < MEASURE {
            return None;
        }
        let speed = self.samples.iter().sum::<f64>() / self.samples.len() as f64;
        self.samples.clear();
        let base = self.base.unwrap_or(0.0);
        match self.probe.take() {
            Some(Dir::Up) if !limited && speed >= base * GAIN_UP => {
                self.base = Some(speed);
                self.cooldown = COOLDOWN_MIN;
                self.overload_hold = OVERLOAD_HOLD_MIN;
                self.next = Dir::Up;
                self.start_probe(limited)
            }
            // Fewer that keep the speed: try fewer still. The speed to keep stays the one
            // before the first step down, so small losses do not add up.
            Some(Dir::Down) if speed >= base * KEEP_DOWN => {
                self.base = Some(base.max(speed));
                self.next = Dir::Down;
                self.start_probe(limited)
            }
            // A miss: back to where it was. After more that did not help, fewer are tried at
            // once (the speed may hold with fewer); after fewer that lost speed, it waits,
            // longer after each miss.
            Some(dir) => {
                self.cur = self.prev;
                self.settle = SETTLE;
                if dir == Dir::Up {
                    self.doubling = false;
                    self.next = Dir::Down;
                    self.hold = 0;
                } else {
                    self.next = Dir::Up;
                    self.hold = self.cooldown;
                    self.cooldown = (self.cooldown * 2).min(COOLDOWN_MAX);
                }
                Some(self.cur)
            }
            None => {
                self.base = Some(speed);
                if self.hold > 0 {
                    self.hold = self.hold.saturating_sub(MEASURE as u32);
                    if self.hold == 0 && self.lift {
                        // The refusal may have passed: more may be tried again.
                        self.lift = false;
                        self.ceiling = self.max;
                        self.next = Dir::Up;
                    }
                    return None;
                }
                self.start_probe(limited)
            }
        }
    }

    fn start_probe(&mut self, limited: bool) -> Option<u32> {
        let up = self.cur < self.limit() && !limited;
        let down = self.cur > 1;
        let dir = match self.next {
            Dir::Up if up => Dir::Up,
            Dir::Down if down => Dir::Down,
            _ if up => Dir::Up,
            _ if down => Dir::Down,
            _ => {
                self.hold = self.cooldown;
                return None;
            }
        };
        self.prev = self.cur;
        self.cur = match dir {
            Dir::Up => {
                let step = if self.doubling {
                    self.cur
                } else {
                    self.cur.div_ceil(4)
                };
                (self.cur + step.max(1)).min(self.limit())
            }
            Dir::Down => self.cur - (self.cur / 4).max(1),
        };
        self.probe = Some(dir);
        self.settle = SETTLE;
        Some(self.cur)
    }

    /// The server refused another connection while `working` were fine: no more than that
    /// for a while. `again`: another refusal of the same new connections, which only lowers the
    /// ceiling. Returns the number to use.
    pub fn overloaded(&mut self, working: u32, again: bool) -> u32 {
        let working = working.max(1);
        if again {
            self.ceiling = self.ceiling.min(working);
            self.cur = self.cur.min(self.ceiling);
            return self.cur;
        }
        self.ceiling = working;
        self.lift = true;
        self.cur = self.cur.min(self.ceiling);
        self.doubling = false;
        self.next = Dir::Down;
        self.remeasure();
        self.hold = self.overload_hold;
        self.overload_hold = (self.overload_hold * 2).min(OVERLOAD_HOLD_MAX);
        self.cur
    }
}

#[derive(Debug)]
struct Ctl {
    /// Set by the user while the task runs; else the engine's (from the task options).
    mode: Option<Mode>,
    /// The most the protocol allows (FTP: 8), set by the engine.
    cap: u32,
    start: u32,
    tuner: Option<Tuner>,
    /// Refused connections lower this in fixed mode, until a while after the last refusal.
    ceiling: u32,
    refused_at: Option<Instant>,
    /// Windows before a fixed number refused is tried again (doubling after each refusal).
    retry: u32,
    window: Option<(Instant, u64)>,
}

/// The number of connections of a running task (see the module).
#[derive(Debug)]
pub struct ConnControl {
    /// Speed is measured over windows this long ([`WINDOW`]; tests use shorter ones).
    window: Duration,
    ctl: Mutex<Ctl>,
    target: AtomicU32,
    /// The last window's speed was held back by a speed limit.
    limited: AtomicBool,
}

impl Default for ConnControl {
    fn default() -> Self {
        Self::with_window(WINDOW)
    }
}

impl ConnControl {
    /// Measuring over windows of `window` (the default is [`WINDOW`]).
    pub fn with_window(window: Duration) -> Self {
        Self {
            window,
            ctl: Mutex::new(Ctl {
                mode: None,
                cap: MAX_CONNECTIONS,
                start: AUTO_START,
                tuner: None,
                ceiling: MAX_CONNECTIONS,
                refused_at: None,
                retry: OVERLOAD_HOLD_MIN,
                window: None,
            }),
            target: AtomicU32::new(1),
            limited: AtomicBool::new(false),
        }
    }

    /// The engine starts a run: `mode` from the task options (unless the user changed it since),
    /// at most `cap` connections, `start` at first when tuning. Returns the target.
    pub fn begin(&self, mode: Mode, cap: u32, start: u32) -> u32 {
        let mut c = self.ctl.lock();
        c.cap = cap.clamp(1, MAX_CONNECTIONS);
        c.start = start.max(1);
        c.ceiling = c.cap;
        c.window = None;
        let mode = *c.mode.get_or_insert(mode);
        Self::apply(&mut c, mode, &self.target)
    }

    /// The user's choice, while the task runs (or before the engine begins).
    pub fn set_mode(&self, mode: Mode) {
        let mut c = self.ctl.lock();
        c.mode = Some(mode);
        // A number chosen by hand is tried even after the server refused more.
        c.ceiling = c.cap;
        c.retry = OVERLOAD_HOLD_MIN;
        Self::apply(&mut c, mode, &self.target);
    }

    fn apply(c: &mut Ctl, mode: Mode, target: &AtomicU32) -> u32 {
        let n = match mode {
            Mode::Fixed(n) => {
                c.tuner = None;
                n.clamp(1, c.cap).min(c.ceiling)
            }
            Mode::Auto { max } => {
                let max = max.clamp(1, c.cap);
                let t = c
                    .tuner
                    .get_or_insert_with(|| Tuner::new(c.start.min(max), max));
                t.set_max(max)
            }
        };
        target.store(n, Ordering::Relaxed);
        n
    }

    pub fn mode(&self) -> Option<Mode> {
        self.ctl.lock().mode
    }

    pub fn is_auto(&self) -> bool {
        matches!(self.mode(), Some(Mode::Auto { .. }))
    }

    /// Connections the download should have now.
    pub fn target(&self) -> u32 {
        self.target.load(Ordering::Relaxed).max(1)
    }

    /// The engine reports the bytes downloaded so far, as often as it likes; each full window
    /// goes to the tuner. `limit`: the speed limit in force (task or global).
    pub fn observe(&self, downloaded: u64, limit: Option<u64>) {
        let now = Instant::now();
        let mut c = self.ctl.lock();
        let Some((since, from)) = c.window else {
            c.window = Some((now, downloaded));
            return;
        };
        let elapsed = now.duration_since(since);
        if elapsed < self.window {
            return;
        }
        c.window = Some((now, downloaded));
        let speed = downloaded.saturating_sub(from) as f64 / elapsed.as_secs_f64();
        let limited = limit.is_some_and(|l| speed >= l as f64 * LIMITED);
        self.limited.store(limited, Ordering::Relaxed);
        if let Some(t) = c.tuner.as_mut() {
            if let Some(n) = t.window(speed, limited) {
                tracing::debug!("connections: {n} ({:.0} KiB/s)", speed / 1024.0);
                self.target.store(n, Ordering::Relaxed);
            }
        } else if c.ceiling < c.cap
            && c.refused_at
                .is_some_and(|t| now.duration_since(t) >= self.window * c.retry)
            && let Some(mode) = c.mode
        {
            // A fixed number the server refused a while ago: tried again.
            c.ceiling = c.cap;
            c.refused_at = None;
            Self::apply(&mut c, mode, &self.target);
        }
    }

    /// The window each connection's speed is taken over: half the tuner's.
    pub fn crew_window(&self) -> Duration {
        self.window / 2
    }

    /// A speed limit held the last window back (no point in more or other connections).
    pub fn limited(&self) -> bool {
        self.limited.load(Ordering::Relaxed)
    }

    /// The server refused another connection while `working` were fine.
    pub fn overloaded(&self, working: u32) {
        let mut c = self.ctl.lock();
        let now = Instant::now();
        // Refusals close together answer the same new connections: each one is one too many.
        let again = c
            .refused_at
            .is_some_and(|t| now.duration_since(t) < self.window * 2);
        let working = if again {
            working.max(1).min(c.ceiling)
        } else {
            if c.refused_at.is_some() {
                c.retry = (c.retry * 2).min(OVERLOAD_HOLD_MAX);
            }
            working.max(1)
        };
        c.ceiling = working;
        c.refused_at = Some(now);
        let n = match c.tuner.as_mut() {
            Some(t) => t.overloaded(working, again),
            None => self.target.load(Ordering::Relaxed).min(working),
        };
        tracing::debug!("connections: at most {working} for now (the server refused more)");
        self.target.store(n, Ordering::Relaxed);
    }
}

/// Slow next to the others: under this share of their median, for this many windows in a row,
/// once it has run this many windows; not when the others are this slow anyway (bytes per
/// second); at most one such replacement in this many windows.
const LAG_SHARE: f64 = 0.25;
const LAG_STRIKES: u32 = 2;
const LAG_MIN_WINDOWS: u32 = 6;
const LAG_FLOOR: f64 = 64.0 * 1024.0;
const LAG_GAP_WINDOWS: u32 = 10;
/// Faded: under this share of its own best (over two windows, so that a burst on a shaky line
/// does not count), for this many windows in a row. Servers that limit
/// each connection often let a new one run fast for a while: a faded connection is replaced by
/// a new one, as long as that pays.
const FADE: f64 = 0.5;
const FADE_STRIKES: u32 = 3;
/// A replacement pays when its best in its first windows is this many times the speed of the
/// connection it replaced.
const PAYS: f64 = 1.5;
const JUDGE_WINDOWS: u32 = 2;
/// Replacements that paid, in a row, before up to half of the connections may be replaced at
/// once (a quarter until then).
const HITS: u32 = 3;
/// Replacements that did not pay, in a row, before replacing pauses; the first pause, doubling
/// after each up to the most.
const MISSES: u32 = 3;
const PAUSE_MIN: Duration = Duration::from_secs(30);
const PAUSE_MAX: Duration = Duration::from_secs(600);
/// Stalled: nothing at all for this many windows in a row (even one that never got going).
const STALL_STRIKES: u32 = 5;
/// Not for a connection that would finish its segment within this long at its speed, nor with
/// less than this left (a new connection costs a handshake and a slow start).
const MIN_LEFT_TIME: f64 = 2.0;
const MIN_LEFT: u64 = 64 * 1024;

#[derive(Debug)]
struct Pace {
    last_at: Instant,
    last_bytes: u64,
    /// Bytes per second over the last window (`None` before the first).
    speed: Option<f64>,
    /// Its best speed over two windows in a row.
    peak: f64,
    windows: u32,
    lag_strikes: u32,
    fade_strikes: u32,
    stall_strikes: u32,
    /// The speed of the connection it replaced, until judged.
    replaces: Option<f64>,
}

/// One connection of a [`Crew`].
#[derive(Debug)]
pub struct Worker {
    cancel: CancellationToken,
    bytes: AtomicU64,
    /// Bytes left of its segment (`u64::MAX` while unknown).
    left: AtomicU64,
    pace: Mutex<Pace>,
    /// Started in place of a replaced connection.
    replacement: bool,
}

impl Worker {
    /// Bytes received.
    pub fn add(&self, n: u64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
    }

    pub fn set_left(&self, left: Option<u64>) {
        self.left.store(left.unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    /// Stopped: the download, or this connection (retired or replaced).
    pub fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.cancel.cancelled()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Started in place of a replaced connection: a refusal of it does not mean the server
    /// wants fewer connections (there are no more than before).
    pub fn is_replacement(&self) -> bool {
        self.replacement
    }
}

/// Unregisters its worker when the connection ends.
pub struct Shift {
    crew: Arc<Crew>,
    worker: Arc<Worker>,
}

impl Shift {
    pub fn worker(&self) -> &Arc<Worker> {
        &self.worker
    }
}

impl Drop for Shift {
    fn drop(&mut self) {
        self.crew
            .workers
            .lock()
            .retain(|w| !Arc::ptr_eq(w, &self.worker));
    }
}

#[derive(Debug)]
struct Replacing {
    misses: u32,
    /// Replacements that paid, in a row.
    hits: u32,
    paused_until: Option<Instant>,
    pause: Duration,
    last_lag: Option<Instant>,
}

/// The connections of a download (see the module).
#[derive(Debug)]
pub struct Crew {
    stop: CancellationToken,
    /// Each connection's speed is taken over windows this long.
    window: Duration,
    workers: Mutex<Vec<Arc<Worker>>>,
    tick: Mutex<Instant>,
    /// Speeds of replaced connections, for the connections started in their place.
    pending: Mutex<VecDeque<f64>>,
    replacing: Mutex<Replacing>,
    replaced: AtomicU32,
}

impl Crew {
    /// Connections end with `stop` too; each one's speed is taken over windows of `window`.
    pub fn new(stop: CancellationToken, window: Duration) -> Arc<Self> {
        Arc::new(Self {
            stop,
            window,
            workers: Mutex::new(Vec::new()),
            tick: Mutex::new(Instant::now()),
            pending: Mutex::new(VecDeque::new()),
            replacing: Mutex::new(Replacing {
                misses: 0,
                hits: 0,
                paused_until: None,
                pause: PAUSE_MIN,
                last_lag: None,
            }),
            replaced: AtomicU32::new(0),
        })
    }

    /// A new connection, until the returned shift is dropped.
    pub fn join(self: &Arc<Self>) -> Shift {
        let now = Instant::now();
        let replaces = self.pending.lock().pop_front();
        let worker = Arc::new(Worker {
            replacement: replaces.is_some(),
            cancel: self.stop.child_token(),
            bytes: AtomicU64::new(0),
            left: AtomicU64::new(u64::MAX),
            pace: Mutex::new(Pace {
                last_at: now,
                last_bytes: 0,
                speed: None,
                peak: 0.0,
                windows: 0,
                lag_strikes: 0,
                fade_strikes: 0,
                stall_strikes: 0,
                replaces,
            }),
        });
        self.workers.lock().push(worker.clone());
        Shift {
            crew: self.clone(),
            worker,
        }
    }

    /// Connections not told to stop.
    pub fn running(&self) -> u32 {
        self.workers
            .lock()
            .iter()
            .filter(|w| !w.is_cancelled())
            .count() as u32
    }

    /// Connections replaced (slow or faded), so far.
    pub fn replaced(&self) -> u32 {
        self.replaced.load(Ordering::Relaxed)
    }

    /// The server refused a connection: no replacing for a few windows (a replacement does
    /// not add a connection, so not for long).
    pub fn pause_replacing(&self) {
        let mut r = self.replacing.lock();
        let until = Instant::now() + self.window * LAG_GAP_WINDOWS;
        r.paused_until = Some(r.paused_until.map_or(until, |t| t.max(until)));
    }

    /// A replacement was refused by the server (which may not have let go of the connection it
    /// replaced yet): it did not pay, and replacing waits a little.
    pub fn replacement_refused(&self) {
        self.judge(false, Instant::now());
        self.pause_replacing();
    }

    fn judge(&self, pays: bool, now: Instant) {
        let mut r = self.replacing.lock();
        if pays {
            r.misses = 0;
            r.hits += 1;
            r.pause = PAUSE_MIN;
            return;
        }
        r.hits = 0;
        r.misses += 1;
        if r.misses >= MISSES {
            tracing::debug!(
                "new connections are no faster: not replacing for {:?}",
                r.pause
            );
            r.misses = 0;
            r.paused_until = Some(now + r.pause);
            r.pause = (r.pause * 2).min(PAUSE_MAX);
        }
    }

    /// Connections to start for `target`: those told to stop count until they are gone, so
    /// that the new ones take over the segments they leave (rather than split others).
    pub fn vacancies(&self, target: u32) -> u32 {
        target.saturating_sub(self.workers.lock().len() as u32)
    }

    /// Called often by the engine: retires the slowest connections beyond `target` and, once
    /// per window, replaces connections far slower than the others or than they were (unless
    /// a speed limit holds the download back). Returns how many connections to start now; the
    /// replacements start as the old ones end ([`Crew::vacancies`]).
    pub fn rebalance(&self, target: u32, limited: bool) -> u32 {
        let now = Instant::now();
        let workers: Vec<Arc<Worker>> = self
            .workers
            .lock()
            .iter()
            .filter(|w| !w.is_cancelled())
            .cloned()
            .collect();
        let new_window = {
            let mut t = self.tick.lock();
            let due = now.duration_since(*t) >= self.window;
            if due {
                *t = now;
            }
            due
        };
        if new_window {
            self.measure(&workers, now);
        }
        let speed = |w: &Arc<Worker>| w.pace.lock().speed.unwrap_or(f64::INFINITY);
        let running = workers.len() as u32;
        if running > target {
            let mut by_speed = workers.clone();
            by_speed.sort_by(|a, b| speed(a).total_cmp(&speed(b)));
            for w in by_speed.iter().take((running - target) as usize) {
                w.cancel.cancel();
            }
            return 0;
        }
        if new_window && !limited {
            for (w, why) in self.to_replace(&workers, now) {
                let s = speed(&w);
                tracing::debug!("replacing a {why} connection ({:.0} KiB/s)", s / 1024.0);
                w.cancel.cancel();
                self.pending.lock().push_back(s);
                self.replaced.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.vacancies(target)
    }

    /// Each connection's speed since it was last measured (or started), its best, and whether
    /// a replacement paid.
    fn measure(&self, workers: &[Arc<Worker>], now: Instant) {
        for w in workers {
            let bytes = w.bytes.load(Ordering::Relaxed);
            let mut p = w.pace.lock();
            let secs = now.duration_since(p.last_at).as_secs_f64().max(1e-3);
            let speed = bytes.saturating_sub(p.last_bytes) as f64 / secs;
            p.last_at = now;
            p.last_bytes = bytes;
            let smooth = p.speed.map_or(speed, |prev| (prev + speed) / 2.0);
            p.speed = Some(speed);
            p.peak = p.peak.max(smooth);
            p.windows += 1;
            if p.windows >= JUDGE_WINDOWS
                && let Some(old) = p.replaces.take()
            {
                let pays = p.peak >= (old * PAYS).max(1.0);
                drop(p);
                self.judge(pays, now);
            }
        }
    }

    /// Connections to replace: slow next to the others, or faded from their own best. At most a
    /// quarter of them at once (half once replacing has paid a few times), none while paused.
    fn to_replace(
        &self,
        workers: &[Arc<Worker>],
        now: Instant,
    ) -> Vec<(Arc<Worker>, &'static str)> {
        let speeds: Vec<Option<f64>> = workers.iter().map(|w| w.pace.lock().speed).collect();
        let mut lagging = Vec::new();
        let mut faded = Vec::new();
        for (i, w) in workers.iter().enumerate() {
            let mut p = w.pace.lock();
            let Some(mine) = p.speed else { continue };
            let mut others: Vec<f64> = speeds
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .filter_map(|(_, s)| *s)
                .collect();
            others.sort_by(f64::total_cmp);
            let median = others.get(others.len() / 2).copied();
            let slow = median.is_some_and(|m| m >= LAG_FLOOR && mine < m * LAG_SHARE);
            p.lag_strikes = if slow { p.lag_strikes + 1 } else { 0 };
            let fading = p.windows >= 2 && p.peak >= LAG_FLOOR && mine < p.peak * FADE;
            p.fade_strikes = if fading { p.fade_strikes + 1 } else { 0 };
            p.stall_strikes = if mine == 0.0 { p.stall_strikes + 1 } else { 0 };
            let left = w.left.load(Ordering::Relaxed);
            if left < MIN_LEFT || (left as f64) < mine * MIN_LEFT_TIME {
                continue;
            }
            if p.lag_strikes >= LAG_STRIKES && p.windows >= LAG_MIN_WINDOWS {
                lagging.push(w.clone());
            } else if p.fade_strikes >= FADE_STRIKES {
                faded.push((w.clone(), "faded"));
            } else if p.stall_strikes >= STALL_STRIKES {
                faded.push((w.clone(), "stalled"));
            }
        }
        let mut r = self.replacing.lock();
        if r.paused_until.is_some_and(|t| now < t) {
            return Vec::new();
        }
        let mut out = Vec::new();
        let gap = self.window * LAG_GAP_WINDOWS;
        if let Some(w) = lagging.into_iter().next()
            && r.last_lag.is_none_or(|t| now.duration_since(t) >= gap)
        {
            r.last_lag = Some(now);
            out.push((w, "slow"));
        }
        let share = if r.hits >= HITS { 2 } else { 4 };
        let most = (workers.len() / share).max(1);
        for (w, why) in faded {
            if out.len() >= most {
                break;
            }
            out.push((w, why));
        }
        for (w, _) in &out {
            let mut p = w.pace.lock();
            p.lag_strikes = 0;
            p.fade_strikes = 0;
            p.stall_strikes = 0;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the tuner against `speed(n)` (bytes per second with n connections) for `windows`
    /// windows and returns the numbers it used.
    fn run(t: &mut Tuner, windows: usize, speed: impl Fn(u32) -> f64) -> Vec<u32> {
        let mut seen = vec![t.current()];
        for _ in 0..windows {
            if let Some(n) = t.window(speed(t.current()), false) {
                seen.push(n);
            }
        }
        seen
    }

    #[test]
    fn climbs_to_where_more_connections_stop_helping() {
        // 1 MB/s per connection, up to 10 MB/s in all.
        let speed = |n: u32| f64::from(n.min(10)) * 1e6;
        let mut t = Tuner::new(4, 32);
        let seen = run(&mut t, 200, speed);
        assert_eq!(&seen[..3], [4, 8, 16], "doubles at first: {seen:?}");
        // 16 brings nothing over 8: back to 8, then fewer that keep the speed, more by quarters.
        let n = t.current();
        assert!((10..=13).contains(&n), "settles near 10: {n} ({seen:?})");
    }

    #[test]
    fn keeps_fewer_connections_when_they_are_as_fast() {
        // The server gives 8 MB/s whatever the number.
        let mut t = Tuner::new(16, 32);
        run(&mut t, 400, |_| 8e6);
        assert!(t.current() <= 4, "{}", t.current());
    }

    #[test]
    fn goes_back_when_more_connections_slow_things_down() {
        // Best at 6; more congest.
        let speed = |n: u32| {
            if n <= 6 {
                f64::from(n) * 1e6
            } else {
                6e6 - f64::from(n - 6) * 0.5e6
            }
        };
        let mut t = Tuner::new(2, 64);
        run(&mut t, 400, speed);
        assert!((5..=7).contains(&t.current()), "{}", t.current());
    }

    #[test]
    fn stays_under_the_ceiling_after_a_refusal_then_tries_again() {
        let mut t = Tuner::new(8, 32);
        assert_eq!(t.overloaded(5, false), 5);
        // Another refusal of the same new connections: one fewer.
        assert_eq!(t.overloaded(4, true), 4);
        let seen = run(&mut t, OVERLOAD_HOLD_MIN as usize, |n| f64::from(n) * 1e6);
        assert!(seen.iter().all(|&n| n <= 4), "{seen:?}");
        // The hold is over: more are tried, and kept while they bring speed.
        let seen = run(&mut t, 40, |n| f64::from(n) * 1e6);
        assert!(seen.iter().any(|&n| n > 4), "{seen:?}");
        // Refused again after more were taken: back under, for the shortest hold again.
        assert_eq!(t.overloaded(6, false), 6);
        let seen = run(&mut t, OVERLOAD_HOLD_MIN as usize, |n| f64::from(n) * 1e6);
        assert!(seen.iter().all(|&n| n <= 6), "{seen:?}");
    }

    #[test]
    fn waits_longer_after_each_refusal_in_a_row() {
        // A server that refuses whatever is tried above 3.
        let mut t = Tuner::new(3, 32);
        let mut held = Vec::new();
        for _ in 0..3 {
            t.overloaded(3, false);
            let mut windows = 0;
            while t.window(3e6, false).is_none() {
                windows += 1;
            }
            held.push(windows);
        }
        assert!(held[0] < held[1] && held[1] < held[2], "{held:?}");
    }

    #[test]
    fn does_not_grow_while_a_limit_holds_the_speed() {
        let mut t = Tuner::new(4, 32);
        for _ in 0..100 {
            t.window(1e6, true);
        }
        assert!(t.current() <= 4);
    }

    #[test]
    fn control_follows_the_user_and_the_engine_cap() {
        let c = ConnControl::default();
        assert_eq!(c.begin(Mode::Fixed(16), 8, 2), 8, "FTP caps at 8");
        c.set_mode(Mode::Fixed(3));
        assert_eq!(c.target(), 3);
        c.set_mode(Mode::Auto { max: 32 });
        assert_eq!(c.target(), 2, "tuning starts at the engine's start");
        assert!(c.is_auto());
        c.overloaded(1);
        assert_eq!(c.target(), 1);
        c.set_mode(Mode::Fixed(6));
        c.overloaded(4);
        assert_eq!(c.target(), 4, "a refused fixed number waits");
        // A second refusal of the same new connections: one fewer.
        c.overloaded(3);
        assert_eq!(c.target(), 3);
        // A choice made before the engine begins wins over the task options.
        let c = ConnControl::default();
        c.set_mode(Mode::Fixed(5));
        assert_eq!(c.begin(Mode::Auto { max: 16 }, 64, 4), 5);
    }

    #[tokio::test(start_paused = true)]
    async fn control_tunes_from_the_bytes_reported() {
        let c = ConnControl::default();
        assert_eq!(c.begin(Mode::Auto { max: 16 }, 64, 4), 4);
        let mut bytes = 0u64;
        // 1 MB/s per connection, no cap: the tuner keeps doubling.
        for _ in 0..200 {
            tokio::time::advance(Duration::from_millis(250)).await;
            bytes += u64::from(c.target()) * 250_000;
            c.observe(bytes, None);
        }
        assert_eq!(c.target(), 16);
    }

    #[tokio::test(start_paused = true)]
    async fn crew_retires_the_slowest_and_replaces_a_laggard() {
        let crew = Crew::new(CancellationToken::new(), Duration::from_secs(1));
        let mut shifts: Vec<Shift> = (0..4).map(|_| crew.join()).collect();
        let tick = |shifts: &[Shift], n: usize| {
            for (i, s) in shifts.iter().enumerate() {
                // The third is a hundred times slower.
                let rate = if i == 2 { 10_000 } else { 1_000_000 };
                s.worker().add(rate * n as u64 / 4);
            }
        };
        assert_eq!(crew.rebalance(4, false), 0);
        // Windows pass; after enough of them the slow one is told to go.
        for _ in 0..40 {
            tokio::time::advance(Duration::from_millis(250)).await;
            tick(&shifts, 1);
            assert_eq!(
                crew.rebalance(4, false),
                0,
                "none to start while it is still there"
            );
            if crew.replaced() > 0 {
                break;
            }
        }
        assert!(shifts[2].worker().is_cancelled(), "the slow one goes");
        assert_eq!(crew.replaced(), 1);
        assert_eq!(crew.running(), 3);
        // Once it is gone, one is started in its place.
        shifts.remove(2);
        assert_eq!(crew.vacancies(4), 1);
        shifts.push(crew.join());
        assert!(shifts[3].worker().is_replacement(), "started in the slow one's place");
        assert!(!shifts[0].worker().is_replacement());
        // Fewer wanted: the slowest go first (the new one, not measured yet, stays).
        assert_eq!(crew.rebalance(2, false), 0);
        assert_eq!(crew.running(), 2);
        assert!(!shifts[3].worker().is_cancelled());
        drop(shifts);
        assert_eq!(crew.running(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn crew_leaves_connections_alone_under_a_speed_limit() {
        let crew = Crew::new(CancellationToken::new(), Duration::from_secs(1));
        let shifts: Vec<Shift> = (0..3).map(|_| crew.join()).collect();
        for _ in 0..40 {
            tokio::time::advance(Duration::from_millis(250)).await;
            shifts[0].worker().add(1_000);
            shifts[1].worker().add(500_000);
            shifts[2].worker().add(500_000);
            assert_eq!(crew.rebalance(3, true), 0);
        }
        assert_eq!(crew.replaced(), 0);
    }

    /// Connections that start at 4 MB/s and drop to 0.2 MB/s after `fast` windows; `new_fast`
    /// says whether the ones started in their place are fast again.
    async fn rotate(new_fast: bool) -> (u32, u32) {
        let crew = Crew::new(CancellationToken::new(), Duration::from_secs(1));
        let mut shifts: Vec<(Shift, u32, bool)> = (0..4).map(|_| (crew.join(), 0, true)).collect();
        let mut started = 0;
        for _ in 0..60 * 4 {
            tokio::time::advance(Duration::from_millis(250)).await;
            for (s, ticks, fresh) in &mut shifts {
                *ticks += 1;
                let fast = *fresh && *ticks <= 8;
                s.worker().add(if fast { 1_000_000 } else { 50_000 });
            }
            crew.rebalance(4, false);
            shifts.retain(|(s, _, _)| !s.worker().is_cancelled());
            for _ in 0..crew.vacancies(4) {
                started += 1;
                shifts.push((crew.join(), 0, new_fast));
            }
        }
        (started, crew.replaced())
    }

    #[tokio::test(start_paused = true)]
    async fn replaces_connections_that_fade_while_new_ones_are_faster() {
        let (started, replaced) = rotate(true).await;
        // Each fades after 2 s and is replaced about 2 s later: dozens in a minute.
        assert!(started >= 20, "{started}");
        assert_eq!(started, replaced);
    }

    #[tokio::test(start_paused = true)]
    async fn stops_replacing_when_new_connections_are_no_faster() {
        let (started, _) = rotate(false).await;
        // The first faded ones are replaced; the new ones are as slow, so after three misses it
        // pauses (30 s, then 60 s): a handful in a minute.
        assert!((3..=8).contains(&started), "{started}");
    }
}
