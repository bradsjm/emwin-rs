use crate::runtime_support::{
    BackpressureTracker, ReceiverEventStream, ReceiverRuntime, lock_unpoisoned,
};
use crate::wxwire_receiver::config::WxWireReceiverConfig;
use crate::wxwire_receiver::error::{
    WxWireLifecycleError, WxWireReceiverError, WxWireReceiverResult,
};
use crate::wxwire_receiver::model::WxWireReceiverFrameEvent;
use crate::wxwire_receiver::transport::WxWireTransport;
mod runtime;
use self::runtime::{default_transport_factory, run_weather_wire_loop};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, watch};

#[cfg(not(test))]
const RECONNECT_BACKOFF_INITIAL: Duration = Duration::from_secs(1);
#[cfg(test)]
const RECONNECT_BACKOFF_INITIAL: Duration = Duration::from_millis(10);
#[cfg(not(test))]
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(30);
#[cfg(test)]
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_millis(100);

/// Snapshot of weather wire runtime telemetry counters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "telemetry-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[non_exhaustive]
pub struct WxWireReceiverTelemetrySnapshot {
    /// Total client starts.
    pub starts_total: u64,
    /// Total stop requests.
    pub stops_total: u64,
    /// Total successful message decodes.
    pub decoded_messages_total: u64,
    /// Total file events emitted.
    pub files_emitted_total: u64,
    /// Total warning events emitted.
    pub warning_events_total: u64,
    /// Total handler failures.
    pub handler_failures_total: u64,
    /// Total dropped events because output queue was full.
    pub event_queue_drop_total: u64,
    /// Total backpressure warnings emitted.
    pub backpressure_warning_emitted_total: u64,
    /// Total idle-timeout reconnect cycles.
    pub idle_reconnects_total: u64,
    /// Total transport reconnect attempts.
    pub reconnect_attempts_total: u64,
    /// Total endpoint connection attempts.
    pub connect_attempts_total: u64,
    /// Total telemetry events emitted.
    pub telemetry_events_emitted_total: u64,
}

#[derive(Debug, Default)]
struct RuntimeTelemetry {
    snapshot: WxWireReceiverTelemetrySnapshot,
    backpressure: BackpressureTracker,
}

/// Events emitted by the weather wire client.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum WxWireReceiverEvent {
    /// Frame-level event (file or warning).
    Frame(WxWireReceiverFrameEvent),
    /// Connected endpoint label.
    Connected(String),
    /// Disconnected from endpoint.
    Disconnected,
    /// Periodic telemetry snapshot.
    Telemetry(WxWireReceiverTelemetrySnapshot),
}

/// Weather wire event handler callback type.
pub type WxWireReceiverEventHandler =
    Arc<dyn Fn(&WxWireReceiverFrameEvent) -> WxWireReceiverResult<()> + Send + Sync>;

type TransportFuture =
    Pin<Box<dyn Future<Output = WxWireReceiverResult<Box<dyn WxWireTransport>>> + Send>>;
type TransportFactory =
    Arc<dyn Fn(String, String, Duration, Duration) -> TransportFuture + Send + Sync>;

/// Trait for weather wire clients.
pub trait WxWireReceiverClient: Send {
    /// Starts the runtime loop.
    fn start(&mut self) -> WxWireReceiverResult<()>;

    /// Stops the runtime loop.
    fn stop(
        &mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = WxWireReceiverResult<()>> + Send + '_>>;

    /// Returns a stream of runtime events.
    fn events(
        &mut self,
    ) -> Result<ReceiverEventStream<WxWireReceiverEvent, WxWireReceiverError>, WxWireReceiverError>;
}

/// Unstable ingress surface for raw stanza injection.
pub trait UnstableWxWireReceiverIngress {
    /// Submits one raw XMPP stanza string to the runtime decoder.
    fn submit_raw_stanza(&self, stanza: String) -> WxWireReceiverResult<()>;
}

/// Builder for validated weather wire client construction.
#[derive(Clone)]
pub struct WxWireReceiverBuilder {
    config: WxWireReceiverConfig,
    transport_factory: TransportFactory,
}

impl std::fmt::Debug for WxWireReceiverBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WxWireReceiverBuilder")
            .field("config", &self.config)
            .finish()
    }
}

impl WxWireReceiverBuilder {
    /// Creates a new builder.
    pub fn new(config: WxWireReceiverConfig) -> Self {
        Self {
            config,
            transport_factory: Arc::new(default_transport_factory),
        }
    }

    /// Overrides transport construction logic.
    ///
    /// This is intended for tests and unstable integrations.
    pub fn with_transport_factory(mut self, factory: TransportFactory) -> Self {
        self.transport_factory = factory;
        self
    }

    /// Validates config and builds a client instance.
    pub fn build(self) -> Result<WxWireReceiver, WxWireReceiverError> {
        self.config.validate()?;
        Ok(WxWireReceiver {
            config: self.config,
            runtime: ReceiverRuntime::default(),
            ingress_tx: None,
            handlers: Vec::new(),
            telemetry: Arc::new(Mutex::new(WxWireReceiverTelemetrySnapshot::default())),
            transport_factory: self.transport_factory,
        })
    }
}

/// Weather wire client runtime.
pub struct WxWireReceiver {
    config: WxWireReceiverConfig,
    runtime: ReceiverRuntime<WxWireReceiverEvent, WxWireReceiverError>,
    ingress_tx: Option<mpsc::Sender<String>>,
    handlers: Vec<WxWireReceiverEventHandler>,
    telemetry: Arc<Mutex<WxWireReceiverTelemetrySnapshot>>,
    transport_factory: TransportFactory,
}

impl std::fmt::Debug for WxWireReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WxWireReceiver")
            .field("config", &self.config)
            .field("running", &self.runtime.is_running())
            .field("handler_count", &self.handlers.len())
            .finish()
    }
}

impl WxWireReceiver {
    /// Returns a builder for the weather wire client.
    pub fn builder(config: WxWireReceiverConfig) -> WxWireReceiverBuilder {
        WxWireReceiverBuilder::new(config)
    }

    /// Returns runtime config.
    pub fn config(&self) -> &WxWireReceiverConfig {
        &self.config
    }

    /// Adds an event handler callback.
    pub fn subscribe(&mut self, handler: WxWireReceiverEventHandler) {
        self.handlers.push(handler);
    }

    /// Returns a snapshot of current telemetry counters.
    pub fn telemetry_snapshot(&self) -> WxWireReceiverTelemetrySnapshot {
        lock_unpoisoned(&self.telemetry).clone()
    }

    fn submit_raw_stanza_internal(&self, stanza: String) -> WxWireReceiverResult<()> {
        let tx = self
            .ingress_tx
            .as_ref()
            .ok_or(WxWireLifecycleError::NotRunning)?;

        tx.try_send(stanza).map_err(|err| match err {
            TrySendError::Full(_) => WxWireLifecycleError::IngressQueueFull.into(),
            TrySendError::Closed(_) => WxWireLifecycleError::IngressQueueClosed.into(),
        })
    }
}

impl UnstableWxWireReceiverIngress for WxWireReceiver {
    fn submit_raw_stanza(&self, stanza: String) -> WxWireReceiverResult<()> {
        self.submit_raw_stanza_internal(stanza)
    }
}

impl WxWireReceiverClient for WxWireReceiver {
    fn start(&mut self) -> WxWireReceiverResult<()> {
        if self.runtime.is_running() {
            return Err(WxWireLifecycleError::AlreadyRunning.into());
        }

        let (event_tx, event_rx) = mpsc::channel(self.config.event_channel_capacity);
        let (ingress_tx, ingress_rx) = mpsc::channel(self.config.inbound_channel_capacity);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let config = self.config.clone();
        let handlers = self.handlers.clone();
        let telemetry = Arc::clone(&self.telemetry);
        let factory = Arc::clone(&self.transport_factory);

        let join_handle = tokio::spawn(async move {
            run_weather_wire_loop(
                config,
                event_tx,
                ingress_rx,
                shutdown_rx,
                handlers,
                telemetry,
                factory,
            )
            .await;
        });

        self.runtime.install(event_rx, shutdown_tx, join_handle);
        self.ingress_tx = Some(ingress_tx);
        Ok(())
    }

    fn stop(
        &mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = WxWireReceiverResult<()>> + Send + '_>> {
        Box::pin(async move {
            if self.runtime.stop().await {
                return Err(WxWireLifecycleError::ShutdownTimeout.into());
            }
            self.ingress_tx = None;
            Ok(())
        })
    }

    fn events(
        &mut self,
    ) -> Result<ReceiverEventStream<WxWireReceiverEvent, WxWireReceiverError>, WxWireReceiverError>
    {
        self.runtime
            .take_events(WxWireLifecycleError::EventStreamTaken.into())
    }
}

#[cfg(test)]
mod tests {
    use super::runtime::try_send_event;
    use super::{
        TransportFactory, UnstableWxWireReceiverIngress, WxWireReceiver, WxWireReceiverClient,
        WxWireReceiverConfig, WxWireReceiverEvent, WxWireReceiverEventHandler,
    };
    use crate::runtime_support::BackpressureTracker;
    use crate::wxwire_receiver::error::{WxWireLifecycleError, WxWireTransportError};
    use crate::wxwire_receiver::model::{WxWireReceiverFrameEvent, WxWireReceiverWarning};
    use crate::wxwire_receiver::transport::WxWireTransport;
    use futures::StreamExt;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::mpsc;
    #[derive(Debug)]
    struct MockTransport {
        label: String,
        rx: mpsc::Receiver<String>,
    }

    impl WxWireTransport for MockTransport {
        fn label(&self) -> String {
            self.label.clone()
        }

        fn next_stanza<'a>(
            &'a mut self,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::wxwire_receiver::error::WxWireReceiverResult<String>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                self.rx.recv().await.ok_or_else(|| {
                    crate::wxwire_receiver::error::WxWireTransportError::StreamEnded.into()
                })
            })
        }

        fn disconnect<'a>(
            &'a mut self,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::wxwire_receiver::error::WxWireReceiverResult<()>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Debug)]
    struct FlakyTransport {
        label: String,
        rx: mpsc::Receiver<String>,
        fail_once: bool,
    }

    impl WxWireTransport for FlakyTransport {
        fn label(&self) -> String {
            self.label.clone()
        }

        fn next_stanza<'a>(
            &'a mut self,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::wxwire_receiver::error::WxWireReceiverResult<String>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                if self.fail_once {
                    self.fail_once = false;
                    return Err(
                        crate::wxwire_receiver::error::WxWireTransportError::ReadFailed(
                            "simulated socket failure".to_string(),
                        )
                        .into(),
                    );
                }
                self.rx.recv().await.ok_or_else(|| {
                    crate::wxwire_receiver::error::WxWireTransportError::StreamEnded.into()
                })
            })
        }

        fn disconnect<'a>(
            &'a mut self,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::wxwire_receiver::error::WxWireReceiverResult<()>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(()) })
        }
    }

    fn valid_config() -> WxWireReceiverConfig {
        WxWireReceiverConfig {
            username: "user".to_string(),
            password: "pass".to_string(),
            idle_timeout_secs: 1,
            telemetry_emit_interval_secs: 1,
            connect_timeout_secs: 1,
            ..WxWireReceiverConfig::default()
        }
    }

    fn mock_factory() -> TransportFactory {
        Arc::new(
            move |_username, _password, _connect_timeout, _write_timeout| {
                let (tx, rx) = mpsc::channel(8);
                let stanza = "<message xmlns='jabber:client' type='groupchat'><body>S</body><x xmlns='nwws-oi' id='id1' issue='2026-03-05T00:00:00Z' ttaaii='NOUS41' cccc='KOKX' awipsid='AFDOKX'>line</x></message>";
                let _ = tx.try_send(stanza.to_string());
                let label = "primary".to_string();
                Box::pin(async move {
                    Ok(Box::new(MockTransport { label, rx }) as Box<dyn WxWireTransport>)
                })
            },
        )
    }

    #[tokio::test]
    async fn client_emits_file_frame_for_valid_message() {
        let mut client = WxWireReceiver::builder(valid_config())
            .with_transport_factory(mock_factory())
            .build()
            .expect("client should build");
        client.start().expect("client should start");

        let mut events = client.events().expect("events should be available");
        let mut saw_file = false;
        for _ in 0..12 {
            if let Ok(Some(Ok(WxWireReceiverEvent::Frame(WxWireReceiverFrameEvent::File(file))))) =
                tokio::time::timeout(Duration::from_millis(250), events.next()).await
            {
                saw_file = file.filename == "AFDOKX.TXT";
                break;
            }
        }

        drop(events);
        client.stop().await.expect("stop should succeed");
        assert!(saw_file);
    }

    #[tokio::test]
    async fn handler_error_isolated() {
        let bad: WxWireReceiverEventHandler = Arc::new(|_evt: &WxWireReceiverFrameEvent| {
            Err(WxWireLifecycleError::Internal("boom".to_string()).into())
        });

        let mut client = WxWireReceiver::builder(valid_config())
            .with_transport_factory(mock_factory())
            .build()
            .expect("client should build");
        client.subscribe(bad);
        client.start().expect("client should start");

        let mut events = client.events().expect("events should be available");
        let mut saw_handler_warning = false;
        for _ in 0..12 {
            if let Ok(Some(Ok(WxWireReceiverEvent::Frame(WxWireReceiverFrameEvent::Warning(
                WxWireReceiverWarning::HandlerError { .. },
            ))))) = tokio::time::timeout(Duration::from_millis(250), events.next()).await
            {
                saw_handler_warning = true;
                break;
            }
        }

        drop(events);
        client.stop().await.expect("stop should succeed");
        assert!(saw_handler_warning);
    }

    #[tokio::test]
    async fn unstable_raw_ingress_works() {
        let mut client = WxWireReceiver::builder(valid_config())
            .with_transport_factory(mock_factory())
            .build()
            .expect("client should build");
        client.start().expect("client should start");

        client
            .submit_raw_stanza("<message xmlns='jabber:client' type='groupchat'><body>S2</body><x xmlns='nwws-oi' id='id2' issue='2026-03-05T00:00:00Z' ttaaii='NOUS41' cccc='KOKX' awipsid='AFDOKX'>line</x></message>".to_string())
            .expect("submit should succeed");

        let mut events = client.events().expect("events should be available");
        let mut saw_file = false;
        for _ in 0..20 {
            if let Ok(Some(Ok(WxWireReceiverEvent::Frame(WxWireReceiverFrameEvent::File(_))))) =
                tokio::time::timeout(Duration::from_millis(250), events.next()).await
            {
                saw_file = true;
                break;
            }
        }

        drop(events);
        client.stop().await.expect("stop should succeed");
        assert!(saw_file);
    }

    #[tokio::test]
    async fn initial_connect_failure_retries_and_recovers() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_factory = Arc::clone(&attempts);
        let factory: TransportFactory = Arc::new(
            move |_username, _password, _connect_timeout, _write_timeout| {
                let current = attempts_for_factory.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if current == 0 {
                        return Err(WxWireTransportError::TcpConnect(
                            "initial connect failure".to_string(),
                        )
                        .into());
                    }
                    let (tx, rx) = mpsc::channel(8);
                    let stanza = "<message xmlns='jabber:client' type='groupchat'><body>S</body><x xmlns='nwws-oi' id='id1' issue='2026-03-05T00:00:00Z' ttaaii='NOUS41' cccc='KOKX' awipsid='AFDOKX'>line</x></message>";
                    let _ = tx.try_send(stanza.to_string());
                    Ok(Box::new(MockTransport {
                        label: "recovered".to_string(),
                        rx,
                    }) as Box<dyn WxWireTransport>)
                })
            },
        );

        let mut client = WxWireReceiver::builder(valid_config())
            .with_transport_factory(factory)
            .build()
            .expect("client should build");
        client.start().expect("client should start");

        let mut events = client.events().expect("events should be available");
        let mut saw_connected = false;
        for _ in 0..40 {
            if let Ok(Some(Ok(WxWireReceiverEvent::Connected(label)))) =
                tokio::time::timeout(Duration::from_millis(100), events.next()).await
                && label == "recovered"
            {
                saw_connected = true;
                break;
            }
        }

        drop(events);
        client.stop().await.expect("stop should succeed");
        assert!(saw_connected);
        assert!(attempts.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn transport_error_emits_disconnected_and_reconnects() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_factory = Arc::clone(&attempts);
        let factory: TransportFactory = Arc::new(
            move |_username, _password, _connect_timeout, _write_timeout| {
                let current = attempts_for_factory.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    let (tx, rx) = mpsc::channel(8);
                    let stanza = "<message xmlns='jabber:client' type='groupchat'><body>S</body><x xmlns='nwws-oi' id='id1' issue='2026-03-05T00:00:00Z' ttaaii='NOUS41' cccc='KOKX' awipsid='AFDOKX'>line</x></message>";
                    let _ = tx.try_send(stanza.to_string());
                    if current == 0 {
                        Ok(Box::new(FlakyTransport {
                            label: "flaky".to_string(),
                            rx,
                            fail_once: true,
                        }) as Box<dyn WxWireTransport>)
                    } else {
                        Ok(Box::new(MockTransport {
                            label: "reconnected".to_string(),
                            rx,
                        }) as Box<dyn WxWireTransport>)
                    }
                })
            },
        );

        let mut client = WxWireReceiver::builder(valid_config())
            .with_transport_factory(factory)
            .build()
            .expect("client should build");
        client.start().expect("client should start");

        let mut events = client.events().expect("events should be available");
        let mut saw_disconnected = false;
        let mut saw_reconnected = false;
        for _ in 0..60 {
            if let Ok(Some(Ok(event))) =
                tokio::time::timeout(Duration::from_millis(100), events.next()).await
            {
                match event {
                    WxWireReceiverEvent::Disconnected => {
                        saw_disconnected = true;
                    }
                    WxWireReceiverEvent::Connected(label) if label == "reconnected" => {
                        saw_reconnected = true;
                        break;
                    }
                    _ => {}
                }
            }
        }

        drop(events);
        client.stop().await.expect("stop should succeed");
        assert!(saw_disconnected);
        assert!(saw_reconnected);
        assert!(attempts.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn failed_backpressure_warning_send_counts_as_drop() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.try_send(Ok(WxWireReceiverEvent::Disconnected))
            .expect("channel should accept initial event");

        let mut telemetry = super::RuntimeTelemetry::default();
        telemetry.snapshot.event_queue_drop_total = 7;
        telemetry.backpressure = BackpressureTracker::new(7, 3);

        try_send_event(&tx, Ok(WxWireReceiverEvent::Disconnected), &mut telemetry);

        assert_eq!(telemetry.snapshot.event_queue_drop_total, 9);
        assert_eq!(telemetry.backpressure.dropped_since_last_report(), 5);

        let queued = rx.try_recv().expect("original event should remain");
        assert!(matches!(queued, Ok(WxWireReceiverEvent::Disconnected)));
    }

    #[derive(Debug)]
    struct HangingDisconnectTransport;

    impl WxWireTransport for HangingDisconnectTransport {
        fn label(&self) -> String {
            "hanging".to_string()
        }

        fn next_stanza<'a>(
            &'a mut self,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::wxwire_receiver::error::WxWireReceiverResult<String>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                futures::future::pending::<()>().await;
                unreachable!()
            })
        }

        fn disconnect<'a>(
            &'a mut self,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = crate::wxwire_receiver::error::WxWireReceiverResult<()>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                futures::future::pending::<()>().await;
                unreachable!()
            })
        }
    }

    #[tokio::test]
    async fn stop_returns_shutdown_timeout_when_transport_hangs() {
        let factory: TransportFactory = Arc::new(
            move |_username, _password, _connect_timeout, _write_timeout| {
                Box::pin(async move {
                    Ok(Box::new(HangingDisconnectTransport) as Box<dyn WxWireTransport>)
                })
            },
        );

        let mut client = WxWireReceiver::builder(valid_config())
            .with_transport_factory(factory)
            .build()
            .expect("client should build");
        client.start().expect("client should start");
        let mut events = client.events().expect("events should be available");
        let connected = tokio::time::timeout(Duration::from_millis(100), events.next())
            .await
            .expect("connected event should arrive before timeout");
        assert!(matches!(
            connected,
            Some(Ok(WxWireReceiverEvent::Connected(_)))
        ));

        let error = client.stop().await.expect_err("stop should time out");
        assert!(matches!(
            error,
            crate::wxwire_receiver::error::WxWireReceiverError::Lifecycle(
                WxWireLifecycleError::ShutdownTimeout
            )
        ));
    }
}
