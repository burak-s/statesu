use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use crate::clock::Clock;

/// Bounded fixed-window limiter per client IP. `governor` grows its key
/// storage without expiry, so the fixed-window form keeps storage bounded and
/// windows are aligned to the clock seam that tests control.
pub struct RateLimiter {
    max_requests_per_window: u64,
    window_secs: i64,
    clock: Arc<dyn Clock>,
    entries: Mutex<HashMap<IpAddr, Window>>,
    /// Storage must never grow without bound; stale windows are swept once
    /// this many distinct clients have been seen.
    max_entries: usize,
}

#[derive(Debug, Clone, Copy)]
struct Window {
    opened_at: i64,
    used: u64,
}

impl RateLimiter {
    pub fn per_minute(rpm: u64, clock: Arc<dyn Clock>) -> RateLimiter {
        RateLimiter::bounded(rpm, 10_000, clock)
    }

    fn bounded(rpm: u64, max_entries: usize, clock: Arc<dyn Clock>) -> RateLimiter {
        RateLimiter {
            max_requests_per_window: rpm,
            window_secs: 60,
            clock,
            entries: Mutex::new(HashMap::new()),
            max_entries,
        }
    }

    /// Returns `Ok(())` when the slot is granted or `Err(retry_after)` in
    /// seconds until the current window closes when the quota is exhausted.
    pub fn take_slot(&self, client: IpAddr) -> Result<(), u64> {
        let now = self.clock.now();
        let open = now.div_euclid(self.window_secs) * self.window_secs;
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let window = entries.entry(client).or_insert(Window {
            opened_at: open,
            used: 0,
        });
        if window.opened_at != open {
            *window = Window {
                opened_at: open,
                used: 0,
            };
        }
        if window.used >= self.max_requests_per_window {
            let retry_after = (open + self.window_secs - now).clamp(1, self.window_secs) as u64;
            return Err(retry_after);
        }
        window.used += 1;
        self.bound_storage(&mut entries, open);
        Ok(())
    }

    fn bound_storage(&self, entries: &mut HashMap<IpAddr, Window>, current_open: i64) {
        if entries.len() < self.max_entries {
            return;
        }
        entries.retain(|_, window| window.opened_at == current_open);
        if entries.len() >= self.max_entries {
            // Do not clear the table: doing so would let a client bypass the
            // quota by flooding distinct source addresses. Evict one slot.
            if let Some(key) = entries.keys().next().copied() {
                entries.remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;

    fn limiter(clock: TestClock, rpm: u64) -> RateLimiter {
        RateLimiter::per_minute(rpm, Arc::new(clock.clone()))
    }

    #[test]
    fn slots_are_granted_until_quota_exhausted() {
        let clock = TestClock::at_start(1000);
        let limiter = limiter(clock.clone(), 2);
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        assert!(limiter.take_slot(ip).is_ok());
        assert!(limiter.take_slot(ip).is_ok());
        let retry = limiter.take_slot(ip).unwrap_err();
        assert_eq!(retry, 20, "time left in the aligned window");
    }

    #[test]
    fn windows_reopen_after_the_minute() {
        let clock = TestClock::at_start(1000);
        let limiter = limiter(clock.clone(), 1);
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        assert!(limiter.take_slot(ip).is_ok());
        clock.advance(61);
        assert!(limiter.take_slot(ip).is_ok());
    }

    #[test]
    fn client_ips_are_limit_independent() {
        let clock = TestClock::at_start(1000);
        let limiter = limiter(clock.clone(), 1);
        let a: IpAddr = "192.0.2.1".parse().unwrap();
        let b: IpAddr = "192.0.2.2".parse().unwrap();
        assert!(limiter.take_slot(a).is_ok());
        assert!(limiter.take_slot(a).is_err());
        assert!(limiter.take_slot(b).is_ok());
    }

    #[test]
    fn stale_sweep_runs_when_storage_hits_the_bound() {
        let clock = Arc::new(TestClock::at_start(1000));
        let limiter = RateLimiter::bounded(10, 2, clock.clone());
        for i in 0..3 {
            let ip: IpAddr = format!("192.0.2.{i}").parse().unwrap();
            let _ = limiter.take_slot(ip);
        }
        let count = limiter.entries.lock().expect("lock").len();
        assert!(count <= 2, "storage swept at the bound, got {count}");
    }
}
