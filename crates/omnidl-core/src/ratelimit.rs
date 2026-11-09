//! Token-bucket rate limiting (docs/development-plan.md §4.2.6). A connection asks for the bytes
//! it just received before passing them on; the task's limiter and the global one are asked in
//! turn, so the stricter one sets the pace.

use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;
// Tokio's clock: the same as std's, except that tests can pause it.
use tokio::time::Instant;

/// Burst allowed after an idle period, as time at the full rate.
const BURST: Duration = Duration::from_millis(250);

#[derive(Debug)]
struct Bucket {
    rate: Option<u64>,
    /// Negative while connections owe bytes they already took.
    tokens: f64,
    last: Instant,
}

/// A shared token bucket. `None` rate is unlimited. Cheap to clone.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    bucket: Arc<Mutex<Bucket>>,
}

impl RateLimiter {
    pub fn new(rate: Option<u64>) -> Self {
        Self {
            bucket: Arc::new(Mutex::new(Bucket {
                rate: rate.filter(|&r| r > 0),
                tokens: 0.0,
                last: Instant::now(),
            })),
        }
    }

    pub fn rate(&self) -> Option<u64> {
        self.bucket.lock().rate
    }

    /// Changes the rate; debts from the old rate are forgiven.
    pub fn set_rate(&self, rate: Option<u64>) {
        let mut b = self.bucket.lock();
        b.rate = rate.filter(|&r| r > 0);
        b.tokens = 0.0;
        b.last = Instant::now();
    }

    /// Takes `n` bytes' worth of tokens, waiting while the bucket is in debt.
    pub async fn acquire(&self, n: u64) {
        let wait = {
            let mut b = self.bucket.lock();
            let Some(rate) = b.rate else {
                return;
            };
            let now = Instant::now();
            let rate = rate as f64;
            let cap = rate * BURST.as_secs_f64();
            b.tokens = (b.tokens + now.duration_since(b.last).as_secs_f64() * rate).min(cap);
            b.last = now;
            b.tokens -= n as f64;
            if b.tokens >= 0.0 {
                return;
            }
            Duration::from_secs_f64(-b.tokens / rate)
        };
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn paces_to_the_rate() {
        let l = RateLimiter::new(Some(100_000));
        let start = tokio::time::Instant::now();
        // 1 MB at 100 kB/s: about 10 s (less the 250 ms burst it starts without).
        for _ in 0..100 {
            l.acquire(10_000).await;
        }
        let took = start.elapsed().as_secs_f64();
        assert!((9.5..10.5).contains(&took), "{took}");
    }

    #[tokio::test(start_paused = true)]
    async fn shares_the_rate_between_connections() {
        let l = RateLimiter::new(Some(50_000));
        let start = tokio::time::Instant::now();
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                let l = l.clone();
                tokio::spawn(async move {
                    for _ in 0..25 {
                        l.acquire(1_000).await;
                    }
                })
            })
            .collect();
        for t in tasks {
            t.await.unwrap();
        }
        // 100 kB at 50 kB/s.
        let took = start.elapsed().as_secs_f64();
        assert!((1.7..2.3).contains(&took), "{took}");
    }

    #[tokio::test]
    async fn unlimited_does_not_wait() {
        let l = RateLimiter::new(None);
        let start = std::time::Instant::now();
        l.acquire(u64::MAX / 2).await;
        assert!(start.elapsed() < Duration::from_millis(50));
        l.set_rate(Some(0));
        assert_eq!(l.rate(), None);
    }
}
