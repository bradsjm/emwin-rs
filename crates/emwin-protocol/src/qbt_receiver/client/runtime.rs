use super::connection::{connect_with_timeout, endpoint_label, write_all_with_timeout};
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
use tokio::io::AsyncReadExt;
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
    let reconnect_delay = Duration::from_secs(config.reconnect_delay_secs.max(1));
    if config.follow_server_list_updates
        && let Err(err) = server_list.load()
    {
        try_send_event(&event_tx, Err(err), &mut telemetry);
    }
    sync_active_servers(&mut telemetry, &server_list);
    emit_telemetry(event_tx.clone(), &mut telemetry, &telemetry_sink);

    let mut attempts_in_pass = 0usize;
    while !*shutdown_rx.borrow() {
        let endpoint_count = server_list.endpoint_count();
        if endpoint_count == 0 {
            try_send_event(
                &event_tx,
                Err(QbtReceiverError::Lifecycle(
                    "no servers configured".to_string(),
                )),
                &mut telemetry,
            );
            update_telemetry_sink(&telemetry_sink, &telemetry);
            tokio::select! {
                _ = tokio::time::sleep(reconnect_delay) => {}
                _ = shutdown_rx.changed() => {}
            }
            continue;
        }

        telemetry.snapshot.connection_attempts_total = telemetry
            .snapshot
            .connection_attempts_total
            .saturating_add(1);
        update_telemetry_sink(&telemetry_sink, &telemetry);

        let (host, port) = server_list
            .next_endpoint()
            .expect("endpoint count checked above");
        attempts_in_pass = attempts_in_pass.saturating_add(1);

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
                attempts_in_pass = 0;
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
                try_send_event(&event_tx, Err(QbtReceiverError::Io(err)), &mut telemetry);
                update_telemetry_sink(&telemetry_sink, &telemetry);
            }
        }

        if !*shutdown_rx.borrow() && completed_failed_pass(attempts_in_pass, endpoint_count) {
            attempts_in_pass = 0;
            tokio::select! {
                _ = tokio::time::sleep(reconnect_delay) => {}
                _ = shutdown_rx.changed() => {}
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
    write_all_with_timeout(
        &mut stream,
        &initial,
        Duration::from_secs(ctx.config.write_timeout_secs),
    )
    .await?;
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
                write_all_with_timeout(
                    &mut stream,
                    &logon,
                    Duration::from_secs(ctx.config.write_timeout_secs),
                )
                .await?;
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
                                    {
                                        if let Err(err) = apply_server_list_update(
                                            ctx.server_list,
                                            list,
                                            ctx.telemetry,
                                        ) {
                                            try_send_event(ctx.event_tx, Err(err), ctx.telemetry);
                                        } else {
                                            emit_telemetry(
                                                ctx.event_tx.clone(),
                                                ctx.telemetry,
                                                ctx.telemetry_sink,
                                            );
                                        }
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

fn completed_failed_pass(attempts_in_pass: usize, endpoint_count: usize) -> bool {
    endpoint_count > 0 && attempts_in_pass >= endpoint_count
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

fn sync_active_servers(telemetry: &mut RuntimeTelemetry, server_list: &ServerListManager) {
    telemetry.snapshot.active_servers = server_list.endpoint_count();
}

fn emit_telemetry(
    event_tx: mpsc::Sender<Result<QbtReceiverEvent, QbtReceiverError>>,
    telemetry: &mut RuntimeTelemetry,
    telemetry_sink: &Arc<Mutex<QbtReceiverTelemetrySnapshot>>,
) {
    telemetry.snapshot.telemetry_events_emitted_total = telemetry
        .snapshot
        .telemetry_events_emitted_total
        .saturating_add(1);
    try_send_event(
        &event_tx,
        Ok(QbtReceiverEvent::Telemetry(telemetry.snapshot.clone())),
        telemetry,
    );
    update_telemetry_sink(telemetry_sink, telemetry);
}

fn apply_server_list_update(
    server_list: &mut ServerListManager,
    list: &crate::qbt_receiver::protocol::model::QbtServerList,
    telemetry: &mut RuntimeTelemetry,
) -> QbtReceiverResult<()> {
    server_list.apply_server_list(list.clone())?;
    sync_active_servers(telemetry, server_list);
    tracing::info!(
        servers_in_update = list.servers.len(),
        active_servers = telemetry.snapshot.active_servers,
        "applied upstream server list update"
    );
    Ok(())
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

#[cfg(test)]
mod runtime_tests {
    use super::{
        RuntimeTelemetry, apply_server_list_update, completed_failed_pass, sync_active_servers,
    };
    use crate::qbt_receiver::client::server_list_manager::ServerListManager;
    use crate::qbt_receiver::protocol::model::QbtServerList;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Debug, Default)]
    struct SharedLogBuffer(Arc<Mutex<Vec<u8>>>);

    impl SharedLogBuffer {
        fn contents(&self) -> String {
            String::from_utf8(
                self.0
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            )
            .expect("logs should be utf-8")
        }
    }

    #[derive(Clone, Debug)]
    struct LogWriter(SharedLogBuffer);

    impl Write for LogWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for SharedLogBuffer {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> LogWriter {
            LogWriter(self.clone())
        }
    }

    #[test]
    fn failed_pass_requires_trying_every_endpoint() {
        assert!(!completed_failed_pass(0, 2));
        assert!(!completed_failed_pass(1, 2));
        assert!(completed_failed_pass(2, 2));
        assert!(!completed_failed_pass(1, 0));
    }

    #[test]
    fn sync_active_servers_uses_normalized_endpoint_count() {
        let mut telemetry = RuntimeTelemetry::default();
        let mut server_list = ServerListManager::new(
            None,
            vec![
                ("b.example".to_string(), 2),
                ("a.example".to_string(), 1),
                ("b.example".to_string(), 2),
            ],
        );

        sync_active_servers(&mut telemetry, &server_list);
        assert_eq!(telemetry.snapshot.active_servers, 2);

        server_list
            .apply_server_list(QbtServerList {
                servers: vec![("c.example".to_string(), 3)],
            })
            .expect("update should apply");
        sync_active_servers(&mut telemetry, &server_list);
        assert_eq!(telemetry.snapshot.active_servers, 1);
    }

    #[test]
    fn successful_server_list_update_logs_and_updates_active_servers() {
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(buffer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let mut telemetry = RuntimeTelemetry::default();
        let mut server_list = ServerListManager::new(None, vec![("a.example".to_string(), 1)]);

        apply_server_list_update(
            &mut server_list,
            &QbtServerList {
                servers: vec![
                    ("b.example".to_string(), 2),
                    ("c.example".to_string(), 3),
                    ("b.example".to_string(), 2),
                ],
            },
            &mut telemetry,
        )
        .expect("update should apply");

        assert_eq!(telemetry.snapshot.active_servers, 2);
        let logs = buffer.contents();
        assert!(logs.contains("applied upstream server list update"));
        assert!(logs.contains("servers_in_update=3"));
        assert!(logs.contains("active_servers=2"));
    }
}
