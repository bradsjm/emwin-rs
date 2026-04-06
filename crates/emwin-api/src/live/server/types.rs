//! Shared state and response payloads for live server mode.
//!
//! Keeping these types in one place helps the HTTP layer, ingest loop, and retention code agree
//! on stable payload shapes without circular dependencies.

use crate::cmd::event_output::{frame_event_name, frame_event_to_json};
use crate::live::server_support::file_download_url;
use emwin_db::{
    AggregateCompleteness, ArchiveFilterInput, ArchivedFeature, ArchivedIssue,
    ArchivedProductDetail, ArchivedProductSummary, CellAggregateBucket, CompletedFileMetadata,
    FacetAggregateBucket, IncidentChange, IncidentChangeAction, IncidentChangeTrigger,
    IncidentDetail, IncidentSummary, PaginatedResponse, PersistenceStats,
    TimeseriesAggregateBucket, build_cell_aggregate_query, build_facet_aggregate_query,
    build_feature_list_query, build_timeseries_aggregate_query,
};
use emwin_live::{FileEventFilter, FileFilterInput, LiveRuntime, LiveTelemetry};
use emwin_protocol::qbt_receiver::QbtFrameEvent;
use emwin_protocol::wxwire_receiver::WxWireReceiverFrameEvent;
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

#[derive(Debug, Clone)]
pub(crate) struct MetricsPayload {
    pub(crate) telemetry: TelemetryPayload,
    pub(crate) persistence: Option<PersistenceStats>,
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
        let mut map =
            serializer.serialize_map(Some(telemetry_fields.len() + persistence_field_count))?;
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
        map.end()
    }
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
    QbtFrame(QbtFrameEvent),
    WxWireFrame(WxWireReceiverFrameEvent),
    FileComplete(Box<CompletedFileEventPayload>),
    Telemetry(TelemetryPayload),
    Error { message: String },
}

impl EventKind {
    pub(crate) fn event_name(&self) -> &'static str {
        match self {
            Self::Connected { .. } => "connected",
            Self::Disconnected => "disconnected",
            Self::QbtFrame(frame) => frame_event_name(frame),
            Self::WxWireFrame(frame) => match frame {
                WxWireReceiverFrameEvent::File(_) => "file",
                WxWireReceiverFrameEvent::Warning(_) => "warning",
                _ => "unknown",
            },
            Self::FileComplete(_) => "product_available",
            Self::Telemetry(_) => "telemetry",
            Self::Error { .. } => "error",
        }
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Connected { endpoint } => serde_json::json!({ "endpoint": endpoint }),
            Self::Disconnected => serde_json::json!({}),
            Self::QbtFrame(frame) => frame_event_to_json(frame, 0),
            Self::WxWireFrame(frame) => match frame {
                WxWireReceiverFrameEvent::File(file) => serde_json::json!({
                    "type": "file",
                    "filename": file.filename,
                    "length": file.data.len(),
                    "subject": file.subject,
                    "id": file.id,
                    "issue_utc": unix_seconds(file.issue_utc),
                    "ttaaii": file.ttaaii,
                    "cccc": file.cccc,
                    "awipsid": file.awipsid,
                }),
                WxWireReceiverFrameEvent::Warning(warning) => serde_json::json!({
                    "type": "warning",
                    "warning": format!("{warning:?}"),
                }),
                _ => serde_json::json!({
                    "type": "unknown",
                }),
            },
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
    pub(crate) live: LiveRuntime,
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

#[derive(Debug, Deserialize, IntoParams, ToSchema, Clone, Default)]
pub(crate) struct ArchiveFilterParams {
    pub(crate) filename: Option<String>,
    pub(crate) source_receiver: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) pil: Option<String>,
    pub(crate) family: Option<String>,
    pub(crate) artifact_kind: Option<String>,
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
    pub(crate) source_timestamp_after: Option<i64>,
    pub(crate) source_timestamp_before: Option<i64>,
    pub(crate) ingested_after: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) ingested_before: Option<chrono::DateTime<chrono::Utc>>,
}

impl ArchiveFilterParams {
    pub(crate) fn into_product_list_query(
        self,
        default_limit: usize,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<emwin_db::ProductListQuery, String> {
        ArchiveFilterInput::from(self)
            .into_product_list_query(default_limit, limit, cursor)
            .map_err(|err| err.to_string())
    }
}

impl From<ArchiveFilterParams> for ArchiveFilterInput {
    fn from(value: ArchiveFilterParams) -> Self {
        Self {
            filename: value.filename,
            source_receiver: value.source_receiver,
            source: value.source,
            pil: value.pil,
            family: value.family,
            artifact_kind: value.artifact_kind,
            container: value.container,
            wmo_prefix: value.wmo_prefix,
            office: value.office,
            office_city: value.office_city,
            office_state: value.office_state,
            bbb_kind: value.bbb_kind,
            cccc: value.cccc,
            ttaaii: value.ttaaii,
            afos: value.afos,
            bbb: value.bbb,
            has_issues: value.has_issues,
            issue_kind: value.issue_kind,
            issue_code: value.issue_code,
            has_vtec: value.has_vtec,
            has_ugc: value.has_ugc,
            has_hvtec: value.has_hvtec,
            has_latlon: value.has_latlon,
            has_time_mot_loc: value.has_time_mot_loc,
            has_wind_hail: value.has_wind_hail,
            state: value.state,
            county: value.county,
            zone: value.zone,
            fire_zone: value.fire_zone,
            marine_zone: value.marine_zone,
            vtec_phenomena: value.vtec_phenomena,
            vtec_significance: value.vtec_significance,
            vtec_action: value.vtec_action,
            vtec_office: value.vtec_office,
            etn: value.etn,
            hvtec_nwslid: value.hvtec_nwslid,
            hvtec_severity: value.hvtec_severity,
            hvtec_cause: value.hvtec_cause,
            hvtec_record: value.hvtec_record,
            wind_hail_kind: value.wind_hail_kind,
            lat: value.lat,
            lon: value.lon,
            distance_miles: value.distance_miles,
            min_lat: value.min_lat,
            max_lat: value.max_lat,
            min_lon: value.min_lon,
            max_lon: value.max_lon,
            min_wind_mph: value.min_wind_mph,
            min_hail_inches: value.min_hail_inches,
            min_size: value.min_size,
            max_size: value.max_size,
            source_timestamp_after: value.source_timestamp_after,
            source_timestamp_before: value.source_timestamp_before,
            ingested_after: value.ingested_after,
            ingested_before: value.ingested_before,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct ProductsQuery {
    #[serde(flatten)]
    #[param(inline)]
    pub(crate) filters: ArchiveFilterParams,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

impl ProductsQuery {
    pub(crate) fn into_product_list_query(self) -> Result<emwin_db::ProductListQuery, String> {
        self.filters
            .into_product_list_query(100, self.limit, self.cursor)
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FeaturesQuery {
    #[serde(flatten)]
    #[param(inline)]
    pub(crate) filters: ArchiveFilterParams,
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

impl FeaturesQuery {
    pub(crate) fn into_feature_list_query(self) -> Result<emwin_db::FeatureListQuery, String> {
        build_feature_list_query(self.filters.into(), self.kind, 100, self.limit, self.cursor)
            .map_err(|err| err.to_string())
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FeaturesGeoJsonQuery {
    #[serde(flatten)]
    #[param(inline)]
    pub(crate) filters: ArchiveFilterParams,
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
}

impl FeaturesGeoJsonQuery {
    pub(crate) fn into_feature_list_query(self) -> Result<emwin_db::FeatureListQuery, String> {
        build_feature_list_query(self.filters.into(), self.kind, 100, self.limit, None)
            .map_err(|err| err.to_string())
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FacetAggregateHttpQuery {
    #[serde(flatten)]
    #[param(inline)]
    pub(crate) filters: ArchiveFilterParams,
    pub(crate) dimension: String,
    pub(crate) limit: Option<usize>,
}

impl FacetAggregateHttpQuery {
    pub(crate) fn into_facet_aggregate_query(
        self,
    ) -> Result<emwin_db::FacetAggregateQuery, String> {
        build_facet_aggregate_query(self.filters.into(), &self.dimension, self.limit)
            .map_err(|err| err.to_string())
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct TimeseriesAggregateHttpQuery {
    #[serde(flatten)]
    #[param(inline)]
    pub(crate) filters: ArchiveFilterParams,
    pub(crate) measure: String,
    pub(crate) start: chrono::DateTime<chrono::Utc>,
    pub(crate) end: chrono::DateTime<chrono::Utc>,
    pub(crate) bucket: String,
}

impl TimeseriesAggregateHttpQuery {
    pub(crate) fn into_timeseries_aggregate_query(
        self,
    ) -> Result<emwin_db::TimeseriesAggregateQuery, String> {
        build_timeseries_aggregate_query(
            self.filters.into(),
            &self.measure,
            self.start,
            self.end,
            &self.bucket,
        )
        .map_err(|err| err.to_string())
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct CellAggregateHttpQuery {
    #[serde(flatten)]
    #[param(inline)]
    pub(crate) filters: ArchiveFilterParams,
    pub(crate) measure: String,
    pub(crate) precision: u8,
    pub(crate) limit: Option<usize>,
}

impl CellAggregateHttpQuery {
    pub(crate) fn into_cell_aggregate_query(self) -> Result<emwin_db::CellAggregateQuery, String> {
        build_cell_aggregate_query(
            self.filters.into(),
            &self.measure,
            self.precision,
            self.limit,
        )
        .map_err(|err| err.to_string())
    }
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
    pub(crate) connected_clients: usize,
    pub(crate) retained_files: usize,
    pub(crate) uptime_secs: u64,
    pub(crate) upstream_endpoint: Option<String>,
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

fn unix_seconds(time: std::time::SystemTime) -> u64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
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
