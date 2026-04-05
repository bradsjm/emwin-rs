use crate::error::{PersistError, PersistResult};
use crate::metadata::CompletedFileMetadata;
use crate::runtime::{MetadataSink, PersistedRequest};
use crate::writer::{BoxFuture, StorageBlobReader};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder, Row};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::info;

mod connection;
mod prepare;
mod query;
#[cfg(test)]
#[cfg(test)]
mod tests;
mod write;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

const INCIDENT_CHANGE_CHANNEL_CAPACITY: usize = 1024;

/// Connection settings for the Postgres/PostGIS metadata sink.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Postgres connection URL.
    pub database_url: String,
    /// Application name reported to Postgres for observability.
    pub application_name: String,
    /// Maximum pool size. Default remains `1` to preserve queue ordering.
    pub max_connections: u32,
    /// Maximum time spent trying to establish the pool before failing.
    pub connect_timeout: Duration,
}

impl PostgresConfig {
    /// Creates a config with conservative defaults for the single-worker runtime.
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            application_name: "emwin-db".to_string(),
            max_connections: 1,
            connect_timeout: Duration::from_secs(5),
        }
    }
}

/// Postgres metadata sink backed by an auto-migrated PostGIS schema.
#[derive(Debug, Clone)]
pub struct PostgresMetadataSink {
    config: PostgresConfig,
    pool: Arc<Mutex<Option<PgPool>>>,
    reconnect_pending: Arc<AtomicBool>,
    blob_reader: Arc<StorageBlobReader>,
    incident_change_tx: broadcast::Sender<IncidentChange>,
}

/// Result of expiring active incident rows whose end time has passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncidentCleanupResult {
    /// Number of incident rows updated to `expired`.
    pub expired_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentChangeAction {
    Created,
    Updated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentChangeTrigger {
    Persist,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IncidentChange {
    pub action: IncidentChangeAction,
    pub trigger: IncidentChangeTrigger,
    pub incident: IncidentSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IncidentKey {
    pub office: String,
    pub phenomena: String,
    pub significance: String,
    pub etn: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentCursor {
    pub latest_product_timestamp_utc: chrono::DateTime<chrono::Utc>,
    pub office: String,
    pub phenomena: String,
    pub significance: String,
    pub etn: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentProductsCursor {
    pub source_timestamp_utc: i64,
    pub product_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedIssueCursor {
    pub source_timestamp_utc: i64,
    pub product_id: i64,
    pub issue_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentListQuery {
    pub office: Option<String>,
    pub phenomena: Option<String>,
    pub significance: Option<String>,
    pub etn: Option<i64>,
    pub status: Option<String>,
    pub updated_after: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_before: Option<chrono::DateTime<chrono::Utc>>,
    pub active_at: Option<chrono::DateTime<chrono::Utc>>,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for IncidentListQuery {
    fn default() -> Self {
        Self {
            office: None,
            phenomena: None,
            significance: None,
            etn: None,
            status: None,
            updated_after: None,
            updated_before: None,
            active_at: None,
            limit: 100,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentProductsQuery {
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for IncidentProductsQuery {
    fn default() -> Self {
        Self {
            limit: 100,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedIssueListQuery {
    pub product_id: Option<i64>,
    pub kind: Option<String>,
    pub code: Option<String>,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for ArchivedIssueListQuery {
    fn default() -> Self {
        Self {
            product_id: None,
            kind: None,
            code: None,
            limit: 100,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct IncidentSummary {
    pub office: String,
    pub phenomena: String,
    pub significance: String,
    pub etn: i64,
    pub current_status: String,
    pub latest_vtec_action: String,
    pub issued_at: chrono::DateTime<chrono::Utc>,
    pub start_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub end_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub last_updated_at: chrono::DateTime<chrono::Utc>,
    pub first_product_id: i64,
    pub latest_product_id: i64,
    pub latest_product_timestamp_utc: chrono::DateTime<chrono::Utc>,
}

pub type IncidentDetail = IncidentSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct ArchivedProductSummary {
    pub product_id: i64,
    pub filename: String,
    pub source_timestamp_utc: i64,
    pub ingested_at: chrono::DateTime<chrono::Utc>,
    pub source_receiver: String,
    pub source_message_id: Option<String>,
    pub size_bytes: i64,
    pub payload_storage_kind: String,
    pub has_metadata_sidecar: bool,
    pub source: String,
    pub family: Option<String>,
    pub artifact_kind: Option<String>,
    pub title: Option<String>,
    pub container: String,
    pub pil: Option<String>,
    pub wmo_prefix: Option<String>,
    pub bbb_kind: Option<String>,
    pub office_code: Option<String>,
    pub office_city: Option<String>,
    pub office_state: Option<String>,
    pub header_kind: Option<String>,
    pub ttaaii: Option<String>,
    pub cccc: Option<String>,
    pub ddhhmm: Option<String>,
    pub bbb: Option<String>,
    pub afos: Option<String>,
    pub has_body: bool,
    pub has_artifact: bool,
    pub has_issues: bool,
    pub has_vtec: bool,
    pub has_ugc: bool,
    pub has_hvtec: bool,
    pub has_latlon: bool,
    pub has_time_mot_loc: bool,
    pub has_wind_hail: bool,
    pub vtec_count: i32,
    pub ugc_count: i32,
    pub hvtec_count: i32,
    pub latlon_count: i32,
    pub time_mot_loc_count: i32,
    pub wind_hail_count: i32,
    pub issue_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct ArchivedProductDetail {
    #[serde(flatten)]
    pub summary: ArchivedProductSummary,
    pub payload_location: Option<String>,
    pub metadata_location: Option<String>,
    pub product_json: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedPayload {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub payload_storage_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct ArchivedIssue {
    pub id: i64,
    pub product_id: i64,
    pub kind: String,
    pub code: String,
    pub message: String,
    pub line: Option<String>,
}

impl PostgresMetadataSink {
    /// Creates a sink that establishes the pool lazily on first use.
    pub fn new(config: PostgresConfig) -> Self {
        let (incident_change_tx, _) = broadcast::channel(INCIDENT_CHANGE_CHANNEL_CAPACITY);
        Self {
            config,
            pool: Arc::new(Mutex::new(None)),
            reconnect_pending: Arc::new(AtomicBool::new(false)),
            blob_reader: Arc::new(StorageBlobReader::new()),
            incident_change_tx,
        }
    }

    /// Connects, validates PostGIS availability, and applies embedded migrations.
    pub async fn connect(config: PostgresConfig) -> PersistResult<Self> {
        let sink = Self::new(config);
        let _ = sink.ensure_pool().await?;
        Ok(sink)
    }

    /// Exposes the initialized pool for integration tests and diagnostics.
    pub fn pool(&self) -> PgPool {
        let guard = self
            .pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .as_ref()
            .cloned()
            .expect("postgres pool is not initialized")
    }

    pub fn describe_target(&self) -> String {
        connection::connection_target(&self.config)
            .unwrap_or_else(|_| "postgres target unavailable".to_string())
    }

    pub fn subscribe_incident_changes(&self) -> broadcast::Receiver<IncidentChange> {
        self.incident_change_tx.subscribe()
    }

    async fn ensure_pool(&self) -> PersistResult<PgPool> {
        {
            let guard = self
                .pool
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(pool) = guard.as_ref() {
                return Ok(pool.clone());
            }
        }

        let pool = connection::connect_pool(&self.config).await?;

        let mut guard = self
            .pool
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

    /// Expires active incidents whose `end_utc` has passed without a newer product update.
    pub async fn expire_active_incidents(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> PersistResult<IncidentCleanupResult> {
        let pool = self.ensure_pool().await?;
        let result = sqlx::query_as::<_, IncidentSummary>(
            "UPDATE incidents
             SET current_status = 'expired',
                 last_updated_at = now()
             WHERE current_status = 'active'
               AND end_utc IS NOT NULL
               AND end_utc < $1
             RETURNING
                office,
                phenomena,
                significance,
                etn,
                current_status,
                latest_vtec_action,
                issued_at,
                start_utc,
                end_utc,
                last_updated_at,
                first_product_id,
                latest_product_id,
                latest_product_timestamp_utc",
        )
        .bind(now)
        .fetch_all(&pool)
        .await;

        match result {
            Ok(incidents) => {
                self.publish_incident_changes(incidents.iter().cloned().map(|incident| {
                    IncidentChange {
                        action: IncidentChangeAction::Updated,
                        trigger: IncidentChangeTrigger::Cleanup,
                        incident,
                    }
                }));
                Ok(IncidentCleanupResult {
                    expired_count: incidents.len() as u64,
                })
            }
            Err(err) => {
                let err = PersistError::from(err);
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn list_incidents(
        &self,
        query: IncidentListQuery,
    ) -> PersistResult<PaginatedResponse<IncidentSummary>> {
        let pool = self.ensure_pool().await?;
        let result = query::list_incidents_query(&pool, query).await;
        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn get_incident(&self, key: &IncidentKey) -> PersistResult<Option<IncidentDetail>> {
        let pool = self.ensure_pool().await?;
        let mut builder = QueryBuilder::<Postgres>::new(query::incident_select_sql());
        builder
            .push(" WHERE office = ")
            .push_bind(&key.office)
            .push(" AND phenomena = ")
            .push_bind(&key.phenomena)
            .push(" AND significance = ")
            .push_bind(&key.significance)
            .push(" AND etn = ")
            .push_bind(key.etn);
        let result = builder
            .build_query_as::<IncidentDetail>()
            .fetch_optional(&pool)
            .await
            .map_err(PersistError::from);

        match result {
            Ok(incident) => Ok(incident),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn list_incident_products(
        &self,
        key: &IncidentKey,
        query: IncidentProductsQuery,
    ) -> PersistResult<PaginatedResponse<ArchivedProductSummary>> {
        let pool = self.ensure_pool().await?;
        let result = query::list_incident_products_query(&pool, key, query).await;
        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn get_archived_product(
        &self,
        product_id: i64,
    ) -> PersistResult<Option<ArchivedProductDetail>> {
        let pool = self.ensure_pool().await?;
        let mut builder =
            QueryBuilder::<Postgres>::new(query::archived_product_detail_select_sql());
        builder.push(" WHERE id = ").push_bind(product_id);
        let result = builder
            .build()
            .fetch_optional(&pool)
            .await
            .map(|row| row.map(query::archived_product_detail_from_row))
            .map_err(PersistError::from);

        match result {
            Ok(product) => Ok(product),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn list_archived_issues(
        &self,
        query: ArchivedIssueListQuery,
    ) -> PersistResult<PaginatedResponse<ArchivedIssue>> {
        let pool = self.ensure_pool().await?;
        let result = query::list_archived_issues_query(&pool, query).await;
        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn get_archived_issue(&self, issue_id: i64) -> PersistResult<Option<ArchivedIssue>> {
        let pool = self.ensure_pool().await?;
        let result = query::get_archived_issue_query(&pool, issue_id).await;
        match result {
            Ok(issue) => Ok(issue),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn read_archived_payload(
        &self,
        product_id: i64,
    ) -> PersistResult<Option<ArchivedPayload>> {
        let pool = self.ensure_pool().await?;
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT filename, payload_storage_kind, payload_location FROM products WHERE id = ",
        );
        builder.push_bind(product_id);
        let row = builder
            .build()
            .fetch_optional(&pool)
            .await
            .map_err(PersistError::from);

        let Some(row) = (match row {
            Ok(row) => row,
            Err(err) => {
                self.handle_runtime_error(&err).await;
                return Err(err);
            }
        }) else {
            return Ok(None);
        };

        let filename = row.get::<String, _>("filename");
        let payload_storage_kind = row.get::<String, _>("payload_storage_kind");
        let payload_location = row.get::<String, _>("payload_location");
        let bytes = self
            .blob_reader
            .read(
                query::parse_blob_storage_kind(&payload_storage_kind)?,
                &payload_location,
            )
            .await?;
        Ok(Some(ArchivedPayload {
            filename,
            bytes,
            payload_storage_kind,
        }))
    }

    async fn handle_runtime_error(&self, err: &PersistError) {
        if err.should_reset_postgres_pool() {
            let connect_target = connection::connection_target(&self.config)
                .unwrap_or_else(|_| "postgres target unavailable".to_string());
            let mut guard = self
                .pool
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if guard.is_some() {
                info!(
                    target = %connect_target,
                    connect_timeout_secs = self.config.connect_timeout.as_secs_f64(),
                    error = %err,
                    "dropping cached postgres pool; next operation will reconnect"
                );
                *guard = None;
                self.reconnect_pending.store(true, Ordering::Release);
            }
        }
    }

    fn publish_incident_changes(&self, changes: impl IntoIterator<Item = IncidentChange>) {
        for change in changes {
            let _ = self.incident_change_tx.send(change);
        }
    }
}

impl MetadataSink<CompletedFileMetadata> for PostgresMetadataSink {
    fn persist<'a>(
        &'a self,
        request: PersistedRequest<CompletedFileMetadata>,
    ) -> BoxFuture<'a, PersistResult<()>> {
        Box::pin(async move {
            let prepared = prepare::PreparedProduct::prepare(&request.metadata, &request.blobs)?;
            let pool = self.ensure_pool().await?;
            let result: PersistResult<Vec<IncidentChange>> = async {
                let mut tx = pool.begin().await?;
                let product_id = write::upsert_product(&mut tx, &prepared).await?;
                let incident_changes =
                    write::replace_children(&mut tx, product_id, &prepared).await?;
                tx.commit().await?;
                query::load_incident_changes(
                    &pool,
                    incident_changes,
                    IncidentChangeTrigger::Persist,
                )
                .await
            }
            .await;

            match result {
                Ok(incident_changes) => {
                    self.publish_incident_changes(incident_changes);
                    Ok(())
                }
                Err(err) => {
                    self.handle_runtime_error(&err).await;
                    Err(err)
                }
            }
        })
    }

    fn backend_name(&self) -> &'static str {
        "database"
    }

    fn target_description(&self) -> Option<String> {
        Some(self.describe_target())
    }
}
