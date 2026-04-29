use super::queue::SharedQueue;
use super::retry::{BackendHealth, FailureContext, retry_delay};
use super::{
    BlobEntry, BlobWriter, MetadataSink, PersistRequest, PersistedRequest, PersistenceConfig,
    PersistenceProducer, PersistenceStats,
};
use crate::error::{PersistError, PersistResult};
use crate::sync::lock_unpoisoned;
use crate::writer::StoredBlob;
use std::sync::Arc;
use tracing::{info, warn};

enum PersistOutcome {
    Persisted(String),
    StaleDropped {
        request_key: String,
        context: FailureContext,
        error: PersistError,
    },
}

struct BlobWriteFailure {
    stored_blobs: Vec<StoredBlob>,
    error: PersistError,
}

pub(super) async fn run_worker<M, W, S>(
    shared: Arc<SharedQueue<M>>,
    config: PersistenceConfig,
    writer: W,
    sink: S,
) -> PersistenceStats
where
    M: Clone + Send + 'static,
    W: BlobWriter,
    S: MetadataSink<M>,
{
    let producer = PersistenceProducer {
        shared: Arc::clone(&shared),
    };
    let mut backend_health = BackendHealth::default();

    loop {
        match shared.available.acquire().await {
            Ok(permit) => permit.forget(),
            Err(_) => break,
        }

        let Some(request) = pop_request(&producer) else {
            if is_closed(&producer) {
                break;
            }
            continue;
        };

        match persist_request_with_retry(
            &producer,
            &writer,
            &sink,
            request,
            &config,
            &mut backend_health,
        )
        .await
        {
            Ok(PersistOutcome::Persisted(request_key)) => {
                let mut guard = lock_unpoisoned(&producer.shared.state);
                guard.stats.persisted_total = guard.stats.persisted_total.saturating_add(1);
                info!(request_key = %request_key, "save complete");
            }
            Ok(PersistOutcome::StaleDropped {
                request_key,
                context,
                error,
            }) => {
                let mut guard = lock_unpoisoned(&producer.shared.state);
                guard.stats.retry_exhausted_total =
                    guard.stats.retry_exhausted_total.saturating_add(1);
                guard.stats.stale_dropped_total = guard.stats.stale_dropped_total.saturating_add(1);
                warn!(
                    request_key = %request_key,
                    stage = context.stage,
                    backend = context.backend,
                    target = %context.target,
                    error = %error,
                    retry_max_attempts = config.retry_max_attempts,
                    "stale persistence request dropped after retry budget exhausted"
                );
            }
            Err((request_key, context, err)) => {
                let mut guard = lock_unpoisoned(&producer.shared.state);
                guard.stats.failed_total = guard.stats.failed_total.saturating_add(1);
                warn!(
                    request_key = %request_key,
                    stage = context.stage,
                    backend = context.backend,
                    target = %context.target,
                    error = %err,
                    "persistence request failed"
                );
            }
        }
    }

    let stats = producer.stats_snapshot();
    info!(
        queue_len = stats.queue_len,
        queue_capacity = stats.queue_capacity,
        enqueued_total = stats.enqueued_total,
        evicted_total = stats.evicted_total,
        persisted_total = stats.persisted_total,
        failed_total = stats.failed_total,
        retry_exhausted_total = stats.retry_exhausted_total,
        stale_dropped_total = stats.stale_dropped_total,
        "persistence runtime stopped"
    );
    stats
}

fn pop_request<M>(producer: &PersistenceProducer<M>) -> Option<PersistRequest<M>> {
    let mut guard = lock_unpoisoned(&producer.shared.state);
    guard.pending.pop_front()
}

fn is_closed<M>(producer: &PersistenceProducer<M>) -> bool {
    let guard = lock_unpoisoned(&producer.shared.state);
    guard.closed && guard.pending.is_empty()
}

async fn write_blobs<W>(
    writer: &W,
    blobs: &[BlobEntry],
) -> Result<Vec<StoredBlob>, BlobWriteFailure>
where
    W: BlobWriter,
{
    let mut stored_blobs = Vec::with_capacity(blobs.len());
    for blob in blobs {
        match writer.write(blob).await {
            Ok(stored) => stored_blobs.push(stored),
            Err(error) => {
                return Err(BlobWriteFailure {
                    stored_blobs,
                    error,
                });
            }
        }
    }

    Ok(stored_blobs)
}

async fn persist_metadata<M, S>(
    sink: &S,
    request_key: &str,
    metadata: M,
    blobs: Vec<StoredBlob>,
) -> PersistResult<()>
where
    M: Send + 'static,
    S: MetadataSink<M>,
{
    sink.persist(PersistedRequest {
        request_key: request_key.to_string(),
        metadata,
        blobs,
    })
    .await
}

async fn persist_request_with_retry<M, W, S>(
    producer: &PersistenceProducer<M>,
    writer: &W,
    sink: &S,
    request: PersistRequest<M>,
    config: &PersistenceConfig,
    backend_health: &mut BackendHealth,
) -> Result<PersistOutcome, (String, FailureContext, PersistError)>
where
    M: Clone + Send + 'static,
    W: BlobWriter,
    S: MetadataSink<M>,
{
    let request_key = request.request_key.clone();
    let mut attempt: u32 = 0;
    let blob_context = FailureContext {
        stage: "blob_write",
        backend: writer.backend_name(),
        target: writer.target_description(),
    };
    let mut blob_cleanup_candidates = Vec::new();
    let stored_blobs = loop {
        match write_blobs(writer, &request.blobs).await {
            Ok(stored_blobs) => break stored_blobs,
            Err(failure) if failure.error.is_retryable() && !should_abort_retry(producer) => {
                blob_cleanup_candidates.extend(failure.stored_blobs);
                if retry_budget_exhausted(config, attempt) {
                    cleanup_stored_blobs(writer, &request_key, &blob_cleanup_candidates).await;
                    return Ok(PersistOutcome::StaleDropped {
                        request_key,
                        context: blob_context.clone(),
                        error: failure.error,
                    });
                }
                let delay = retry_delay(config, attempt);
                backend_health.note_retryable_failure(
                    &request_key,
                    &blob_context,
                    &failure.error,
                    delay,
                    attempt + 1,
                    config.failure_log_cooldown,
                );
                tokio::time::sleep(delay).await;
                attempt = attempt.saturating_add(1);
            }
            Err(failure) => return Err((request_key, blob_context.clone(), failure.error)),
        }
    };

    attempt = 0;
    let metadata_context = FailureContext {
        stage: "metadata_persist",
        backend: sink.backend_name(),
        target: sink
            .target_description()
            .unwrap_or_else(|| "unavailable".to_string()),
    };
    loop {
        match persist_metadata(
            sink,
            &request_key,
            request.metadata.clone(),
            stored_blobs.clone(),
        )
        .await
        {
            Ok(()) => {
                backend_health.note_recovered(&request_key);
                return Ok(PersistOutcome::Persisted(request_key));
            }
            Err(err) if err.is_retryable() && !should_abort_retry(producer) => {
                if retry_budget_exhausted(config, attempt) {
                    cleanup_stored_blobs(writer, &request_key, &stored_blobs).await;
                    return Ok(PersistOutcome::StaleDropped {
                        request_key,
                        context: metadata_context.clone(),
                        error: err,
                    });
                }
                let delay = retry_delay(config, attempt);
                backend_health.note_retryable_failure(
                    &request_key,
                    &metadata_context,
                    &err,
                    delay,
                    attempt + 1,
                    config.failure_log_cooldown,
                );
                tokio::time::sleep(delay).await;
                attempt = attempt.saturating_add(1);
            }
            Err(err) => return Err((request_key, metadata_context.clone(), err)),
        }
    }
}

async fn cleanup_stored_blobs<W>(writer: &W, request_key: &str, stored_blobs: &[StoredBlob])
where
    W: BlobWriter,
{
    for blob in stored_blobs {
        if let Err(err) = writer.delete(blob).await {
            warn!(
                request_key = %request_key,
                location = %blob.location,
                error = %err,
                "failed to clean up blob after metadata retry exhaustion"
            );
        }
    }
}

fn retry_budget_exhausted(config: &PersistenceConfig, attempt: u32) -> bool {
    attempt.saturating_add(1) >= config.retry_max_attempts.max(1)
}

fn should_abort_retry<M>(producer: &PersistenceProducer<M>) -> bool {
    let guard = lock_unpoisoned(&producer.shared.state);
    guard.closed
}
