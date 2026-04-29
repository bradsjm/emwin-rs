#![allow(missing_docs)]

use super::urls::{
    archive_issue_url, archive_product_raw_url, archive_product_url, incident_detail_url,
    incident_products_url,
};
use crate::server_support::file_download_url;
use emwin_service::{
    ArchivedFeature, ArchivedIssue, ArchivedProductDetail, ArchivedProductSummary,
    CellAggregateBucket, CompletedFileMetadata, FacetAggregateBucket, IncidentChange,
    IncidentChangeAction, IncidentChangeTrigger, IncidentDetail, IncidentSummary, LiveTelemetry,
    PersistenceStats, ProcessingStats, TimeseriesAggregateBucket,
};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use utoipa::ToSchema;

/// Downloadable file payload advertised by the HTTP API.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct CompletedFilePayload {
    #[serde(skip)]
    #[allow(dead_code)]
    pub(crate) metadata: CompletedFileMetadata,
    pub(crate) filename: String,
    pub(crate) size: usize,
    pub(crate) timestamp_utc: u64,
    #[schema(value_type = Object)]
    pub(crate) product: serde_json::Value,
    pub(crate) download_url: String,
}

impl CompletedFilePayload {
    pub(crate) fn from_metadata(metadata: CompletedFileMetadata) -> Self {
        let download_url = file_download_url(&metadata.filename);
        Self {
            filename: metadata.filename.clone(),
            size: metadata.size,
            timestamp_utc: metadata.timestamp_utc,
            product: serde_json::to_value(metadata.product_detail())
                .expect("product detail should serialize"),
            metadata,
            download_url,
        }
    }
}

/// Lightweight file payload advertised in the SSE event stream.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct CompletedFileEventPayload {
    #[serde(skip)]
    pub(crate) metadata: CompletedFileMetadata,
    pub(crate) filename: String,
    pub(crate) size: usize,
    pub(crate) timestamp_utc: u64,
    #[schema(value_type = Object)]
    pub(crate) product: serde_json::Value,
    pub(crate) download_url: String,
}

impl CompletedFileEventPayload {
    pub(crate) fn from_metadata(metadata: CompletedFileMetadata) -> Self {
        Self {
            filename: metadata.filename.clone(),
            size: metadata.size,
            timestamp_utc: metadata.timestamp_utc,
            product: serde_json::to_value(metadata.product_summary())
                .expect("product summary should serialize"),
            download_url: file_download_url(&metadata.filename),
            metadata,
        }
    }
}

pub(crate) type TelemetryPayload = LiveTelemetry;

#[derive(Debug, Clone, Serialize, ToSchema)]
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
    pub(crate) processing: ProcessingStats,
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

        let processing_field_count = 6;
        let persistence_field_count = usize::from(self.persistence.is_some()) * 8;
        let archive_field_count = 4 + usize::from(self.archive.last_error.is_some());
        let mut map = serializer.serialize_map(Some(
            telemetry_fields.len()
                + processing_field_count
                + persistence_field_count
                + archive_field_count,
        ))?;
        for (key, value) in telemetry_fields {
            map.serialize_entry(key, value)?;
        }
        map.serialize_entry("processing_queue_len", &self.processing.queue_len)?;
        map.serialize_entry("processing_queue_capacity", &self.processing.queue_capacity)?;
        map.serialize_entry("processing_enqueued_total", &self.processing.enqueued_total)?;
        map.serialize_entry("processing_evicted_total", &self.processing.evicted_total)?;
        map.serialize_entry(
            "processing_completed_total",
            &self.processing.completed_total,
        )?;
        map.serialize_entry("processing_failed_total", &self.processing.failed_total)?;
        if let Some(persistence) = self.persistence {
            map.serialize_entry("persistence_queue_len", &persistence.queue_len)?;
            map.serialize_entry("persistence_queue_capacity", &persistence.queue_capacity)?;
            map.serialize_entry("persistence_enqueued_total", &persistence.enqueued_total)?;
            map.serialize_entry("persistence_evicted_total", &persistence.evicted_total)?;
            map.serialize_entry("persistence_persisted_total", &persistence.persisted_total)?;
            map.serialize_entry("persistence_failed_total", &persistence.failed_total)?;
            map.serialize_entry(
                "persistence_retry_exhausted_total",
                &persistence.retry_exhausted_total,
            )?;
            map.serialize_entry(
                "persistence_stale_dropped_total",
                &persistence.stale_dropped_total,
            )?;
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

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FilesResponse {
    pub(crate) files: Vec<CompletedFilePayload>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct IncidentSummaryPayload {
    pub(crate) office: String,
    pub(crate) phenomena: String,
    pub(crate) significance: String,
    pub(crate) etn: i64,
    pub(crate) current_status: String,
    pub(crate) latest_vtec_action: String,
    pub(crate) issued_at: chrono::DateTime<chrono::Utc>,
    pub(crate) start_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) end_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_updated_at: chrono::DateTime<chrono::Utc>,
    pub(crate) first_product_id: i64,
    pub(crate) latest_product_id: i64,
    pub(crate) latest_product_timestamp_utc: chrono::DateTime<chrono::Utc>,
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
            office: incident.office,
            phenomena: incident.phenomena,
            significance: incident.significance,
            etn: incident.etn,
            current_status: incident.current_status,
            latest_vtec_action: incident.latest_vtec_action,
            issued_at: incident.issued_at,
            start_utc: incident.start_utc,
            end_utc: incident.end_utc,
            last_updated_at: incident.last_updated_at,
            first_product_id: incident.first_product_id,
            latest_product_id: incident.latest_product_id,
            latest_product_timestamp_utc: incident.latest_product_timestamp_utc,
            detail_url,
            products_url,
            latest_product_url,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct IncidentDetailPayload {
    pub(crate) office: String,
    pub(crate) phenomena: String,
    pub(crate) significance: String,
    pub(crate) etn: i64,
    pub(crate) current_status: String,
    pub(crate) latest_vtec_action: String,
    pub(crate) issued_at: chrono::DateTime<chrono::Utc>,
    pub(crate) start_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) end_utc: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) last_updated_at: chrono::DateTime<chrono::Utc>,
    pub(crate) first_product_id: i64,
    pub(crate) latest_product_id: i64,
    pub(crate) latest_product_timestamp_utc: chrono::DateTime<chrono::Utc>,
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
            office: incident.office,
            phenomena: incident.phenomena,
            significance: incident.significance,
            etn: incident.etn,
            current_status: incident.current_status,
            latest_vtec_action: incident.latest_vtec_action,
            issued_at: incident.issued_at,
            start_utc: incident.start_utc,
            end_utc: incident.end_utc,
            last_updated_at: incident.last_updated_at,
            first_product_id: incident.first_product_id,
            latest_product_id: incident.latest_product_id,
            latest_product_timestamp_utc: incident.latest_product_timestamp_utc,
            products_url,
            first_product_url,
            latest_product_url,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ArchiveProductSummaryPayload {
    pub(crate) product_id: i64,
    pub(crate) filename: String,
    pub(crate) source_timestamp_utc: i64,
    pub(crate) ingested_at: chrono::DateTime<chrono::Utc>,
    pub(crate) source_receiver: String,
    pub(crate) source_message_id: Option<String>,
    pub(crate) size_bytes: i64,
    pub(crate) has_metadata_sidecar: bool,
    pub(crate) source: String,
    pub(crate) family: Option<String>,
    pub(crate) artifact_kind: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) container: String,
    pub(crate) pil: Option<String>,
    pub(crate) wmo_prefix: Option<String>,
    pub(crate) bbb_kind: Option<String>,
    pub(crate) office_code: Option<String>,
    pub(crate) office_city: Option<String>,
    pub(crate) office_state: Option<String>,
    pub(crate) header_kind: Option<String>,
    pub(crate) ttaaii: Option<String>,
    pub(crate) cccc: Option<String>,
    pub(crate) ddhhmm: Option<String>,
    pub(crate) bbb: Option<String>,
    pub(crate) afos: Option<String>,
    pub(crate) has_body: bool,
    pub(crate) has_artifact: bool,
    pub(crate) has_issues: bool,
    pub(crate) has_vtec: bool,
    pub(crate) has_ugc: bool,
    pub(crate) has_hvtec: bool,
    pub(crate) has_latlon: bool,
    pub(crate) has_time_mot_loc: bool,
    pub(crate) has_wind_hail: bool,
    pub(crate) vtec_count: i32,
    pub(crate) ugc_count: i32,
    pub(crate) hvtec_count: i32,
    pub(crate) latlon_count: i32,
    pub(crate) time_mot_loc_count: i32,
    pub(crate) wind_hail_count: i32,
    pub(crate) issue_count: i32,
    pub(crate) detail_url: String,
    pub(crate) raw_url: String,
}

impl ArchiveProductSummaryPayload {
    pub(crate) fn from_product(product: ArchivedProductSummary) -> Self {
        let detail_url = archive_product_url(product.product_id);
        let raw_url = archive_product_raw_url(product.product_id);
        Self {
            product_id: product.product_id,
            filename: product.filename,
            source_timestamp_utc: product.source_timestamp_utc,
            ingested_at: product.ingested_at,
            source_receiver: product.source_receiver,
            source_message_id: product.source_message_id,
            size_bytes: product.size_bytes,
            has_metadata_sidecar: product.has_metadata_sidecar,
            source: product.source,
            family: product.family,
            artifact_kind: product.artifact_kind,
            title: product.title,
            container: product.container,
            pil: product.pil,
            wmo_prefix: product.wmo_prefix,
            bbb_kind: product.bbb_kind,
            office_code: product.office_code,
            office_city: product.office_city,
            office_state: product.office_state,
            header_kind: product.header_kind,
            ttaaii: product.ttaaii,
            cccc: product.cccc,
            ddhhmm: product.ddhhmm,
            bbb: product.bbb,
            afos: product.afos,
            has_body: product.has_body,
            has_artifact: product.has_artifact,
            has_issues: product.has_issues,
            has_vtec: product.has_vtec,
            has_ugc: product.has_ugc,
            has_hvtec: product.has_hvtec,
            has_latlon: product.has_latlon,
            has_time_mot_loc: product.has_time_mot_loc,
            has_wind_hail: product.has_wind_hail,
            vtec_count: product.vtec_count,
            ugc_count: product.ugc_count,
            hvtec_count: product.hvtec_count,
            latlon_count: product.latlon_count,
            time_mot_loc_count: product.time_mot_loc_count,
            wind_hail_count: product.wind_hail_count,
            issue_count: product.issue_count,
            detail_url,
            raw_url,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ArchiveProductDetailPayload {
    #[serde(flatten)]
    pub(crate) summary: ArchiveProductSummaryPayload,
    pub(crate) payload_location: Option<String>,
    pub(crate) metadata_location: Option<String>,
    #[schema(value_type = Object)]
    pub(crate) product_json: serde_json::Value,
    pub(crate) raw_url: String,
}

impl ArchiveProductDetailPayload {
    pub(crate) fn from_product(product: ArchivedProductDetail) -> Self {
        let raw_url = archive_product_raw_url(product.summary.product_id);
        Self {
            summary: ArchiveProductSummaryPayload::from_product(product.summary),
            payload_location: product.payload_location,
            metadata_location: product.metadata_location,
            product_json: product.product_json,
            raw_url,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ArchiveIssuePayload {
    pub(crate) id: i64,
    pub(crate) product_id: i64,
    pub(crate) kind: String,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) line: Option<String>,
    pub(crate) detail_url: String,
    pub(crate) product_url: String,
}

impl ArchiveIssuePayload {
    pub(crate) fn from_issue(issue: ArchivedIssue) -> Self {
        let detail_url = archive_issue_url(issue.id);
        let product_url = archive_product_url(issue.product_id);
        Self {
            id: issue.id,
            product_id: issue.product_id,
            kind: issue.kind,
            code: issue.code,
            message: issue.message,
            line: issue.line,
            detail_url,
            product_url,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct ArchivedFeaturePayload {
    pub(crate) feature_id: String,
    pub(crate) feature_kind: emwin_service::FeatureKind,
    pub(crate) product_id: i64,
    pub(crate) source_timestamp_utc: i64,
    #[schema(value_type = Object)]
    pub(crate) geometry: serde_json::Value,
    #[schema(value_type = Object)]
    pub(crate) properties: serde_json::Value,
    pub(crate) product_url: String,
    pub(crate) product_raw_url: String,
}

impl ArchivedFeaturePayload {
    pub(crate) fn from_feature(feature: ArchivedFeature) -> Self {
        let product_url = archive_product_url(feature.product_id);
        let product_raw_url = archive_product_raw_url(feature.product_id);
        Self {
            feature_id: feature.feature_id,
            feature_kind: feature.feature_kind,
            product_id: feature.product_id,
            source_timestamp_utc: feature.source_timestamp_utc,
            geometry: feature.geometry,
            properties: feature.properties,
            product_url,
            product_raw_url,
        }
    }

    pub(crate) fn into_geojson_feature(self) -> GeoJsonFeature {
        let mut properties = match self.properties {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        properties.insert(
            "feature_kind".to_string(),
            serde_json::json!(self.feature_kind),
        );
        properties.insert("product_id".to_string(), serde_json::json!(self.product_id));
        properties.insert(
            "source_timestamp_utc".to_string(),
            serde_json::json!(self.source_timestamp_utc),
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
            self.feature_id,
            self.geometry,
            serde_json::Value::Object(properties),
        )
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct IncidentsResponse {
    pub(crate) items: Vec<IncidentSummaryPayload>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ProductsResponse {
    pub(crate) items: Vec<ArchiveProductSummaryPayload>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FeaturesResponse {
    pub(crate) items: Vec<ArchivedFeaturePayload>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct GeoJsonFeature {
    pub(crate) id: String,
    #[serde(rename = "type")]
    #[schema(rename = "type")]
    pub(crate) kind: &'static str,
    #[schema(value_type = Object)]
    pub(crate) geometry: serde_json::Value,
    #[schema(value_type = Object)]
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

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FeatureCollectionResponse {
    #[serde(rename = "type")]
    #[schema(rename = "type")]
    pub(crate) kind: &'static str,
    pub(crate) features: Vec<GeoJsonFeature>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FacetAggregateResponse {
    pub(crate) dimension: String,
    pub(crate) partial: bool,
    pub(crate) approximate: bool,
    pub(crate) reason: Option<String>,
    pub(crate) items: Vec<FacetAggregateBucket>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TimeseriesAggregateResponse {
    pub(crate) measure: String,
    pub(crate) bucket: String,
    pub(crate) start: chrono::DateTime<chrono::Utc>,
    pub(crate) end: chrono::DateTime<chrono::Utc>,
    pub(crate) partial: bool,
    pub(crate) approximate: bool,
    pub(crate) reason: Option<String>,
    pub(crate) items: Vec<TimeseriesAggregateBucket>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct CellAggregateResponse {
    pub(crate) measure: String,
    pub(crate) precision: u8,
    pub(crate) partial: bool,
    pub(crate) approximate: bool,
    pub(crate) reason: Option<String>,
    pub(crate) items: Vec<CellAggregateBucket>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct IncidentProductsResponse {
    pub(crate) items: Vec<ArchiveProductSummaryPayload>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct IncidentResponse {
    pub(crate) incident: IncidentDetailPayload,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ArchiveProductResponse {
    pub(crate) product: ArchiveProductDetailPayload,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ArchiveIssuesResponse {
    pub(crate) items: Vec<ArchiveIssuePayload>,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct ArchiveIssueResponse {
    pub(crate) issue: ArchiveIssuePayload,
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HealthResponse {
    pub(crate) status: &'static str,
    pub(crate) archive: ArchiveStatus,
    pub(crate) connected_clients: usize,
    pub(crate) retained_files: usize,
    pub(crate) uptime_secs: u64,
    pub(crate) upstream_endpoint: Option<String>,
}
