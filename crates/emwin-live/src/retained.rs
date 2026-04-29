#[cfg(any(test, feature = "test-support"))]
use crate::file_pipeline::build_completed_file_metadata;
use bytes::Bytes;
#[cfg(any(test, feature = "test-support"))]
use emwin_protocol::ingest::ProductOrigin;
use emwin_service::{CompletedFileMetadata, RetainedFile};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
struct StoredRetainedFile {
    generation: u64,
    file: RetainedFile,
}

/// Bounded in-memory store for completed files.
#[derive(Debug)]
pub(crate) struct RetainedFiles {
    by_name: HashMap<String, StoredRetainedFile>,
    order: VecDeque<(String, u64)>,
    next_generation: u64,
    max_entries: usize,
    ttl: Duration,
}

impl RetainedFiles {
    pub(crate) fn new(max_entries: usize, ttl: Duration) -> Self {
        Self {
            by_name: HashMap::new(),
            order: VecDeque::new(),
            next_generation: 1,
            max_entries: max_entries.max(1),
            ttl: ttl.max(Duration::from_secs(1)),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn insert(
        &mut self,
        filename: String,
        data: Bytes,
        timestamp_utc: u64,
        origin: ProductOrigin,
        completed_at: SystemTime,
    ) -> CompletedFileMetadata {
        let metadata = build_completed_file_metadata(&filename, timestamp_utc, origin, &data);
        self.insert_processed(filename, data, metadata, completed_at)
    }

    pub(crate) fn insert_processed(
        &mut self,
        filename: String,
        data: Bytes,
        metadata: CompletedFileMetadata,
        completed_at: SystemTime,
    ) -> CompletedFileMetadata {
        self.evict_expired();

        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);

        self.order.push_back((filename.clone(), generation));
        self.by_name.insert(
            filename,
            StoredRetainedFile {
                generation,
                file: RetainedFile {
                    data,
                    completed_at,
                    metadata: metadata.clone(),
                },
            },
        );

        while self.by_name.len() > self.max_entries {
            if let Some((oldest, generation)) = self.order.pop_front() {
                if self
                    .by_name
                    .get(&oldest)
                    .is_some_and(|stored| stored.generation == generation)
                {
                    self.by_name.remove(&oldest);
                }
            } else {
                break;
            }
        }
        self.compact_stale_order_entries();

        metadata
    }

    pub(crate) fn list(&mut self) -> Vec<CompletedFileMetadata> {
        self.evict_expired();
        self.order
            .iter()
            .rev()
            .filter_map(|(name, generation)| {
                self.by_name
                    .get(name)
                    .filter(|stored| stored.generation == *generation)
            })
            .map(|stored| stored.file.metadata.clone())
            .collect()
    }

    pub(crate) fn get(&mut self, filename: &str) -> Option<RetainedFile> {
        self.evict_expired();
        self.by_name.get(filename).map(|stored| stored.file.clone())
    }

    pub(crate) fn len(&mut self) -> usize {
        self.evict_expired();
        self.by_name.len()
    }

    fn evict_expired(&mut self) {
        let now = SystemTime::now();
        self.order.retain(|(name, generation)| {
            let Some(stored) = self.by_name.get(name) else {
                return false;
            };
            if stored.generation != *generation {
                return false;
            }
            let age = now
                .duration_since(stored.file.completed_at)
                .unwrap_or(Duration::from_secs(0));
            if age > self.ttl {
                self.by_name.remove(name);
                return false;
            }
            true
        });
    }

    fn compact_stale_order_entries(&mut self) {
        if self.order.len() <= self.max_entries.saturating_mul(4).max(16) {
            return;
        }
        self.order.retain(|(name, generation)| {
            self.by_name
                .get(name)
                .is_some_and(|stored| stored.generation == *generation)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::RetainedFiles;
    use bytes::Bytes;
    use emwin_protocol::ingest::ProductOrigin;
    use std::time::{Duration, SystemTime};

    #[test]
    fn retained_files_evict_by_capacity_and_ttl() {
        let mut files = RetainedFiles::new(2, Duration::from_secs(1));
        let now = SystemTime::now();

        files.insert(
            "one.txt".to_string(),
            Bytes::from_static(b"one"),
            1,
            ProductOrigin::Qbt,
            now,
        );
        files.insert(
            "two.txt".to_string(),
            Bytes::from_static(b"two"),
            2,
            ProductOrigin::Qbt,
            now,
        );
        files.insert(
            "three.txt".to_string(),
            Bytes::from_static(b"three"),
            3,
            ProductOrigin::Qbt,
            now,
        );

        let listed = files.list();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].filename, "three.txt");
        assert_eq!(listed[1].filename, "two.txt");

        let mut ttl_files = RetainedFiles::new(2, Duration::from_secs(1));
        ttl_files.insert(
            "expired.txt".to_string(),
            Bytes::from_static(b"expired"),
            4,
            ProductOrigin::Qbt,
            now - Duration::from_secs(2),
        );
        ttl_files.insert(
            "fresh.txt".to_string(),
            Bytes::from_static(b"fresh"),
            5,
            ProductOrigin::Qbt,
            now,
        );
        let listed = ttl_files.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "fresh.txt");
    }

    #[test]
    fn retained_files_replaces_duplicate_without_duplicate_listing() {
        let mut files = RetainedFiles::new(4, Duration::from_secs(60));
        let now = SystemTime::now();

        files.insert(
            "same.txt".to_string(),
            Bytes::from_static(b"old"),
            1,
            ProductOrigin::Qbt,
            now,
        );
        files.insert(
            "same.txt".to_string(),
            Bytes::from_static(b"new"),
            2,
            ProductOrigin::Qbt,
            now,
        );

        let listed = files.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "same.txt");
        assert_eq!(
            files.get("same.txt").expect("file should exist").data,
            Bytes::from_static(b"new")
        );
    }
}
