//! Relay state management and metrics.
//!
//! This module provides shared state for the relay runtime, including metrics
//! tracking, client management, and quality monitoring.

use bytes::Bytes;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use super::QbtRelayConfig;
use crate::qbt_receiver::protocol::server_list_wire::build_server_list_wire;
use crate::runtime_support::lock_unpoisoned;

/// Relay metrics tracked via atomic counters.
#[derive(Default)]
pub(super) struct Metrics {
    pub(super) upstream_connection_attempts_total: AtomicU64,
    pub(super) upstream_connection_success_total: AtomicU64,
    pub(super) upstream_connection_fail_total: AtomicU64,
    pub(super) upstream_disconnect_total: AtomicU64,
    pub(super) downstream_connections_accepted_total: AtomicU64,
    pub(super) downstream_connections_rejected_over_capacity_total: AtomicU64,
    pub(super) downstream_disconnect_auth_timeout_total: AtomicU64,
    pub(super) downstream_disconnect_slow_client_total: AtomicU64,
    pub(super) downstream_disconnect_lagged_total: AtomicU64,
    pub(super) downstream_active_clients: AtomicU64,
    pub(super) bytes_in_total: AtomicU64,
    pub(super) bytes_attempted_total: AtomicU64,
    pub(super) bytes_forwarded_total: AtomicU64,
    pub(super) bytes_dropped_total: AtomicU64,
    pub(super) forwarding_paused: AtomicBool,
    pub(super) forwarding_pause_events_total: AtomicU64,
    pub(super) rolling_quality_milli: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct QbtRelayClientMeta {
    pub email: String,
    pub peer: String,
    pub connected_at_unix_secs: u64,
    pub last_auth_unix_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct QbtRelayMetricsSnapshot {
    upstream_connection_attempts_total: u64,
    upstream_connection_success_total: u64,
    upstream_connection_fail_total: u64,
    upstream_disconnect_total: u64,
    downstream_connections_accepted_total: u64,
    downstream_connections_rejected_over_capacity_total: u64,
    downstream_disconnect_auth_timeout_total: u64,
    downstream_disconnect_slow_client_total: u64,
    downstream_disconnect_lagged_total: u64,
    downstream_active_clients: u64,
    bytes_in_total: u64,
    bytes_attempted_total: u64,
    bytes_forwarded_total: u64,
    bytes_dropped_total: u64,
    forwarding_paused: bool,
    forwarding_pause_events_total: u64,
    rolling_quality: f64,
    active_users: Vec<QbtRelayClientMeta>,
}

#[derive(Debug, Serialize)]
pub struct QbtRelayHealthSnapshot {
    pub status: &'static str,
    pub forwarding_paused: bool,
    pub downstream_active_clients: u64,
}

/// Quality tracking bucket for a single time window.
#[derive(Default, Clone, Copy)]
struct QualityBucket {
    attempted: u64,
    forwarded: u64,
}

/// Rolling window for tracking forwarding quality.
pub(super) struct QualityWindow {
    buckets: Vec<QualityBucket>,
    index: usize,
}

impl QualityWindow {
    /// Creates a new quality window with the specified number of buckets.
    pub(super) fn new(size: usize) -> Self {
        let window_size = size.max(1);
        Self {
            buckets: vec![QualityBucket::default(); window_size],
            index: 0,
        }
    }

    /// Rotates to the next bucket, clearing it for new data.
    pub(super) fn rotate(&mut self) {
        self.index = (self.index + 1) % self.buckets.len();
        self.buckets[self.index] = QualityBucket::default();
    }

    /// Records attempted bytes in the current bucket.
    pub(super) fn add_attempted(&mut self, bytes: u64) {
        self.buckets[self.index].attempted =
            self.buckets[self.index].attempted.saturating_add(bytes);
    }

    /// Records successfully forwarded bytes in the current bucket.
    pub(super) fn add_forwarded(&mut self, bytes: u64) {
        self.buckets[self.index].forwarded =
            self.buckets[self.index].forwarded.saturating_add(bytes);
    }

    /// Calculates the forwarding quality ratio (0.0 to 1.0).
    ///
    /// Returns 1.0 if no bytes have been attempted (empty window).
    pub(super) fn ratio(&self) -> f64 {
        let attempted = self
            .buckets
            .iter()
            .fold(0_u64, |sum, bucket| sum.saturating_add(bucket.attempted));
        let forwarded = self
            .buckets
            .iter()
            .fold(0_u64, |sum, bucket| sum.saturating_add(bucket.forwarded));
        if attempted == 0 {
            1.0
        } else {
            forwarded as f64 / attempted as f64
        }
    }
}

/// Shared state for the relay runtime.
pub struct QbtRelayState {
    /// Metrics counters for telemetry.
    pub(super) metrics: Metrics,
    /// Connected client metadata keyed by client ID.
    pub(super) clients: Mutex<HashMap<u64, QbtRelayClientMeta>>,
    /// Counter for generating unique client IDs.
    pub(super) next_client_id: AtomicU64,
    /// Rolling quality window for monitoring.
    pub(super) quality_window: Mutex<QualityWindow>,
    /// Cached server list wire frame to send to new clients.
    latest_server_list_wire: RwLock<Bytes>,
}

impl QbtRelayState {
    /// Creates relay state from validated relay configuration.
    pub fn new(config: &QbtRelayConfig) -> Self {
        Self::from_upstream_servers(&config.upstream_servers, config.quality_window_secs)
    }

    /// Creates state from upstream servers list.
    ///
    /// Initializes the server list wire frame and quality window.
    pub fn from_upstream_servers(servers: &[(String, u16)], quality_window_secs: usize) -> Self {
        Self {
            metrics: Metrics::default(),
            clients: Mutex::new(HashMap::new()),
            next_client_id: AtomicU64::new(1),
            quality_window: Mutex::new(QualityWindow::new(quality_window_secs)),
            latest_server_list_wire: RwLock::new(build_server_list_wire(servers)),
        }
    }

    /// Returns the cached server list wire frame.
    pub fn latest_server_list_wire(&self) -> Bytes {
        self.latest_server_list_wire
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Captures a snapshot of current metrics.
    pub fn metrics_snapshot(&self) -> QbtRelayMetricsSnapshot {
        let users = lock_unpoisoned(&self.clients)
            .values()
            .cloned()
            .collect::<Vec<_>>();
        QbtRelayMetricsSnapshot {
            upstream_connection_attempts_total: self
                .metrics
                .upstream_connection_attempts_total
                .load(Ordering::Relaxed),
            upstream_connection_success_total: self
                .metrics
                .upstream_connection_success_total
                .load(Ordering::Relaxed),
            upstream_connection_fail_total: self
                .metrics
                .upstream_connection_fail_total
                .load(Ordering::Relaxed),
            upstream_disconnect_total: self
                .metrics
                .upstream_disconnect_total
                .load(Ordering::Relaxed),
            downstream_connections_accepted_total: self
                .metrics
                .downstream_connections_accepted_total
                .load(Ordering::Relaxed),
            downstream_connections_rejected_over_capacity_total: self
                .metrics
                .downstream_connections_rejected_over_capacity_total
                .load(Ordering::Relaxed),
            downstream_disconnect_auth_timeout_total: self
                .metrics
                .downstream_disconnect_auth_timeout_total
                .load(Ordering::Relaxed),
            downstream_disconnect_slow_client_total: self
                .metrics
                .downstream_disconnect_slow_client_total
                .load(Ordering::Relaxed),
            downstream_disconnect_lagged_total: self
                .metrics
                .downstream_disconnect_lagged_total
                .load(Ordering::Relaxed),
            downstream_active_clients: self
                .metrics
                .downstream_active_clients
                .load(Ordering::Relaxed),
            bytes_in_total: self.metrics.bytes_in_total.load(Ordering::Relaxed),
            bytes_attempted_total: self.metrics.bytes_attempted_total.load(Ordering::Relaxed),
            bytes_forwarded_total: self.metrics.bytes_forwarded_total.load(Ordering::Relaxed),
            bytes_dropped_total: self.metrics.bytes_dropped_total.load(Ordering::Relaxed),
            forwarding_paused: self.metrics.forwarding_paused.load(Ordering::Relaxed),
            forwarding_pause_events_total: self
                .metrics
                .forwarding_pause_events_total
                .load(Ordering::Relaxed),
            rolling_quality: self.metrics.rolling_quality_milli.load(Ordering::Relaxed) as f64
                / 1000.0,
            active_users: users,
        }
    }

    /// Captures a health status snapshot.
    pub fn health_snapshot(&self) -> QbtRelayHealthSnapshot {
        QbtRelayHealthSnapshot {
            status: "ok",
            forwarding_paused: self.metrics.forwarding_paused.load(Ordering::Relaxed),
            downstream_active_clients: self
                .metrics
                .downstream_active_clients
                .load(Ordering::Relaxed),
        }
    }

    pub(super) fn add_attempted(&self, bytes: u64) {
        self.metrics
            .bytes_attempted_total
            .fetch_add(bytes, Ordering::Relaxed);
        let mut window = lock_unpoisoned(&self.quality_window);
        window.add_attempted(bytes);
    }

    pub(super) fn add_forwarded(&self, bytes: u64) {
        self.metrics
            .bytes_forwarded_total
            .fetch_add(bytes, Ordering::Relaxed);
        let mut window = lock_unpoisoned(&self.quality_window);
        window.add_forwarded(bytes);
    }

    pub(super) fn set_latest_server_list_wire(&self, bytes: Bytes) {
        let mut guard = self
            .latest_server_list_wire
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::QbtRelayState;
    use crate::qbt_receiver::QbtRelayConfig;
    use std::net::SocketAddr;
    use std::time::Duration;

    #[test]
    fn new_initializes_server_list_wire_and_quality_window_from_config() {
        let config = QbtRelayConfig {
            email: "relay@example.com".to_string(),
            upstream_servers: vec![
                ("primary.example".to_string(), 2211),
                ("backup.example".to_string(), 2212),
            ],
            bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("valid socket"),
            max_clients: 10,
            auth_timeout: Duration::from_secs(720),
            client_buffer_bytes: 65_536,
            reconnect_delay: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(5),
            quality_window_secs: 30,
            quality_pause_threshold: 0.95,
            metrics_log_interval: Duration::from_secs(30),
        };

        let state = QbtRelayState::new(&config);
        let decoded_server_list = state
            .latest_server_list_wire()
            .iter()
            .map(|byte| byte ^ 0xFF)
            .collect::<Vec<_>>();

        assert_eq!(
            decoded_server_list,
            b"/ServerList/primary.example:2211|backup.example:2212\0".to_vec()
        );

        let bucket_count = state
            .quality_window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .buckets
            .len();
        assert_eq!(bucket_count, 30);
    }
}
