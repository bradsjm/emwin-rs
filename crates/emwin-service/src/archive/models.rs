use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// Stable paginated response envelope used by archive query APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// Change action emitted when the incident projection mutates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncidentChangeAction {
    Created,
    Updated,
}

/// Source that triggered an incident change notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncidentChangeTrigger {
    Persist,
    Cleanup,
}

/// Result returned after incident cleanup expires active rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentCleanupResult {
    pub expired_count: u64,
}

/// Incident change event delivered to subscribers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct IncidentChange {
    pub action: IncidentChangeAction,
    pub trigger: IncidentChangeTrigger,
    pub incident: IncidentSummary,
}

/// Summary row for one active or historical incident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct IncidentSummary {
    pub office: String,
    pub phenomena: String,
    pub significance: String,
    pub etn: i64,
    pub current_status: String,
    pub latest_vtec_action: String,
    pub issued_at: DateTime<Utc>,
    pub start_utc: Option<DateTime<Utc>>,
    pub end_utc: Option<DateTime<Utc>>,
    pub last_updated_at: DateTime<Utc>,
    pub first_product_id: i64,
    pub latest_product_id: i64,
    pub latest_product_timestamp_utc: DateTime<Utc>,
}

pub type IncidentDetail = IncidentSummary;

/// Archive summary row for one persisted product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ArchivedProductSummary {
    pub product_id: i64,
    pub filename: String,
    pub source_timestamp_utc: i64,
    pub ingested_at: DateTime<Utc>,
    pub source_receiver: String,
    pub source_message_id: Option<String>,
    pub size_bytes: i64,
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

/// Archive detail row for one persisted product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ArchivedProductDetail {
    #[serde(flatten)]
    pub summary: ArchivedProductSummary,
    pub payload_location: Option<String>,
    pub metadata_location: Option<String>,
    #[schema(value_type = Object)]
    pub product_json: Value,
}

/// Raw payload bytes returned from archive storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedPayload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

/// Persisted parse/QC issue record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ArchivedIssue {
    pub id: i64,
    pub product_id: i64,
    pub kind: String,
    pub code: String,
    pub message: String,
    pub line: Option<String>,
}

/// Archive spatial feature record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ArchivedFeature {
    pub feature_id: String,
    pub feature_kind: super::query::FeatureKind,
    pub product_id: i64,
    pub source_timestamp_utc: i64,
    pub geometry: Value,
    pub properties: Value,
}

/// One bucket in a facet aggregate response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FacetAggregateBucket {
    pub value: String,
    pub count: i64,
}

/// One bucket in a timeseries aggregate response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TimeseriesAggregateBucket {
    pub bucket_start: DateTime<Utc>,
    pub bucket_end: DateTime<Utc>,
    pub count: i64,
}

/// One bucket in a cell aggregate response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CellAggregateBucket {
    pub cell: String,
    pub count: i64,
}

/// Aggregate completeness metadata shared across aggregate endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AggregateCompleteness {
    pub partial: bool,
    pub approximate: bool,
    pub reason: Option<String>,
}

impl AggregateCompleteness {
    pub const fn exact() -> Self {
        Self {
            partial: false,
            approximate: false,
            reason: None,
        }
    }
}

/// Facet aggregate response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FacetAggregateResult {
    pub completeness: AggregateCompleteness,
    pub items: Vec<FacetAggregateBucket>,
}

/// Timeseries aggregate response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TimeseriesAggregateResult {
    pub completeness: AggregateCompleteness,
    pub items: Vec<TimeseriesAggregateBucket>,
}

/// Cell aggregate response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CellAggregateResult {
    pub completeness: AggregateCompleteness,
    pub items: Vec<CellAggregateBucket>,
}
