use super::payloads::{IncidentEventPayload, TelemetryPayload};
use emwin_db::PostgresMetadataSink;
use emwin_service::{
    ArchiveQueryService, CompletedFileMetadata,
    IncidentBroadcastEvent as ServiceIncidentBroadcastEvent, LiveBroadcastEvent, LiveStatsSnapshot,
    RetainedFile,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::{broadcast, watch};

#[derive(Debug, Clone)]
pub(crate) struct IncidentBroadcastEvent {
    pub(crate) id: u64,
    pub(crate) payload: IncidentEventPayload,
}

pub(crate) type SharedLiveRuntime = Arc<emwin_live::LiveRuntime>;
pub(crate) type SharedArchiveQueryService = Arc<dyn ArchiveQueryService>;
pub(crate) type SharedAlertStore = Arc<PostgresMetadataSink>;

#[derive(Debug, Clone, Serialize)]
pub struct ApiArchiveStatus {
    pub(crate) configured: bool,
    pub(crate) healthy: bool,
    pub(crate) errors_total: u64,
    pub(crate) pool_timeouts_total: u64,
    pub(crate) last_error: Option<String>,
}

#[derive(Clone)]
pub struct ApiServices {
    pub(crate) runtime: SharedLiveRuntime,
    pub(crate) archive: SharedArchiveQueryService,
    pub(crate) alert_store: Option<SharedAlertStore>,
}

impl ApiServices {
    pub fn from_live_runtime(runtime: emwin_live::LiveRuntime) -> Self {
        let alert_store = runtime.alert_store();
        let archive = runtime.archive_query_service();
        let shared = Arc::new(runtime);
        Self {
            runtime: shared,
            archive: archive.unwrap_or_else(|| Arc::new(NotConfiguredArchiveService)),
            alert_store: alert_store.map(Arc::new),
        }
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<LiveBroadcastEvent> {
        self.runtime.subscribe_events()
    }

    pub(crate) fn telemetry_snapshot(&self) -> TelemetryPayload {
        self.runtime.telemetry_snapshot()
    }

    pub(crate) fn stats_snapshot(&self) -> LiveStatsSnapshot {
        self.runtime.stats_snapshot()
    }

    pub(crate) async fn shutdown(&self) -> emwin_service::ServiceResult<()> {
        self.runtime
            .shutdown()
            .await
            .map_err(|err| emwin_service::ServiceError::Runtime(err.to_string()))
    }

    pub(crate) fn list_retained_files(&self) -> Vec<CompletedFileMetadata> {
        self.runtime.list_retained_files()
    }

    pub(crate) fn get_retained_file(&self, filename: &str) -> Option<RetainedFile> {
        self.runtime.get_retained_file(filename)
    }

    pub(crate) fn subscribe_incident_changes(
        &self,
    ) -> Option<broadcast::Receiver<ServiceIncidentBroadcastEvent>> {
        self.runtime.subscribe_incident_changes()
    }

    pub(crate) fn archive_status_snapshot(&self) -> ApiArchiveStatus {
        let configured = self.runtime.archive_configured();
        let last_error = self.runtime.archive_last_error();
        ApiArchiveStatus {
            configured,
            healthy: !configured || last_error.is_none(),
            errors_total: self.runtime.archive_errors_total(),
            pool_timeouts_total: self.runtime.archive_pool_timeouts_total(),
            last_error,
        }
    }

    pub(crate) fn alert_store(&self) -> Option<&PostgresMetadataSink> {
        self.alert_store.as_deref()
    }
}

struct NotConfiguredArchiveService;

impl emwin_service::ArchiveQueryService for NotConfiguredArchiveService {
    fn list_incidents(
        &self,
        _query: emwin_service::IncidentListQuery,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<
            emwin_service::PaginatedResponse<emwin_service::IncidentSummary>,
        >,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn get_incident<'a>(
        &'a self,
        _key: &'a emwin_service::IncidentKey,
    ) -> emwin_service::archive::BoxFuture<
        'a,
        emwin_service::ServiceResult<Option<emwin_service::IncidentDetail>>,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn list_incident_products<'a>(
        &'a self,
        _key: &'a emwin_service::IncidentKey,
        _query: emwin_service::IncidentProductsQuery,
    ) -> emwin_service::archive::BoxFuture<
        'a,
        emwin_service::ServiceResult<
            emwin_service::PaginatedResponse<emwin_service::ArchivedProductSummary>,
        >,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn list_archived_products(
        &self,
        _query: emwin_service::ProductListQuery,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<
            emwin_service::PaginatedResponse<emwin_service::ArchivedProductSummary>,
        >,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn get_archived_product(
        &self,
        _product_id: i64,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<Option<emwin_service::ArchivedProductDetail>>,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn list_archived_issues(
        &self,
        _query: emwin_service::ArchivedIssueListQuery,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<
            emwin_service::PaginatedResponse<emwin_service::ArchivedIssue>,
        >,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn get_archived_issue(
        &self,
        _issue_id: i64,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<Option<emwin_service::ArchivedIssue>>,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn read_archived_payload(
        &self,
        _product_id: i64,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<Option<emwin_service::ArchivedPayload>>,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn list_archived_features(
        &self,
        _query: emwin_service::FeatureListQuery,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<
            emwin_service::PaginatedResponse<emwin_service::ArchivedFeature>,
        >,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn list_facet_aggregate(
        &self,
        _query: emwin_service::FacetAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<emwin_service::FacetAggregateResult>,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn list_timeseries_aggregate(
        &self,
        _query: emwin_service::TimeseriesAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<emwin_service::TimeseriesAggregateResult>,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }

    fn list_cell_aggregate(
        &self,
        _query: emwin_service::CellAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        emwin_service::ServiceResult<emwin_service::CellAggregateResult>,
    > {
        Box::pin(async {
            Err(emwin_service::ServiceError::NotConfigured(
                "archive database is not configured".to_string(),
            ))
        })
    }
}

pub(crate) struct AppState {
    pub(crate) services: ApiServices,
    pub(crate) incident_event_tx: broadcast::Sender<IncidentBroadcastEvent>,
    pub(crate) shutdown_rx: watch::Receiver<bool>,
    pub(crate) connected_clients: AtomicUsize,
    pub(crate) max_clients: usize,
    pub(crate) next_incident_event_id: AtomicU64,
    pub(crate) openapi_auth_token: Option<String>,
    pub(crate) alerting_apprise_api_url: Option<String>,
    pub(crate) quiet: bool,
}

pub(crate) struct ClientGuard {
    pub(crate) state: Arc<AppState>,
    pub(crate) peer: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct HttpServerOptions {
    pub bind: String,
    pub cors_origin: Option<String>,
    pub max_clients: usize,
    pub stats_interval_secs: u64,
    pub quiet: bool,
    pub openapi_auth_token: Option<String>,
    pub alerting_apprise_api_url: Option<String>,
}

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.state.connected_clients.fetch_sub(1, Ordering::Relaxed);
        super::super::log_info(
            self.state.quiet,
            &format!("sse client disconnected peer={}", self.peer),
        );
    }
}
