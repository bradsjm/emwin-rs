use crate::error::PersistResult;
use crate::writer::{BlobEntry, BlobWriter, StoredBlob};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::info;

mod queue;
mod retry;
#[cfg(test)]
mod tests;
mod worker;

use queue::SharedQueue;
use worker::run_worker;

const DEFAULT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_RETRY_MAX_DELAY: Duration = Duration::from_secs(60);
const DEFAULT_FAILURE_LOG_COOLDOWN: Duration = Duration::from_secs(30);

/// Configuration for the background persistence runtime.
#[derive(Debug, Clone, Copy)]
pub struct PersistenceConfig {
    /// Maximum number of queued requests kept in memory.
    pub queue_capacity: usize,
    /// Initial backoff applied after retryable persistence failures.
    pub retry_initial_delay: Duration,
    /// Upper bound for retry backoff during sustained outages.
    pub retry_max_delay: Duration,
    /// Minimum spacing between repeated warning logs for the same backend failure class.
    pub failure_log_cooldown: Duration,
}

impl PersistenceConfig {
    /// Creates a persistence config, coercing zero capacity to one.
    pub fn new(queue_capacity: usize) -> Self {
        Self {
            queue_capacity: queue_capacity.max(1),
            retry_initial_delay: DEFAULT_RETRY_INITIAL_DELAY,
            retry_max_delay: DEFAULT_RETRY_MAX_DELAY,
            failure_log_cooldown: DEFAULT_FAILURE_LOG_COOLDOWN,
        }
    }

    /// Overrides retry delays while keeping queue sizing unchanged.
    pub fn with_retry_delays(mut self, initial_delay: Duration, max_delay: Duration) -> Self {
        self.retry_initial_delay = initial_delay;
        self.retry_max_delay = max_delay.max(initial_delay);
        self
    }

    /// Overrides warning log throttling while keeping other defaults unchanged.
    pub fn with_failure_log_cooldown(mut self, cooldown: Duration) -> Self {
        self.failure_log_cooldown = cooldown;
        self
    }
}

/// Request submitted to the persistence runtime.
#[derive(Debug, Clone)]
pub struct PersistRequest<M> {
    /// Stable identifier used in logs, metrics, and eviction reporting.
    pub request_key: String,
    /// Caller-provided metadata handed to the sink after blob persistence succeeds.
    pub metadata: M,
    /// Raw payloads to persist before metadata commit.
    pub blobs: Vec<BlobEntry>,
}

/// Completed persistence request passed to the metadata sink.
#[derive(Debug)]
pub struct PersistedRequest<M> {
    /// Stable identifier copied from the original request.
    pub request_key: String,
    /// Caller-provided metadata.
    pub metadata: M,
    /// Stable references to the persisted blobs.
    pub blobs: Vec<StoredBlob>,
}

/// Persists metadata after all referenced blobs have been written successfully.
pub trait MetadataSink<M>: Send + Sync + 'static {
    /// Commits metadata and blob references for one completed request.
    fn persist<'a>(
        &'a self,
        request: PersistedRequest<M>,
    ) -> crate::writer::BoxFuture<'a, PersistResult<()>>;

    /// Stable backend label for diagnostics.
    fn backend_name(&self) -> &'static str {
        "metadata"
    }

    /// Human-readable target description for diagnostics.
    fn target_description(&self) -> Option<String> {
        None
    }
}

/// Metadata sink that intentionally discards metadata writes.
#[derive(Debug, Default)]
pub struct NoopMetadataSink;

impl<M: Send + 'static> MetadataSink<M> for NoopMetadataSink {
    fn persist<'a>(
        &'a self,
        _request: PersistedRequest<M>,
    ) -> crate::writer::BoxFuture<'a, PersistResult<()>> {
        Box::pin(async { Ok(()) })
    }

    fn backend_name(&self) -> &'static str {
        "noop"
    }
}

/// Snapshot of the runtime queue and outcome counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PersistenceStats {
    /// Number of requests currently waiting in the queue.
    pub queue_len: usize,
    /// Maximum number of requests the queue can hold before eviction starts.
    pub queue_capacity: usize,
    /// Number of requests accepted by producers.
    pub enqueued_total: u64,
    /// Number of queued requests evicted to admit newer work.
    pub evicted_total: u64,
    /// Number of requests fully persisted.
    pub persisted_total: u64,
    /// Number of requests that failed during blob or metadata persistence.
    pub failed_total: u64,
}

/// Result returned to producers after enqueueing a persistence request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueueResult {
    /// Whether the request was accepted into the queue.
    pub accepted: bool,
    /// Key of the evicted request when the queue was full.
    pub evicted_oldest_key: Option<String>,
    /// Queue length after enqueue processing completes.
    pub queue_len: usize,
}

/// Cloneable producer used by ingest code to enqueue background persistence work.
#[derive(Debug)]
pub struct PersistenceProducer<M> {
    shared: Arc<SharedQueue<M>>,
}

/// Background runtime draining queued persistence work.
#[derive(Debug)]
pub struct PersistenceRuntime<M> {
    producer: PersistenceProducer<M>,
    task: JoinHandle<PersistenceStats>,
}

impl<M: Clone + Send + 'static> PersistenceRuntime<M> {
    /// Spawns a background worker that drains queued requests until shutdown.
    pub fn spawn<W, S>(config: PersistenceConfig, writer: W, sink: S) -> Self
    where
        W: BlobWriter,
        S: MetadataSink<M>,
    {
        let shared = SharedQueue::new(config.queue_capacity);
        let producer = PersistenceProducer {
            shared: Arc::clone(&shared),
        };
        let worker_producer = producer.clone();
        let task = tokio::spawn(async move { run_worker(shared, config, writer, sink).await });

        info!(
            queue_capacity = config.queue_capacity.max(1),
            "persistence runtime started"
        );

        Self {
            producer: worker_producer,
            task,
        }
    }

    /// Returns a cloneable producer handle for hot-path enqueue operations.
    pub fn producer(&self) -> PersistenceProducer<M> {
        self.producer.clone()
    }

    /// Returns a point-in-time snapshot of queue depth and cumulative outcomes.
    pub fn stats_snapshot(&self) -> PersistenceStats {
        self.producer.stats_snapshot()
    }

    /// Closes the queue, drops queued requests, and returns final runtime stats.
    pub async fn shutdown(self) -> PersistResult<PersistenceStats> {
        let dropped = self.producer.close();
        if dropped > 0 {
            info!(
                dropped_requests = dropped,
                "dropped queued persistence requests during shutdown"
            );
        }
        Ok(self.task.await?)
    }
}
