//! Shared state and response payloads for live server mode.
//!
//! Keeping these types in one place helps the HTTP layer, ingest loop, and retention code agree
//! on stable payload shapes without circular dependencies.

use crate::server_support::file_download_url;
use emwin_live::{FileEventFilter, FileFilterInput};
use emwin_service::{
    AggregateCompleteness, ArchiveFilterInput, ArchivedFeature, ArchivedIssue,
    ArchivedProductDetail, ArchivedProductSummary, CellAggregateBucket, CompletedFileMetadata,
    FacetAggregateBucket, IncidentBroadcastEvent as ServiceIncidentBroadcastEvent, IncidentChange,
    IncidentChangeAction, IncidentChangeStream, IncidentChangeTrigger, IncidentDetail,
    IncidentSummary, LiveBroadcastEvent, LiveEventService, LiveStatsSnapshot, LiveTelemetry,
    PaginatedResponse, PersistenceStats, ReceiverFrame, RetainedFile, RetainedFileService,
    TimeseriesAggregateBucket,
};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::{broadcast, watch};
use utoipa::{IntoParams, ToSchema};

pub(crate) const API_PREFIX: &str = "/v1";
pub(crate) const OPENAPI_JSON_PATH: &str = "/openapi.json";
pub(crate) const OPENAPI_AUTH_SCHEME_NAME: &str = "bearer_auth";

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

/// Downloadable file payload advertised by the HTTP API.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CompletedFilePayload {
    #[serde(flatten)]
    pub(crate) metadata: CompletedFileMetadata,
    pub(crate) download_url: String,
}

impl CompletedFilePayload {
    pub(crate) fn from_metadata(metadata: CompletedFileMetadata) -> Self {
        let download_url = file_download_url(&metadata.filename);
        Self {
            metadata,
            download_url,
        }
    }
}

/// Lightweight file payload advertised in the SSE event stream.
#[derive(Debug, Clone)]
pub(crate) struct CompletedFileEventPayload {
    pub(crate) metadata: CompletedFileMetadata,
    pub(crate) download_url: String,
}

impl CompletedFileEventPayload {
    pub(crate) fn from_metadata(metadata: CompletedFileMetadata) -> Self {
        Self {
            download_url: file_download_url(&metadata.filename),
            metadata,
        }
    }
}

impl Serialize for CompletedFileEventPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("CompletedFileEventPayload", 5)?;
        state.serialize_field("filename", &self.metadata.filename)?;
        state.serialize_field("size", &self.metadata.size)?;
        state.serialize_field("timestamp_utc", &self.metadata.timestamp_utc)?;
        state.serialize_field("product", &self.metadata.product_summary)?;
        state.serialize_field("download_url", &self.download_url)?;
        state.end()
    }
}

pub(crate) type TelemetryPayload = LiveTelemetry;

pub(crate) type SharedLiveService = Arc<dyn LiveEventService>;
pub(crate) type SharedRetainedFileService = Arc<dyn RetainedFileService>;
pub(crate) type SharedArchiveQueryService = Arc<dyn emwin_service::ArchiveQueryService>;
pub(crate) type SharedIncidentChangeStream = Arc<dyn IncidentChangeStream>;

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
        let shared = Arc::new(runtime);
        Self {
            live: shared.clone(),
            retained_files: shared.clone(),
            archive: shared.clone(),
            incident_stream: shared.clone(),
            archive_status: Arc::new(LiveRuntimeArchiveStatusService { runtime: shared }),
        }
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<LiveBroadcastEvent> {
        self.live.subscribe_events()
    }

    pub(crate) fn telemetry_snapshot(&self) -> LiveTelemetry {
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
}

#[derive(Debug, Clone)]
pub(crate) struct MetricsPayload {
    pub(crate) telemetry: TelemetryPayload,
    pub(crate) persistence: Option<PersistenceStats>,
    pub(crate) archive: ArchiveStatus,
}

impl Serialize for MetricsPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let telemetry = serde_json::to_value(&self.telemetry).map_err(serde::ser::Error::custom)?;
        let Some(telemetry_fields) = telemetry.as_object() else {
            return Err(serde::ser::Error::custom(
                "telemetry payload must serialize as an object",
            ));
        };

        let persistence_field_count = usize::from(self.persistence.is_some()) * 6;
        let archive_field_count = 4 + usize::from(self.archive.last_error.is_some());
        let mut map = serializer.serialize_map(Some(
            telemetry_fields.len() + persistence_field_count + archive_field_count,
        ))?;
        for (key, value) in telemetry_fields {
            map.serialize_entry(key, value)?;
        }
        if let Some(persistence) = self.persistence {
            map.serialize_entry("persistence_queue_len", &persistence.queue_len)?;
            map.serialize_entry("persistence_queue_capacity", &persistence.queue_capacity)?;
            map.serialize_entry("persistence_enqueued_total", &persistence.enqueued_total)?;
            map.serialize_entry("persistence_evicted_total", &persistence.evicted_total)?;
            map.serialize_entry("persistence_persisted_total", &persistence.persisted_total)?;
            map.serialize_entry("persistence_failed_total", &persistence.failed_total)?;
        }
        map.serialize_entry("archive_configured", &self.archive.configured)?;
        map.serialize_entry("archive_healthy", &self.archive.healthy)?;
        map.serialize_entry("archive_errors_total", &self.archive.errors_total)?;
        map.serialize_entry(
            "archive_pool_timeouts_total",
            &self.archive.pool_timeouts_total,
        )?;
        if let Some(last_error) = &self.archive.last_error {
            map.serialize_entry("archive_last_error", last_error)?;
        }
        map.end()
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ArchiveStatus {
    pub(crate) configured: bool,
    pub(crate) healthy: bool,
    pub(crate) errors_total: u64,
    pub(crate) pool_timeouts_total: u64,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct IncidentEventPayload {
    pub(crate) action: IncidentChangeAction,
    pub(crate) trigger: IncidentChangeTrigger,
    pub(crate) incident: IncidentSummaryPayload,
}

impl IncidentEventPayload {
    pub(crate) const EVENT_NAME: &'static str = "incident_change";

    pub(crate) fn from_change(change: IncidentChange) -> Self {
        Self {
            action: change.action,
            trigger: change.trigger,
            incident: IncidentSummaryPayload::from_incident(change.incident),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum EventKind {
    Connected { endpoint: String },
    Disconnected,
    ReceiverFrame(ReceiverFrame),
    FileComplete(Box<CompletedFileEventPayload>),
    Telemetry(TelemetryPayload),
    Error { message: String },
}

impl EventKind {
    pub(crate) fn event_name(&self) -> &str {
        match self {
            Self::Connected { .. } => "connected",
            Self::Disconnected => "disconnected",
            Self::ReceiverFrame(frame) => frame.event_name.as_str(),
            Self::FileComplete(_) => "product_available",
            Self::Telemetry(_) => "telemetry",
            Self::Error { .. } => "error",
        }
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Connected { endpoint } => serde_json::json!({ "endpoint": endpoint }),
            Self::Disconnected => serde_json::json!({}),
            Self::ReceiverFrame(frame) => frame.payload.clone(),
            Self::FileComplete(file) => {
                serde_json::to_value(file).unwrap_or_else(|_| serde_json::json!({}))
            }
            Self::Telemetry(snapshot) => {
                serde_json::to_value(snapshot).unwrap_or_else(|_| serde_json::json!({}))
            }
            Self::Error { message } => serde_json::json!({ "message": message }),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EventFilter {
    pub(crate) event_names: Option<BTreeSet<String>>,
    pub(crate) file: FileEventFilter,
}

impl EventFilter {
    #[cfg(test)]
    pub(crate) fn from_query(query: EventsQuery) -> Self {
        Self::try_from_query(query).expect("query should compile")
    }

    pub(crate) fn try_from_query(query: EventsQuery) -> Result<Self, EventFilterQueryError> {
        let event_names = csv_values(query.event.as_deref(), normalize_lower);
        let file_input = FileFilterInput::from(query);
        let file =
            FileEventFilter::try_from_input(&file_input).map_err(|err| EventFilterQueryError {
                message: err.message,
            })?;

        Ok(Self { event_names, file })
    }

    pub(crate) fn matches(&self, event: &EventKind) -> bool {
        if let Some(event_names) = &self.event_names {
            let event_name = normalize_lower(event.event_name());
            if !event_names.contains(&event_name) {
                return false;
            }
        }

        if !self.file.has_constraints() {
            return true;
        }

        match event {
            EventKind::FileComplete(file) => self.file.matches_metadata(&file.metadata),
            _ => false,
        }
    }
}

fn csv_values(raw: Option<&str>, normalize: fn(&str) -> String) -> Option<BTreeSet<String>> {
    let values = raw
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize)
        .collect::<BTreeSet<_>>();

    (!values.is_empty()).then_some(values)
}

fn normalize_lower(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_upper(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventFilterQueryError {
    pub(crate) message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IncidentEventFilter {
    pub(crate) actions: Option<BTreeSet<String>>,
    pub(crate) offices: Option<BTreeSet<String>>,
    pub(crate) phenomena: Option<BTreeSet<String>>,
    pub(crate) significance: Option<BTreeSet<String>>,
    pub(crate) statuses: Option<BTreeSet<String>>,
    pub(crate) etns: Option<BTreeSet<i64>>,
}

impl IncidentEventFilter {
    pub(crate) fn from_query(query: IncidentEventsQuery) -> Self {
        Self {
            actions: csv_values(query.action.as_deref(), normalize_lower),
            offices: csv_values(query.office.as_deref(), normalize_upper),
            phenomena: csv_values(query.phenomena.as_deref(), normalize_upper),
            significance: csv_values(query.significance.as_deref(), normalize_upper),
            statuses: csv_values(query.status.as_deref(), normalize_lower),
            etns: csv_i64_values(query.etn.as_deref()),
        }
    }

    pub(crate) fn matches(&self, event: &IncidentEventPayload) -> bool {
        if let Some(actions) = &self.actions
            && !actions
                .contains(normalize_lower(incident_change_action_name(event.action)).as_str())
        {
            return false;
        }
        if let Some(offices) = &self.offices
            && !offices.contains(event.incident.incident.office.as_str())
        {
            return false;
        }
        if let Some(phenomena) = &self.phenomena
            && !phenomena.contains(event.incident.incident.phenomena.as_str())
        {
            return false;
        }
        if let Some(significance) = &self.significance
            && !significance.contains(event.incident.incident.significance.as_str())
        {
            return false;
        }
        if let Some(statuses) = &self.statuses
            && !statuses.contains(event.incident.incident.current_status.as_str())
        {
            return false;
        }
        if let Some(etns) = &self.etns
            && !etns.contains(&event.incident.incident.etn)
        {
            return false;
        }
        true
    }
}

fn incident_change_action_name(action: IncidentChangeAction) -> &'static str {
    match action {
        IncidentChangeAction::Created => "created",
        IncidentChangeAction::Updated => "updated",
    }
}

fn csv_i64_values(raw: Option<&str>) -> Option<BTreeSet<i64>> {
    let values = raw
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| value.parse::<i64>().ok())
        .collect::<BTreeSet<_>>();
    (!values.is_empty()).then_some(values)
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
    pub(crate) quiet: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct IncidentsQuery {
    pub(crate) office: Option<String>,
    pub(crate) phenomena: Option<String>,
    pub(crate) significance: Option<String>,
    pub(crate) etn: Option<i64>,
    pub(crate) status: Option<String>,
    pub(crate) updated_after: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) updated_before: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) active_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

macro_rules! define_archive_filter_params {
    (@fields [$($fields:tt)*]) => {
        #[derive(Debug, Deserialize, IntoParams, ToSchema, Clone, Default)]
        pub(crate) struct ArchiveFilterParams {
            $($fields)*
        }
    };
    (@fields [$($fields:tt)*] $field:ident, string; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, bool_string; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, f64; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* #[schema(value_type = Option<f64>)] pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, usize; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* #[schema(value_type = Option<usize>)] pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, i64; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* #[schema(value_type = Option<i64>)] pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, datetime_utc; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* #[schema(value_type = Option<String>, format = DateTime)] pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    ($( $rows:tt )*) => {
        define_archive_filter_params!(@fields [] $( $rows )*);
    };
}

macro_rules! archive_filter_input_value_from_param {
    ($value:ident, $field:ident, string) => {
        $value.$field
    };
    ($value:ident, $field:ident, bool_string) => {
        $value.$field
    };
    ($value:ident, $field:ident, f64) => {
        parse_query_value(stringify!($field), $value.$field)?
    };
    ($value:ident, $field:ident, usize) => {
        parse_query_value(stringify!($field), $value.$field)?
    };
    ($value:ident, $field:ident, i64) => {
        parse_query_value(stringify!($field), $value.$field)?
    };
    ($value:ident, $field:ident, datetime_utc) => {
        parse_datetime_value(stringify!($field), $value.$field)?
    };
}

macro_rules! build_archive_filter_input_from_params {
    ($value:ident; $( $field:ident, $kind:ident; )*) => {
        ArchiveFilterInput {
            $($field: archive_filter_input_value_from_param!($value, $field, $kind),)*
        }
    };
}

emwin_service::emwin_archive_filter_fields!(define_archive_filter_params);

impl ArchiveFilterParams {
    pub(crate) fn into_archive_filter_input(self) -> Result<ArchiveFilterInput, String> {
        Ok(emwin_service::emwin_archive_filter_fields!(
            build_archive_filter_input_from_params,
            self
        ))
    }

    pub(crate) fn into_product_list_query(
        self,
        default_limit: usize,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<emwin_service::ProductListQuery, String> {
        self.into_archive_filter_input()?
            .into_product_list_query(default_limit, limit, cursor)
            .map_err(|err| err.to_string())
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct ProductsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FeaturesQuery {
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FeaturesGeoJsonQuery {
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FacetAggregateHttpQuery {
    pub(crate) dimension: String,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct TimeseriesAggregateHttpQuery {
    pub(crate) measure: String,
    pub(crate) start: chrono::DateTime<chrono::Utc>,
    pub(crate) end: chrono::DateTime<chrono::Utc>,
    pub(crate) bucket: String,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct CellAggregateHttpQuery {
    pub(crate) measure: String,
    pub(crate) precision: u8,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct IncidentProductsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct IncidentEventsQuery {
    pub(crate) action: Option<String>,
    pub(crate) office: Option<String>,
    pub(crate) phenomena: Option<String>,
    pub(crate) significance: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) etn: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct EventsQuery {
    pub(crate) event: Option<String>,
    pub(crate) filename: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) pil: Option<String>,
    pub(crate) family: Option<String>,
    pub(crate) container: Option<String>,
    pub(crate) wmo_prefix: Option<String>,
    pub(crate) office: Option<String>,
    pub(crate) office_city: Option<String>,
    pub(crate) office_state: Option<String>,
    pub(crate) bbb_kind: Option<String>,
    pub(crate) cccc: Option<String>,
    pub(crate) ttaaii: Option<String>,
    pub(crate) afos: Option<String>,
    pub(crate) bbb: Option<String>,
    pub(crate) has_issues: Option<String>,
    pub(crate) issue_kind: Option<String>,
    pub(crate) issue_code: Option<String>,
    pub(crate) has_vtec: Option<String>,
    pub(crate) has_ugc: Option<String>,
    pub(crate) has_hvtec: Option<String>,
    pub(crate) has_latlon: Option<String>,
    pub(crate) has_time_mot_loc: Option<String>,
    pub(crate) has_wind_hail: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) county: Option<String>,
    pub(crate) zone: Option<String>,
    pub(crate) fire_zone: Option<String>,
    pub(crate) marine_zone: Option<String>,
    pub(crate) vtec_phenomena: Option<String>,
    pub(crate) vtec_significance: Option<String>,
    pub(crate) vtec_action: Option<String>,
    pub(crate) vtec_office: Option<String>,
    pub(crate) etn: Option<String>,
    pub(crate) hvtec_nwslid: Option<String>,
    pub(crate) hvtec_severity: Option<String>,
    pub(crate) hvtec_cause: Option<String>,
    pub(crate) hvtec_record: Option<String>,
    pub(crate) wind_hail_kind: Option<String>,
    pub(crate) lat: Option<f64>,
    pub(crate) lon: Option<f64>,
    pub(crate) distance_miles: Option<f64>,
    pub(crate) min_lat: Option<f64>,
    pub(crate) max_lat: Option<f64>,
    pub(crate) min_lon: Option<f64>,
    pub(crate) max_lon: Option<f64>,
    pub(crate) min_wind_mph: Option<f64>,
    pub(crate) min_hail_inches: Option<f64>,
    pub(crate) min_size: Option<usize>,
    pub(crate) max_size: Option<usize>,
}

impl From<EventsQuery> for FileFilterInput {
    fn from(query: EventsQuery) -> Self {
        Self {
            filename: query.filename,
            source: query.source,
            pil: query.pil,
            family: query.family,
            container: query.container,
            wmo_prefix: query.wmo_prefix,
            office: query.office,
            office_city: query.office_city,
            office_state: query.office_state,
            bbb_kind: query.bbb_kind,
            cccc: query.cccc,
            ttaaii: query.ttaaii,
            afos: query.afos,
            bbb: query.bbb,
            has_issues: query.has_issues,
            issue_kind: query.issue_kind,
            issue_code: query.issue_code,
            has_vtec: query.has_vtec,
            has_ugc: query.has_ugc,
            has_hvtec: query.has_hvtec,
            has_latlon: query.has_latlon,
            has_time_mot_loc: query.has_time_mot_loc,
            has_wind_hail: query.has_wind_hail,
            state: query.state,
            county: query.county,
            zone: query.zone,
            fire_zone: query.fire_zone,
            marine_zone: query.marine_zone,
            vtec_phenomena: query.vtec_phenomena,
            vtec_significance: query.vtec_significance,
            vtec_action: query.vtec_action,
            vtec_office: query.vtec_office,
            etn: query.etn,
            hvtec_nwslid: query.hvtec_nwslid,
            hvtec_severity: query.hvtec_severity,
            hvtec_cause: query.hvtec_cause,
            hvtec_record: query.hvtec_record,
            wind_hail_kind: query.wind_hail_kind,
            lat: query.lat,
            lon: query.lon,
            distance_miles: query.distance_miles,
            min_lat: query.min_lat,
            max_lat: query.max_lat,
            min_lon: query.min_lon,
            max_lon: query.max_lon,
            min_wind_mph: query.min_wind_mph,
            min_hail_inches: query.min_hail_inches,
            min_size: query.min_size,
            max_size: query.max_size,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct FilesResponse {
    pub(crate) files: Vec<CompletedFilePayload>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct IncidentSummaryPayload {
    #[serde(flatten)]
    pub(crate) incident: IncidentSummary,
    pub(crate) detail_url: String,
    pub(crate) products_url: String,
    pub(crate) latest_product_url: String,
}

impl IncidentSummaryPayload {
    pub(crate) fn from_incident(incident: IncidentSummary) -> Self {
        let detail_url = incident_detail_url(&incident);
        let products_url = incident_products_url(&incident);
        let latest_product_url = archive_product_url(incident.latest_product_id);
        Self {
            incident,
            detail_url,
            products_url,
            latest_product_url,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct IncidentDetailPayload {
    #[serde(flatten)]
    pub(crate) incident: IncidentDetail,
    pub(crate) products_url: String,
    pub(crate) first_product_url: String,
    pub(crate) latest_product_url: String,
}

impl IncidentDetailPayload {
    pub(crate) fn from_incident(incident: IncidentDetail) -> Self {
        let products_url = incident_products_url(&incident);
        let first_product_url = archive_product_url(incident.first_product_id);
        let latest_product_url = archive_product_url(incident.latest_product_id);
        Self {
            incident,
            products_url,
            first_product_url,
            latest_product_url,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveProductSummaryPayload {
    #[serde(flatten)]
    pub(crate) product: ArchivedProductSummary,
    pub(crate) detail_url: String,
    pub(crate) raw_url: String,
}

impl ArchiveProductSummaryPayload {
    pub(crate) fn from_product(product: ArchivedProductSummary) -> Self {
        let detail_url = archive_product_url(product.product_id);
        let raw_url = archive_product_raw_url(product.product_id);
        Self {
            product,
            detail_url,
            raw_url,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveProductDetailPayload {
    #[serde(flatten)]
    pub(crate) product: ArchivedProductDetail,
    pub(crate) raw_url: String,
}

impl ArchiveProductDetailPayload {
    pub(crate) fn from_product(product: ArchivedProductDetail) -> Self {
        let raw_url = archive_product_raw_url(product.summary.product_id);
        Self { product, raw_url }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveIssuePayload {
    #[serde(flatten)]
    pub(crate) issue: ArchivedIssue,
    pub(crate) detail_url: String,
    pub(crate) product_url: String,
}

impl ArchiveIssuePayload {
    pub(crate) fn from_issue(issue: ArchivedIssue) -> Self {
        let detail_url = archive_issue_url(issue.id);
        let product_url = archive_product_url(issue.product_id);
        Self {
            issue,
            detail_url,
            product_url,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ArchivedFeaturePayload {
    #[serde(flatten)]
    pub(crate) feature: ArchivedFeature,
    pub(crate) product_url: String,
    pub(crate) product_raw_url: String,
}

impl ArchivedFeaturePayload {
    pub(crate) fn from_feature(feature: ArchivedFeature) -> Self {
        let product_url = archive_product_url(feature.product_id);
        let product_raw_url = archive_product_raw_url(feature.product_id);
        Self {
            feature,
            product_url,
            product_raw_url,
        }
    }

    pub(crate) fn into_geojson_feature(self) -> GeoJsonFeature {
        let mut properties = match self.feature.properties {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        properties.insert(
            "feature_kind".to_string(),
            serde_json::json!(self.feature.feature_kind),
        );
        properties.insert(
            "product_id".to_string(),
            serde_json::json!(self.feature.product_id),
        );
        properties.insert(
            "source_timestamp_utc".to_string(),
            serde_json::json!(self.feature.source_timestamp_utc),
        );
        properties.insert(
            "product_url".to_string(),
            serde_json::json!(self.product_url),
        );
        properties.insert(
            "product_raw_url".to_string(),
            serde_json::json!(self.product_raw_url),
        );

        GeoJsonFeature::new(
            self.feature.feature_id,
            self.feature.geometry,
            serde_json::Value::Object(properties),
        )
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct IncidentsResponse {
    #[serde(flatten)]
    pub(crate) page: PaginatedResponse<IncidentSummaryPayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProductsResponse {
    #[serde(flatten)]
    pub(crate) page: PaginatedResponse<ArchiveProductSummaryPayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FeaturesResponse {
    #[serde(flatten)]
    pub(crate) page: PaginatedResponse<ArchivedFeaturePayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GeoJsonFeature {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) kind: &'static str,
    pub(crate) geometry: serde_json::Value,
    pub(crate) properties: serde_json::Value,
}

impl GeoJsonFeature {
    pub(crate) fn new(
        id: String,
        geometry: serde_json::Value,
        properties: serde_json::Value,
    ) -> Self {
        Self {
            id,
            kind: "Feature",
            geometry,
            properties,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct FeatureCollectionResponse {
    #[serde(rename = "type")]
    pub(crate) kind: &'static str,
    pub(crate) features: Vec<GeoJsonFeature>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FacetAggregateResponse {
    pub(crate) dimension: String,
    #[serde(flatten)]
    pub(crate) completeness: AggregateCompleteness,
    pub(crate) items: Vec<FacetAggregateBucket>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TimeseriesAggregateResponse {
    pub(crate) measure: String,
    pub(crate) bucket: String,
    pub(crate) start: chrono::DateTime<chrono::Utc>,
    pub(crate) end: chrono::DateTime<chrono::Utc>,
    #[serde(flatten)]
    pub(crate) completeness: AggregateCompleteness,
    pub(crate) items: Vec<TimeseriesAggregateBucket>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CellAggregateResponse {
    pub(crate) measure: String,
    pub(crate) precision: u8,
    #[serde(flatten)]
    pub(crate) completeness: AggregateCompleteness,
    pub(crate) items: Vec<CellAggregateBucket>,
}

#[derive(Debug, Serialize)]
pub(crate) struct IncidentProductsResponse {
    #[serde(flatten)]
    pub(crate) page: PaginatedResponse<ArchiveProductSummaryPayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct IncidentResponse {
    pub(crate) incident: IncidentDetailPayload,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveProductResponse {
    pub(crate) product: ArchiveProductDetailPayload,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveIssuesResponse {
    #[serde(flatten)]
    pub(crate) page: PaginatedResponse<ArchiveIssuePayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveIssueResponse {
    pub(crate) issue: ArchiveIssuePayload,
}

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub(crate) status: &'static str,
    pub(crate) archive: ArchiveStatus,
    pub(crate) connected_clients: usize,
    pub(crate) retained_files: usize,
    pub(crate) uptime_secs: u64,
    pub(crate) upstream_endpoint: Option<String>,
}

fn parse_query_value<T>(name: &'static str, raw: Option<String>) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value
            .parse::<T>()
            .map(Some)
            .map_err(|err| format!("invalid `{name}` query parameter `{value}`: {err}")),
        None => Ok(None),
    }
}

fn parse_datetime_value(
    name: &'static str,
    raw: Option<String>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    match raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => chrono::DateTime::parse_from_rfc3339(value)
            .map(|timestamp| Some(timestamp.with_timezone(&chrono::Utc)))
            .map_err(|err| format!("invalid `{name}` query parameter `{value}`: {err}")),
        None => Ok(None),
    }
}

pub(crate) fn incident_detail_url(incident: &IncidentSummary) -> String {
    format!(
        "{API_PREFIX}/incidents/{}/{}/{}/{}",
        incident.office, incident.phenomena, incident.significance, incident.etn
    )
}

pub(crate) fn incident_products_url(incident: &IncidentSummary) -> String {
    format!(
        "{API_PREFIX}/incidents/{}/{}/{}/{}/products",
        incident.office, incident.phenomena, incident.significance, incident.etn
    )
}

pub(crate) fn archive_product_url(product_id: i64) -> String {
    format!("{API_PREFIX}/products/{product_id}")
}

pub(crate) fn archive_product_raw_url(product_id: i64) -> String {
    format!("{API_PREFIX}/products/{product_id}/raw")
}

pub(crate) fn archive_issue_url(issue_id: i64) -> String {
    format!("{API_PREFIX}/issues/{issue_id}")
}

#[derive(Debug, Clone)]
pub struct HttpServerOptions {
    pub bind: String,
    pub cors_origin: Option<String>,
    pub max_clients: usize,
    pub stats_interval_secs: u64,
    pub quiet: bool,
    pub openapi_auth_token: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct ArchiveIssuesQuery {
    pub(crate) product_id: Option<i64>,
    pub(crate) kind: Option<String>,
    pub(crate) code: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

pub(crate) struct ClientGuard {
    pub(crate) state: Arc<AppState>,
    pub(crate) peer: SocketAddr,
}

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.state.connected_clients.fetch_sub(1, Ordering::Relaxed);
        super::log_info(
            self.state.quiet,
            &format!("sse client disconnected peer={}", self.peer),
        );
    }
}

#[cfg(test)]
mod tests;
