use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use crate::clock::Clock;

/// Process-local sliding-window limits. Multiple windows share one history,
/// so a short burst limit and a daily budget are enforced atomically.
/// The key table is bounded; live entries are never evicted to admit new keys.
pub struct RateLimiter<K = IpAddr> {
    windows: Vec<(u64, i64)>,
    longest_window: i64,
    clock: Arc<dyn Clock>,
    entries: Mutex<HashMap<K, VecDeque<i64>>>,
    max_entries: usize,
}

impl<K: Eq + Hash> RateLimiter<K> {
    pub fn per_minute(rpm: u64, clock: Arc<dyn Clock>) -> Self {
        Self::with_windows(&[(rpm, 60)], clock)
    }

    /// Each pair is (maximum accepted requests, rolling window in seconds).
    pub fn with_windows(windows: &[(u64, i64)], clock: Arc<dyn Clock>) -> Self {
        Self::bounded(windows, 10_000, clock)
    }

    fn bounded(windows: &[(u64, i64)], max_entries: usize, clock: Arc<dyn Clock>) -> Self {
        let longest_window = windows
            .iter()
            .map(|(_, seconds)| *seconds)
            .max()
            .unwrap_or(0);
        assert!(longest_window > 0 && windows.iter().all(|(_, seconds)| *seconds > 0));
        Self {
            windows: windows.to_vec(),
            longest_window,
            clock,
            entries: Mutex::new(HashMap::new()),
            max_entries,
        }
    }

    /// Rejections do not extend the window. Returns the wait until every
    /// exhausted budget permits another request, suitable for Retry-After.
    pub fn take_slot(&self, key: K) -> Result<(), u64> {
        let now = self.clock.now();
        let cutoff = now.saturating_sub(self.longest_window);
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if !entries.contains_key(&key) && entries.len() >= self.max_entries {
            entries.retain(|_, requests| requests.back().is_some_and(|last| *last > cutoff));
            if entries.len() >= self.max_entries {
                // Rotating source addresses must not reset a blocked client's
                // counter. Reject new keys until an existing slot expires.
                let retry_after = entries
                    .values()
                    .filter_map(|requests| requests.back())
                    .map(|last| last.saturating_add(self.longest_window).saturating_sub(now))
                    .min()
                    .unwrap_or(self.longest_window)
                    .max(1);
                return Err(retry_after as u64);
            }
        }

        let requests = entries.entry(key).or_default();
        while requests.front().is_some_and(|oldest| *oldest <= cutoff) {
            requests.pop_front();
        }
        let mut retry_after = 0;
        for &(limit, seconds) in &self.windows {
            if limit == 0 {
                retry_after = retry_after.max(seconds);
                continue;
            }
            let window_start = now.saturating_sub(seconds);
            let mut count = 0u64;
            for &accepted_at in requests.iter().rev() {
                if accepted_at <= window_start {
                    break;
                }
                count += 1;
                if count == limit {
                    retry_after = retry_after.max(
                        accepted_at
                            .saturating_add(seconds)
                            .saturating_sub(now)
                            .max(1),
                    );
                    break;
                }
            }
        }
        if retry_after > 0 {
            return Err(retry_after as u64);
        }
        // Keep history ordered even if the wall clock steps backwards.
        // Future-dated entries expire conservatively instead of being
        // hidden behind an older timestamp in the reverse scan.
        requests.push_back(requests.back().copied().unwrap_or(now).max(now));
        Ok(())
    }
}

/// IPv6 privacy-address rotation inside one /64 must not create fresh
/// signup/upload budgets. Mapped IPv4 addresses share their native key.
pub fn ip_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.into(),
            None => std::net::Ipv6Addr::from(u128::from(v6) & (u128::MAX << 64)).into(),
        },
        v4 => v4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;

    fn limiter(clock: TestClock, rpm: u64) -> RateLimiter {
        RateLimiter::per_minute(rpm, Arc::new(clock))
    }

    #[test]
    fn slots_are_granted_until_quota_exhausted() {
        let clock = TestClock::at_start(1000);
        let limiter = limiter(clock.clone(), 2);
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        assert!(limiter.take_slot(ip).is_ok());
        assert!(limiter.take_slot(ip).is_ok());
        assert_eq!(limiter.take_slot(ip), Err(60));
        clock.advance(10);
        assert_eq!(limiter.take_slot(ip), Err(50));
    }

    #[test]
    fn windows_reopen_after_the_minute() {
        let clock = TestClock::at_start(1000);
        let limiter = limiter(clock.clone(), 1);
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        assert!(limiter.take_slot(ip).is_ok());
        clock.advance(59);
        assert_eq!(limiter.take_slot(ip), Err(1));
        clock.advance(1);
        assert!(limiter.take_slot(ip).is_ok());
    }

    #[test]
    fn minute_boundaries_do_not_reset_a_recent_burst() {
        let clock = TestClock::at_start(1019);
        let limiter = limiter(clock.clone(), 1);
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        assert!(limiter.take_slot(ip).is_ok());
        clock.advance(1);
        assert_eq!(limiter.take_slot(ip), Err(59));
    }

    #[test]
    fn clock_rollback_cannot_hide_unexpired_requests() {
        let clock = TestClock::at_start(1000);
        let limiter = limiter(clock.clone(), 2);
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        assert!(limiter.take_slot(ip).is_ok());
        clock.set(995);
        assert!(limiter.take_slot(ip).is_ok());
        clock.set(1055);
        assert_eq!(limiter.take_slot(ip), Err(5));
        clock.set(1060);
        assert!(limiter.take_slot(ip).is_ok());
    }

    #[test]
    fn client_ips_are_limit_independent() {
        let limiter = limiter(TestClock::at_start(1000), 1);
        let a: IpAddr = "192.0.2.1".parse().unwrap();
        let b: IpAddr = "192.0.2.2".parse().unwrap();
        assert!(limiter.take_slot(a).is_ok());
        assert!(limiter.take_slot(a).is_err());
        assert!(limiter.take_slot(b).is_ok());
    }

    #[test]
    fn short_and_long_windows_share_a_budget() {
        let clock = Arc::new(TestClock::at_start(1000));
        let limiter: RateLimiter = RateLimiter::with_windows(&[(2, 60), (3, 3600)], clock.clone());
        let ip = "192.0.2.1".parse().unwrap();
        assert!(limiter.take_slot(ip).is_ok());
        assert!(limiter.take_slot(ip).is_ok());
        assert_eq!(limiter.take_slot(ip), Err(60));
        clock.advance(60);
        assert!(limiter.take_slot(ip).is_ok());
        assert_eq!(limiter.take_slot(ip), Err(3540));
        clock.advance(3540);
        assert!(limiter.take_slot(ip).is_ok());
    }

    #[test]
    fn full_table_keeps_live_counters_and_reclaims_expired_slots() {
        let clock = Arc::new(TestClock::at_start(1000));
        let limiter = RateLimiter::bounded(&[(1, 60)], 2, clock.clone());
        let a: IpAddr = "192.0.2.1".parse().unwrap();
        let b: IpAddr = "192.0.2.2".parse().unwrap();
        let c: IpAddr = "192.0.2.3".parse().unwrap();
        assert!(limiter.take_slot(a).is_ok());
        assert!(limiter.take_slot(b).is_ok());
        assert_eq!(limiter.take_slot(c), Err(60));
        assert_eq!(limiter.take_slot(a), Err(60));
        assert_eq!(limiter.entries.lock().unwrap().len(), 2);
        clock.advance(60);
        assert!(limiter.take_slot(c).is_ok());
        assert!(limiter.take_slot(a).is_ok());
        assert_eq!(limiter.entries.lock().unwrap().len(), 2);
    }

    #[test]
    fn concurrent_requests_cannot_overshoot_the_budget() {
        let clock = TestClock::at_start(1000);
        let limiter = Arc::new(limiter(clock.clone(), 37));
        let barrier = Arc::new(std::sync::Barrier::new(33));
        let workers: Vec<_> = (0..32)
            .map(|_| {
                let limiter = limiter.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut accepted = 0;
                    for _ in 0..128 {
                        match limiter.take_slot("192.0.2.1".parse().unwrap()) {
                            Ok(()) => accepted += 1,
                            Err(wait) => assert_eq!(wait, 60),
                        }
                    }
                    accepted
                })
            })
            .collect();
        barrier.wait();
        let accepted: usize = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum();
        assert_eq!(
            accepted, 37,
            "4,096 simultaneous attempts share one exact budget"
        );
        clock.advance(60);
        for _ in 0..37 {
            assert!(limiter.take_slot("192.0.2.1".parse().unwrap()).is_ok());
        }
        assert_eq!(limiter.take_slot("192.0.2.1".parse().unwrap()), Err(60));
    }

    #[test]
    fn ten_thousand_live_keys_survive_source_rotation_without_budget_resets() {
        let clock = Arc::new(TestClock::at_start(1000));
        let limiter = RateLimiter::<u32>::per_minute(1, clock.clone());
        for key in 0..10_000 {
            assert!(limiter.take_slot(key).is_ok());
        }
        for key in 10_000..10_512 {
            assert_eq!(limiter.take_slot(key), Err(60));
            assert_eq!(
                limiter.take_slot(0),
                Err(60),
                "source churn must not evict a live counter"
            );
        }
        clock.advance(59);
        assert_eq!(limiter.take_slot(20_000), Err(1));
        clock.advance(1);
        assert!(limiter.take_slot(20_000).is_ok());
        assert!(limiter.take_slot(0).is_ok());
    }

    #[test]
    fn randomized_requests_match_an_independent_rolling_window_model() {
        let clock = Arc::new(TestClock::at_start(1_000_000));
        let limits = [(23u64, 300i64), (3, 7), (11, 61)];
        let limiter = RateLimiter::<u8>::with_windows(&limits, clock.clone());
        // The reference keeps the entire accepted history and filters each
        // window independently; it does not prune or reverse-scan queues.
        let mut accepted: HashMap<u8, Vec<i64>> = HashMap::new();
        let mut seed = 0x5a17_29ce_84b3_d601u64;
        let mut granted = 0;
        let mut denied = 0;
        for step in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            if step % 512 == 0 {
                clock.advance(100);
            } else if seed & 15 == 0 {
                clock.advance(1);
            }
            let now = clock.now();
            let key = ((seed >> 8) & 15) as u8;
            let history = accepted.entry(key).or_default();
            let mut wait = 0;
            for &(limit, seconds) in &limits {
                let live: Vec<_> = history
                    .iter()
                    .copied()
                    .filter(|at| *at > now - seconds)
                    .collect();
                if live.len() >= limit as usize {
                    wait = wait.max(live[live.len() - limit as usize] + seconds - now);
                }
            }
            let expected = if wait > 0 { Err(wait as u64) } else { Ok(()) };
            assert_eq!(
                limiter.take_slot(key),
                expected,
                "request {step}, key {key}, time {now}"
            );
            if expected.is_ok() {
                history.push(now);
                granted += 1;
            } else {
                denied += 1;
            }
        }
        assert!(
            granted > 1000 && denied > 1000,
            "exercise both outcomes: {granted}/{denied}"
        );
    }

    #[test]
    fn ipv6_privacy_addresses_and_mapped_ipv4_share_keys() {
        assert_eq!(
            ip_key("2001:db8:1:2::1".parse().unwrap()),
            ip_key("2001:db8:1:2:ffff::abcd".parse().unwrap())
        );
        assert_ne!(
            ip_key("2001:db8:1:2::1".parse().unwrap()),
            ip_key("2001:db8:1:3::1".parse().unwrap())
        );
        assert_eq!(
            ip_key("::ffff:192.0.2.1".parse().unwrap()),
            ip_key("192.0.2.1".parse().unwrap())
        );
    }
}
