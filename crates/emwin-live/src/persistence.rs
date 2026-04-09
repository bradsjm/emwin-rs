//! CLI adapters for the async persistence runtime.
//!
//! This module keeps storage runtime wiring out of command handlers while allowing live modes to
//! enqueue completed products without waiting for backend I/O.

use crate::error::{LiveError, LiveResult};
use crate::file_pipeline::build_persist_request;
use emwin_db::{
    BlobWriter, NoopMetadataSink, ObjectStoreBlobWriter, PersistRequest, PersistenceConfig,
    PersistenceProducer, PersistenceRuntime, PostgresConfig, PostgresMetadataSink,
};
use emwin_service::{CompletedFileMetadata, IncidentCleanupResult, PersistenceStats};
use object_store::parse_url_opts;
use std::time::Duration;
use tokio::sync::watch;
use tracing::warn;
use url::Url;

pub(crate) type FilePersistenceRuntime = PersistenceRuntime<CompletedFileMetadata>;
pub(crate) type FilePersistenceProducer = PersistenceProducer<CompletedFileMetadata>;

pub(crate) const INCIDENT_CLEANUP_INTERVAL: Duration = Duration::from_secs(300);

pub(crate) struct StartedPersistenceRuntime {
    pub(crate) runtime: FilePersistenceRuntime,
    pub(crate) postgres_sink: Option<PostgresMetadataSink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageTarget(Url);

pub(crate) async fn start_runtime_with_postgres(
    output_dir: String,
    queue_capacity: usize,
    postgres_database_url: Option<&str>,
    max_db_connections: u32,
    application_name: &str,
) -> LiveResult<StartedPersistenceRuntime> {
    let writer = build_blob_writer(parse_storage_target(&output_dir)?)?;
    let (runtime, postgres_sink) = if let Some(database_url) = postgres_database_url {
        let mut config = PostgresConfig::new(database_url);
        config.application_name = application_name.to_string();
        config.max_connections = max_db_connections.max(1);
        let sink = PostgresMetadataSink::new(config);
        (
            PersistenceRuntime::spawn(PersistenceConfig::new(queue_capacity), writer, sink.clone()),
            Some(sink),
        )
    } else {
        (
            PersistenceRuntime::spawn(
                PersistenceConfig::new(queue_capacity),
                writer,
                NoopMetadataSink,
            ),
            None,
        )
    };
    Ok(StartedPersistenceRuntime {
        runtime,
        postgres_sink,
    })
}

fn build_blob_writer(target: StorageTarget) -> LiveResult<Box<dyn BlobWriter>> {
    let writer: Box<dyn BlobWriter> = Box::new(ObjectStoreBlobWriter::new(target.0)?);
    Ok(writer)
}

fn parse_storage_target(raw: &str) -> LiveResult<StorageTarget> {
    if raw.is_empty() {
        return Err(LiveError::invalid_argument("--output-dir cannot be empty"));
    }
    let url = Url::parse(raw).map_err(|err| {
        LiveError::invalid_argument(format!("invalid object store output URI `{raw}`: {err}"))
    })?;

    if url.query().is_some() || url.fragment().is_some() {
        return Err(LiveError::invalid_argument(format!(
            "object store output URI must not include query or fragment components: `{raw}`"
        )));
    }

    parse_url_opts(&url, std::env::vars()).map_err(|err| {
        LiveError::invalid_argument(format!("invalid object store output URI `{raw}`: {err}"))
    })?;

    Ok(StorageTarget(url))
}

pub(crate) fn enqueue_completed_product(
    producer: &FilePersistenceProducer,
    filename: &str,
    data: &[u8],
    metadata: CompletedFileMetadata,
) -> LiveResult<bool> {
    let request: PersistRequest<CompletedFileMetadata> =
        build_persist_request(filename, data, metadata)?;
    let result = producer.enqueue(request);
    if let Some(evicted_oldest_key) = result.evicted_oldest_key {
        warn!(
            evicted_request = %evicted_oldest_key,
            queued_request = %filename,
            queue_len = result.queue_len,
            "persistence queue evicted oldest request"
        );
    }
    Ok(result.accepted)
}

pub(crate) async fn shutdown_runtime(
    runtime: FilePersistenceRuntime,
) -> LiveResult<PersistenceStats> {
    runtime
        .shutdown()
        .await
        .map(|stats| PersistenceStats {
            queue_len: stats.queue_len,
            queue_capacity: stats.queue_capacity,
            enqueued_total: stats.enqueued_total,
            evicted_total: stats.evicted_total,
            persisted_total: stats.persisted_total,
            failed_total: stats.failed_total,
        })
        .map_err(Into::into)
}

pub(crate) async fn run_incident_cleanup_loop(
    sink: PostgresMetadataSink,
    mut shutdown_rx: watch::Receiver<bool>,
) -> LiveResult<()> {
    let mut interval = tokio::time::interval(INCIDENT_CLEANUP_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await;

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            _ = interval.tick() => {
                match sink.expire_active_incidents(chrono::Utc::now()).await {
                    Ok(IncidentCleanupResult { expired_count }) => {
                        if expired_count > 0 {
                            tracing::info!(
                                backend = "database",
                                target = %sink.describe_target(),
                                expired_count,
                                "expired stale incidents"
                            );
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            backend = "database",
                            target = %sink.describe_target(),
                            stage = "incident_cleanup",
                            error = %err,
                            "incident cleanup pass failed"
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{StorageTarget, parse_storage_target};
    use url::Url;

    #[test]
    fn file_url_targets_are_required_for_local_filesystem() {
        assert_eq!(
            parse_storage_target("file:///tmp/emwin").expect("file uri should parse"),
            StorageTarget(Url::parse("file:///tmp/emwin").expect("url should parse"))
        );
    }

    #[test]
    fn object_store_targets_accept_supported_urls() {
        assert_eq!(
            parse_storage_target("s3://bucket").expect("bucket root should parse"),
            StorageTarget(Url::parse("s3://bucket").expect("url should parse"))
        );
        assert_eq!(
            parse_storage_target("s3://bucket/prefix/nested").expect("s3 prefix should parse"),
            StorageTarget(Url::parse("s3://bucket/prefix/nested").expect("url should parse"))
        );
        assert_eq!(
            parse_storage_target("https://example.com/archive").expect("http target should parse"),
            StorageTarget(Url::parse("https://example.com/archive").expect("url should parse"))
        );
    }

    #[test]
    fn storage_target_rejects_invalid_uris() {
        for value in [
            "./out",
            "/tmp/emwin",
            "s3:///prefix",
            "://missing-scheme",
            "s3://[::1",
            "",
        ] {
            assert!(parse_storage_target(value).is_err(), "{value} should fail");
        }
    }
}
