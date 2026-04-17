//! Track connection health and decide when the runtime should reconnect.
//!
//! The watchdog collapses two failure signals into one policy object: time since the last
//! successful read and the current streak of exceptions. That keeps the client loop simple and
//! makes reconnection policy easy to test in isolation.

use crate::runtime_support::lock_unpoisoned;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::time::{Duration, Instant};

/// Receives health signals from the client runtime.
pub trait HealthObserver: Send + Sync {
    /// Called when data is successfully received.
    fn on_data_received(&self);
    /// Called when an exception/error occurs.
    fn on_exception(&self);
    /// Returns true if the connection should be closed due to health issues.
    fn should_close(&self) -> bool;
}

/// Connection health policy used by the runtime read loop.
#[derive(Debug)]
pub struct Watchdog {
    /// Timeout duration for data reception.
    timeout: Duration,
    /// Maximum allowed consecutive exceptions.
    max_exceptions: u32,
    /// Current exception count.
    exception_count: AtomicU32,
    /// Timestamp of last data reception.
    last_data: Mutex<Instant>,
}

impl Watchdog {
    /// Creates a watchdog with a minimum timeout of one second.
    pub fn new(timeout_secs: u64, max_exceptions: u32) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs.max(1)),
            max_exceptions,
            exception_count: AtomicU32::new(0),
            last_data: Mutex::new(Instant::now()),
        }
    }

    /// Evaluates the close policy against a caller-supplied instant.
    ///
    /// Accepting `now` as an argument keeps the logic deterministic in tests.
    pub fn should_close_at(&self, now: Instant) -> bool {
        let exceptions = self.exception_count.load(Ordering::Relaxed);
        if exceptions > self.max_exceptions {
            return true;
        }

        let last = lock_unpoisoned(&self.last_data);
        now.duration_since(*last) > self.timeout
    }
}

impl HealthObserver for Watchdog {
    fn on_data_received(&self) {
        self.exception_count.store(0, Ordering::Relaxed);
        let mut last = lock_unpoisoned(&self.last_data);
        *last = Instant::now();
    }

    fn on_exception(&self) {
        self.exception_count.fetch_add(1, Ordering::Relaxed);
    }

    fn should_close(&self) -> bool {
        self.should_close_at(Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::{HealthObserver, Watchdog};
    use tokio::time::{Duration, Instant};

    #[test]
    fn watchdog_timeout_and_data_reset() {
        let w = Watchdog::new(2, 10);
        let now = Instant::now();
        assert!(!w.should_close_at(now + Duration::from_secs(1)));
        assert!(w.should_close_at(now + Duration::from_secs(3)));

        w.on_data_received();
        assert!(!w.should_close_at(now + Duration::from_secs(1)));
        assert!(w.should_close_at(now + Duration::from_secs(3)));
    }

    #[test]
    fn watchdog_exception_limit() {
        let w = Watchdog::new(100, 2);
        w.on_exception();
        w.on_exception();
        assert!(!w.should_close_at(Instant::now()));
        w.on_exception();
        assert!(w.should_close_at(Instant::now()));
    }
}
