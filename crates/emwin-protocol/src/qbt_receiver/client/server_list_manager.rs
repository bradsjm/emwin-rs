//! Server list lifecycle management for EMWIN clients.
//!
//! This module provides a [`ServerListManager`] that handles:
//! - Loading persisted server lists from disk
//! - Saving server lists atomically to disk
//! - Applying server list updates from the feed
//! - Providing rotated access to primary and satellite endpoints
//! - Managing endpoint availability with deterministic sorted rotation
//!
//! # Endpoint Rotation
//!
//! The manager maintains two queues of available endpoints:
//! - Primary servers (regular feed servers)
//! - Satellite servers (backup/high-priority servers)
//!
//! When requesting an endpoint, the manager returns the next available
//! server from the primary queue, falling back to satellite servers
//! if necessary.
//!
//! # Persistence
//!
//! Server lists can be persisted to disk in JSON format. The persisted
//! format includes both primary and satellite server lists along with
//! a version identifier for compatibility tracking.

use crate::qbt_receiver::error::{QbtReceiverError, QbtReceiverResult};
use crate::qbt_receiver::protocol::model::QbtServerList;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ServerListManager {
    path: Option<PathBuf>,
    current: QbtServerList,
    primary_available: VecDeque<(String, u16)>,
    satellite_available: VecDeque<(String, u16)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedServerList {
    servers: Vec<(String, u16)>,
    sat_servers: Vec<(String, u16)>,
    version: String,
}

impl ServerListManager {
    pub fn new(path: Option<PathBuf>, default_servers: Vec<(String, u16)>) -> Self {
        let mut manager = Self {
            path,
            current: QbtServerList {
                servers: default_servers,
                sat_servers: Vec::new(),
            },
            primary_available: VecDeque::new(),
            satellite_available: VecDeque::new(),
        };
        manager.rebuild_available();
        manager
    }

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

        if !persisted.servers.is_empty() || !persisted.sat_servers.is_empty() {
            self.current = QbtServerList {
                servers: persisted.servers,
                sat_servers: persisted.sat_servers,
            };
            self.rebuild_available();
        }

        Ok(())
    }

    pub fn save(&self) -> QbtReceiverResult<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };

        save_atomic(path, &self.current)
    }

    pub fn apply_server_list(&mut self, list: QbtServerList) -> QbtReceiverResult<()> {
        if list.servers.is_empty() && list.sat_servers.is_empty() {
            return Err(QbtReceiverError::Lifecycle(
                "server list update contained no valid endpoints".to_string(),
            ));
        }

        self.current = list;
        self.rebuild_available();
        self.save()
    }

    pub fn next_endpoint(&mut self) -> Option<(String, u16)> {
        if let Some(endpoint) = self.primary_available.pop_front() {
            self.primary_available.push_back(endpoint.clone());
            return Some(endpoint);
        }

        let endpoint = self.satellite_available.pop_front()?;
        self.satellite_available.push_back(endpoint.clone());
        Some(endpoint)
    }

    pub fn mark_bad_endpoint(&mut self, endpoint: &(String, u16)) {
        self.primary_available
            .retain(|candidate| candidate != endpoint);
        self.satellite_available
            .retain(|candidate| candidate != endpoint);
    }

    fn rebuild_available(&mut self) {
        let mut primary = self.current.servers.clone();
        let mut satellite = self.current.sat_servers.clone();

        sort_dedup_endpoints(&mut primary);
        sort_dedup_endpoints(&mut satellite);

        self.primary_available = VecDeque::from(primary);
        self.satellite_available = VecDeque::from(satellite);
    }
}

fn sort_dedup_endpoints(endpoints: &mut Vec<(String, u16)>) {
    endpoints.sort_unstable();
    endpoints.dedup();
}

fn save_atomic(path: &Path, server_list: &QbtServerList) -> QbtReceiverResult<()> {
    let persisted = PersistedServerList {
        servers: server_list.servers.clone(),
        sat_servers: server_list.sat_servers.clone(),
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
    use std::collections::HashSet;

    #[test]
    fn uses_primary_servers_before_satellite_servers() {
        let mut mgr = ServerListManager::new(None, vec![("a".to_string(), 1)]);
        mgr.apply_server_list(QbtServerList {
            servers: vec![("a".to_string(), 1), ("b".to_string(), 2)],
            sat_servers: vec![("sat".to_string(), 3)],
        })
        .expect("server list should apply");

        let mut seen = HashSet::new();
        for _ in 0..12 {
            let endpoint = mgr.next_endpoint().expect("endpoint should exist");
            assert_ne!(endpoint, ("sat".to_string(), 3));
            seen.insert(endpoint);
        }

        assert_eq!(seen.len(), 2);
        assert!(seen.contains(&("a".to_string(), 1)));
        assert!(seen.contains(&("b".to_string(), 2)));
    }

    #[test]
    fn falls_back_to_satellite_when_primary_pool_is_exhausted() {
        let mut mgr = ServerListManager::new(None, vec![("a".to_string(), 1)]);
        mgr.apply_server_list(QbtServerList {
            servers: vec![("a".to_string(), 1), ("b".to_string(), 2)],
            sat_servers: vec![("sat".to_string(), 3)],
        })
        .expect("server list should apply");

        mgr.mark_bad_endpoint(&("a".to_string(), 1));
        mgr.mark_bad_endpoint(&("b".to_string(), 2));

        for _ in 0..6 {
            assert_eq!(mgr.next_endpoint(), Some(("sat".to_string(), 3)));
        }
    }

    #[test]
    fn mark_bad_endpoint_removes_until_next_list_update() {
        let mut mgr = ServerListManager::new(None, vec![("a".to_string(), 1)]);
        mgr.apply_server_list(QbtServerList {
            servers: vec![("a".to_string(), 1), ("b".to_string(), 2)],
            sat_servers: vec![("sat".to_string(), 3)],
        })
        .expect("server list should apply");

        mgr.mark_bad_endpoint(&("b".to_string(), 2));
        for _ in 0..10 {
            let endpoint = mgr.next_endpoint().expect("endpoint should exist");
            assert_ne!(endpoint, ("b".to_string(), 2));
        }

        mgr.apply_server_list(QbtServerList {
            servers: vec![("a".to_string(), 1), ("b".to_string(), 2)],
            sat_servers: vec![("sat".to_string(), 3)],
        })
        .expect("server list should apply again");

        let mut saw_b = false;
        for _ in 0..20 {
            if mgr.next_endpoint() == Some(("b".to_string(), 2)) {
                saw_b = true;
                break;
            }
        }
        assert!(saw_b, "expected refreshed list to reintroduce endpoint b");
    }
}
