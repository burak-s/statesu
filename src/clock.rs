use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Time is the only seam that needs polymorphism in tests (expiry checks,
/// rate limit windows, JWT lifetimes), so it is the trait boundary.
pub trait Clock: Send + Sync {
    /// Seconds since the Unix epoch, UTC.
    fn now(&self) -> i64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            // SystemTime before the epoch would be an environment anomaly.
            .unwrap_or(0)
    }
}

/// Controllable clock for tests: construction time is inherited from
/// [`SystemClock`] or set explicitly, and `advance` moves time forward.
#[derive(Debug, Clone)]
pub struct TestClock {
    now: Arc<AtomicI64>,
}

impl TestClock {
    pub fn at_start(value: i64) -> TestClock {
        TestClock {
            now: Arc::new(AtomicI64::new(value)),
        }
    }

    pub fn advance(&self, seconds: i64) {
        self.now.fetch_add(seconds, Ordering::SeqCst);
    }

    pub fn set(&self, value: i64) {
        self.now.store(value, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_gives_unix_seconds() {
        let now = SystemClock.now();
        assert!(now > 1_600_000_000, "clock not advancing: {now}");
    }

    #[test]
    fn test_clock_advances() {
        let clock = TestClock::at_start(100);
        assert_eq!(clock.now(), 100);
        clock.advance(5);
        assert_eq!(clock.now(), 105);
        clock.set(5000);
        assert_eq!(clock.now(), 5000);
    }
}
