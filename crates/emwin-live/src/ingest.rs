use crate::error::LiveResult;
use crate::events::{publish, publish_incident_change};
use crate::product_processor::ProductWorkItem;
use crate::shared::lock_unpoisoned;
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
use tokio::sync::watch;

pub(crate) async fn run_qbt_ingest_loop(
    config: QbtReceiverConfig,
    state: Arc<AppState>,
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
                handle_ingest_event(item, &state)?;
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
                handle_ingest_event(item, &state)?;
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
) -> LiveResult<()> {
    match item {
        Ok(IngestEvent::Connected { endpoint }) => {
            let mut guard = lock_unpoisoned(&state.upstream_endpoint);
            *guard = Some(endpoint.clone());
            if !state.quiet {
                tracing::info!("upstream connected endpoint={endpoint}");
            }
            publish(state, LiveEventKind::Connected { endpoint });
        }
        Ok(IngestEvent::Disconnected) => {
            let mut guard = lock_unpoisoned(&state.upstream_endpoint);
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
            let mut guard = lock_unpoisoned(&state.telemetry);
            *guard = telemetry.clone();
            publish(state, LiveEventKind::Telemetry(telemetry));
        }
        Ok(IngestEvent::Product(product)) => {
            if matches!(product.origin, ProductOrigin::Qbt) {
                state.data_blocks_total.fetch_add(1, Ordering::Relaxed);
            }

            let filename = product.filename.clone();
            let result = state.product_processor.enqueue(ProductWorkItem::new(
                product.filename,
                product.data,
                product.source_timestamp_utc,
                product.origin,
            ));
            if let Some(evicted_oldest_filename) = result.evicted_oldest_filename {
                tracing::warn!(
                    evicted_filename = %evicted_oldest_filename,
                    queued_filename = %filename,
                    queue_len = result.queue_len,
                    "product processing queue evicted oldest product"
                );
            }
            if !result.accepted {
                tracing::warn!(filename = %filename, "product processing queue closed");
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
    use crate::product_processor::ProductProcessorProducer;
    use crate::types::AppState;
    use bytes::Bytes;
    use emwin_protocol::ingest::IngestEvent;
    use emwin_protocol::qbt_receiver::QbtCompletedFile;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    fn test_state() -> Arc<AppState> {
        AppState::new(None, ProductProcessorProducer::new(16), None, true, 16, 60)
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

    #[test]
    fn completed_product_is_queued_for_processing() {
        let state = test_state();
        let event = Ok(IngestEvent::Product(
            QbtCompletedFile {
                filename: "AFDBOX.TXT".to_string(),
                data: Bytes::from_static(b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n"),
                timestamp_utc: SystemTime::UNIX_EPOCH + Duration::from_secs(1704070800),
            }
            .into(),
        ));

        handle_ingest_event(event, &state).expect("event should succeed");

        let stats = state.product_processor.stats_snapshot();
        assert_eq!(stats.queue_len, 1);
        assert_eq!(stats.enqueued_total, 1);
    }
}
