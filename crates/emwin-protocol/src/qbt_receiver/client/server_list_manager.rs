//! Server list lifecycle management for EMWIN clients.
//!
//! This module provides a [`ServerListManager`] that handles:
//! - Loading persisted server lists from disk
//! - Saving server lists atomically to disk
//! - Applying server list updates from the feed
//! - Providing deterministic round-robin access to EMWIN endpoints
//!
//! # Persistence
//!
//! Server lists can be persisted to disk in JSON format. The persisted
//! format includes only EMWIN server endpoints plus a version identifier.

use crate::qbt_receiver::error::{QbtReceiverError, QbtReceiverResult};
use crate::qbt_receiver::protocol::model::QbtServerList;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
/// Manages the current QBT server list and optional on-disk persistence.
pub struct ServerListManager {
    path: Option<PathBuf>,
    current: QbtServerList,
    available: VecDeque<(String, u16)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedServerList {
    servers: Vec<(String, u16)>,
    version: String,
}

impl ServerListManager {
    /// Creates a manager for the persisted server list and fallback endpoints.
    pub fn new(path: Option<PathBuf>, default_servers: Vec<(String, u16)>) -> Self {
        let mut manager = Self {
            path,
            current: QbtServerList {
                servers: default_servers,
            },
            available: VecDeque::new(),
        };
        manager.rebuild_available();
        manager
    }

    /// Loads a persisted server list from disk when a path is configured.
    pub fn load(&mut self) -> QbtReceiverResult<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };

        if !path.exists() {
            return Ok(());
        }

        let bytes = fs::read(path)?;
        let persisted: PersistedServerList = serde_json::from_slice(&bytes)
            .map_err(|e| QbtReceiverError::Lifecycle(e.to_string()))?;

        if !persisted.servers.is_empty() {
            self.current = QbtServerList {
                servers: persisted.servers,
            };
            self.rebuild_available();
        }

        Ok(())
    }

    /// Persists the current server list to disk when a path is configured.
    pub fn save(&self) -> QbtReceiverResult<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };

        save_atomic(path, &self.current)
    }

    /// Replaces the current endpoint set with a feed-provided server list.
    pub fn apply_server_list(&mut self, list: QbtServerList) -> QbtReceiverResult<()> {
        if list.servers.is_empty() {
            return Err(QbtReceiverError::Lifecycle(
                "server list update contained no valid endpoints".to_string(),
            ));
        }

        self.current = list;
        self.rebuild_available();
        self.save()
    }

    /// Returns the next endpoint in round-robin order.
    pub fn next_endpoint(&mut self) -> Option<(String, u16)> {
        let endpoint = self.available.pop_front()?;
        self.available.push_back(endpoint.clone());
        Some(endpoint)
    }

    /// Returns the number of active endpoints in the current list.
    pub fn endpoint_count(&self) -> usize {
        self.available.len()
    }

    fn rebuild_available(&mut self) {
        let mut servers = self.current.servers.clone();
        sort_dedup_endpoints(&mut servers);
        self.current.servers = servers.clone();
        self.available = VecDeque::from(servers);
    }
}

fn sort_dedup_endpoints(endpoints: &mut Vec<(String, u16)>) {
    endpoints.sort_unstable();
    endpoints.dedup();
}

fn save_atomic(path: &Path, server_list: &QbtServerList) -> QbtReceiverResult<()> {
    let persisted = PersistedServerList {
        servers: server_list.servers.clone(),
        version: "1.0".to_string(),
    };

    let data = serde_json::to_vec_pretty(&persisted).map_err(|e| {
        QbtReceiverError::Lifecycle(format!("failed to serialize server list: {e}"))
    })?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ServerListManager;
    use crate::qbt_receiver::protocol::model::QbtServerList;

    #[test]
    fn rotates_servers_in_sorted_deduplicated_order() {
        let mut mgr = ServerListManager::new(None, vec![("a".to_string(), 1)]);
        mgr.apply_server_list(QbtServerList {
            servers: vec![
                ("b".to_string(), 2),
                ("a".to_string(), 1),
                ("b".to_string(), 2),
            ],
        })
        .expect("server list should apply");

        assert_eq!(mgr.next_endpoint(), Some(("a".to_string(), 1)));
        assert_eq!(mgr.next_endpoint(), Some(("b".to_string(), 2)));
        assert_eq!(mgr.next_endpoint(), Some(("a".to_string(), 1)));
    }

    #[test]
    fn apply_server_list_rejects_empty_updates() {
        let mut mgr = ServerListManager::new(None, vec![("a".to_string(), 1)]);
        assert!(mgr.apply_server_list(QbtServerList::default()).is_err());
    }

    #[test]
    fn endpoint_count_tracks_active_servers() {
        let mut mgr = ServerListManager::new(None, vec![("a".to_string(), 1)]);
        assert_eq!(mgr.endpoint_count(), 1);
        mgr.apply_server_list(QbtServerList {
            servers: vec![("b".to_string(), 2), ("c".to_string(), 3)],
        })
        .expect("server list should apply");
        assert_eq!(mgr.endpoint_count(), 2);
    }
}
