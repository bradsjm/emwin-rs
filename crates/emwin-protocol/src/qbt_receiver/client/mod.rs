//! QbtReceiver runtime for EMWIN protocol connections.
//!
//! This module provides a full-featured client implementation with:
//! - Connection management with timeout and retry
//! - Automatic reconnection with endpoint rotation
//! - Authentication heartbeat
//! - Watchdog health monitoring
//! - Event streaming with backpressure handling
//! - Server list persistence and management

pub mod connection;
mod runtime;
pub mod server_list_manager;
pub mod watchdog;

use crate::qbt_receiver::config::QbtReceiverConfig;
use crate::qbt_receiver::error::{QbtReceiverError, QbtReceiverResult};
use crate::qbt_receiver::protocol::model::QbtFrameEvent;
use crate::runtime_support::{BackpressureTracker, ReceiverEventStream, ReceiverRuntime};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

use self::runtime::run_connection_loop;

/// Capacity of the event channel between client and consumers.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Interval between telemetry snapshot emissions (in seconds).
const TELEMETRY_EMIT_INTERVAL_SECS: u64 = 5;

/// Maximum connection timeout (in seconds).
const MAX_CONNECT_TIMEOUT_SECS: u64 = 5;

/// Snapshot of client telemetry counters.
///
/// This structure tracks various metrics about the client's operation,
/// useful for monitoring and debugging.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "telemetry-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[non_exhaustive]
pub struct QbtReceiverTelemetrySnapshot {
    /// Total connection attempts made.
    pub connection_attempts_total: u64,
    /// Total successful connections.
    pub connection_success_total: u64,
    /// Total failed connection attempts.
    pub connection_fail_total: u64,
    /// Total disconnections (expected and unexpected).
    pub disconnect_total: u64,
    /// Total watchdog timeouts.
    pub watchdog_timeouts_total: u64,
    /// Total watchdog exception events.
    pub watchdog_exception_events_total: u64,
    /// Total authentication logon messages sent.
    pub auth_logon_sent_total: u64,
    /// Total bytes received.
    pub bytes_in_total: u64,
    /// Total frame events decoded.
    pub frame_events_total: u64,
    /// Total data blocks emitted to handlers.
    pub data_blocks_emitted_total: u64,
    /// Total server list updates received.
    pub server_list_updates_total: u64,
    /// Total checksum mismatches detected.
    pub checksum_mismatch_total: u64,
    /// Total decompression failures.
    pub decompression_failed_total: u64,
    /// Total decoder recovery events.
    pub decoder_recovery_events_total: u64,
    /// Total handler failures.
    pub handler_failures_total: u64,
    /// Total backpressure warnings emitted.
    pub backpressure_warning_emitted_total: u64,
    /// Total events dropped due to channel full.
    pub event_queue_drop_total: u64,
    /// Total telemetry events emitted.
    pub telemetry_events_emitted_total: u64,
}

/// Internal runtime telemetry with tracking for backpressure reporting.
#[derive(Debug, Default)]
struct RuntimeTelemetry {
    /// Current snapshot of counters.
    snapshot: QbtReceiverTelemetrySnapshot,
    /// Shared backpressure/drop accounting.
    backpressure: BackpressureTracker,
}

/// Events emitted by the EMWIN client.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum QbtReceiverEvent {
    /// A protocol frame event (data block, server list, or warning).
    Frame(QbtFrameEvent),
    /// Connected to a server endpoint.
    Connected(String),
    /// Disconnected from the current endpoint.
    Disconnected,
    /// Periodic telemetry snapshot.
    Telemetry(QbtReceiverTelemetrySnapshot),
}

/// Type alias for event handler callbacks.
///
/// Handlers receive frame events and can return errors which will be
/// converted to warnings and emitted to other handlers.
pub type QbtReceiverEventHandler =
    Arc<dyn Fn(&QbtFrameEvent) -> QbtReceiverResult<()> + Send + Sync>;

/// Trait for EMWIN client implementations.
///
/// This trait defines the interface for starting, stopping, and
/// receiving events from a client connection.
pub trait QbtReceiverClient: Send {
    /// Starts the client connection loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is already running.
    fn start(&mut self) -> QbtReceiverResult<()>;

    /// Stops the client and cleans up resources.
    ///
    /// # Errors
    ///
    /// Returns an error if cleanup fails.
    fn stop(
        &mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = QbtReceiverResult<()>> + Send + '_>>;

    /// Returns a stream of client events.
    ///
    /// This can only be called once; subsequent calls return an error.
    fn events(
        &mut self,
    ) -> Result<ReceiverEventStream<QbtReceiverEvent, QbtReceiverError>, QbtReceiverError>;
}

/// Builder for constructing a [`QbtReceiver`] with validation.
#[derive(Debug, Clone)]
pub struct QbtReceiverBuilder {
    config: QbtReceiverConfig,
}

impl QbtReceiverBuilder {
    /// Creates a new client builder with the given configuration.
    pub fn new(config: QbtReceiverConfig) -> Self {
        Self { config }
    }

    /// Builds a [`QbtReceiver`] after validating the configuration.
    ///
    /// # Errors
    ///
    /// Returns a [`QbtReceiverConfigError`](crate::qbt_receiver::error::QbtReceiverConfigError) if validation fails.
    pub fn build(self) -> Result<QbtReceiver, QbtReceiverError> {
        self.config.validate()?;
        Ok(QbtReceiver {
            config: self.config,
            runtime: ReceiverRuntime::default(),
            handlers: Vec::new(),
            telemetry: Arc::new(Mutex::new(QbtReceiverTelemetrySnapshot::default())),
        })
    }
}

/// EMWIN client implementation.
///
/// This is the main client type that manages the connection lifecycle,
/// event streaming, and telemetry. Use [`QbtReceiver::builder`] to construct
/// an instance with validated configuration.
pub struct QbtReceiver {
    /// QbtReceiver configuration.
    config: QbtReceiverConfig,
    /// Shared runtime lifecycle and event channel state.
    runtime: ReceiverRuntime<QbtReceiverEvent, QbtReceiverError>,
    /// Registered event handlers.
    handlers: Vec<QbtReceiverEventHandler>,
    /// Shared telemetry snapshot.
    telemetry: Arc<Mutex<QbtReceiverTelemetrySnapshot>>,
}

impl std::fmt::Debug for QbtReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QbtReceiver")
            .field("config", &self.config)
            .field("running", &self.runtime.is_running())
            .field("handler_count", &self.handlers.len())
            .finish()
    }
}

impl QbtReceiver {
    /// Creates a client builder with the given configuration.
    pub fn builder(config: QbtReceiverConfig) -> QbtReceiverBuilder {
        QbtReceiverBuilder::new(config)
    }

    pub fn config(&self) -> &QbtReceiverConfig {
        &self.config
    }

    pub fn subscribe(&mut self, handler: QbtReceiverEventHandler) {
        self.handlers.push(handler);
    }

    pub fn telemetry_snapshot(&self) -> QbtReceiverTelemetrySnapshot {
        self.telemetry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl QbtReceiverClient for QbtReceiver {
    fn start(&mut self) -> QbtReceiverResult<()> {
        if self.runtime.is_running() {
            return Err(QbtReceiverError::Lifecycle(
                "client already running".to_string(),
            ));
        }

        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let config = self.config.clone();
        let handlers = self.handlers.clone();
        let telemetry = Arc::clone(&self.telemetry);

        let join_handle = tokio::spawn(async move {
            run_connection_loop(config, event_tx, shutdown_rx, handlers, telemetry).await;
        });

        self.runtime.install(event_rx, shutdown_tx, join_handle);
        Ok(())
    }

    fn stop(
        &mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = QbtReceiverResult<()>> + Send + '_>> {
        Box::pin(async move {
            self.runtime.stop().await;
            Ok(())
        })
    }

    fn events(
        &mut self,
    ) -> Result<ReceiverEventStream<QbtReceiverEvent, QbtReceiverError>, QbtReceiverError> {
        self.runtime.take_events(QbtReceiverError::Lifecycle(
            "event stream already taken".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::runtime::{dispatch_events, try_send_event};
    use super::{QbtReceiverEvent, QbtReceiverTelemetrySnapshot, RuntimeTelemetry};
    use crate::qbt_receiver::client::QbtReceiverEventHandler;
    use crate::qbt_receiver::error::QbtReceiverError;
    use crate::qbt_receiver::protocol::model::{QbtFrameEvent, QbtProtocolWarning, QbtServerList};
    use crate::runtime_support::BackpressureTracker;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn handler_error_isolated() {
        let called_ok = Arc::new(AtomicUsize::new(0));
        let called_ok_clone = Arc::clone(&called_ok);

        let bad: QbtReceiverEventHandler =
            Arc::new(|_evt: &QbtFrameEvent| -> Result<(), QbtReceiverError> {
                Err(QbtReceiverError::Lifecycle("boom".to_string()))
            });
        let good: QbtReceiverEventHandler = Arc::new(
            move |_evt: &QbtFrameEvent| -> Result<(), QbtReceiverError> {
                called_ok_clone.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        );

        let handlers = vec![bad, good];
        let (tx, mut rx) = mpsc::channel(16);
        let events = vec![QbtFrameEvent::ServerListUpdate(QbtServerList::default())];
        let mut telemetry = RuntimeTelemetry::default();

        dispatch_events(&tx, &handlers, events, &mut telemetry);

        let mut saw_warning = false;
        let mut saw_frame = false;
        while let Ok(item) =
            tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await
        {
            match item {
                Some(Ok(QbtReceiverEvent::Frame(QbtFrameEvent::Warning(
                    QbtProtocolWarning::HandlerError { .. },
                )))) => {
                    saw_warning = true;
                }
                Some(Ok(QbtReceiverEvent::Frame(QbtFrameEvent::ServerListUpdate(_)))) => {
                    saw_frame = true;
                }
                Some(_) => {}
                None => break,
            }
            if saw_warning && saw_frame {
                break;
            }
        }

        assert!(saw_warning);
        assert!(saw_frame);
        assert_eq!(called_ok.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn backpressure_drop_emits_warning_with_counters() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut telemetry = RuntimeTelemetry {
            snapshot: QbtReceiverTelemetrySnapshot {
                decoder_recovery_events_total: 3,
                event_queue_drop_total: 0,
                ..QbtReceiverTelemetrySnapshot::default()
            },
            backpressure: BackpressureTracker::default(),
        };

        tx.try_send(Ok(QbtReceiverEvent::Disconnected))
            .expect("seed event should fit");

        try_send_event(
            &tx,
            Ok(QbtReceiverEvent::Frame(QbtFrameEvent::ServerListUpdate(
                QbtServerList::default(),
            ))),
            &mut telemetry,
        );

        assert_eq!(telemetry.snapshot.event_queue_drop_total, 1);
        assert_eq!(telemetry.backpressure.dropped_since_last_report(), 1);

        let _ = rx.recv().await;

        try_send_event(
            &tx,
            Ok(QbtReceiverEvent::Frame(QbtFrameEvent::ServerListUpdate(
                QbtServerList::default(),
            ))),
            &mut telemetry,
        );

        let warning_item = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv())
            .await
            .expect("warning should be emitted before timeout")
            .expect("channel should still be open");

        match warning_item {
            Ok(QbtReceiverEvent::Frame(QbtFrameEvent::Warning(
                QbtProtocolWarning::BackpressureDrop {
                    dropped_since_last_report,
                    total_dropped_events,
                    decoder_recovery_events,
                },
            ))) => {
                assert_eq!(dropped_since_last_report, 1);
                assert_eq!(total_dropped_events, 1);
                assert_eq!(decoder_recovery_events, 3);
            }
            other => panic!("expected backpressure warning, got {other:?}"),
        }

        assert_eq!(telemetry.snapshot.event_queue_drop_total, 2);
        assert_eq!(telemetry.backpressure.dropped_since_last_report(), 1);
    }

    #[tokio::test]
    async fn backpressure_drop_warning_reports_and_resets_window() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut telemetry = RuntimeTelemetry {
            snapshot: QbtReceiverTelemetrySnapshot {
                decoder_recovery_events_total: 5,
                event_queue_drop_total: 7,
                ..QbtReceiverTelemetrySnapshot::default()
            },
            backpressure: BackpressureTracker::new(7, 2),
        };

        try_send_event(
            &tx,
            Ok(QbtReceiverEvent::Frame(QbtFrameEvent::ServerListUpdate(
                QbtServerList::default(),
            ))),
            &mut telemetry,
        );

        assert_eq!(telemetry.snapshot.event_queue_drop_total, 7);
        assert_eq!(telemetry.backpressure.dropped_since_last_report(), 0);

        let first = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv())
            .await
            .expect("first item should arrive")
            .expect("first item should exist");
        let second = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv())
            .await
            .expect("second item should arrive")
            .expect("second item should exist");

        match first {
            Ok(QbtReceiverEvent::Frame(QbtFrameEvent::Warning(
                QbtProtocolWarning::BackpressureDrop {
                    dropped_since_last_report,
                    total_dropped_events,
                    decoder_recovery_events,
                },
            ))) => {
                assert_eq!(dropped_since_last_report, 2);
                assert_eq!(total_dropped_events, 7);
                assert_eq!(decoder_recovery_events, 5);
            }
            other => panic!("expected first item to be warning, got {other:?}"),
        }

        assert!(matches!(
            second,
            Ok(QbtReceiverEvent::Frame(QbtFrameEvent::ServerListUpdate(_)))
        ));
    }
}
