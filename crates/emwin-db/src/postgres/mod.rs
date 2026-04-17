//! Postgres-backed persistence, archive query, and incident projection implementation.
//!
//! Module ownership is split by responsibility:
//! - `connection`: pool creation and migration bootstrap
//! - `sink`: metadata persistence orchestration
//! - `write`: transactional write-side helpers for products and child rows
//! - `query`: archive and incident read-path helpers
//! - `archive_service`: payload loading through persisted blob locations
//! - `prepare`: write-side normalization and projection shaping

pub(super) use crate::error::{PersistError, PersistResult};
use crate::sync::lock_unpoisoned;
use crate::writer::StorageBlobReader;
pub(super) use emwin_service::{
    IncidentChange, IncidentChangeAction, IncidentChangeTrigger, IncidentCursor, IncidentKey,
    IncidentListQuery, IncidentProductsCursor, IncidentProductsQuery, IncidentSummary,
    PaginatedResponse, ProductListQuery,
};
pub(super) use sqlx::{PgPool, Postgres, QueryBuilder};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::broadcast;
use tracing::info;

mod alerting;
mod archive_service;
mod connection;
mod prepare;
mod query;
mod sink;
#[cfg(test)]
mod tests;
mod write;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

const INCIDENT_CHANGE_CHANNEL_CAPACITY: usize = 1024;
/// Default Postgres pool size used by `PostgresConfig::new`.
pub const DEFAULT_MAX_DB_CONNECTIONS: u32 = 10;
pub use alerting::AlertContactPointRecord;

/// Connection settings for the Postgres/PostGIS metadata sink.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Postgres connection URL.
    pub database_url: String,
    /// Application name reported to Postgres for observability.
    pub application_name: String,
    /// Maximum pool size.
    pub max_connections: u32,
    /// Maximum time spent trying to establish the pool before failing.
    pub connect_timeout: Duration,
}

impl PostgresConfig {
    /// Creates a config with shared live/archive-safe defaults.
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            application_name: "emwin-db".to_string(),
            max_connections: DEFAULT_MAX_DB_CONNECTIONS,
            connect_timeout: Duration::from_secs(5),
        }
    }
}

/// Postgres metadata sink backed by an auto-migrated PostGIS schema.
#[derive(Debug, Clone)]
pub struct PostgresMetadataSink {
    config: PostgresConfig,
    pool: Arc<Mutex<Option<PgPool>>>,
    pool_init: Arc<AsyncMutex<()>>,
    reconnect_pending: Arc<AtomicBool>,
    blob_reader: Arc<StorageBlobReader>,
    incident_change_tx: broadcast::Sender<IncidentChange>,
}

impl PostgresMetadataSink {
    /// Creates a sink that establishes the pool lazily on first use.
    pub fn new(config: PostgresConfig) -> Self {
        let (incident_change_tx, _) = broadcast::channel(INCIDENT_CHANGE_CHANNEL_CAPACITY);
        Self {
            config,
            pool: Arc::new(Mutex::new(None)),
            pool_init: Arc::new(AsyncMutex::new(())),
            reconnect_pending: Arc::new(AtomicBool::new(false)),
            blob_reader: Arc::new(StorageBlobReader::new()),
            incident_change_tx,
        }
    }

    /// Connects, validates PostGIS availability, and applies embedded migrations.
    pub async fn connect(config: PostgresConfig) -> crate::error::PersistResult<Self> {
        let sink = Self::new(config);
        let _ = sink.ensure_pool().await?;
        Ok(sink)
    }

    /// Exposes the initialized pool for integration tests and diagnostics.
    pub fn pool(&self) -> PgPool {
        let guard = lock_unpoisoned(&self.pool);
        guard
            .as_ref()
            .cloned()
            .expect("postgres pool is not initialized")
    }

    /// Returns the configured connection target for diagnostics.
    pub fn describe_target(&self) -> String {
        connection::connection_target(&self.config)
            .unwrap_or_else(|_| "postgres target unavailable".to_string())
    }

    /// Subscribes to incident change broadcasts emitted by this sink.
    pub fn subscribe_incident_changes(&self) -> broadcast::Receiver<IncidentChange> {
        self.incident_change_tx.subscribe()
    }

    pub(crate) async fn ensure_pool(&self) -> crate::error::PersistResult<PgPool> {
        {
            let guard = lock_unpoisoned(&self.pool);
            if let Some(pool) = guard.as_ref() {
                return Ok(pool.clone());
            }
        }

        let _init_guard = self.pool_init.lock().await;

        {
            let guard = lock_unpoisoned(&self.pool);
            if let Some(pool) = guard.as_ref() {
                return Ok(pool.clone());
            }
        }

        let pool: PgPool = connection::connect_pool(&self.config).await?;

        let mut guard = lock_unpoisoned(&self.pool);
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.clone());
        }
        *guard = Some(pool.clone());
        if self.reconnect_pending.swap(false, Ordering::AcqRel) {
            let connect_target = connection::connection_target(&self.config)
                .unwrap_or_else(|_| "postgres target unavailable".to_string());
            info!(
                target = %connect_target,
                connect_timeout_secs = self.config.connect_timeout.as_secs_f64(),
                application_name = %self.config.application_name,
                "postgres reconnect succeeded"
            );
        }
        Ok(pool)
    }
}
