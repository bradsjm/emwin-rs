//! CLI adapters for the async persistence runtime.
//!
//! This module keeps storage runtime wiring out of command handlers while allowing live modes to
//! enqueue completed products without waiting for backend I/O.

use crate::live::file_pipeline::build_persist_request;
use emwin_db::{
    BlobWriter, CompletedFileMetadata, FilesystemBlobWriter, IncidentCleanupResult,
    NoopMetadataSink, PersistRequest, PersistenceConfig, PersistenceProducer, PersistenceRuntime,
    PersistenceStats, PostgresConfig, PostgresMetadataSink, S3BlobWriter,
};
use std::path::PathBuf;
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
enum StorageTarget {
    Filesystem(PathBuf),
    S3 {
        bucket: String,
        prefix: Option<String>,
    },
}

pub(crate) async fn start_runtime_with_postgres(
    output_dir: String,
    queue_capacity: usize,
    postgres_database_url: Option<&str>,
    application_name: &str,
) -> crate::error::CliResult<StartedPersistenceRuntime> {
    let writer = build_blob_writer(parse_storage_target(&output_dir)?)?;
    let (runtime, postgres_sink) = if let Some(database_url) = postgres_database_url {
        let mut config = PostgresConfig::new(database_url);
        config.application_name = application_name.to_string();
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

fn build_blob_writer(target: StorageTarget) -> crate::error::CliResult<Box<dyn BlobWriter>> {
    let writer: Box<dyn BlobWriter> = match target {
        StorageTarget::Filesystem(path) => Box::new(FilesystemBlobWriter::new(path)),
        StorageTarget::S3 { bucket, prefix } => Box::new(S3BlobWriter::new(bucket, prefix)?),
    };
    Ok(writer)
}

fn parse_storage_target(raw: &str) -> crate::error::CliResult<StorageTarget> {
    if raw.is_empty() {
        return Err(crate::error::CliError::invalid_argument(
            "--output-dir cannot be empty",
        ));
    }

    if raw.starts_with("s3://") {
        return parse_s3_target(raw);
    }

    if raw.contains("://") {
        return Err(crate::error::CliError::invalid_argument(format!(
            "--output-dir only supports filesystem paths or s3://bucket[/prefix], got `{raw}`"
        )));
    }

    Ok(StorageTarget::Filesystem(PathBuf::from(raw)))
}

fn parse_s3_target(raw: &str) -> crate::error::CliResult<StorageTarget> {
    let url = Url::parse(raw).map_err(|err| {
        crate::error::CliError::invalid_argument(format!("invalid S3 output URI `{raw}`: {err}"))
    })?;

    if url.scheme() != "s3" {
        return Err(crate::error::CliError::invalid_argument(format!(
            "--output-dir only supports filesystem paths or s3://bucket[/prefix], got `{raw}`"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(crate::error::CliError::invalid_argument(format!(
            "S3 output URI must not include credentials: `{raw}`"
        )));
    }
    if url.port().is_some() {
        return Err(crate::error::CliError::invalid_argument(format!(
            "S3 output URI must not include a port: `{raw}`"
        )));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(crate::error::CliError::invalid_argument(format!(
            "S3 output URI must not include query or fragment components: `{raw}`"
        )));
    }

    let bucket = url.host_str().ok_or_else(|| {
        crate::error::CliError::invalid_argument(format!(
            "S3 output URI must include a bucket name: `{raw}`"
        ))
    })?;

    let raw_path = url.path().trim_matches('/');
    if raw_path.split('/').any(|segment| segment.is_empty()) && !raw_path.is_empty() {
        return Err(crate::error::CliError::invalid_argument(format!(
            "S3 output URI contains an empty prefix segment: `{raw}`"
        )));
    }

    let prefix = (!raw_path.is_empty()).then(|| raw_path.to_string());
    Ok(StorageTarget::S3 {
        bucket: bucket.to_string(),
        prefix,
    })
}

pub(crate) fn enqueue_completed_product(
    producer: &FilePersistenceProducer,
    filename: &str,
    data: &[u8],
    metadata: CompletedFileMetadata,
) -> crate::error::CliResult<bool> {
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
) -> crate::error::CliResult<PersistenceStats> {
    runtime.shutdown().await.map_err(Into::into)
}

pub(crate) async fn run_incident_cleanup_loop(
    sink: PostgresMetadataSink,
    mut shutdown_rx: watch::Receiver<bool>,
) -> crate::error::CliResult<()> {
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
    use std::path::PathBuf;

    #[test]
    fn filesystem_targets_remain_plain_paths() {
        assert_eq!(
            parse_storage_target("./out").expect("filesystem path should parse"),
            StorageTarget::Filesystem(PathBuf::from("./out"))
        );
        assert_eq!(
            parse_storage_target("/tmp/emwin").expect("absolute filesystem path should parse"),
            StorageTarget::Filesystem(PathBuf::from("/tmp/emwin"))
        );
    }

    #[test]
    fn s3_targets_accept_bucket_and_prefix() {
        assert_eq!(
            parse_storage_target("s3://bucket").expect("bucket root should parse"),
            StorageTarget::S3 {
                bucket: "bucket".to_string(),
                prefix: None,
            }
        );
        assert_eq!(
            parse_storage_target("s3://bucket/prefix/nested").expect("bucket prefix should parse"),
            StorageTarget::S3 {
                bucket: "bucket".to_string(),
                prefix: Some("prefix/nested".to_string()),
            }
        );
        assert_eq!(
            parse_storage_target("s3://bucket/prefix/nested/")
                .expect("trailing slash should normalize"),
            StorageTarget::S3 {
                bucket: "bucket".to_string(),
                prefix: Some("prefix/nested".to_string()),
            }
        );
    }

    #[test]
    fn storage_target_rejects_invalid_uris() {
        for value in [
            "https://example.com/out",
            "s3:///prefix",
            "s3://bucket?x=1",
            "s3://bucket#frag",
            "s3://user@bucket/prefix",
            "s3://bucket:9000/prefix",
            "",
        ] {
            assert!(parse_storage_target(value).is_err(), "{value} should fail");
        }
    }

    #[test]
    fn storage_target_does_not_accept_endpoint_urls_in_output_dir() {
        assert!(parse_storage_target("s3://bucket/http://localhost:9000").is_err());
    }
}
