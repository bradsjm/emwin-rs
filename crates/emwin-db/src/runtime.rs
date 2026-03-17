use crate::error::{PersistError, PersistResult};
use crate::writer::{BlobEntry, BlobWriter, BoxFuture, StoredBlob};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tracing::{info, warn};

const DEFAULT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_RETRY_MAX_DELAY: Duration = Duration::from_secs(60);
const DEFAULT_FAILURE_LOG_COOLDOWN: Duration = Duration::from_secs(30);

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
    fn persist<'a>(&'a self, request: PersistedRequest<M>) -> BoxFuture<'a, PersistResult<()>>;

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
    fn persist<'a>(&'a self, _request: PersistedRequest<M>) -> BoxFuture<'a, PersistResult<()>> {
        Box::pin(async { Ok(()) })
    }

    fn backend_name(&self) -> &'static str {
        "noop"
    }
}

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

#[derive(Debug)]
struct SharedQueue<M> {
    state: Mutex<QueueState<M>>,
    available: Semaphore,
    capacity: usize,
}

#[derive(Debug)]
struct QueueState<M> {
    pending: VecDeque<PersistRequest<M>>,
    closed: bool,
    stats: PersistenceStats,
}

/// Cloneable producer used by ingest code to enqueue background persistence work.
#[derive(Debug)]
pub struct PersistenceProducer<M> {
    shared: Arc<SharedQueue<M>>,
}

impl<M> Clone for PersistenceProducer<M> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<M> PersistenceProducer<M> {
    /// Attempts to enqueue a request without blocking the caller on backend I/O.
    pub fn enqueue(&self, request: PersistRequest<M>) -> EnqueueResult {
        let mut guard = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.closed {
            return EnqueueResult {
                accepted: false,
                evicted_oldest_key: None,
                queue_len: guard.pending.len(),
            };
        }

        let evicted_oldest_key = if guard.pending.len() == self.shared.capacity {
            guard.stats.evicted_total = guard.stats.evicted_total.saturating_add(1);
            guard.pending.pop_front().map(|item| item.request_key)
        } else {
            guard.stats.enqueued_total = guard.stats.enqueued_total.saturating_add(1);
            self.shared.available.add_permits(1);
            None
        };

        guard.pending.push_back(request);
        if evicted_oldest_key.is_some() {
            guard.stats.enqueued_total = guard.stats.enqueued_total.saturating_add(1);
        }

        EnqueueResult {
            accepted: true,
            evicted_oldest_key,
            queue_len: guard.pending.len(),
        }
    }

    /// Returns a point-in-time snapshot of queue depth and cumulative outcomes.
    pub fn stats_snapshot(&self) -> PersistenceStats {
        let guard = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        PersistenceStats {
            queue_len: guard.pending.len(),
            queue_capacity: self.shared.capacity,
            ..guard.stats
        }
    }

    fn close(&self) -> usize {
        let mut guard = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.closed {
            return 0;
        }
        guard.closed = true;
        let dropped = guard.pending.len();
        guard.pending.clear();
        self.shared.available.add_permits(1);
        dropped
    }
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
        let shared = Arc::new(SharedQueue {
            state: Mutex::new(QueueState {
                pending: VecDeque::with_capacity(config.queue_capacity),
                closed: false,
                stats: PersistenceStats::default(),
            }),
            available: Semaphore::new(0),
            capacity: config.queue_capacity.max(1),
        });
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

async fn run_worker<M, W, S>(
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
                let mut guard = producer
                    .shared
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.stats.persisted_total = guard.stats.persisted_total.saturating_add(1);
                info!(request_key = %request_key, "save complete");
            }
            Err((request_key, context, err)) => {
                let mut guard = producer
                    .shared
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    let mut guard = producer
        .shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.pending.pop_front()
}

fn is_closed<M>(producer: &PersistenceProducer<M>) -> bool {
    let guard = producer
        .shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    let guard = producer
        .shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.closed
}

fn retry_delay(config: &PersistenceConfig, attempt: u32) -> Duration {
    let multiplier = 1u64.checked_shl(attempt.min(6)).unwrap_or(64);
    config
        .retry_initial_delay
        .saturating_mul(u32::try_from(multiplier).unwrap_or(u32::MAX))
        .min(config.retry_max_delay)
}

#[derive(Debug, Default)]
struct BackendHealth {
    degraded: Option<DegradedBackend>,
}

#[derive(Debug)]
struct DegradedBackend {
    stage: &'static str,
    backend: &'static str,
    target: String,
    failure_class: &'static str,
    last_error: String,
    last_logged_at: Instant,
    suppressed_failures: u64,
}

#[derive(Debug, Clone)]
struct FailureContext {
    stage: &'static str,
    backend: &'static str,
    target: String,
}

impl BackendHealth {
    fn note_retryable_failure(
        &mut self,
        request_key: &str,
        context: &FailureContext,
        err: &PersistError,
        retry_delay: Duration,
        attempt: u32,
        failure_log_cooldown: Duration,
    ) {
        let failure_class = err.failure_class();
        let error_text = err.to_string();
        let now = Instant::now();
        match self.degraded.as_mut() {
            Some(current)
                if current.failure_class == failure_class
                    && current.stage == context.stage
                    && current.backend == context.backend
                    && current.target == context.target
                    && current.last_error == error_text
                    && now.duration_since(current.last_logged_at) < failure_log_cooldown =>
            {
                current.suppressed_failures = current.suppressed_failures.saturating_add(1);
            }
            Some(current) => {
                warn!(
                    request_key = %request_key,
                    stage = context.stage,
                    backend = context.backend,
                    target = %context.target,
                    failure_class,
                    error = %err,
                    retry_delay_secs = retry_delay.as_secs(),
                    retry_attempt = attempt,
                    suppressed_failures = current.suppressed_failures,
                    "persistence backend unavailable; retrying"
                );
                *current = DegradedBackend {
                    stage: context.stage,
                    backend: context.backend,
                    target: context.target.clone(),
                    failure_class,
                    last_error: error_text,
                    last_logged_at: now,
                    suppressed_failures: 0,
                };
            }
            None => {
                warn!(
                    request_key = %request_key,
                    stage = context.stage,
                    backend = context.backend,
                    target = %context.target,
                    failure_class,
                    error = %err,
                    retry_delay_secs = retry_delay.as_secs(),
                    retry_attempt = attempt,
                    "persistence backend unavailable; retrying"
                );
                self.degraded = Some(DegradedBackend {
                    stage: context.stage,
                    backend: context.backend,
                    target: context.target.clone(),
                    failure_class,
                    last_error: error_text,
                    last_logged_at: now,
                    suppressed_failures: 0,
                });
            }
        }
    }

    fn note_recovered(&mut self, request_key: &str) {
        let Some(degraded) = self.degraded.take() else {
            return;
        };
        info!(
            request_key = %request_key,
            stage = degraded.stage,
            backend = degraded.backend,
            target = %degraded.target,
            failure_class = degraded.failure_class,
            suppressed_failures = degraded.suppressed_failures,
            "persistence backend recovered"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BlobEntry, EnqueueResult, MetadataSink, NoopMetadataSink, PersistRequest, PersistedRequest,
        PersistenceConfig, PersistenceProducer, PersistenceRuntime, PersistenceStats, QueueState,
        SharedQueue,
    };
    use crate::error::{PersistError, PersistResult};
    use crate::writer::{BlobRole, BlobStorageKind, BlobWriter, BoxFuture, FilesystemBlobWriter};
    use std::collections::VecDeque;
    use std::io::ErrorKind;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio::sync::Semaphore;
    use tokio::sync::oneshot;
    use tokio::time::Duration;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Debug, Default)]
    struct RecordingWriter {
        deletes: Arc<Mutex<Vec<String>>>,
        writes: Arc<Mutex<Vec<String>>>,
    }

    impl BlobWriter for RecordingWriter {
        fn write<'a>(
            &'a self,
            entry: &'a BlobEntry,
        ) -> BoxFuture<'a, PersistResult<crate::writer::StoredBlob>> {
            let writes = Arc::clone(&self.writes);
            Box::pin(async move {
                writes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(entry.relative_path.clone());
                Ok(crate::writer::StoredBlob {
                    kind: BlobStorageKind::Filesystem,
                    role: entry.role,
                    location: entry.relative_path.clone(),
                    size_bytes: entry.bytes.len(),
                    content_type: entry.content_type.clone(),
                })
            })
        }

        fn delete<'a>(
            &'a self,
            blob: &'a crate::writer::StoredBlob,
        ) -> BoxFuture<'a, PersistResult<()>> {
            let deletes = Arc::clone(&self.deletes);
            Box::pin(async move {
                deletes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(blob.location.clone());
                Ok(())
            })
        }
    }

    #[derive(Debug, Default)]
    struct RecordingSink {
        persisted: Arc<Mutex<Vec<String>>>,
    }

    impl MetadataSink<String> for RecordingSink {
        fn persist<'a>(
            &'a self,
            request: PersistedRequest<String>,
        ) -> BoxFuture<'a, PersistResult<()>> {
            let persisted = Arc::clone(&self.persisted);
            Box::pin(async move {
                persisted
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(request.request_key);
                Ok(())
            })
        }
    }

    #[derive(Debug, Default)]
    struct FailingWriter;

    impl BlobWriter for FailingWriter {
        fn write<'a>(
            &'a self,
            _entry: &'a BlobEntry,
        ) -> BoxFuture<'a, PersistResult<crate::writer::StoredBlob>> {
            Box::pin(async { Err(PersistError::Io(std::io::Error::other("boom"))) })
        }

        fn delete<'a>(
            &'a self,
            _blob: &'a crate::writer::StoredBlob,
        ) -> BoxFuture<'a, PersistResult<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Debug, Default)]
    struct FailingSink;

    impl MetadataSink<String> for FailingSink {
        fn persist<'a>(
            &'a self,
            _request: PersistedRequest<String>,
        ) -> BoxFuture<'a, PersistResult<()>> {
            Box::pin(async { Err(PersistError::InvalidRequest("sink failed".to_string())) })
        }
    }

    #[derive(Debug)]
    struct TransientWriter {
        attempts: Arc<Mutex<u32>>,
        notify_success: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    }

    impl TransientWriter {
        fn new(notify_success: oneshot::Sender<()>) -> Self {
            Self {
                attempts: Arc::new(Mutex::new(0)),
                notify_success: Arc::new(Mutex::new(Some(notify_success))),
            }
        }
    }

    impl BlobWriter for TransientWriter {
        fn write<'a>(
            &'a self,
            entry: &'a BlobEntry,
        ) -> BoxFuture<'a, PersistResult<crate::writer::StoredBlob>> {
            let attempts = Arc::clone(&self.attempts);
            let notify_success = Arc::clone(&self.notify_success);
            Box::pin(async move {
                let mut count = attempts
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *count += 1;
                if *count == 1 {
                    return Err(PersistError::Io(std::io::Error::from(
                        ErrorKind::StorageFull,
                    )));
                }
                drop(count);

                if let Some(sender) = notify_success
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    let _ = sender.send(());
                }

                Ok(crate::writer::StoredBlob {
                    kind: BlobStorageKind::Filesystem,
                    role: entry.role,
                    location: entry.relative_path.clone(),
                    size_bytes: entry.bytes.len(),
                    content_type: entry.content_type.clone(),
                })
            })
        }

        fn delete<'a>(
            &'a self,
            _blob: &'a crate::writer::StoredBlob,
        ) -> BoxFuture<'a, PersistResult<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Debug)]
    struct FlakySink {
        attempts: Arc<Mutex<u32>>,
        persisted: Arc<Mutex<Vec<String>>>,
        notify_success: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    }

    impl FlakySink {
        fn new(notify_success: oneshot::Sender<()>) -> Self {
            Self {
                attempts: Arc::new(Mutex::new(0)),
                persisted: Arc::new(Mutex::new(Vec::new())),
                notify_success: Arc::new(Mutex::new(Some(notify_success))),
            }
        }
    }

    impl MetadataSink<String> for FlakySink {
        fn persist<'a>(
            &'a self,
            request: PersistedRequest<String>,
        ) -> BoxFuture<'a, PersistResult<()>> {
            let attempts = Arc::clone(&self.attempts);
            let persisted = Arc::clone(&self.persisted);
            let notify_success = Arc::clone(&self.notify_success);
            Box::pin(async move {
                let mut count = attempts
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *count += 1;
                if *count == 1 {
                    return Err(PersistError::Sqlx(sqlx::Error::PoolTimedOut));
                }
                drop(count);

                persisted
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(request.request_key);
                if let Some(sender) = notify_success
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    let _ = sender.send(());
                }
                Ok(())
            })
        }
    }

    #[derive(Debug)]
    struct BlockingWriter {
        writes: Arc<Mutex<Vec<String>>>,
        started: Arc<Mutex<Option<oneshot::Sender<()>>>>,
        release: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
    }

    impl BlockingWriter {
        fn new(started: oneshot::Sender<()>, release: oneshot::Receiver<()>) -> Self {
            Self {
                writes: Arc::new(Mutex::new(Vec::new())),
                started: Arc::new(Mutex::new(Some(started))),
                release: Arc::new(Mutex::new(Some(release))),
            }
        }
    }

    impl BlobWriter for BlockingWriter {
        fn write<'a>(
            &'a self,
            entry: &'a BlobEntry,
        ) -> BoxFuture<'a, PersistResult<crate::writer::StoredBlob>> {
            let writes = Arc::clone(&self.writes);
            let started = Arc::clone(&self.started);
            let release = Arc::clone(&self.release);
            Box::pin(async move {
                writes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(entry.relative_path.clone());

                if let Some(sender) = started
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    let _ = sender.send(());
                }

                let receiver = release
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                if let Some(receiver) = receiver {
                    let _ = receiver.await;
                }

                Ok(crate::writer::StoredBlob {
                    kind: BlobStorageKind::Filesystem,
                    role: entry.role,
                    location: entry.relative_path.clone(),
                    size_bytes: entry.bytes.len(),
                    content_type: entry.content_type.clone(),
                })
            })
        }

        fn delete<'a>(
            &'a self,
            _blob: &'a crate::writer::StoredBlob,
        ) -> BoxFuture<'a, PersistResult<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Debug, Default)]
    struct S3FailingWriter;

    impl BlobWriter for S3FailingWriter {
        fn write<'a>(
            &'a self,
            _entry: &'a BlobEntry,
        ) -> BoxFuture<'a, PersistResult<crate::writer::StoredBlob>> {
            Box::pin(async {
                Err(PersistError::s3_response(
                    "put_object",
                    503,
                    "service unavailable",
                ))
            })
        }

        fn delete<'a>(
            &'a self,
            _blob: &'a crate::writer::StoredBlob,
        ) -> BoxFuture<'a, PersistResult<()>> {
            Box::pin(async { Ok(()) })
        }

        fn backend_name(&self) -> &'static str {
            "s3"
        }

        fn target_description(&self) -> String {
            "s3://example-bucket/archive".to_string()
        }
    }

    #[derive(Debug, Default)]
    struct DatabaseFailingSink;

    impl MetadataSink<String> for DatabaseFailingSink {
        fn persist<'a>(
            &'a self,
            _request: PersistedRequest<String>,
        ) -> BoxFuture<'a, PersistResult<()>> {
            Box::pin(async { Err(PersistError::InvalidRequest("sink failed".to_string())) })
        }

        fn backend_name(&self) -> &'static str {
            "database"
        }

        fn target_description(&self) -> Option<String> {
            Some("127.0.0.1/emwin".to_string())
        }
    }

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

        fn make_writer(&'a self) -> Self::Writer {
            LogWriter(self.clone())
        }
    }

    fn request(name: &str) -> PersistRequest<String> {
        PersistRequest {
            request_key: name.to_string(),
            metadata: name.to_string(),
            blobs: vec![BlobEntry::new(
                BlobRole::Payload,
                name,
                name.as_bytes().to_vec(),
                Some("text/plain"),
            )],
        }
    }

    async fn wait_for_stats<M, F>(runtime: &PersistenceRuntime<M>, predicate: F)
    where
        M: Clone + Send + 'static,
        F: Fn(&PersistenceStats) -> bool,
    {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let stats = runtime.stats_snapshot();
                if predicate(&stats) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime should reach expected stats");
    }

    #[test]
    fn stats_snapshot_reports_live_queue_state() {
        let producer = PersistenceProducer {
            shared: Arc::new(SharedQueue {
                state: Mutex::new(QueueState {
                    pending: VecDeque::from([request("queued")]),
                    closed: false,
                    stats: PersistenceStats {
                        queue_len: 0,
                        queue_capacity: 0,
                        enqueued_total: 4,
                        evicted_total: 1,
                        persisted_total: 2,
                        failed_total: 1,
                    },
                }),
                available: Semaphore::new(0),
                capacity: 8,
            }),
        };

        assert_eq!(
            producer.stats_snapshot(),
            PersistenceStats {
                queue_len: 1,
                queue_capacity: 8,
                enqueued_total: 4,
                evicted_total: 1,
                persisted_total: 2,
                failed_total: 1,
            }
        );
    }

    #[tokio::test]
    async fn queue_evicts_oldest_request_when_full() {
        let writer = RecordingWriter::default();
        let writes = Arc::clone(&writer.writes);
        let sink = RecordingSink::default();
        let persisted = Arc::clone(&sink.persisted);
        let runtime = PersistenceRuntime::spawn(PersistenceConfig::new(2), writer, sink);
        let producer = runtime.producer();

        assert_eq!(
            producer.enqueue(request("one")),
            EnqueueResult {
                accepted: true,
                evicted_oldest_key: None,
                queue_len: 1,
            }
        );
        assert_eq!(
            producer.enqueue(request("two")),
            EnqueueResult {
                accepted: true,
                evicted_oldest_key: None,
                queue_len: 2,
            }
        );
        let result = producer.enqueue(request("three"));
        assert_eq!(result.evicted_oldest_key.as_deref(), Some("one"));

        wait_for_stats(&runtime, |stats| stats.persisted_total == 2).await;

        let stats = runtime.shutdown().await.expect("shutdown should succeed");
        assert_eq!(stats.queue_len, 0);
        assert_eq!(stats.queue_capacity, 2);
        assert_eq!(stats.evicted_total, 1);
        assert_eq!(stats.persisted_total, 2);
        assert_eq!(
            writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &["two", "three"]
        );
        assert_eq!(
            persisted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &["two", "three"]
        );
    }

    #[tokio::test]
    async fn writer_failure_does_not_persist_metadata() {
        let sink = RecordingSink::default();
        let persisted = Arc::clone(&sink.persisted);
        let runtime = PersistenceRuntime::spawn(PersistenceConfig::new(4), FailingWriter, sink);
        let producer = runtime.producer();

        let result = producer.enqueue(request("broken"));
        assert!(result.accepted);

        wait_for_stats(&runtime, |stats| stats.failed_total == 1).await;

        let stats = runtime.shutdown().await.expect("shutdown should succeed");
        assert_eq!(stats.queue_len, 0);
        assert_eq!(stats.failed_total, 1);
        assert!(
            persisted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[tokio::test]
    async fn sink_failure_keeps_written_blobs() {
        let writer = RecordingWriter::default();
        let deletes = Arc::clone(&writer.deletes);
        let runtime = PersistenceRuntime::spawn(PersistenceConfig::new(4), writer, FailingSink);
        let producer = runtime.producer();

        let result = producer.enqueue(request("broken"));
        assert!(result.accepted);

        wait_for_stats(&runtime, |stats| stats.failed_total == 1).await;

        let stats = runtime.shutdown().await.expect("shutdown should succeed");
        assert_eq!(stats.queue_len, 0);
        assert_eq!(stats.failed_total, 1);
        assert!(
            deletes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[tokio::test]
    async fn filesystem_writer_keeps_blobs_when_sink_fails() {
        let temp = tempdir().expect("tempdir should succeed");
        let runtime = PersistenceRuntime::spawn(
            PersistenceConfig::new(4),
            FilesystemBlobWriter::new(temp.path().to_path_buf()),
            FailingSink,
        );
        let producer = runtime.producer();

        let result = producer.enqueue(PersistRequest {
            request_key: "product".to_string(),
            metadata: "product".to_string(),
            blobs: vec![
                BlobEntry::new(
                    BlobRole::Payload,
                    "nested/product.txt",
                    b"payload".to_vec(),
                    Some("text/plain"),
                ),
                BlobEntry::new(
                    BlobRole::MetadataSidecar,
                    "nested/product.JSON",
                    br#"{"ok":true}"#.to_vec(),
                    Some("application/json"),
                ),
            ],
        });
        assert!(result.accepted);

        wait_for_stats(&runtime, |stats| stats.failed_total == 1).await;

        let stats = runtime.shutdown().await.expect("shutdown should succeed");
        assert_eq!(stats.queue_len, 0);
        assert_eq!(stats.failed_total, 1);
        assert_eq!(
            std::fs::read_to_string(temp.path().join("nested/product.txt"))
                .expect("payload should exist"),
            "payload"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("nested/product.JSON"))
                .expect("metadata should exist"),
            "{\"ok\":true}"
        );
    }

    #[tokio::test]
    async fn filesystem_writer_persists_blobs() {
        let temp = tempdir().expect("tempdir should succeed");
        let runtime = PersistenceRuntime::spawn(
            PersistenceConfig::new(4),
            FilesystemBlobWriter::new(temp.path().to_path_buf()),
            NoopMetadataSink,
        );
        let producer = runtime.producer();

        let result = producer.enqueue(PersistRequest {
            request_key: "product".to_string(),
            metadata: (),
            blobs: vec![
                BlobEntry::new(
                    BlobRole::Payload,
                    "nested/product.txt",
                    b"payload".to_vec(),
                    Some("text/plain"),
                ),
                BlobEntry::new(
                    BlobRole::MetadataSidecar,
                    "nested/product.JSON",
                    br#"{"ok":true}"#.to_vec(),
                    Some("application/json"),
                ),
            ],
        });
        assert!(result.accepted);

        wait_for_stats(&runtime, |stats| stats.persisted_total == 1).await;

        let stats = runtime.shutdown().await.expect("shutdown should succeed");
        assert_eq!(stats.queue_len, 0);
        assert_eq!(stats.persisted_total, 1);
        assert_eq!(
            std::fs::read_to_string(temp.path().join("nested/product.txt"))
                .expect("payload should exist"),
            "payload"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("nested/product.JSON"))
                .expect("metadata should exist"),
            "{\"ok\":true}"
        );
    }

    #[tokio::test]
    async fn retryable_writer_failure_recovers_without_dropping_request() {
        let (success_tx, success_rx) = oneshot::channel();
        let writer = TransientWriter::new(success_tx);
        let attempts = Arc::clone(&writer.attempts);
        let sink = RecordingSink::default();
        let persisted = Arc::clone(&sink.persisted);
        let config = PersistenceConfig::new(4)
            .with_retry_delays(Duration::from_millis(5), Duration::from_millis(5))
            .with_failure_log_cooldown(Duration::from_millis(1));
        let runtime = PersistenceRuntime::spawn(config, writer, sink);
        let producer = runtime.producer();

        assert!(producer.enqueue(request("retry-writer")).accepted);
        success_rx.await.expect("writer should recover");

        let stats = runtime.shutdown().await.expect("shutdown should succeed");
        assert_eq!(stats.failed_total, 0);
        assert_eq!(stats.persisted_total, 1);
        assert_eq!(
            *attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            2
        );
        assert_eq!(
            persisted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &["retry-writer"]
        );
    }

    #[tokio::test]
    async fn retryable_sink_failure_reuses_written_blobs_until_recovered() {
        let writer = RecordingWriter::default();
        let writes = Arc::clone(&writer.writes);
        let (success_tx, success_rx) = oneshot::channel();
        let sink = FlakySink::new(success_tx);
        let persisted = Arc::clone(&sink.persisted);
        let attempts = Arc::clone(&sink.attempts);
        let config = PersistenceConfig::new(4)
            .with_retry_delays(Duration::from_millis(5), Duration::from_millis(5))
            .with_failure_log_cooldown(Duration::from_millis(1));
        let runtime = PersistenceRuntime::spawn(config, writer, sink);
        let producer = runtime.producer();

        assert!(producer.enqueue(request("retry-sink")).accepted);
        success_rx.await.expect("sink should recover");

        let stats = runtime.shutdown().await.expect("shutdown should succeed");
        assert_eq!(stats.failed_total, 0);
        assert_eq!(stats.persisted_total, 1);
        assert_eq!(
            *attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            2
        );
        assert_eq!(
            writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &["retry-sink"]
        );
        assert_eq!(
            persisted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &["retry-sink"]
        );
    }

    #[tokio::test]
    async fn shutdown_drops_queued_requests() {
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let writer = BlockingWriter::new(started_tx, release_rx);
        let writes = Arc::clone(&writer.writes);
        let sink = RecordingSink::default();
        let persisted = Arc::clone(&sink.persisted);
        let runtime = PersistenceRuntime::spawn(PersistenceConfig::new(4), writer, sink);
        let producer = runtime.producer();

        assert!(producer.enqueue(request("one")).accepted);
        started_rx.await.expect("first write should start");
        assert!(producer.enqueue(request("two")).accepted);
        assert!(producer.enqueue(request("three")).accepted);

        let shutdown_task = tokio::spawn(async move { runtime.shutdown().await });
        let _ = release_tx.send(());

        let stats = shutdown_task
            .await
            .expect("shutdown task should join")
            .expect("shutdown should succeed");
        assert_eq!(stats.queue_len, 0);
        assert_eq!(stats.persisted_total, 1);
        assert_eq!(stats.failed_total, 0);
        assert_eq!(
            writes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &["one"]
        );
        assert_eq!(
            persisted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &["one"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retry_logs_include_s3_backend_context() {
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(buffer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let config = PersistenceConfig::new(4)
            .with_retry_delays(Duration::from_millis(1), Duration::from_millis(1))
            .with_failure_log_cooldown(Duration::from_millis(1));
        let runtime = PersistenceRuntime::spawn(config, S3FailingWriter, NoopMetadataSink);
        let producer = runtime.producer();

        assert!(producer.enqueue(request("broken-s3")).accepted);
        tokio::time::sleep(Duration::from_millis(20)).await;

        let _ = runtime.shutdown().await.expect("shutdown should succeed");
        let logs = buffer.contents();
        assert!(logs.contains("persistence backend unavailable; retrying"));
        assert!(logs.contains("blob_write"));
        assert!(logs.contains("s3"));
        assert!(logs.contains("target=s3://example-bucket/archive"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn final_failure_logs_include_database_context() {
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(buffer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let runtime = PersistenceRuntime::spawn(
            PersistenceConfig::new(4),
            RecordingWriter::default(),
            DatabaseFailingSink,
        );
        let producer = runtime.producer();

        assert!(producer.enqueue(request("broken-db")).accepted);
        wait_for_stats(&runtime, |stats| stats.failed_total == 1).await;

        let _ = runtime.shutdown().await.expect("shutdown should succeed");
        let logs = buffer.contents();
        assert!(logs.contains("persistence request failed"));
        assert!(logs.contains("metadata_persist"));
        assert!(logs.contains("database"));
        assert!(logs.contains("target=127.0.0.1/emwin"));
    }
}
