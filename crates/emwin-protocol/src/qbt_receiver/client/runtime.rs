use super::connection::{connect_with_timeout, endpoint_label};
use super::server_list_manager::ServerListManager;
use super::watchdog::{HealthObserver, Watchdog};
use super::{
    MAX_CONNECT_TIMEOUT_SECS, QbtReceiverConfig, QbtReceiverError, QbtReceiverEvent,
    QbtReceiverEventHandler, QbtReceiverResult, QbtReceiverTelemetrySnapshot, RuntimeTelemetry,
    TELEMETRY_EMIT_INTERVAL_SECS,
};
use crate::qbt_receiver::protocol::auth::{REAUTH_INTERVAL_SECS, build_logon_message, xor_ff};
use crate::qbt_receiver::protocol::codec::{QbtFrameDecoder, QbtProtocolDecoder};
use crate::qbt_receiver::protocol::model::{QbtAuthMessage, QbtFrameEvent, QbtProtocolWarning};
use crate::runtime_support::try_send_with_backpressure_warning;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

pub(super) async fn run_connection_loop(
    config: QbtReceiverConfig,
    event_tx: mpsc::Sender<Result<QbtReceiverEvent, QbtReceiverError>>,
    mut shutdown_rx: watch::Receiver<bool>,
    handlers: Vec<QbtReceiverEventHandler>,
    telemetry_sink: Arc<Mutex<QbtReceiverTelemetrySnapshot>>,
) {
    let mut telemetry = RuntimeTelemetry::default();
    let mut server_list =
        ServerListManager::new(config.server_list_path.clone(), config.servers.clone());
    if config.follow_server_list_updates
        && let Err(err) = server_list.load()
    {
        try_send_event(&event_tx, Err(err), &mut telemetry);
    }

    while !*shutdown_rx.borrow() {
        telemetry.snapshot.connection_attempts_total = telemetry
            .snapshot
            .connection_attempts_total
            .saturating_add(1);
        update_telemetry_sink(&telemetry_sink, &telemetry);

        let Some((host, port)) = server_list.next_endpoint() else {
            try_send_event(
                &event_tx,
                Err(QbtReceiverError::Lifecycle(
                    "no servers configured".to_string(),
                )),
                &mut telemetry,
            );
            update_telemetry_sink(&telemetry_sink, &telemetry);
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(config.reconnect_delay_secs.max(1))) => {}
                _ = shutdown_rx.changed() => {}
            }
            continue;
        };

        let connect = connect_with_timeout(
            &host,
            port,
            Duration::from_secs(
                config
                    .connection_timeout_secs
                    .clamp(1, MAX_CONNECT_TIMEOUT_SECS),
            ),
        )
        .await;

        match connect {
            Ok(stream) => {
                telemetry.snapshot.connection_success_total = telemetry
                    .snapshot
                    .connection_success_total
                    .saturating_add(1);
                try_send_event(
                    &event_tx,
                    Ok(QbtReceiverEvent::Connected(endpoint_label(&host, port))),
                    &mut telemetry,
                );
                update_telemetry_sink(&telemetry_sink, &telemetry);

                let mut session_ctx = ConnectedSessionContext {
                    config: &config,
                    event_tx: &event_tx,
                    shutdown_rx: &mut shutdown_rx,
                    handlers: &handlers,
                    server_list: &mut server_list,
                    telemetry: &mut telemetry,
                    telemetry_sink: &telemetry_sink,
                };

                let run = run_connected_session(stream, &mut session_ctx).await;

                if let Err(err) = run {
                    try_send_event(&event_tx, Err(err), &mut telemetry);
                }

                if !*shutdown_rx.borrow() && config.follow_server_list_updates {
                    server_list.mark_bad_endpoint(&(host.clone(), port));
                }

                telemetry.snapshot.disconnect_total =
                    telemetry.snapshot.disconnect_total.saturating_add(1);
                try_send_event(
                    &event_tx,
                    Ok(QbtReceiverEvent::Disconnected),
                    &mut telemetry,
                );
                update_telemetry_sink(&telemetry_sink, &telemetry);
            }
            Err(err) => {
                telemetry.snapshot.connection_fail_total =
                    telemetry.snapshot.connection_fail_total.saturating_add(1);
                if config.follow_server_list_updates {
                    server_list.mark_bad_endpoint(&(host.clone(), port));
                }
                try_send_event(&event_tx, Err(QbtReceiverError::Io(err)), &mut telemetry);
                update_telemetry_sink(&telemetry_sink, &telemetry);
            }
        }

        tokio::task::yield_now().await;
    }

    update_telemetry_sink(&telemetry_sink, &telemetry);
}

struct ConnectedSessionContext<'a> {
    config: &'a QbtReceiverConfig,
    event_tx: &'a mpsc::Sender<Result<QbtReceiverEvent, QbtReceiverError>>,
    shutdown_rx: &'a mut watch::Receiver<bool>,
    handlers: &'a [QbtReceiverEventHandler],
    server_list: &'a mut ServerListManager,
    telemetry: &'a mut RuntimeTelemetry,
    telemetry_sink: &'a Arc<Mutex<QbtReceiverTelemetrySnapshot>>,
}

async fn run_connected_session(
    mut stream: tokio::net::TcpStream,
    ctx: &mut ConnectedSessionContext<'_>,
) -> QbtReceiverResult<()> {
    let mut decoder = QbtProtocolDecoder::new(ctx.config.decode.clone());
    let watchdog = Watchdog::new(ctx.config.watchdog_timeout_secs, ctx.config.max_exceptions);
    let mut auth_interval = tokio::time::interval(Duration::from_secs(REAUTH_INTERVAL_SECS));
    auth_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    auth_interval.tick().await;
    let mut telemetry_interval =
        tokio::time::interval(Duration::from_secs(TELEMETRY_EMIT_INTERVAL_SECS));
    telemetry_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let auth = QbtAuthMessage {
        email: ctx.config.email.clone(),
    };
    let initial = xor_ff(build_logon_message(&auth.email).as_bytes());
    stream.write_all(&initial).await?;
    ctx.telemetry.snapshot.auth_logon_sent_total = ctx
        .telemetry
        .snapshot
        .auth_logon_sent_total
        .saturating_add(1);
    update_telemetry_sink(ctx.telemetry_sink, ctx.telemetry);

    let mut buf = vec![0u8; 8192];

    loop {
        if *ctx.shutdown_rx.borrow() {
            return Ok(());
        }

        tokio::select! {
            _ = ctx.shutdown_rx.changed() => {
                return Ok(());
            }
            _ = auth_interval.tick() => {
                let logon = xor_ff(build_logon_message(&auth.email).as_bytes());
                stream.write_all(&logon).await?;
                ctx.telemetry.snapshot.auth_logon_sent_total = ctx.telemetry.snapshot.auth_logon_sent_total.saturating_add(1);
                update_telemetry_sink(ctx.telemetry_sink, ctx.telemetry);
            }
            _ = telemetry_interval.tick() => {
                ctx.telemetry.snapshot.telemetry_events_emitted_total = ctx.telemetry
                    .snapshot
                    .telemetry_events_emitted_total
                    .saturating_add(1);
                try_send_event(
                    ctx.event_tx,
                    Ok(QbtReceiverEvent::Telemetry(ctx.telemetry.snapshot.clone())),
                    ctx.telemetry,
                );
                update_telemetry_sink(ctx.telemetry_sink, ctx.telemetry);
            }
            read = tokio::time::timeout(Duration::from_secs(1), stream.read(&mut buf)) => {
                match read {
                    Ok(Ok(0)) => return Ok(()),
                    Ok(Ok(n)) => {
                        watchdog.on_data_received();
                        ctx.telemetry.snapshot.bytes_in_total = ctx.telemetry.snapshot.bytes_in_total.saturating_add(n as u64);
                        match decoder.feed(&buf[..n]) {
                            Ok(events) => {
                                ctx.telemetry.snapshot.decoder_recovery_events_total = ctx.telemetry
                                    .snapshot
                                    .decoder_recovery_events_total
                                    .saturating_add(count_decoder_recoveries(&events) as u64);
                                ctx.telemetry.snapshot.frame_events_total = ctx.telemetry
                                    .snapshot
                                    .frame_events_total
                                    .saturating_add(events.len() as u64);
                                ctx.telemetry.snapshot.data_blocks_emitted_total = ctx.telemetry
                                    .snapshot
                                    .data_blocks_emitted_total
                                    .saturating_add(count_data_blocks(&events) as u64);
                                ctx.telemetry.snapshot.server_list_updates_total = ctx.telemetry
                                    .snapshot
                                    .server_list_updates_total
                                    .saturating_add(count_server_list_updates(&events) as u64);
                                ctx.telemetry.snapshot.checksum_mismatch_total = ctx.telemetry
                                    .snapshot
                                    .checksum_mismatch_total
                                    .saturating_add(count_checksum_mismatches(&events) as u64);
                                ctx.telemetry.snapshot.decompression_failed_total = ctx.telemetry
                                    .snapshot
                                    .decompression_failed_total
                                    .saturating_add(count_decompression_failures(&events) as u64);
                                for event in &events {
                                    if ctx.config.follow_server_list_updates
                                        && let QbtFrameEvent::ServerListUpdate(list) = event
                                        && let Err(err) = ctx.server_list.apply_server_list(list.clone()) {
                                        try_send_event(ctx.event_tx, Err(err), ctx.telemetry);
                                    }
                                }
                                dispatch_events(ctx.event_tx, ctx.handlers, events, ctx.telemetry);
                                update_telemetry_sink(ctx.telemetry_sink, ctx.telemetry);
                            }
                            Err(err) => {
                                watchdog.on_exception();
                                ctx.telemetry.snapshot.watchdog_exception_events_total = ctx.telemetry
                                    .snapshot
                                    .watchdog_exception_events_total
                                    .saturating_add(1);
                                decoder.reset();
                                try_send_event(ctx.event_tx, Err(QbtReceiverError::Protocol(err)), ctx.telemetry);
                                update_telemetry_sink(ctx.telemetry_sink, ctx.telemetry);
                            }
                        }
                    }
                    Ok(Err(err)) => {
                        watchdog.on_exception();
                        ctx.telemetry.snapshot.watchdog_exception_events_total = ctx.telemetry
                            .snapshot
                            .watchdog_exception_events_total
                            .saturating_add(1);
                        update_telemetry_sink(ctx.telemetry_sink, ctx.telemetry);
                        return Err(QbtReceiverError::Io(err));
                    }
                    Err(_elapsed) => {
                        if watchdog.should_close() {
                            ctx.telemetry.snapshot.watchdog_timeouts_total = ctx.telemetry
                                .snapshot
                                .watchdog_timeouts_total
                                .saturating_add(1);
                            update_telemetry_sink(ctx.telemetry_sink, ctx.telemetry);
                            return Err(QbtReceiverError::Lifecycle("watchdog timeout".to_string()));
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn dispatch_events(
    event_tx: &mpsc::Sender<Result<QbtReceiverEvent, QbtReceiverError>>,
    handlers: &[QbtReceiverEventHandler],
    events: Vec<QbtFrameEvent>,
    telemetry: &mut RuntimeTelemetry,
) {
    for event in events {
        for handler in handlers {
            if let Err(err) = handler(&event) {
                telemetry.snapshot.handler_failures_total =
                    telemetry.snapshot.handler_failures_total.saturating_add(1);
                let warning = QbtFrameEvent::Warning(QbtProtocolWarning::HandlerError {
                    message: err.to_string(),
                });
                try_send_event(event_tx, Ok(QbtReceiverEvent::Frame(warning)), telemetry);
            }
        }
        try_send_event(event_tx, Ok(QbtReceiverEvent::Frame(event)), telemetry);
    }
}

fn count_decoder_recoveries(events: &[QbtFrameEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                QbtFrameEvent::Warning(QbtProtocolWarning::DecoderRecovered { .. })
            )
        })
        .count()
}

fn count_data_blocks(events: &[QbtFrameEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, QbtFrameEvent::DataBlock(_)))
        .count()
}

fn count_server_list_updates(events: &[QbtFrameEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, QbtFrameEvent::ServerListUpdate(_)))
        .count()
}

fn count_checksum_mismatches(events: &[QbtFrameEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                QbtFrameEvent::Warning(QbtProtocolWarning::ChecksumMismatch { .. })
            )
        })
        .count()
}

fn count_decompression_failures(events: &[QbtFrameEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                QbtFrameEvent::Warning(QbtProtocolWarning::DecompressionFailed { .. })
            )
        })
        .count()
}

pub(super) fn try_send_event(
    event_tx: &mpsc::Sender<Result<QbtReceiverEvent, QbtReceiverError>>,
    event: Result<QbtReceiverEvent, QbtReceiverError>,
    telemetry: &mut RuntimeTelemetry,
) {
    let decoder_recovery_events = telemetry.snapshot.decoder_recovery_events_total;
    try_send_with_backpressure_warning(
        event_tx,
        event,
        &mut telemetry.backpressure,
        |tracker| {
            QbtReceiverEvent::Frame(QbtFrameEvent::Warning(
                QbtProtocolWarning::BackpressureDrop {
                    dropped_since_last_report: tracker.dropped_since_last_report(),
                    total_dropped_events: tracker.event_queue_drop_total(),
                    decoder_recovery_events,
                },
            ))
        },
        || {
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

pub(super) fn update_telemetry_sink(
    telemetry_sink: &Arc<Mutex<QbtReceiverTelemetrySnapshot>>,
    telemetry: &RuntimeTelemetry,
) {
    let mut guard = telemetry_sink
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = telemetry.snapshot.clone();
}
