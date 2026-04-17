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
            Ok(request_key) => {
                let mut guard = lock_unpoisoned(&producer.shared.state);
                guard.stats.persisted_total = guard.stats.persisted_total.saturating_add(1);
                info!(request_key = %request_key, "save complete");
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

async fn write_blobs<W>(writer: &W, blobs: &[BlobEntry]) -> PersistResult<Vec<StoredBlob>>
where
    W: BlobWriter,
{
    let mut stored_blobs = Vec::with_capacity(blobs.len());
    for blob in blobs {
        let stored = writer.write(blob).await?;
        stored_blobs.push(stored);
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
) -> Result<String, (String, FailureContext, PersistError)>
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
    let stored_blobs = loop {
        match write_blobs(writer, &request.blobs).await {
            Ok(stored_blobs) => break stored_blobs,
            Err(err) if err.is_retryable() && !should_abort_retry(producer) => {
                let delay = retry_delay(config, attempt);
                backend_health.note_retryable_failure(
                    &request_key,
                    &blob_context,
                    &err,
                    delay,
                    attempt + 1,
                    config.failure_log_cooldown,
                );
                tokio::time::sleep(delay).await;
                attempt = attempt.saturating_add(1);
            }
            Err(err) => return Err((request_key, blob_context.clone(), err)),
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
                return Ok(request_key);
            }
            Err(err) if err.is_retryable() && !should_abort_retry(producer) => {
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
            Err(err) => {
                return Err((request_key, metadata_context.clone(), err));
            }
        }
    }
}

fn should_abort_retry<M>(producer: &PersistenceProducer<M>) -> bool {
    let guard = lock_unpoisoned(&producer.shared.state);
    guard.closed
}
