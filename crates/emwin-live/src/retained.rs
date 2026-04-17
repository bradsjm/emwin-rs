use crate::file_pipeline::build_completed_file_metadata;
use emwin_protocol::ingest::ProductOrigin;
use emwin_service::{CompletedFileMetadata, RetainedFile};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, SystemTime};

/// Bounded in-memory store for completed files.
#[derive(Debug)]
pub(crate) struct RetainedFiles {
    by_name: HashMap<String, RetainedFile>,
    order: VecDeque<String>,
    max_entries: usize,
    ttl: Duration,
}

impl RetainedFiles {
    pub(crate) fn new(max_entries: usize, ttl: Duration) -> Self {
        Self {
            by_name: HashMap::new(),
            order: VecDeque::new(),
            max_entries: max_entries.max(1),
            ttl: ttl.max(Duration::from_secs(1)),
        }
    }

    pub(crate) fn insert(
        &mut self,
        filename: String,
        data: Vec<u8>,
        timestamp_utc: u64,
        origin: ProductOrigin,
        completed_at: SystemTime,
    ) -> CompletedFileMetadata {
        self.evict_expired();

        let metadata = build_completed_file_metadata(&filename, timestamp_utc, origin, &data);

        if self.by_name.contains_key(&filename) {
            self.order.retain(|name| name != &filename);
        }
        self.order.push_back(filename.clone());
        self.by_name.insert(
            filename,
            RetainedFile {
                data,
                completed_at,
                metadata: metadata.clone(),
            },
        );

        while self.by_name.len() > self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.by_name.remove(&oldest);
            } else {
                break;
            }
        }

        metadata
    }

    pub(crate) fn list(&mut self) -> Vec<CompletedFileMetadata> {
        self.evict_expired();
        self.order
            .iter()
            .rev()
            .filter_map(|name| self.by_name.get(name))
            .map(|file| file.metadata.clone())
            .collect()
    }

    pub(crate) fn get(&mut self, filename: &str) -> Option<RetainedFile> {
        self.evict_expired();
        self.by_name.get(filename).cloned()
    }

    pub(crate) fn len(&mut self) -> usize {
        self.evict_expired();
        self.by_name.len()
    }

    fn evict_expired(&mut self) {
        let now = SystemTime::now();
        self.order.retain(|name| {
            let Some(file) = self.by_name.get(name) else {
                return false;
            };
            let age = now
                .duration_since(file.completed_at)
                .unwrap_or(Duration::from_secs(0));
            if age > self.ttl {
                self.by_name.remove(name);
                return false;
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::RetainedFiles;
    use emwin_protocol::ingest::ProductOrigin;
    use std::time::{Duration, SystemTime};

    #[test]
    fn retained_files_evict_by_capacity_and_ttl() {
        let mut files = RetainedFiles::new(2, Duration::from_secs(1));
        let now = SystemTime::now();

        files.insert(
            "one.txt".to_string(),
            b"one".to_vec(),
            1,
            ProductOrigin::Qbt,
            now,
        );
        files.insert(
            "two.txt".to_string(),
            b"two".to_vec(),
            2,
            ProductOrigin::Qbt,
            now,
        );
        files.insert(
            "three.txt".to_string(),
            b"three".to_vec(),
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
            b"expired".to_vec(),
            4,
            ProductOrigin::Qbt,
            now - Duration::from_secs(2),
        );
        ttl_files.insert(
            "fresh.txt".to_string(),
            b"fresh".to_vec(),
            5,
            ProductOrigin::Qbt,
            now,
        );
        let listed = ttl_files.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename, "fresh.txt");
    }
}
