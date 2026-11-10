//! Downloads a link for a while and prints, every second, the speed and the connections (open
//! and wanted), to compare fixed numbers with tuning on a real server:
//!
//! ```text
//! cargo run -p omnidl-http --release --example bench -- <url> [auto | <n>] [seconds] [max]
//! ```
//!
//! `auto` (the default) tunes up to `max` (16); a number keeps that many connections. The file
//! goes to a temporary folder that is deleted at the end. `OMNIDL_COOKIE` and `OMNIDL_UA` set the
//! `Cookie` and `User-Agent` headers (a link behind a browser check needs the browser's own).
//! `RUST_LOG=omnidl_core=debug` shows the tuner's and the crew's decisions.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use omnidl_core::{
    Checkpoint, ConnControl, DownloadEngine, Job, JobOptions, Outcome, Progress, RateLimiter,
    Result, ResumeState,
};
use omnidl_http::HttpEngine;
use tokio_util::sync::CancellationToken;

struct Nothing;

#[async_trait]
impl Checkpoint for Nothing {
    async fn started(&self, _: &ResumeState) -> Result<()> {
        Ok(())
    }

    async fn save(&self, _: &ResumeState) -> Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    if std::env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .init();
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(url) = args.first().cloned() else {
        eprintln!("usage: bench <url> [auto | <n>] [seconds] [max]");
        std::process::exit(2);
    };
    let mode = args.get(1).map_or("auto", String::as_str).to_owned();
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let max: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(16);

    let dir = tempfile::tempdir().expect("a temporary folder");
    let mut options = JobOptions::default();
    match mode.parse::<u32>() {
        Ok(n) => {
            options.auto_connections = false;
            options.connections = n;
        }
        Err(_) => {
            options.auto_connections = true;
            options.auto_max_connections = max;
        }
    }
    options.cookies = std::env::var("OMNIDL_COOKIE").ok();
    if let Ok(ua) = std::env::var("OMNIDL_UA") {
        options.user_agent = ua;
    }
    let cancel = CancellationToken::new();
    let progress = Arc::new(Progress::default());
    let conns = Arc::new(ConnControl::default());
    let job = Job {
        id: uuid::Uuid::new_v4(),
        source: url.clone(),
        save_dir: dir.path().to_path_buf(),
        file_name: None,
        resume: None,
        options,
        cancel: cancel.clone(),
        progress: progress.clone(),
        task_limit: RateLimiter::new(None),
        global_limit: RateLimiter::new(None),
        checkpoint: Arc::new(Nothing),
        conns: conns.clone(),
    };
    println!(
        "{url}\nmode {mode}, {secs} s, into {}",
        dir.path().display()
    );
    let run = tokio::spawn(async move { HttpEngine::new().run(job).await });

    let started = Instant::now();
    let (mut last, mut peak) = (0u64, 0f64);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.tick().await;
    println!(
        "{:>4}  {:>9}  {:>10}  {:>5}  {:>6}",
        "s", "MiB", "MiB/s", "open", "target"
    );
    while !run.is_finished() && started.elapsed() < Duration::from_secs(secs) {
        tick.tick().await;
        let done = progress.downloaded();
        let speed = done.saturating_sub(last) as f64 / (1 << 20) as f64;
        last = done;
        peak = peak.max(speed);
        println!(
            "{:>4}  {:>9.1}  {:>10.2}  {:>5}  {:>6}",
            started.elapsed().as_secs(),
            done as f64 / (1 << 20) as f64,
            speed,
            progress.connections(),
            conns.target(),
        );
    }
    cancel.cancel();
    let outcome = run.await.expect("the download task");
    let secs = started.elapsed().as_secs_f64();
    let done = progress.downloaded() as f64 / (1 << 20) as f64;
    match outcome {
        Outcome::Completed { total, .. } => println!("completed: {total} bytes"),
        Outcome::Stopped(_) => println!("stopped after {secs:.0} s"),
        Outcome::Failed(e, _) => println!("failed: {e}"),
    }
    println!(
        "{done:.1} MiB in {secs:.1} s: average {:.2} MiB/s, best second {peak:.2} MiB/s",
        done / secs
    );
    // The partial file goes with the folder.
    drop(dir);
}
