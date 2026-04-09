use super::{
    RECONNECT_BACKOFF_INITIAL, RECONNECT_BACKOFF_MAX, RuntimeTelemetry, TransportFactory,
    TransportFuture, WxWireReceiverConfig, WxWireReceiverError, WxWireReceiverEvent,
    WxWireReceiverEventHandler, WxWireReceiverResult, WxWireReceiverTelemetrySnapshot,
};
use crate::runtime_support::try_send_with_backpressure_warning;
use crate::wxwire_receiver::codec::{WxWireDecoder, WxWireFrameDecoder};
use crate::wxwire_receiver::config::WXWIRE_PRIMARY_HOST;
use crate::wxwire_receiver::model::{WxWireReceiverFrameEvent, WxWireReceiverWarning};
use crate::wxwire_receiver::transport::{WxWireTransport, XmppWxWireTransport};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tracing::warn;

pub(super) async fn run_weather_wire_loop(
    config: WxWireReceiverConfig,
    event_tx: mpsc::Sender<Result<WxWireReceiverEvent, WxWireReceiverError>>,
    mut ingress_rx: mpsc::Receiver<String>,
    mut shutdown_rx: watch::Receiver<bool>,
    handlers: Vec<WxWireReceiverEventHandler>,
    telemetry_sink: Arc<Mutex<WxWireReceiverTelemetrySnapshot>>,
    transport_factory: TransportFactory,
) {
    let mut telemetry = RuntimeTelemetry::default();
    telemetry.snapshot.starts_total = telemetry.snapshot.starts_total.saturating_add(1);

    let mut decoder = WxWireDecoder;
    let mut telemetry_tick =
        tokio::time::interval(Duration::from_secs(config.telemetry_emit_interval_secs));
    telemetry_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    telemetry_tick.tick().await;
    if *shutdown_rx.borrow() {
        telemetry.snapshot.stops_total = telemetry.snapshot.stops_total.saturating_add(1);
        try_send_event(
            &event_tx,
            Ok(WxWireReceiverEvent::Disconnected),
            &mut telemetry,
        );
        update_telemetry_sink(&telemetry_sink, &telemetry);
        return;
    }

    let connect_timeout = Duration::from_secs(config.connect_timeout_secs);
    let mut transport: Option<Box<dyn WxWireTransport>> = None;
    let mut last_message_time = Instant::now();
    let mut reconnect_backoff = RECONNECT_BACKOFF_INITIAL;

    loop {
        if *shutdown_rx.borrow() {
            telemetry.snapshot.stops_total = telemetry.snapshot.stops_total.saturating_add(1);
            if let Some(mut connected) = transport.take() {
                let _ = connected.disconnect().await;
            }
            try_send_event(
                &event_tx,
                Ok(WxWireReceiverEvent::Disconnected),
                &mut telemetry,
            );
            update_telemetry_sink(&telemetry_sink, &telemetry);
            return;
        }

        if transport.is_none() {
            match connect_single_endpoint(
                &transport_factory,
                config.username.clone(),
                config.password.clone(),
                connect_timeout,
                &mut telemetry,
            )
            .await
            {
                Ok(connected) => {
                    reconnect_backoff = RECONNECT_BACKOFF_INITIAL;
                    last_message_time = Instant::now();
                    let label = connected.label();
                    try_send_event(
                        &event_tx,
                        Ok(WxWireReceiverEvent::Connected(label)),
                        &mut telemetry,
                    );
                    update_telemetry_sink(&telemetry_sink, &telemetry);
                    transport = Some(connected);
                }
                Err(err) => {
                    warn!(error = %err, "wxwire connection attempt failed");
                    let warning =
                        WxWireReceiverFrameEvent::Warning(WxWireReceiverWarning::TransportError {
                            message: err.to_string(),
                        });
                    dispatch_frame_events(&event_tx, &handlers, vec![warning], &mut telemetry);
                    update_telemetry_sink(&telemetry_sink, &telemetry);
                    telemetry.snapshot.reconnect_attempts_total = telemetry
                        .snapshot
                        .reconnect_attempts_total
                        .saturating_add(1);
                    if wait_reconnect_backoff(&mut shutdown_rx, reconnect_backoff).await {
                        continue;
                    }
                    reconnect_backoff =
                        (reconnect_backoff.saturating_mul(2)).min(RECONNECT_BACKOFF_MAX);
                    continue;
                }
            }
        }

        let mut connected_transport = transport.take().expect("checked is_some above");

        enum NextAction {
            Stay,
            Shutdown,
            Reconnect,
        }
        let mut action = NextAction::Stay;

        {
            let next_stanza = connected_transport.next_stanza();
            tokio::pin!(next_stanza);

            tokio::select! {
                _ = shutdown_rx.changed() => {
                    telemetry.snapshot.stops_total = telemetry.snapshot.stops_total.saturating_add(1);
                    action = NextAction::Shutdown;
                }
                _ = telemetry_tick.tick() => {
                    telemetry.snapshot.telemetry_events_emitted_total = telemetry
                        .snapshot
                        .telemetry_events_emitted_total
                        .saturating_add(1);
                    try_send_event(
                        &event_tx,
                        Ok(WxWireReceiverEvent::Telemetry(telemetry.snapshot.clone())),
                        &mut telemetry,
                    );
                    update_telemetry_sink(&telemetry_sink, &telemetry);
                }
                maybe_raw = ingress_rx.recv() => {
                    if let Some(raw) = maybe_raw {
                        last_message_time = Instant::now();
                        match decoder.feed(&raw) {
                            Ok(frame_events) => {
                                telemetry.snapshot.decoded_messages_total = telemetry
                                    .snapshot
                                    .decoded_messages_total
                                    .saturating_add(1);
                                dispatch_frame_events(&event_tx, &handlers, frame_events, &mut telemetry);
                                update_telemetry_sink(&telemetry_sink, &telemetry);
                            }
                            Err(err) => {
                                let warning = WxWireReceiverFrameEvent::Warning(WxWireReceiverWarning::DecoderRecovered {
                                    error: err.to_string(),
                                });
                                dispatch_frame_events(&event_tx, &handlers, vec![warning], &mut telemetry);
                                decoder.reset();
                                update_telemetry_sink(&telemetry_sink, &telemetry);
                            }
                        }
                    }
                }
                transport_event = tokio::time::timeout(Duration::from_secs(1), &mut next_stanza) => {
                    match transport_event {
                        Ok(Ok(stanza)) => {
                            last_message_time = Instant::now();
                            match decoder.feed(&stanza) {
                                Ok(frame_events) => {
                                    telemetry.snapshot.decoded_messages_total = telemetry
                                        .snapshot
                                        .decoded_messages_total
                                        .saturating_add(1);
                                    dispatch_frame_events(&event_tx, &handlers, frame_events, &mut telemetry);
                                    update_telemetry_sink(&telemetry_sink, &telemetry);
                                }
                                Err(err) => {
                                    let warning = WxWireReceiverFrameEvent::Warning(WxWireReceiverWarning::DecoderRecovered {
                                        error: err.to_string(),
                                    });
                                    dispatch_frame_events(&event_tx, &handlers, vec![warning], &mut telemetry);
                                    decoder.reset();
                                    update_telemetry_sink(&telemetry_sink, &telemetry);
                                }
                            }
                        }
                        Ok(Err(err)) => {
                            warn!(error = %err, "wxwire transport error, reconnecting");
                            let warning = WxWireReceiverFrameEvent::Warning(WxWireReceiverWarning::TransportError {
                                message: err.to_string(),
                            });
                            dispatch_frame_events(&event_tx, &handlers, vec![warning], &mut telemetry);
                            update_telemetry_sink(&telemetry_sink, &telemetry);
                            action = NextAction::Reconnect;
                        }
                        Err(_) => {
                            if last_message_time.elapsed() >= Duration::from_secs(config.idle_timeout_secs) {
                                let message = format!(
                                    "no accepted room message for {}s",
                                    config.idle_timeout_secs
                                );
                                warn!(%message, "wxwire idle timeout");
                                let warning = WxWireReceiverFrameEvent::Warning(WxWireReceiverWarning::TransportError {
                                    message,
                                });
                                dispatch_frame_events(&event_tx, &handlers, vec![warning], &mut telemetry);
                                update_telemetry_sink(&telemetry_sink, &telemetry);
                                last_message_time = Instant::now();
                            }
                        }
                    }
                }
            }
        }

        match action {
            NextAction::Stay => {
                transport = Some(connected_transport);
            }
            NextAction::Shutdown => {
                let _ = connected_transport.disconnect().await;
                try_send_event(
                    &event_tx,
                    Ok(WxWireReceiverEvent::Disconnected),
                    &mut telemetry,
                );
                update_telemetry_sink(&telemetry_sink, &telemetry);
                return;
            }
            NextAction::Reconnect => {
                telemetry.snapshot.reconnect_attempts_total = telemetry
                    .snapshot
                    .reconnect_attempts_total
                    .saturating_add(1);
                let _ = connected_transport.disconnect().await;
                try_send_event(
                    &event_tx,
                    Ok(WxWireReceiverEvent::Disconnected),
                    &mut telemetry,
                );
                update_telemetry_sink(&telemetry_sink, &telemetry);
                if wait_reconnect_backoff(&mut shutdown_rx, reconnect_backoff).await {
                    continue;
                }
                reconnect_backoff =
                    (reconnect_backoff.saturating_mul(2)).min(RECONNECT_BACKOFF_MAX);
            }
        }
    }
}

async fn wait_reconnect_backoff(
    shutdown_rx: &mut watch::Receiver<bool>,
    duration: Duration,
) -> bool {
    tokio::select! {
        _ = shutdown_rx.changed() => true,
        _ = tokio::time::sleep(duration) => false,
    }
}

async fn connect_single_endpoint(
    factory: &TransportFactory,
    username: String,
    password: String,
    connect_timeout: Duration,
    telemetry: &mut RuntimeTelemetry,
) -> WxWireReceiverResult<Box<dyn WxWireTransport>> {
    telemetry.snapshot.connect_attempts_total =
        telemetry.snapshot.connect_attempts_total.saturating_add(1);
    factory(username, password, connect_timeout).await
}

pub(super) fn default_transport_factory(
    username: String,
    password: String,
    connect_timeout: Duration,
) -> TransportFuture {
    Box::pin(async move {
        let transport = XmppWxWireTransport::connect(
            WXWIRE_PRIMARY_HOST,
            username.as_str(),
            password.as_str(),
            connect_timeout,
        )
        .await?;
        Ok(Box::new(transport) as Box<dyn WxWireTransport>)
    })
}

fn dispatch_frame_events(
    event_tx: &mpsc::Sender<Result<WxWireReceiverEvent, WxWireReceiverError>>,
    handlers: &[WxWireReceiverEventHandler],
    frame_events: Vec<WxWireReceiverFrameEvent>,
    telemetry: &mut RuntimeTelemetry,
) {
    for frame_event in frame_events {
        if matches!(frame_event, WxWireReceiverFrameEvent::File(_)) {
            telemetry.snapshot.files_emitted_total =
                telemetry.snapshot.files_emitted_total.saturating_add(1);
        }
        if matches!(frame_event, WxWireReceiverFrameEvent::Warning(_)) {
            telemetry.snapshot.warning_events_total =
                telemetry.snapshot.warning_events_total.saturating_add(1);
        }

        for handler in handlers {
            if let Err(err) = handler(&frame_event) {
                telemetry.snapshot.handler_failures_total =
                    telemetry.snapshot.handler_failures_total.saturating_add(1);
                let warning =
                    WxWireReceiverFrameEvent::Warning(WxWireReceiverWarning::HandlerError {
                        message: err.to_string(),
                    });
                telemetry.snapshot.warning_events_total =
                    telemetry.snapshot.warning_events_total.saturating_add(1);
                try_send_event(event_tx, Ok(WxWireReceiverEvent::Frame(warning)), telemetry);
            }
        }

        try_send_event(
            event_tx,
            Ok(WxWireReceiverEvent::Frame(frame_event)),
            telemetry,
        );
    }
}

pub(super) fn try_send_event(
    event_tx: &mpsc::Sender<Result<WxWireReceiverEvent, WxWireReceiverError>>,
    event: Result<WxWireReceiverEvent, WxWireReceiverError>,
    telemetry: &mut RuntimeTelemetry,
) {
    try_send_with_backpressure_warning(
        event_tx,
        event,
        &mut telemetry.backpressure,
        |tracker| {
            WxWireReceiverEvent::Frame(WxWireReceiverFrameEvent::Warning(
                WxWireReceiverWarning::BackpressureDrop {
                    dropped_since_last_report: tracker.dropped_since_last_report(),
                    total_dropped_events: tracker.event_queue_drop_total(),
                },
            ))
        },
        || {
            telemetry.snapshot.warning_events_total =
                telemetry.snapshot.warning_events_total.saturating_add(1);
            telemetry.snapshot.backpressure_warning_emitted_total = telemetry
                .snapshot
                .backpressure_warning_emitted_total
                .saturating_add(1);
        },
        |tracker| {
            telemetry.snapshot.event_queue_drop_total = tracker.event_queue_drop_total();
        },
    );
}

fn update_telemetry_sink(
    sink: &Arc<Mutex<WxWireReceiverTelemetrySnapshot>>,
    telemetry: &RuntimeTelemetry,
) {
    let mut guard = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = telemetry.snapshot.clone();
}
