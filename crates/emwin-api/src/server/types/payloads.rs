use super::urls::{
    archive_issue_url, archive_product_raw_url, archive_product_url, incident_detail_url,
    incident_products_url,
};
use crate::server_support::file_download_url;
use emwin_service::{
    AggregateCompleteness, ArchivedFeature, ArchivedIssue, ArchivedProductDetail,
    ArchivedProductSummary, CellAggregateBucket, CompletedFileMetadata, FacetAggregateBucket,
    IncidentChange, IncidentChangeAction, IncidentChangeTrigger, IncidentDetail, IncidentSummary,
    LiveTelemetry, PaginatedResponse, PersistenceStats, TimeseriesAggregateBucket,
};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Serialize, Serializer};

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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ArchiveStatus {
    pub(crate) configured: bool,
    pub(crate) healthy: bool,
    pub(crate) errors_total: u64,
    pub(crate) pool_timeouts_total: u64,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum EventKind {
    Connected { endpoint: String },
    Disconnected,
    ReceiverFrame(emwin_service::ReceiverFrame),
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
