use super::{EnqueueResult, PersistRequest, PersistenceProducer, PersistenceStats};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

#[derive(Debug)]
pub(super) struct SharedQueue<M> {
    pub(super) state: Mutex<QueueState<M>>,
    pub(super) available: Semaphore,
    pub(super) capacity: usize,
}

#[derive(Debug)]
pub(super) struct QueueState<M> {
    pub(super) pending: VecDeque<PersistRequest<M>>,
    pub(super) closed: bool,
    pub(super) stats: PersistenceStats,
}

impl<M> SharedQueue<M> {
    pub(super) fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(QueueState {
                pending: VecDeque::with_capacity(capacity.max(1)),
                closed: false,
                stats: PersistenceStats::default(),
            }),
            available: Semaphore::new(0),
            capacity: capacity.max(1),
        })
    }
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

    pub(super) fn close(&self) -> usize {
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
