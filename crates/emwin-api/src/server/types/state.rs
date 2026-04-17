use super::payloads::{EventKind, IncidentEventPayload, TelemetryPayload};
use emwin_db::PostgresMetadataSink;
use emwin_service::{
    CompletedFileMetadata, IncidentBroadcastEvent as ServiceIncidentBroadcastEvent,
    IncidentChangeStream, LiveBroadcastEvent, LiveEventService, LiveStatsSnapshot, RetainedFile,
    RetainedFileService,
};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::{broadcast, watch};

/// Lightweight broadcast notification stored in the SSE ring buffer.
#[derive(Debug, Clone)]
pub(crate) struct BroadcastEvent {
    pub(crate) id: u64,
    pub(crate) kind: EventKind,
}

#[derive(Debug, Clone)]
pub(crate) struct IncidentBroadcastEvent {
    pub(crate) id: u64,
    pub(crate) payload: IncidentEventPayload,
}

pub(crate) type SharedLiveService = Arc<dyn LiveEventService>;
pub(crate) type SharedRetainedFileService = Arc<dyn RetainedFileService>;
pub(crate) type SharedArchiveQueryService = Arc<dyn emwin_service::ArchiveQueryService>;
pub(crate) type SharedIncidentChangeStream = Arc<dyn IncidentChangeStream>;
pub(crate) type SharedAlertStore = Arc<PostgresMetadataSink>;

#[derive(Debug, Clone, Serialize)]
pub struct ApiArchiveStatus {
    pub(crate) configured: bool,
    pub(crate) healthy: bool,
    pub(crate) errors_total: u64,
    pub(crate) pool_timeouts_total: u64,
    pub(crate) last_error: Option<String>,
}

pub(crate) trait ArchiveStatusService: Send + Sync {
    fn archive_status_snapshot(&self) -> ApiArchiveStatus;
}

pub(crate) type SharedArchiveStatusService = Arc<dyn ArchiveStatusService>;

#[derive(Clone)]
pub struct ApiServices {
    pub(crate) live: SharedLiveService,
    pub(crate) retained_files: SharedRetainedFileService,
    pub(crate) archive: SharedArchiveQueryService,
    pub(crate) incident_stream: SharedIncidentChangeStream,
    pub(crate) archive_status: SharedArchiveStatusService,
    pub(crate) alert_store: Option<SharedAlertStore>,
}

struct LiveRuntimeArchiveStatusService {
    runtime: Arc<emwin_live::LiveRuntime>,
}

impl ArchiveStatusService for LiveRuntimeArchiveStatusService {
    fn archive_status_snapshot(&self) -> ApiArchiveStatus {
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
}

impl ApiServices {
    pub fn from_live_runtime(runtime: emwin_live::LiveRuntime) -> Self {
        let alert_store = runtime.alert_store();
        let shared = Arc::new(runtime);
        Self {
            live: shared.clone(),
            retained_files: shared.clone(),
            archive: shared.clone(),
            incident_stream: shared.clone(),
            archive_status: Arc::new(LiveRuntimeArchiveStatusService { runtime: shared }),
            alert_store: alert_store.map(Arc::new),
        }
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<LiveBroadcastEvent> {
        self.live.subscribe_events()
    }

    pub(crate) fn telemetry_snapshot(&self) -> TelemetryPayload {
        self.live.telemetry_snapshot()
    }

    pub(crate) fn stats_snapshot(&self) -> LiveStatsSnapshot {
        self.live.stats_snapshot()
    }

    pub(crate) async fn shutdown(&self) -> emwin_service::ServiceResult<()> {
        self.live.shutdown().await
    }

    pub(crate) fn list_retained_files(&self) -> Vec<CompletedFileMetadata> {
        self.retained_files.list_retained_files()
    }

    pub(crate) fn get_retained_file(&self, filename: &str) -> Option<RetainedFile> {
        self.retained_files.get_retained_file(filename)
    }

    pub(crate) fn subscribe_incident_changes(
        &self,
    ) -> Option<broadcast::Receiver<ServiceIncidentBroadcastEvent>> {
        self.incident_stream.subscribe_incident_changes()
    }

    pub(crate) fn archive_status_snapshot(&self) -> ApiArchiveStatus {
        self.archive_status.archive_status_snapshot()
    }

    pub(crate) fn alert_store(&self) -> Option<&PostgresMetadataSink> {
        self.alert_store.as_deref()
    }
}

pub(crate) struct AppState {
    pub(crate) services: ApiServices,
    pub(crate) event_tx: broadcast::Sender<BroadcastEvent>,
    pub(crate) incident_event_tx: broadcast::Sender<IncidentBroadcastEvent>,
    pub(crate) shutdown_rx: watch::Receiver<bool>,
    pub(crate) connected_clients: AtomicUsize,
    pub(crate) max_clients: usize,
    pub(crate) next_event_id: AtomicU64,
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
