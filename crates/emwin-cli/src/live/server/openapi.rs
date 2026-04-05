//! OpenAPI contract for live server mode.
//!
//! Runtime handlers own behavior. This module owns the generated spec and the documented schema
//! types presented to OpenAPI consumers.
#![allow(dead_code)]

use super::types::{OPENAPI_AUTH_SCHEME_NAME, OPENAPI_JSON_PATH};
use utoipa::ToSchema;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon),
    paths(
        super::server_http::incidents_handler,
        super::server_http::incident_handler,
        super::server_http::incident_products_handler,
        super::server_http::archive_product_handler,
        super::server_http::archive_product_raw_handler,
        super::server_http::archive_issues_handler,
        super::server_http::archive_issue_handler,
        super::server_http::incident_events_handler,
        super::server_http::events_handler,
        super::server_http::files_handler,
        super::server_http::file_download_handler,
        super::server_http::health_handler,
        super::server_http::metrics_handler,
    ),
    components(
        schemas(
            FilesResponseSchema,
            CompletedFileSchema,
            IncidentsResponseSchema,
            IncidentSummarySchema,
            IncidentResponseSchema,
            IncidentDetailSchema,
            IncidentProductsResponseSchema,
            ArchiveProductSummarySchema,
            ArchiveProductResponseSchema,
            ArchiveProductDetailSchema,
            ArchiveIssuesResponseSchema,
            ArchiveIssueResponseSchema,
            ArchiveIssueSchema,
            HealthResponseSchema,
            SseEventEnvelope,
        )
    ),
    tags(
        (name = "live", description = "In-memory live server endpoints and SSE streams"),
        (name = "archive", description = "Postgres-backed archived product endpoints"),
        (name = "admin", description = "Operational health and metrics endpoints")
    ),
    info(
        title = "emwin-cli server API",
        version = "v1",
        description = "Versioned HTTP and SSE API for emwin-cli server mode."
    )
)]
pub(crate) struct SecureApiDoc;

#[derive(OpenApi)]
#[openapi(
    modifiers(&PublicSecurityRemover),
    paths(
        super::server_http::incidents_handler,
        super::server_http::incident_handler,
        super::server_http::incident_products_handler,
        super::server_http::archive_product_handler,
        super::server_http::archive_product_raw_handler,
        super::server_http::archive_issues_handler,
        super::server_http::archive_issue_handler,
        super::server_http::incident_events_handler,
        super::server_http::events_handler,
        super::server_http::files_handler,
        super::server_http::file_download_handler,
        super::server_http::health_handler,
        super::server_http::metrics_handler,
    ),
    components(
        schemas(
            FilesResponseSchema,
            CompletedFileSchema,
            IncidentsResponseSchema,
            IncidentSummarySchema,
            IncidentResponseSchema,
            IncidentDetailSchema,
            IncidentProductsResponseSchema,
            ArchiveProductSummarySchema,
            ArchiveProductResponseSchema,
            ArchiveProductDetailSchema,
            ArchiveIssuesResponseSchema,
            ArchiveIssueResponseSchema,
            ArchiveIssueSchema,
            HealthResponseSchema,
            SseEventEnvelope,
        )
    ),
    tags(
        (name = "live", description = "In-memory live server endpoints and SSE streams"),
        (name = "archive", description = "Postgres-backed archived product endpoints"),
        (name = "admin", description = "Operational health and metrics endpoints")
    ),
    info(
        title = "emwin-cli server API",
        version = "v1",
        description = "Versioned HTTP and SSE API for emwin-cli server mode."
    )
)]
pub(crate) struct PublicApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::new);
        components.add_security_scheme(
            OPENAPI_AUTH_SCHEME_NAME,
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("API key")
                    .description(Some(
                        "Bearer token required for versioned HTTP and SSE API routes.",
                    ))
                    .build(),
            ),
        );
    }
}

struct PublicSecurityRemover;

impl Modify for PublicSecurityRemover {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        for path_item in openapi.paths.paths.values_mut() {
            if let Some(operation) = path_item.get.as_mut() {
                operation.security = None;
            }
            if let Some(operation) = path_item.post.as_mut() {
                operation.security = None;
            }
            if let Some(operation) = path_item.put.as_mut() {
                operation.security = None;
            }
            if let Some(operation) = path_item.delete.as_mut() {
                operation.security = None;
            }
            if let Some(operation) = path_item.options.as_mut() {
                operation.security = None;
            }
            if let Some(operation) = path_item.head.as_mut() {
                operation.security = None;
            }
            if let Some(operation) = path_item.patch.as_mut() {
                operation.security = None;
            }
            if let Some(operation) = path_item.trace.as_mut() {
                operation.security = None;
            }
        }
    }
}

#[derive(Debug, ToSchema)]
pub(crate) struct SseEventEnvelope {
    #[schema(example = "42")]
    pub(crate) id: String,
    #[schema(example = "file_complete")]
    pub(crate) event: String,
    #[schema(value_type = Object)]
    pub(crate) data: serde_json::Value,
}

#[derive(Debug, ToSchema)]
pub(crate) struct FilesResponseSchema {
    pub(crate) files: Vec<CompletedFileSchema>,
}

#[derive(Debug, ToSchema)]
pub(crate) struct CompletedFileSchema {
    #[schema(example = "AFDBOX.TXT")]
    pub(crate) filename: String,
    #[schema(example = 2140)]
    pub(crate) size: usize,
    #[schema(example = 1767488000u64)]
    pub(crate) timestamp_utc: u64,
    #[schema(value_type = Object)]
    pub(crate) product: serde_json::Value,
    #[schema(example = "/v1/live/files/AFDBOX.TXT")]
    pub(crate) download_url: String,
}

#[derive(Debug, ToSchema)]
pub(crate) struct IncidentsResponseSchema {
    pub(crate) items: Vec<IncidentSummarySchema>,
    #[schema(example = "cursor-token")]
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, ToSchema)]
pub(crate) struct IncidentResponseSchema {
    pub(crate) incident: IncidentDetailSchema,
}

#[derive(Debug, ToSchema)]
pub(crate) struct IncidentSummarySchema {
    #[schema(example = "KOAX")]
    pub(crate) office: String,
    #[schema(example = "FF")]
    pub(crate) phenomena: String,
    #[schema(example = "W")]
    pub(crate) significance: String,
    #[schema(example = 2001)]
    pub(crate) etn: i64,
    #[schema(example = "active")]
    pub(crate) current_status: String,
    #[schema(example = "NEW")]
    pub(crate) latest_vtec_action: String,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:00Z")]
    pub(crate) issued_at: chrono::DateTime<chrono::Utc>,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:00Z")]
    pub(crate) start_utc: Option<chrono::DateTime<chrono::Utc>>,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T18:00:00Z")]
    pub(crate) end_utc: Option<chrono::DateTime<chrono::Utc>>,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:01Z")]
    pub(crate) last_updated_at: chrono::DateTime<chrono::Utc>,
    #[schema(example = 10)]
    pub(crate) first_product_id: i64,
    #[schema(example = 10)]
    pub(crate) latest_product_id: i64,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:00Z")]
    pub(crate) latest_product_timestamp_utc: chrono::DateTime<chrono::Utc>,
    #[schema(example = "/v1/live/incidents/KOAX/FF/W/2001")]
    pub(crate) detail_url: String,
    #[schema(example = "/v1/live/incidents/KOAX/FF/W/2001/products")]
    pub(crate) products_url: String,
    #[schema(example = "/v1/archive/products/10")]
    pub(crate) latest_product_url: String,
}

#[derive(Debug, ToSchema)]
pub(crate) struct IncidentDetailSchema {
    #[schema(example = "KOAX")]
    pub(crate) office: String,
    #[schema(example = "FF")]
    pub(crate) phenomena: String,
    #[schema(example = "W")]
    pub(crate) significance: String,
    #[schema(example = 2001)]
    pub(crate) etn: i64,
    #[schema(example = "active")]
    pub(crate) current_status: String,
    #[schema(example = "NEW")]
    pub(crate) latest_vtec_action: String,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:00Z")]
    pub(crate) issued_at: chrono::DateTime<chrono::Utc>,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:00Z")]
    pub(crate) start_utc: Option<chrono::DateTime<chrono::Utc>>,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T18:00:00Z")]
    pub(crate) end_utc: Option<chrono::DateTime<chrono::Utc>>,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:01Z")]
    pub(crate) last_updated_at: chrono::DateTime<chrono::Utc>,
    #[schema(example = 10)]
    pub(crate) first_product_id: i64,
    #[schema(example = 10)]
    pub(crate) latest_product_id: i64,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:00Z")]
    pub(crate) latest_product_timestamp_utc: chrono::DateTime<chrono::Utc>,
    #[schema(example = "/v1/live/incidents/KOAX/FF/W/2001/products")]
    pub(crate) products_url: String,
    #[schema(example = "/v1/archive/products/10")]
    pub(crate) first_product_url: String,
    #[schema(example = "/v1/archive/products/10")]
    pub(crate) latest_product_url: String,
}

#[derive(Debug, ToSchema)]
pub(crate) struct IncidentProductsResponseSchema {
    pub(crate) items: Vec<ArchiveProductSummarySchema>,
    #[schema(example = "cursor-token")]
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, ToSchema)]
pub(crate) struct ArchiveProductResponseSchema {
    pub(crate) product: ArchiveProductDetailSchema,
}

#[derive(Debug, ToSchema)]
pub(crate) struct ArchiveIssuesResponseSchema {
    pub(crate) items: Vec<ArchiveIssueSchema>,
    #[schema(example = "cursor-token")]
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, ToSchema)]
pub(crate) struct ArchiveIssueResponseSchema {
    pub(crate) issue: ArchiveIssueSchema,
}

#[derive(Debug, ToSchema)]
pub(crate) struct ArchiveIssueSchema {
    #[schema(example = 10)]
    pub(crate) id: i64,
    #[schema(example = 42)]
    pub(crate) product_id: i64,
    #[schema(example = "text_product_parse")]
    pub(crate) kind: String,
    #[schema(example = "invalid_wmo_header")]
    pub(crate) code: String,
    #[schema(example = "failed to parse WMO header")]
    pub(crate) message: String,
    #[schema(example = "INVALID HEADER")]
    pub(crate) line: Option<String>,
    #[schema(example = "/v1/archive/issues/10")]
    pub(crate) detail_url: String,
    #[schema(example = "/v1/archive/products/42")]
    pub(crate) product_url: String,
}

#[derive(Debug, ToSchema)]
pub(crate) struct ArchiveProductSummarySchema {
    #[schema(example = 10)]
    pub(crate) product_id: i64,
    #[schema(example = "AFDBOX.TXT")]
    pub(crate) filename: String,
    #[schema(example = 1767488000i64)]
    pub(crate) source_timestamp_utc: i64,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:01Z")]
    pub(crate) ingested_at: chrono::DateTime<chrono::Utc>,
    #[schema(example = "qbt")]
    pub(crate) source_receiver: String,
    pub(crate) source_message_id: Option<String>,
    #[schema(example = 2140)]
    pub(crate) size_bytes: i64,
    #[schema(example = "filesystem")]
    pub(crate) payload_storage_kind: String,
    #[schema(example = true)]
    pub(crate) has_metadata_sidecar: bool,
    #[schema(example = "text_header")]
    pub(crate) source: String,
    pub(crate) family: Option<String>,
    pub(crate) artifact_kind: Option<String>,
    pub(crate) title: Option<String>,
    #[schema(example = "raw")]
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
    #[schema(example = "/v1/archive/products/10")]
    pub(crate) detail_url: String,
    #[schema(example = "/v1/archive/products/10/raw")]
    pub(crate) raw_url: String,
}

#[derive(Debug, ToSchema)]
pub(crate) struct ArchiveProductDetailSchema {
    #[schema(example = 10)]
    pub(crate) product_id: i64,
    #[schema(example = "AFDBOX.TXT")]
    pub(crate) filename: String,
    #[schema(example = 1767488000i64)]
    pub(crate) source_timestamp_utc: i64,
    #[schema(value_type = String, format = DateTime, example = "2025-03-05T12:00:01Z")]
    pub(crate) ingested_at: chrono::DateTime<chrono::Utc>,
    #[schema(example = "qbt")]
    pub(crate) source_receiver: String,
    pub(crate) source_message_id: Option<String>,
    #[schema(example = 2140)]
    pub(crate) size_bytes: i64,
    #[schema(example = "filesystem")]
    pub(crate) payload_storage_kind: String,
    #[schema(example = true)]
    pub(crate) has_metadata_sidecar: bool,
    #[schema(example = "text_header")]
    pub(crate) source: String,
    pub(crate) family: Option<String>,
    pub(crate) artifact_kind: Option<String>,
    pub(crate) title: Option<String>,
    #[schema(example = "raw")]
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
    #[schema(example = "s3://emwin/qbt/2025/03/05/BOX/AFDBOX.TXT")]
    pub(crate) payload_location: String,
    pub(crate) metadata_location: Option<String>,
    #[schema(value_type = Object)]
    pub(crate) product_json: serde_json::Value,
    #[schema(example = "/v1/archive/products/10/raw")]
    pub(crate) raw_url: String,
}

#[derive(Debug, ToSchema)]
pub(crate) struct HealthResponseSchema {
    #[schema(example = "ok")]
    pub(crate) status: String,
    #[schema(example = 2)]
    pub(crate) connected_clients: usize,
    #[schema(example = 17)]
    pub(crate) retained_files: usize,
    #[schema(example = 320)]
    pub(crate) uptime_secs: u64,
    #[schema(example = "wxmesg.upstateweather.com:2211")]
    pub(crate) upstream_endpoint: Option<String>,
}

pub(crate) fn openapi_json(auth_enabled: bool) -> utoipa::openapi::OpenApi {
    if auth_enabled {
        SecureApiDoc::openapi()
    } else {
        PublicApiDoc::openapi()
    }
}

pub(crate) fn swagger_ui_mount(auth_enabled: bool) -> utoipa_swagger_ui::SwaggerUi {
    utoipa_swagger_ui::SwaggerUi::new("/").url(OPENAPI_JSON_PATH, openapi_json(auth_enabled))
}
