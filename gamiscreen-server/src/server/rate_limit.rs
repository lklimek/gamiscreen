use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Instant;

/// Maximum login attempts per IP within the time window.
const MAX_ATTEMPTS: usize = 10;
/// Time window for rate limiting (in seconds).
const WINDOW_SECS: u64 = 60;

/// Simple in-memory per-IP rate limiter for login attempts.
///
/// Tracks timestamps of recent attempts per IP address. Attempts older than the
/// configured window are pruned on each check. Thread-safe via `std::sync::Mutex`
/// (non-async — held only briefly for HashMap operations).
#[derive(Debug)]
pub struct LoginRateLimiter {
    attempts: Mutex<HashMap<IpAddr, Vec<Instant>>>,
    max_attempts: usize,
    window_secs: u64,
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new(MAX_ATTEMPTS, WINDOW_SECS)
    }
}

impl LoginRateLimiter {
    /// Create a new rate limiter with custom limits.
    pub fn new(max_attempts: usize, window_secs: u64) -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
            max_attempts,
            window_secs,
        }
    }

    /// Record a login attempt from the given IP and return whether it is allowed.
    ///
    /// Returns `Ok(())` if the attempt is within limits, or `Err(seconds)` with
    /// the number of seconds the caller should wait before retrying.
    pub fn check_rate_limit(&self, ip: IpAddr) -> Result<(), u64> {
        let now = Instant::now();
        let window = std::time::Duration::from_secs(self.window_secs);

        let mut map = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        let entries = map.entry(ip).or_default();

        // Prune expired entries
        entries.retain(|&t| now.duration_since(t) < window);

        if entries.len() >= self.max_attempts {
            // Calculate retry-after from oldest entry in window
            let oldest = entries[0];
            let elapsed = now.duration_since(oldest);
            let retry_after = if elapsed < window {
                (window - elapsed).as_secs() + 1
            } else {
                1
            };
            return Err(retry_after);
        }

        entries.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn localhost() -> IpAddr {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }

    fn other_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))
    }

    #[test]
    fn allows_attempts_under_limit() {
        let limiter = LoginRateLimiter::new(3, 60);
        assert!(limiter.check_rate_limit(localhost()).is_ok());
        assert!(limiter.check_rate_limit(localhost()).is_ok());
        assert!(limiter.check_rate_limit(localhost()).is_ok());
    }

    #[test]
    fn blocks_after_limit_exceeded() {
        let limiter = LoginRateLimiter::new(2, 60);
        assert!(limiter.check_rate_limit(localhost()).is_ok());
        assert!(limiter.check_rate_limit(localhost()).is_ok());
        let result = limiter.check_rate_limit(localhost());
        assert!(result.is_err());
        // retry_after should be > 0
        assert!(result.unwrap_err() > 0);
    }

    #[test]
    fn separate_limits_per_ip() {
        let limiter = LoginRateLimiter::new(1, 60);
        assert!(limiter.check_rate_limit(localhost()).is_ok());
        // localhost is now blocked
        assert!(limiter.check_rate_limit(localhost()).is_err());
        // but other IP is fine
        assert!(limiter.check_rate_limit(other_ip()).is_ok());
    }

    #[test]
    fn window_expiry_allows_new_attempts() {
        // Use a very short window so entries expire immediately
        let limiter = LoginRateLimiter::new(1, 0);
        assert!(limiter.check_rate_limit(localhost()).is_ok());
        // With window_secs=0, all previous entries are expired
        assert!(limiter.check_rate_limit(localhost()).is_ok());
    }
}
