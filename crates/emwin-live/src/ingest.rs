use crate::archive_postprocess::post_process_archive;
use crate::error::LiveResult;
use crate::events::{publish, publish_incident_change};
use crate::persistence::{FilePersistenceProducer, enqueue_completed_product};
use crate::types::{AppState, LiveEventKind, LiveTelemetry};
use emwin_db::PostgresMetadataSink;
use emwin_protocol::ingest::{
    IngestConfig, IngestError, IngestEvent, IngestReceiver, IngestTelemetry, IngestWarning,
    ProductOrigin,
};
use emwin_protocol::qbt_receiver::{QbtProtocolWarning, QbtReceiverConfig};
use emwin_protocol::wxwire_receiver::WxWireReceiverConfig;
use emwin_service::ReceiverFrame;
use futures::StreamExt;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use tokio::sync::watch;

pub(crate) async fn run_qbt_ingest_loop(
    config: QbtReceiverConfig,
    state: Arc<AppState>,
    post_process_archives: bool,
    persistence: Option<FilePersistenceProducer>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> LiveResult<()> {
    state
        .active_servers
        .store(normalized_server_count(&config.servers), Ordering::Relaxed);
    let mut ingest = IngestReceiver::build(IngestConfig::Qbt(config))?;
    ingest.start()?;

    let mut events = ingest.events()?;
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
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

pub(crate) async fn run_wxwire_ingest_loop(
    config: WxWireReceiverConfig,
    state: Arc<AppState>,
    post_process_archives: bool,
    persistence: Option<FilePersistenceProducer>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> LiveResult<()> {
    let mut ingest = IngestReceiver::build(IngestConfig::WxWire(config))?;
    ingest.start()?;

    let mut events = ingest.events()?;
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
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

pub(crate) async fn run_incident_event_relay_loop(
    sink: PostgresMetadataSink,
    state: Arc<AppState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> LiveResult<()> {
    let mut rx = sink.subscribe_incident_changes();
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            received = rx.recv() => match received {
                Ok(change) => publish_incident_change(&state, change),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                    if !state.quiet {
                        tracing::info!("incident relay lagged dropped={dropped}");
                    }
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
) -> LiveResult<()> {
    match item {
        Ok(IngestEvent::Connected { endpoint }) => {
            let mut guard = state
                .upstream_endpoint
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = Some(endpoint.clone());
            if !state.quiet {
                tracing::info!("upstream connected endpoint={endpoint}");
            }
            publish(state, LiveEventKind::Connected { endpoint });
        }
        Ok(IngestEvent::Disconnected) => {
            let mut guard = state
                .upstream_endpoint
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = None;
            if !state.quiet {
                tracing::info!("upstream disconnected");
            }
            publish(state, LiveEventKind::Disconnected);
        }
        Ok(IngestEvent::Telemetry(snapshot)) => {
            let telemetry = match snapshot {
                IngestTelemetry::Qbt(value) => {
                    state
                        .active_servers
                        .store(value.active_servers, Ordering::Relaxed);
                    LiveTelemetry::Qbt(serde_json::to_value(value)?)
                }
                IngestTelemetry::WxWire(value) => {
                    LiveTelemetry::WxWire(serde_json::to_value(value)?)
                }
                _ => LiveTelemetry::Unavailable,
            };
            let mut guard = state
                .telemetry
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = telemetry.clone();
            publish(state, LiveEventKind::Telemetry(telemetry));
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
                let queued = enqueue_completed_product(
                    persistence,
                    &delivered.filename,
                    &delivered.data,
                    retained_meta.clone(),
                )?;
                if !queued {
                    tracing::warn!(filename = %delivered.filename, "persistence queue closed");
                }
            }
            publish(
                state,
                LiveEventKind::ProductAvailable(Box::new(retained_meta)),
            );
            if !state.quiet {
                tracing::info!(
                    "file complete name={} bytes={}",
                    delivered.filename,
                    delivered.data.len()
                );
            }
        }
        Ok(IngestEvent::Warning(warning)) => match warning {
            IngestWarning::Qbt(value) => {
                if let QbtProtocolWarning::BackpressureDrop { .. } = value
                    && !state.quiet
                {
                    tracing::info!("qbt ingest backpressure warning");
                }
                publish(
                    state,
                    LiveEventKind::ReceiverFrame(ReceiverFrame {
                        receiver: "qbt".to_string(),
                        event_name: "warning".to_string(),
                        payload: serde_json::json!({
                            "type": "warning",
                            "warning": format!("{value:?}"),
                        }),
                    }),
                );
            }
            IngestWarning::WxWire(value) => {
                publish(
                    state,
                    LiveEventKind::ReceiverFrame(ReceiverFrame {
                        receiver: "wxwire".to_string(),
                        event_name: "warning".to_string(),
                        payload: serde_json::json!({
                            "type": "warning",
                            "warning": format!("{value:?}"),
                        }),
                    }),
                );
            }
            _ => publish(
                state,
                LiveEventKind::Error {
                    message: format!("ingest warning: {warning:?}"),
                },
            ),
        },
        Err(err) => {
            tracing::error!("client error: {err}");
            publish(
                state,
                LiveEventKind::Error {
                    message: err.to_string(),
                },
            );
        }
        Ok(_) => {}
    }

    Ok(())
}

fn normalized_server_count(servers: &[(String, u16)]) -> usize {
    let mut normalized = servers.to_vec();
    normalized.sort_unstable();
    normalized.dedup();
    normalized.len()
}

#[cfg(test)]
mod tests {
    use super::{handle_ingest_event, normalized_server_count};
    use crate::persistence::FilePersistenceProducer;
    use crate::types::{AppState, LiveEventKind};
    use bytes::Bytes;
    use emwin_protocol::ingest::IngestEvent;
    use emwin_protocol::qbt_receiver::QbtCompletedFile;
    use std::io::Write;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    fn test_state() -> Arc<AppState> {
        AppState::new(None, None, true, 16, 60)
    }

    #[test]
    fn normalized_server_count_matches_round_robin_dedup() {
        assert_eq!(
            normalized_server_count(&[
                ("b.example".to_string(), 2),
                ("a.example".to_string(), 1),
                ("b.example".to_string(), 2),
            ]),
            2
        );
    }

    fn persistence_producer() -> FilePersistenceProducer {
        let temp = tempdir().expect("tempdir should succeed");
        let runtime = emwin_db::PersistenceRuntime::spawn(
            emwin_db::PersistenceConfig::new(16),
            Box::new(
                emwin_db::ObjectStoreBlobWriter::new(
                    url::Url::from_directory_path(temp.path()).expect("directory url should build"),
                )
                .expect("writer should build"),
            ),
            emwin_db::NoopMetadataSink,
        );
        runtime.producer()
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
    async fn completed_product_is_retained_and_published() {
        let state = test_state();
        let mut rx = state.event_tx.subscribe();
        let event = Ok(IngestEvent::Product(
            QbtCompletedFile {
                filename: "AFDBOX.TXT".to_string(),
                data: Bytes::from_static(b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n"),
                timestamp_utc: SystemTime::UNIX_EPOCH + Duration::from_secs(1704070800),
            }
            .into(),
        ));

        handle_ingest_event(event, &state, true, None).expect("event should succeed");

        let received = rx.recv().await.expect("event should publish");
        match received.kind {
            LiveEventKind::ProductAvailable(metadata) => {
                assert_eq!(metadata.filename, "AFDBOX.TXT");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn archive_products_are_post_processed_before_persistence() {
        let state = test_state();
        let producer = persistence_producer();
        let zip_bytes = archive(&[(
            "nested/AFDBOX.TXT",
            b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n",
        )]);
        let event = Ok(IngestEvent::Product(
            QbtCompletedFile {
                filename: "AFDBOX.ZIP".to_string(),
                data: Bytes::from(zip_bytes),
                timestamp_utc: SystemTime::UNIX_EPOCH + Duration::from_secs(1704070800),
            }
            .into(),
        ));

        handle_ingest_event(event, &state, true, Some(&producer)).expect("event should succeed");

        let listed = state
            .retained_files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "nested/AFDBOX.TXT");
    }
}
