use super::queue::{QueueState, SharedQueue};
use super::{
    BlobEntry, BlobWriter, EnqueueResult, MetadataSink, NoopMetadataSink, PersistRequest,
    PersistedRequest, PersistenceConfig, PersistenceProducer, PersistenceRuntime, PersistenceStats,
};
use crate::error::{PersistError, PersistResult};
use crate::writer::{BoxFuture, FilesystemBlobWriter};
use crate::{BlobRole, BlobStorageKind};
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

    fn make_writer(&'a self) -> LogWriter {
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
