//! QBT relay runtime for low-latency passthrough delivery.

mod auth;
mod runtime;
mod state;

use thiserror::Error;

pub use runtime::run as run_qbt_relay;
pub use state::{QbtRelayHealthSnapshot, QbtRelayMetricsSnapshot, QbtRelayState};

use std::net::SocketAddr;
use std::time::Duration;

/// Relay runtime configuration owned by `emwin-protocol`.
#[derive(Debug, Clone)]
pub struct QbtRelayConfig {
    /// Authentication email used when connecting upstream.
    pub email: String,
    /// Upstream QBT server endpoints to rotate through.
    pub upstream_servers: Vec<(String, u16)>,
    /// Downstream bind address for relay clients.
    pub bind_addr: SocketAddr,
    /// Maximum number of downstream clients.
    pub max_clients: usize,
    /// Timeout for downstream authentication.
    pub auth_timeout: Duration,
    /// Per-client outbound buffer budget.
    pub client_buffer_bytes: usize,
    /// Delay between upstream reconnect attempts.
    pub reconnect_delay: Duration,
    /// Timeout for establishing upstream connections.
    pub connect_timeout: Duration,
    /// Number of seconds of history used for quality tracking.
    pub quality_window_secs: usize,
    /// Threshold below which forwarding is paused.
    pub quality_pause_threshold: f64,
    /// Interval between relay metrics logs.
    pub metrics_log_interval: Duration,
}

impl QbtRelayConfig {
    /// Normalizes runtime bounds and clamps ratios to valid ranges.
    pub fn normalized(mut self) -> Self {
        self.max_clients = self.max_clients.max(1);
        self.auth_timeout = self.auth_timeout.max(Duration::from_secs(1));
        self.client_buffer_bytes = self.client_buffer_bytes.max(1);
        self.reconnect_delay = self.reconnect_delay.max(Duration::from_secs(1));
        self.connect_timeout = self.connect_timeout.max(Duration::from_secs(1));
        self.quality_window_secs = self.quality_window_secs.max(1);
        self.quality_pause_threshold = self.quality_pause_threshold.clamp(0.0, 1.0);
        self.metrics_log_interval = self.metrics_log_interval.max(Duration::from_secs(1));
        self
    }

    /// Validates the required relay configuration fields.
    pub fn validate(&self) -> QbtRelayResult<()> {
        if self.email.trim().is_empty() {
            return Err(QbtRelayError::Config("email must not be empty".to_string()));
        }
        if self.upstream_servers.is_empty() {
            return Err(QbtRelayError::Config(
                "at least one upstream server is required".to_string(),
            ));
        }
        Ok(())
    }
}

/// Result type returned by relay configuration and runtime operations.
pub type QbtRelayResult<T> = Result<T, QbtRelayError>;

/// Relay configuration and runtime failures.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum QbtRelayError {
    /// Invalid configuration supplied to the relay runtime.
    #[error("invalid relay config: {0}")]
    Config(String),
    /// A relay I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// A background relay task failed to join.
    #[error("relay task join failed: {0}")]
    TaskJoin(String),
}

#[cfg(test)]
mod tests {
    use super::QbtRelayConfig;
    use std::net::SocketAddr;
    use std::time::Duration;

    #[test]
    fn relay_config_normalization_applies_runtime_bounds() {
        let config = QbtRelayConfig {
            email: "relay@example.com".to_string(),
            upstream_servers: vec![("example.com".to_string(), 2211)],
            bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("valid socket"),
            max_clients: 0,
            auth_timeout: Duration::from_secs(0),
            client_buffer_bytes: 0,
            reconnect_delay: Duration::from_secs(0),
            connect_timeout: Duration::from_secs(0),
            quality_window_secs: 0,
            quality_pause_threshold: 10.0,
            metrics_log_interval: Duration::from_secs(0),
        }
        .normalized();

        assert_eq!(config.max_clients, 1);
        assert_eq!(config.auth_timeout, Duration::from_secs(1));
        assert_eq!(config.client_buffer_bytes, 1);
        assert_eq!(config.reconnect_delay, Duration::from_secs(1));
        assert_eq!(config.connect_timeout, Duration::from_secs(1));
        assert_eq!(config.quality_window_secs, 1);
        assert_eq!(config.quality_pause_threshold, 1.0);
        assert_eq!(config.metrics_log_interval, Duration::from_secs(1));
    }
}
