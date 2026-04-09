use super::query;
use super::{PostgresMetadataSink, connection};
use crate::error::{PersistError, PersistResult};
use emwin_service::{
    ArchivedFeature, ArchivedIssue, ArchivedIssueListQuery, ArchivedPayload, ArchivedProductDetail,
    ArchivedProductSummary, CellAggregateQuery, CellAggregateResult, FacetAggregateQuery,
    FacetAggregateResult, FeatureListQuery, IncidentChange, IncidentChangeAction,
    IncidentChangeTrigger, IncidentCleanupResult, IncidentDetail, IncidentKey, IncidentListQuery,
    IncidentProductsQuery, IncidentSummary, PaginatedResponse, ProductListQuery,
    TimeseriesAggregateQuery, TimeseriesAggregateResult,
};
use sqlx::{Postgres, QueryBuilder, Row};
use std::sync::atomic::Ordering;
use tracing::info;

impl PostgresMetadataSink {
    /// Expires active incidents whose `end_utc` has passed without a newer product update.
    pub async fn expire_active_incidents(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> PersistResult<IncidentCleanupResult> {
        let pool = self.ensure_pool().await?;
        let result = sqlx::query(
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
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| query::incident_summary_from_row(&row))
                .collect::<Vec<_>>()
        });

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
            .build()
            .fetch_optional(&pool)
            .await
            .map(|row| row.map(|row| query::incident_summary_from_row(&row)))
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

    pub async fn list_archived_products(
        &self,
        query: ProductListQuery,
    ) -> PersistResult<PaginatedResponse<ArchivedProductSummary>> {
        let pool = self.ensure_pool().await?;
        let result = query::list_archived_products_query(&pool, query).await;
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
            .map(|row| row.map(|row| query::archived_product_detail_from_row(&row)))
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
            "SELECT filename, payload_location FROM products WHERE id = ",
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
        let payload_location = row.get::<String, _>("payload_location");
        let bytes = self.blob_reader.read(&payload_location).await?;
        Ok(Some(ArchivedPayload { filename, bytes }))
    }

    pub async fn list_archived_features(
        &self,
        query: FeatureListQuery,
    ) -> PersistResult<PaginatedResponse<ArchivedFeature>> {
        let pool = self.ensure_pool().await?;
        let result = query::list_archived_features_query(&pool, query).await;
        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn list_facet_aggregate(
        &self,
        query: FacetAggregateQuery,
    ) -> PersistResult<FacetAggregateResult> {
        let pool = self.ensure_pool().await?;
        let result = query::list_facet_aggregate_query(&pool, query).await;
        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn list_timeseries_aggregate(
        &self,
        query: TimeseriesAggregateQuery,
    ) -> PersistResult<TimeseriesAggregateResult> {
        let pool = self.ensure_pool().await?;
        let result = query::list_timeseries_aggregate_query(&pool, query).await;
        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub async fn list_cell_aggregate(
        &self,
        query: CellAggregateQuery,
    ) -> PersistResult<CellAggregateResult> {
        let pool = self.ensure_pool().await?;
        let result = query::list_cell_aggregate_query(&pool, query).await;
        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                self.handle_runtime_error(&err).await;
                Err(err)
            }
        }
    }

    pub(super) async fn handle_runtime_error(&self, err: &PersistError) {
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

    pub(super) fn publish_incident_changes(
        &self,
        changes: impl IntoIterator<Item = IncidentChange>,
    ) {
        for change in changes {
            let _ = self.incident_change_tx.send(change);
        }
    }
}
