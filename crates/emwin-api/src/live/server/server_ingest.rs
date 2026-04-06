//! Feed ingest events into the live server state.
//!
//! This module translates runtime events into retained files, broadcast notifications, and
//! telemetry snapshots without leaking transport-specific details into the HTTP layer.

use super::types::{
    AppState, CompletedFileEventPayload, EventKind, IncidentEventPayload, TelemetryPayload,
};
use crate::live::archive_postprocess::post_process_archive;
use crate::live::persistence::FilePersistenceProducer;
use emwin_db::{PersistenceStats, PostgresMetadataSink};
use emwin_protocol::ingest::{
    IngestConfig, IngestError, IngestEvent, IngestReceiver, IngestTelemetry, IngestWarning,
    ProductOrigin,
};
use emwin_protocol::qbt_receiver::{QbtFrameEvent, QbtProtocolWarning, QbtReceiverConfig};
use emwin_protocol::wxwire_receiver::{WxWireReceiverConfig, WxWireReceiverFrameEvent};
use futures::StreamExt;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};
use tokio::sync::watch;
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServerStatsSnapshot {
    uptime_secs: u64,
    data_blocks_total: u64,
    received_servers: usize,
    received_sat_servers: usize,
    retained_files: usize,
    connected_clients: usize,
    upstream: String,
    persistence: Option<PersistenceStats>,
}

/// Runs the QBT ingest loop until shutdown or receiver termination.
pub(super) async fn run_qbt_ingest_loop(
    config: QbtReceiverConfig,
    state: Arc<AppState>,
    post_process_archives: bool,
    persistence: Option<FilePersistenceProducer>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> crate::error::CliResult<()> {
    let mut ingest = IngestReceiver::build(IngestConfig::Qbt(config))?;
    ingest.start()?;

    let mut events = ingest.events()?;
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            item = events.next() => {
                let Some(item) = item else {
                    break;
                };
                handle_ingest_event(item, &state, post_process_archives, persistence.as_ref())?;
            }
        }
    }

    drop(events);
    ingest.stop().await?;

    Ok(())
}

pub(super) async fn run_wxwire_ingest_loop(
    config: WxWireReceiverConfig,
    state: Arc<AppState>,
    post_process_archives: bool,
    persistence: Option<FilePersistenceProducer>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> crate::error::CliResult<()> {
    let mut ingest = IngestReceiver::build(IngestConfig::WxWire(config))?;
    ingest.start()?;

    let mut events = ingest.events()?;
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            item = events.next() => {
                let Some(item) = item else {
                    break;
                };
                handle_ingest_event(item, &state, post_process_archives, persistence.as_ref())?;
            }
        }
    }

    drop(events);
    ingest.stop().await?;

    Ok(())
}

pub(super) async fn run_incident_event_relay_loop(
    sink: PostgresMetadataSink,
    state: Arc<AppState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> crate::error::CliResult<()> {
    let mut rx = sink.subscribe_incident_changes();
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            received = rx.recv() => match received {
                Ok(change) => {
                    super::publish_incident_change(
                        &state,
                        IncidentEventPayload::from_change(change),
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                    super::log_info(
                        state.quiet,
                        &format!("incident relay lagged dropped={dropped}"),
                    );
                }
            }
        }
    }

    Ok(())
}

fn handle_ingest_event(
    item: Result<IngestEvent, IngestError>,
    state: &Arc<AppState>,
    post_process_archives: bool,
    persistence: Option<&FilePersistenceProducer>,
) -> crate::error::CliResult<()> {
    match item {
        Ok(IngestEvent::Connected { endpoint }) => {
            {
                let mut guard = state
                    .upstream_endpoint
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *guard = Some(endpoint.clone());
            }
            super::log_info(
                state.quiet,
                &format!("upstream connected endpoint={endpoint}"),
            );
            super::publish(
                state,
                EventKind::Connected {
                    endpoint: endpoint.clone(),
                },
            );
        }
        Ok(IngestEvent::Disconnected) => {
            {
                let mut guard = state
                    .upstream_endpoint
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *guard = None;
            }
            super::log_info(state.quiet, "upstream disconnected");
            super::publish(state, EventKind::Disconnected);
        }
        Ok(IngestEvent::Telemetry(snapshot)) => {
            let telemetry_value = match snapshot {
                IngestTelemetry::Qbt(value) => TelemetryPayload::Qbt(value),
                IngestTelemetry::WxWire(value) => TelemetryPayload::WxWire(value),
                _ => TelemetryPayload::Unavailable,
            };
            {
                let mut guard = state
                    .telemetry
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *guard = telemetry_value.clone();
            }
            super::publish(state, EventKind::Telemetry(telemetry_value));
        }
        Ok(IngestEvent::Product(product)) => {
            if matches!(product.origin, ProductOrigin::Qbt) {
                state.data_blocks_total.fetch_add(1, Ordering::Relaxed);
            }

            let delivered =
                match post_process_archive(post_process_archives, &product.filename, &product.data)
                {
                    Ok(delivered) => delivered,
                    Err(err) => {
                        tracing::warn!(
                            archive_filename = %product.filename,
                            error = %err,
                            "Corrupt Zip File Received"
                        );
                        return Ok(());
                    }
                };
            let completed_at = SystemTime::now();
            let timestamp_utc = product
                .source_timestamp_utc
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            let retained_meta = {
                let mut guard = state
                    .retained_files
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.insert(
                    delivered.filename.clone(),
                    delivered.data.to_vec(),
                    timestamp_utc,
                    product.origin.clone(),
                    completed_at,
                )
            };
            if let Some(persistence) = persistence {
                let queued = crate::live::persistence::enqueue_completed_product(
                    persistence,
                    &delivered.filename,
                    &delivered.data,
                    retained_meta.clone(),
                )?;
                if !queued {
                    tracing::warn!(filename = %delivered.filename, "persistence queue closed");
                }
            }
            super::publish(
                state,
                EventKind::FileComplete(Box::new(CompletedFileEventPayload::from_metadata(
                    retained_meta,
                ))),
            );
            super::log_info(
                state.quiet,
                &format!(
                    "file complete name={} bytes={}",
                    delivered.filename,
                    delivered.data.len()
                ),
            );
        }
        Ok(IngestEvent::Warning(warning)) => match warning {
            IngestWarning::Qbt(value) => {
                if let QbtProtocolWarning::BackpressureDrop { .. } = value {
                    super::log_info(state.quiet, "qbt ingest backpressure warning");
                }
                super::publish(state, EventKind::QbtFrame(QbtFrameEvent::Warning(value)));
            }
            IngestWarning::WxWire(value) => {
                super::publish(
                    state,
                    EventKind::WxWireFrame(WxWireReceiverFrameEvent::Warning(value)),
                );
            }
            _ => {
                super::publish(
                    state,
                    EventKind::Error {
                        message: format!("ingest warning: {warning:?}"),
                    },
                );
            }
        },
        Err(err) => {
            super::log_error(&format!("client error: {err}"));
            super::publish(
                state,
                EventKind::Error {
                    message: err.to_string(),
                },
            );
        }
        Ok(_) => {}
    }

    Ok(())
}

pub(super) async fn run_stats_loop(
    state: Arc<AppState>,
    stats_interval_secs: u64,
    persistence: Option<FilePersistenceProducer>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> crate::error::CliResult<()> {
    if stats_interval_secs == 0 {
        let _ = shutdown_rx.changed().await;
        return Ok(());
    }

    let mut interval = tokio::time::interval(Duration::from_secs(stats_interval_secs.max(1)));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            _ = interval.tick() => {
                if state.quiet {
                    continue;
                }

                let snapshot = build_server_stats_snapshot(&state, persistence.as_ref());
                if let Some(persistence) = snapshot.persistence {
                    info!(
                        uptime_secs = snapshot.uptime_secs,
                        data_blocks_total = snapshot.data_blocks_total,
                        received_servers = snapshot.received_servers,
                        received_sat_servers = snapshot.received_sat_servers,
                        retained_files = snapshot.retained_files,
                        connected_clients = snapshot.connected_clients,
                        upstream = snapshot.upstream,
                        persistence_queue_len = persistence.queue_len,
                        persistence_queue_capacity = persistence.queue_capacity,
                        persistence_enqueued_total = persistence.enqueued_total,
                        persistence_evicted_total = persistence.evicted_total,
                        persistence_persisted_total = persistence.persisted_total,
                        persistence_failed_total = persistence.failed_total,
                        "server stats snapshot"
                    );
                } else {
                    info!(
                        uptime_secs = snapshot.uptime_secs,
                        data_blocks_total = snapshot.data_blocks_total,
                        received_servers = snapshot.received_servers,
                        received_sat_servers = snapshot.received_sat_servers,
                        retained_files = snapshot.retained_files,
                        connected_clients = snapshot.connected_clients,
                        upstream = snapshot.upstream,
                        "server stats snapshot"
                    );
                }
            }
        }
    }

    Ok(())
}

fn build_server_stats_snapshot(
    state: &AppState,
    persistence: Option<&FilePersistenceProducer>,
) -> ServerStatsSnapshot {
    let endpoint = state
        .upstream_endpoint
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    ServerStatsSnapshot {
        uptime_secs: state.started_at.elapsed().as_secs(),
        data_blocks_total: state.data_blocks_total.load(Ordering::Relaxed),
        received_servers: state.received_servers.load(Ordering::Relaxed),
        received_sat_servers: state.received_sat_servers.load(Ordering::Relaxed),
        retained_files: state
            .retained_files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
        connected_clients: state.connected_clients.load(Ordering::Relaxed),
        upstream: endpoint.unwrap_or_else(|| "disconnected".to_string()),
        persistence: persistence.map(FilePersistenceProducer::stats_snapshot),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_server_stats_snapshot, handle_ingest_event};
    use crate::live::file_pipeline::build_persist_request;
    use crate::live::server::types::{AppState, EventKind, TelemetryPayload};
    use crate::live::server_support::RetainedFiles;
    use bytes::Bytes;
    use emwin_db::{NoopMetadataSink, PersistenceConfig, PersistenceRuntime};
    use emwin_protocol::ingest::{IngestEvent, ProductOrigin};
    use emwin_protocol::qbt_receiver::QbtCompletedFile;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, AtomicUsize};
    use std::time::{Duration, Instant, SystemTime};
    use tempfile::tempdir;
    use tokio::sync::{broadcast, watch};

    fn test_state() -> Arc<AppState> {
        let (_, shutdown_rx) = watch::channel(false);
        Arc::new(AppState {
            event_tx: broadcast::channel(16).0,
            incident_event_tx: broadcast::channel(16).0,
            shutdown_rx,
            retained_files: Mutex::new(RetainedFiles::new(16, Duration::from_secs(60))),
            telemetry: Mutex::new(TelemetryPayload::Unavailable),
            persistence: None,
            archive: None,
            connected_clients: AtomicUsize::new(0),
            max_clients: 16,
            next_event_id: AtomicU64::new(1),
            next_incident_event_id: AtomicU64::new(1),
            data_blocks_total: AtomicU64::new(0),
            received_servers: AtomicUsize::new(0),
            received_sat_servers: AtomicUsize::new(0),
            started_at: Instant::now(),
            upstream_endpoint: Mutex::new(None),
            openapi_auth_token: None,
            quiet: true,
        })
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            writer
                .start_file(name, options)
                .expect("start file should succeed");
            writer.write_all(body).expect("write body should succeed");
        }
        writer.finish().expect("finish should succeed").into_inner()
    }

    #[tokio::test]
    async fn server_stats_snapshot_includes_persistence_metrics_when_enabled() {
        let state = test_state();
        state
            .connected_clients
            .store(3, std::sync::atomic::Ordering::Relaxed);
        state
            .data_blocks_total
            .store(5, std::sync::atomic::Ordering::Relaxed);
        state
            .received_servers
            .store(2, std::sync::atomic::Ordering::Relaxed);
        state
            .received_sat_servers
            .store(1, std::sync::atomic::Ordering::Relaxed);
        *state
            .upstream_endpoint
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some("example:2211".to_string());

        let temp = tempdir().expect("tempdir should succeed");
        let runtime = PersistenceRuntime::spawn(
            PersistenceConfig::new(4),
            emwin_db::FilesystemBlobWriter::new(temp.path().to_path_buf()),
            NoopMetadataSink,
        );
        let producer = runtime.producer();
        let metadata = RetainedFiles::new(1, Duration::from_secs(60)).insert(
            "TEST.TXT".to_string(),
            b"payload".to_vec(),
            1,
            ProductOrigin::Qbt,
            SystemTime::UNIX_EPOCH,
        );
        let request = build_persist_request("TEST.TXT", b"payload", metadata)
            .expect("persist request should build");
        let queued = producer.enqueue(request);
        assert!(queued.accepted);

        let snapshot = build_server_stats_snapshot(&state, Some(&producer));
        let persistence = snapshot
            .persistence
            .expect("persistence stats should exist");
        assert_eq!(snapshot.connected_clients, 3);
        assert_eq!(snapshot.data_blocks_total, 5);
        assert_eq!(snapshot.received_servers, 2);
        assert_eq!(snapshot.received_sat_servers, 1);
        assert_eq!(snapshot.upstream, "example:2211");
        assert_eq!(persistence.queue_capacity, 4);
        assert_eq!(persistence.enqueued_total, 1);

        runtime.shutdown().await.expect("shutdown should succeed");
    }

    #[test]
    fn server_stats_snapshot_omits_persistence_metrics_when_disabled() {
        let state = test_state();
        let snapshot = build_server_stats_snapshot(&state, None);
        assert_eq!(snapshot.upstream, "disconnected");
        assert!(snapshot.persistence.is_none());
    }

    #[test]
    fn product_event_post_processes_archives_before_retention_and_publish() {
        let state = test_state();
        let mut rx = state.event_tx.subscribe();
        let data = archive(&[(
            "nested/TAFPDKGA.TXT",
            b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n",
        )]);

        handle_ingest_event(
            Ok(IngestEvent::Product(
                QbtCompletedFile {
                    filename: "TAFPDKGA.ZIP".to_string(),
                    data: Bytes::from(data),
                    timestamp_utc: SystemTime::UNIX_EPOCH + Duration::from_secs(1),
                }
                .into(),
            )),
            &state,
            true,
            None,
        )
        .expect("ingest event should succeed");

        let retained = state
            .retained_files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get("nested/TAFPDKGA.TXT")
            .expect("retained file should exist");
        assert_eq!(retained.metadata.product.pil.as_deref(), Some("TAF"));
        assert_eq!(retained.metadata.filename, "nested/TAFPDKGA.TXT");
        assert!(
            state
                .retained_files
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get("TAFPDKGA.ZIP")
                .is_none()
        );

        let published = rx.try_recv().expect("file complete event should publish");
        match published.kind {
            EventKind::FileComplete(file) => {
                assert_eq!(file.metadata.filename, "nested/TAFPDKGA.TXT");
                assert_eq!(file.download_url, "/v1/files/nested%2FTAFPDKGA.TXT");
            }
            _ => panic!("expected file_complete event"),
        }

        let persisted = crate::live::file_pipeline::build_persist_request(
            &retained.metadata.filename,
            &retained.data,
            retained.metadata.clone(),
        )
        .expect("persist request should build");
        assert_eq!(
            persisted.request_key,
            "qbt/1970/01/01/FFC/nws_text_product/19700101T000001Z-e56e022c-TAFPDKGA.TXT"
        );
    }

    #[test]
    fn corrupt_archive_is_dropped_before_retention_and_publish() {
        let state = test_state();
        let mut rx = state.event_tx.subscribe();

        handle_ingest_event(
            Ok(IngestEvent::Product(
                QbtCompletedFile {
                    filename: "BROKEN.ZIP".to_string(),
                    data: Bytes::from_static(b"not a zip"),
                    timestamp_utc: SystemTime::UNIX_EPOCH + Duration::from_secs(1),
                }
                .into(),
            )),
            &state,
            true,
            None,
        )
        .expect("corrupt archive should not fail handler");

        assert_eq!(
            state
                .retained_files
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            0
        );
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}
