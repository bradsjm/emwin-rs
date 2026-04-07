//! JSON and raw output helpers for archive query commands.

use emwin_service::{
    AggregateCompleteness, ArchivedFeature, ArchivedIssue, ArchivedProductDetail,
    ArchivedProductSummary, CellAggregateBucket, FacetAggregateBucket, IncidentDetail,
    IncidentSummary, PaginatedResponse, TimeseriesAggregateBucket,
};
use serde::Serialize;
use std::io::Write;

const API_PREFIX: &str = "/v1";

#[derive(Debug, Serialize)]
pub(crate) struct IncidentSummaryPayload {
    #[serde(flatten)]
    incident: IncidentSummary,
    detail_url: String,
    products_url: String,
    latest_product_url: String,
}

impl IncidentSummaryPayload {
    pub(crate) fn from_incident(incident: IncidentSummary) -> Self {
        let detail_url = incident_detail_url(&incident);
        let products_url = incident_products_url(
            &incident.office,
            &incident.phenomena,
            &incident.significance,
            incident.etn,
        );
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
    incident: IncidentDetail,
    products_url: String,
    first_product_url: String,
    latest_product_url: String,
}

impl IncidentDetailPayload {
    pub(crate) fn from_incident(incident: IncidentDetail) -> Self {
        let products_url = incident_products_url(
            &incident.office,
            &incident.phenomena,
            &incident.significance,
            incident.etn,
        );
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
    product: ArchivedProductSummary,
    detail_url: String,
    raw_url: String,
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
    product: ArchivedProductDetail,
    raw_url: String,
}

impl ArchiveProductDetailPayload {
    pub(crate) fn from_product(product: ArchivedProductDetail) -> Self {
        let raw_url = archive_product_raw_url(product.summary.product_id);
        Self { product, raw_url }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct IncidentsResponse {
    #[serde(flatten)]
    page: PaginatedResponse<IncidentSummaryPayload>,
}

impl IncidentsResponse {
    pub(crate) fn from_page(page: PaginatedResponse<IncidentSummary>) -> Self {
        Self {
            page: PaginatedResponse {
                items: page
                    .items
                    .into_iter()
                    .map(IncidentSummaryPayload::from_incident)
                    .collect(),
                next_cursor: page.next_cursor,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct IncidentResponse {
    incident: IncidentDetailPayload,
}

impl IncidentResponse {
    pub(crate) fn from_incident(incident: IncidentDetail) -> Self {
        Self {
            incident: IncidentDetailPayload::from_incident(incident),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct IncidentProductsResponse {
    #[serde(flatten)]
    page: PaginatedResponse<ArchiveProductSummaryPayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProductsResponse {
    #[serde(flatten)]
    page: PaginatedResponse<ArchiveProductSummaryPayload>,
}

impl ProductsResponse {
    pub(crate) fn from_page(page: PaginatedResponse<ArchivedProductSummary>) -> Self {
        Self {
            page: PaginatedResponse {
                items: page
                    .items
                    .into_iter()
                    .map(ArchiveProductSummaryPayload::from_product)
                    .collect(),
                next_cursor: page.next_cursor,
            },
        }
    }
}

impl IncidentProductsResponse {
    pub(crate) fn from_page(page: PaginatedResponse<ArchivedProductSummary>) -> Self {
        Self {
            page: PaginatedResponse {
                items: page
                    .items
                    .into_iter()
                    .map(ArchiveProductSummaryPayload::from_product)
                    .collect(),
                next_cursor: page.next_cursor,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveProductResponse {
    product: ArchiveProductDetailPayload,
}

impl ArchiveProductResponse {
    pub(crate) fn from_product(product: ArchivedProductDetail) -> Self {
        Self {
            product: ArchiveProductDetailPayload::from_product(product),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveIssuePayload {
    #[serde(flatten)]
    issue: ArchivedIssue,
    detail_url: String,
    product_url: String,
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

#[derive(Debug, Serialize)]
pub(crate) struct ArchivedFeaturePayload {
    #[serde(flatten)]
    feature: ArchivedFeature,
    product_url: String,
    product_raw_url: String,
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

    fn into_geojson_feature(self) -> GeoJsonFeature {
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

        GeoJsonFeature {
            id: self.feature.feature_id,
            kind: "Feature",
            geometry: self.feature.geometry,
            properties: serde_json::Value::Object(properties),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveIssuesResponse {
    #[serde(flatten)]
    page: PaginatedResponse<ArchiveIssuePayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FeaturesResponse {
    #[serde(flatten)]
    page: PaginatedResponse<ArchivedFeaturePayload>,
}

impl FeaturesResponse {
    pub(crate) fn from_page(page: PaginatedResponse<ArchivedFeature>) -> Self {
        Self {
            page: PaginatedResponse {
                items: page
                    .items
                    .into_iter()
                    .map(ArchivedFeaturePayload::from_feature)
                    .collect(),
                next_cursor: page.next_cursor,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct GeoJsonFeature {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    geometry: serde_json::Value,
    properties: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct FeatureCollectionResponse {
    #[serde(rename = "type")]
    kind: &'static str,
    features: Vec<GeoJsonFeature>,
}

impl FeatureCollectionResponse {
    pub(crate) fn from_page(page: PaginatedResponse<ArchivedFeature>) -> Self {
        Self {
            kind: "FeatureCollection",
            features: page
                .items
                .into_iter()
                .map(ArchivedFeaturePayload::from_feature)
                .map(ArchivedFeaturePayload::into_geojson_feature)
                .collect(),
        }
    }
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

impl ArchiveIssuesResponse {
    pub(crate) fn from_page(page: PaginatedResponse<ArchivedIssue>) -> Self {
        Self {
            page: PaginatedResponse {
                items: page
                    .items
                    .into_iter()
                    .map(ArchiveIssuePayload::from_issue)
                    .collect(),
                next_cursor: page.next_cursor,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveIssueResponse {
    issue: ArchiveIssuePayload,
}

impl ArchiveIssueResponse {
    pub(crate) fn from_issue(issue: ArchivedIssue) -> Self {
        Self {
            issue: ArchiveIssuePayload::from_issue(issue),
        }
    }
}

pub(crate) fn write_json<T>(writer: &mut dyn Write, value: &T) -> crate::error::CliResult<()>
where
    T: Serialize,
{
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn write_raw_bytes(writer: &mut dyn Write, bytes: &[u8]) -> crate::error::CliResult<()> {
    writer.write_all(bytes)?;
    Ok(())
}

fn incident_detail_url(incident: &IncidentSummary) -> String {
    incident_products_base_url(
        &incident.office,
        &incident.phenomena,
        &incident.significance,
        incident.etn,
    )
}

fn incident_products_url(office: &str, phenomena: &str, significance: &str, etn: i64) -> String {
    format!(
        "{}/products",
        incident_products_base_url(office, phenomena, significance, etn)
    )
}

fn incident_products_base_url(
    office: &str,
    phenomena: &str,
    significance: &str,
    etn: i64,
) -> String {
    format!("{API_PREFIX}/incidents/{office}/{phenomena}/{significance}/{etn}")
}

fn archive_product_url(product_id: i64) -> String {
    format!("{API_PREFIX}/products/{product_id}")
}

fn archive_product_raw_url(product_id: i64) -> String {
    format!("{API_PREFIX}/products/{product_id}/raw")
}

fn archive_issue_url(issue_id: i64) -> String {
    format!("{API_PREFIX}/issues/{issue_id}")
}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveIssueResponse, ArchiveIssuesResponse, ArchiveProductResponse, IncidentResponse,
        IncidentsResponse, write_json,
    };
    use chrono::{TimeZone, Utc};
    use emwin_service::{
        ArchivedIssue, ArchivedProductDetail, ArchivedProductSummary, IncidentDetail,
        IncidentSummary, PaginatedResponse,
    };

    #[test]
    fn incidents_response_matches_http_shape() {
        let response = IncidentsResponse::from_page(PaginatedResponse {
            items: vec![incident_summary()],
            next_cursor: Some("cursor-1".to_string()),
        });

        let value = serde_json::to_value(response).expect("response should serialize");
        assert_eq!(value["items"][0]["office"], "KOAX");
        assert_eq!(
            value["items"][0]["detail_url"],
            "/v1/incidents/KOAX/FF/W/2001"
        );
        assert_eq!(
            value["items"][0]["products_url"],
            "/v1/incidents/KOAX/FF/W/2001/products"
        );
        assert_eq!(value["items"][0]["latest_product_url"], "/v1/products/42");
        assert_eq!(value["next_cursor"], "cursor-1");
    }

    #[test]
    fn incident_and_product_responses_include_related_urls() {
        let incident = IncidentResponse::from_incident(incident_detail());
        let product = ArchiveProductResponse::from_product(archived_product_detail());

        let incident_json = serde_json::to_value(incident).expect("incident should serialize");
        let product_json = serde_json::to_value(product).expect("product should serialize");

        assert_eq!(
            incident_json["incident"]["products_url"],
            "/v1/incidents/KOAX/FF/W/2001/products"
        );
        assert_eq!(
            incident_json["incident"]["first_product_url"],
            "/v1/products/41"
        );
        assert_eq!(product_json["product"]["raw_url"], "/v1/products/42/raw");
    }

    #[test]
    fn write_json_emits_compact_json_with_newline() {
        let mut buf = Vec::new();
        write_json(
            &mut buf,
            &IncidentsResponse::from_page(PaginatedResponse::<IncidentSummary> {
                items: vec![incident_summary()],
                next_cursor: None,
            }),
        )
        .expect("json write should succeed");

        let output = String::from_utf8(buf).expect("json should be utf8");
        assert!(output.ends_with('\n'));
        assert!(!output.contains("\n\n"));
        assert!(output.starts_with("{\"items\":"));
    }

    #[test]
    fn archive_issue_responses_include_related_urls() {
        let list = ArchiveIssuesResponse::from_page(PaginatedResponse {
            items: vec![archived_issue()],
            next_cursor: Some("cursor-1".to_string()),
        });
        let detail = ArchiveIssueResponse::from_issue(archived_issue());

        let list_json = serde_json::to_value(list).expect("list should serialize");
        let detail_json = serde_json::to_value(detail).expect("detail should serialize");

        assert_eq!(list_json["items"][0]["code"], "invalid_wmo_header");
        assert_eq!(list_json["items"][0]["detail_url"], "/v1/issues/7");
        assert_eq!(list_json["items"][0]["product_url"], "/v1/products/42");
        assert_eq!(list_json["next_cursor"], "cursor-1");
        assert_eq!(detail_json["issue"]["id"], 7);
    }

    fn incident_summary() -> IncidentSummary {
        IncidentSummary {
            office: "KOAX".to_string(),
            phenomena: "FF".to_string(),
            significance: "W".to_string(),
            etn: 2001,
            current_status: "active".to_string(),
            latest_vtec_action: "CON".to_string(),
            issued_at: Utc.with_ymd_and_hms(2025, 3, 5, 12, 0, 0).unwrap(),
            start_utc: Some(Utc.with_ymd_and_hms(2025, 3, 5, 12, 0, 0).unwrap()),
            end_utc: Some(Utc.with_ymd_and_hms(2025, 3, 5, 18, 0, 0).unwrap()),
            last_updated_at: Utc.with_ymd_and_hms(2025, 3, 5, 13, 0, 0).unwrap(),
            first_product_id: 41,
            latest_product_id: 42,
            latest_product_timestamp_utc: Utc.with_ymd_and_hms(2025, 3, 5, 13, 0, 0).unwrap(),
        }
    }

    fn incident_detail() -> IncidentDetail {
        incident_summary()
    }

    fn archived_product_detail() -> ArchivedProductDetail {
        ArchivedProductDetail {
            summary: ArchivedProductSummary {
                product_id: 42,
                filename: "FFWOAX.TXT".to_string(),
                source_timestamp_utc: 1_741_178_000,
                ingested_at: Utc.with_ymd_and_hms(2025, 3, 5, 13, 0, 0).unwrap(),
                source_receiver: "qbt".to_string(),
                source_message_id: Some("message-1".to_string()),
                size_bytes: 2048,
                payload_storage_kind: "s3".to_string(),
                has_metadata_sidecar: true,
                source: "text_header".to_string(),
                family: Some("nws_text_product".to_string()),
                artifact_kind: Some("product".to_string()),
                title: Some("Flash Flood Warning".to_string()),
                container: "raw".to_string(),
                pil: Some("FFW".to_string()),
                wmo_prefix: Some("WGUS53".to_string()),
                bbb_kind: None,
                office_code: Some("OAX".to_string()),
                office_city: Some("Omaha/Valley".to_string()),
                office_state: Some("NE".to_string()),
                header_kind: Some("afos".to_string()),
                ttaaii: Some("WGUS53".to_string()),
                cccc: Some("KOAX".to_string()),
                ddhhmm: Some("051200".to_string()),
                bbb: None,
                afos: Some("FFWOAX".to_string()),
                has_body: true,
                has_artifact: true,
                has_issues: false,
                has_vtec: true,
                has_ugc: true,
                has_hvtec: false,
                has_latlon: false,
                has_time_mot_loc: false,
                has_wind_hail: false,
                vtec_count: 1,
                ugc_count: 1,
                hvtec_count: 0,
                latlon_count: 0,
                time_mot_loc_count: 0,
                wind_hail_count: 0,
                issue_count: 0,
            },
            payload_location: Some("s3://bucket/qbt/FFWOAX.TXT".to_string()),
            metadata_location: Some("s3://bucket/qbt/FFWOAX.JSON".to_string()),
            product_json: serde_json::json!({"schema_version": 2}),
        }
    }

    fn archived_issue() -> ArchivedIssue {
        ArchivedIssue {
            id: 7,
            product_id: 42,
            kind: "text_product_parse".to_string(),
            code: "invalid_wmo_header".to_string(),
            message: "failed to parse WMO header".to_string(),
            line: Some("INVALID HEADER".to_string()),
        }
    }
}
