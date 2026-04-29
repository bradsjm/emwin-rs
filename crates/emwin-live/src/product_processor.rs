//! Bounded product processing between receiver ingest and live publication.
//!
//! Receiver tasks enqueue completed products here without doing archive decompression, metadata
//! projection, JSON sidecar generation, CRC calculation, or retained-cache updates on the ingest
//! event loop. When the bounded queue is full, the oldest queued product is evicted so newer live
//! data remains fresh.

use crate::archive_postprocess::post_process_archive;
use crate::error::{LiveError, LiveResult};
use crate::events::publish;
use crate::file_pipeline::{build_completed_file_metadata, build_persist_request};
use crate::persistence::FilePersistenceProducer;
use crate::shared::lock_unpoisoned;
use crate::types::{AppState, LiveEventKind};
use bytes::Bytes;
use emwin_db::PersistRequest;
use emwin_protocol::ingest::ProductOrigin;
use emwin_service::{CompletedFileMetadata, ProcessingStats};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;

#[derive(Debug)]
pub(crate) struct ProductProcessorRuntime {
    producer: ProductProcessorProducer,
    task: JoinHandle<LiveResult<()>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProductProcessorProducer {
    shared: Arc<SharedProductQueue>,
}

#[derive(Debug)]
struct SharedProductQueue {
    state: Mutex<ProductQueueState>,
    available: Semaphore,
    capacity: usize,
    enqueued_total: AtomicU64,
    evicted_total: AtomicU64,
    completed_total: AtomicU64,
    failed_total: AtomicU64,
}

#[derive(Debug)]
struct ProductQueueState {
    pending: VecDeque<ProductWorkItem>,
    closed: bool,
}

#[derive(Debug)]
pub(crate) struct ProductWorkItem {
    filename: String,
    data: Bytes,
    source_timestamp_utc: SystemTime,
    origin: ProductOrigin,
}

#[derive(Debug)]
struct ProcessedProduct {
    filename: String,
    data: Bytes,
    metadata: CompletedFileMetadata,
    persist_request: Option<PersistRequest<CompletedFileMetadata>>,
}

#[derive(Debug)]
pub(crate) struct ProductEnqueueResult {
    pub(crate) accepted: bool,
    pub(crate) evicted_oldest_filename: Option<String>,
    pub(crate) queue_len: usize,
}

impl ProductProcessorRuntime {
    pub(crate) fn spawn(
        producer: ProductProcessorProducer,
        state: Arc<AppState>,
        post_process_archives: bool,
        persistence: Option<FilePersistenceProducer>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        let task_producer = producer.clone();
        let task = tokio::spawn(async move {
            run_product_processor(
                task_producer,
                state,
                post_process_archives,
                persistence,
                shutdown_rx,
            )
            .await
        });
        Self { producer, task }
    }

    pub(crate) async fn shutdown(self) -> LiveResult<()> {
        self.producer.close();
        self.task.await.map_err(|err| {
            LiveError::runtime(format!("product processor task join failed: {err}"))
        })??;
        Ok(())
    }
}

impl ProductProcessorProducer {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            shared: Arc::new(SharedProductQueue {
                state: Mutex::new(ProductQueueState {
                    pending: VecDeque::with_capacity(capacity.max(1)),
                    closed: false,
                }),
                available: Semaphore::new(0),
                capacity: capacity.max(1),
                enqueued_total: AtomicU64::new(0),
                evicted_total: AtomicU64::new(0),
                completed_total: AtomicU64::new(0),
                failed_total: AtomicU64::new(0),
            }),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn stopped_for_tests(capacity: usize) -> Self {
        let producer = Self::new(capacity);
        producer.close();
        producer
    }

    pub(crate) fn enqueue(&self, item: ProductWorkItem) -> ProductEnqueueResult {
        let mut guard = lock_unpoisoned(&self.shared.state);
        if guard.closed {
            return ProductEnqueueResult {
                accepted: false,
                evicted_oldest_filename: None,
                queue_len: guard.pending.len(),
            };
        }

        let evicted_oldest_filename = if guard.pending.len() == self.shared.capacity {
            self.shared.evicted_total.fetch_add(1, Ordering::Relaxed);
            guard.pending.pop_front().map(|item| item.filename)
        } else {
            self.shared.available.add_permits(1);
            None
        };

        self.shared.enqueued_total.fetch_add(1, Ordering::Relaxed);
        guard.pending.push_back(item);

        ProductEnqueueResult {
            accepted: true,
            evicted_oldest_filename,
            queue_len: guard.pending.len(),
        }
    }

    pub(crate) fn stats_snapshot(&self) -> ProcessingStats {
        let guard = lock_unpoisoned(&self.shared.state);
        ProcessingStats {
            queue_len: guard.pending.len(),
            queue_capacity: self.shared.capacity,
            enqueued_total: self.shared.enqueued_total.load(Ordering::Relaxed),
            evicted_total: self.shared.evicted_total.load(Ordering::Relaxed),
            completed_total: self.shared.completed_total.load(Ordering::Relaxed),
            failed_total: self.shared.failed_total.load(Ordering::Relaxed),
        }
    }

    fn close(&self) {
        let mut guard = lock_unpoisoned(&self.shared.state);
        if guard.closed {
            return;
        }
        guard.closed = true;
        self.shared.available.add_permits(1);
    }

    fn pop(&self) -> Option<ProductWorkItem> {
        lock_unpoisoned(&self.shared.state).pending.pop_front()
    }

    fn is_closed(&self) -> bool {
        let guard = lock_unpoisoned(&self.shared.state);
        guard.closed && guard.pending.is_empty()
    }

    fn note_completed(&self) {
        self.shared.completed_total.fetch_add(1, Ordering::Relaxed);
    }

    fn note_failed(&self) {
        self.shared.failed_total.fetch_add(1, Ordering::Relaxed);
    }
}

impl ProductWorkItem {
    pub(crate) fn new(
        filename: String,
        data: Bytes,
        source_timestamp_utc: SystemTime,
        origin: ProductOrigin,
    ) -> Self {
        Self {
            filename,
            data,
            source_timestamp_utc,
            origin,
        }
    }
}

async fn run_product_processor(
    producer: ProductProcessorProducer,
    state: Arc<AppState>,
    post_process_archives: bool,
    persistence: Option<FilePersistenceProducer>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> LiveResult<()> {
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                producer.close();
            }
            permit = producer.shared.available.acquire() => {
                match permit {
                    Ok(permit) => permit.forget(),
                    Err(_) => break,
                }
            }
        }

        let Some(item) = producer.pop() else {
            if producer.is_closed() {
                break;
            }
            continue;
        };

        match process_product(item, post_process_archives, persistence.is_some()).await {
            Ok(processed) => {
                let completed_at = SystemTime::now();
                let retained_meta = {
                    let mut guard = lock_unpoisoned(&state.retained_files);
                    guard.insert_processed(
                        processed.filename.clone(),
                        processed.data.clone(),
                        processed.metadata,
                        completed_at,
                    )
                };
                if let (Some(persistence), Some(request)) =
                    (&persistence, processed.persist_request)
                {
                    let result = persistence.enqueue(request);
                    if let Some(evicted_oldest_key) = result.evicted_oldest_key {
                        tracing::warn!(
                            evicted_request = %evicted_oldest_key,
                            queued_request = %processed.filename,
                            queue_len = result.queue_len,
                            "persistence queue evicted oldest request"
                        );
                    }
                    if !result.accepted {
                        tracing::warn!(filename = %processed.filename, "persistence queue closed");
                    }
                }
                publish(
                    &state,
                    LiveEventKind::ProductAvailable(Box::new(retained_meta)),
                );
                if !state.quiet {
                    tracing::info!(
                        "file complete name={} bytes={}",
                        processed.filename,
                        processed.data.len()
                    );
                }
                producer.note_completed();
            }
            Err(err) => {
                producer.note_failed();
                tracing::warn!(error = %err, "product processing failed");
            }
        }
    }

    Ok(())
}

async fn process_product(
    item: ProductWorkItem,
    post_process_archives: bool,
    build_persistence_request: bool,
) -> LiveResult<ProcessedProduct> {
    tokio::task::spawn_blocking(move || {
        let delivered = post_process_archive(post_process_archives, &item.filename, &item.data)
            .map_err(|err| {
                tracing::warn!(
                    archive_filename = %item.filename,
                    error = %err,
                    "Corrupt Zip File Received"
                );
                LiveError::runtime(err.to_string())
            })?;
        let timestamp_utc = item
            .source_timestamp_utc
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let metadata = build_completed_file_metadata(
            &delivered.filename,
            timestamp_utc,
            item.origin,
            &delivered.data,
        );
        let persist_request = if build_persistence_request {
            Some(build_persist_request(
                &delivered.filename,
                &delivered.data,
                metadata.clone(),
            )?)
        } else {
            None
        };

        Ok(ProcessedProduct {
            filename: delivered.filename,
            data: delivered.data,
            metadata,
            persist_request,
        })
    })
    .await
    .map_err(|err| LiveError::runtime(format!("product processing task failed: {err}")))?
}

#[cfg(test)]
mod tests {
    use super::{ProductProcessorProducer, ProductProcessorRuntime, ProductWorkItem};
    use crate::types::AppState;
    use bytes::Bytes;
    use emwin_protocol::ingest::ProductOrigin;
    use std::sync::Arc;
    use std::time::SystemTime;
    use tokio::sync::watch;

    #[test]
    fn product_queue_evicts_oldest_when_full() {
        let producer = ProductProcessorProducer::new(2);
        assert!(producer.enqueue(item("one")).accepted);
        assert!(producer.enqueue(item("two")).accepted);
        let result = producer.enqueue(item("three"));

        assert_eq!(result.evicted_oldest_filename.as_deref(), Some("one"));
        let stats = producer.stats_snapshot();
        assert_eq!(stats.queue_len, 2);
        assert_eq!(stats.enqueued_total, 3);
        assert_eq!(stats.evicted_total, 1);
    }

    #[tokio::test]
    async fn shutdown_drains_queued_products() {
        let producer = ProductProcessorProducer::new(4);
        let state = AppState::new(None, producer.clone(), None, true, 16, 60);
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let runtime = ProductProcessorRuntime::spawn(
            producer.clone(),
            Arc::clone(&state),
            false,
            None,
            shutdown_rx,
        );

        assert!(producer.enqueue(text_item("one.txt")).accepted);
        assert!(producer.enqueue(text_item("two.txt")).accepted);

        runtime.shutdown().await.expect("shutdown should succeed");

        let stats = producer.stats_snapshot();
        assert_eq!(stats.queue_len, 0);
        assert_eq!(stats.completed_total, 2);
        let retained = state
            .retained_files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .list();
        assert_eq!(retained.len(), 2);
    }

    fn item(filename: &str) -> ProductWorkItem {
        ProductWorkItem::new(
            filename.to_string(),
            Bytes::from_static(b"payload"),
            SystemTime::UNIX_EPOCH,
            ProductOrigin::Qbt,
        )
    }

    fn text_item(filename: &str) -> ProductWorkItem {
        ProductWorkItem::new(
            filename.to_string(),
            Bytes::from_static(b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n"),
            SystemTime::UNIX_EPOCH,
            ProductOrigin::Qbt,
        )
    }
}
