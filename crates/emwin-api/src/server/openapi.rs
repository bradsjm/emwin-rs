//! OpenAPI contract for live server mode.
//!
//! Runtime handlers own behavior. This module owns the generated spec and the documented schema
//! types presented to OpenAPI consumers.

use super::types::{
    AlertContactPointInputPayload, AlertContactPointPayload, AlertContactPointsResponse,
    AlertDeliveriesResponse, AlertDeliveryAttemptPayload, AlertEventPayload,
    AlertRuleEventsResponse, AlertRuleInputPayload, AlertRulePayload,
    AlertRuleSimulationRequestPayload, AlertRuleSimulationWindowPayload, AlertRuleTargetPayload,
    AlertRulesResponse, AlertSilenceInputPayload, AlertSilencePayload, AlertSilencesResponse,
    AlertSimulationResultPayload, AlertSimulationSamplePayload, AlertTestResponse,
    ArchiveFilterParams, ArchiveIssuePayload, ArchiveIssueResponse, ArchiveIssuesResponse,
    ArchiveProductDetailPayload, ArchiveProductResponse, ArchiveProductSummaryPayload,
    ArchiveStatus, ArchivedFeaturePayload, CellAggregateResponse, CompletedFileEventPayload,
    CompletedFilePayload, FacetAggregateResponse, FeatureCollectionResponse, FeaturesResponse,
    FilesResponse, GeoJsonFeature, HealthResponse, IncidentDetailPayload, IncidentProductsResponse,
    IncidentResponse, IncidentSummaryPayload, IncidentsResponse, OPENAPI_AUTH_SCHEME_NAME,
    OPENAPI_JSON_PATH, ProductsResponse, TimeseriesAggregateResponse,
};
use emwin_service::{CellAggregateBucket, FacetAggregateBucket, TimeseriesAggregateBucket};
use utoipa::ToSchema;
use utoipa::openapi::path::ParameterIn;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{IntoParams, Modify, OpenApi};

struct ArchiveFilterParamsFixup;

impl Modify for ArchiveFilterParamsFixup {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let archive_filter_params = ArchiveFilterParams::into_params(|| Some(ParameterIn::Query));
        for path in [
            "/v1/products",
            "/v1/features",
            "/v1/features/geojson",
            "/v1/aggregates/facets",
            "/v1/aggregates/timeseries",
            "/v1/aggregates/cells",
        ] {
            let Some(path_item) = openapi.paths.paths.get_mut(path) else {
                continue;
            };
            let Some(operation) = path_item.get.as_mut() else {
                continue;
            };
            let params = operation.parameters.get_or_insert_with(Vec::new);
            params.retain(|param| param.name != "filters");
            for archive_param in &archive_filter_params {
                if params
                    .iter()
                    .all(|existing| existing.name != archive_param.name)
                {
                    params.push(archive_param.clone());
                }
            }
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon, &ArchiveFilterParamsFixup),
    paths(
        super::server_http::archive::products_handler,
        super::server_http::archive::features_handler,
        super::server_http::archive::features_geojson_handler,
        super::server_http::archive::facet_aggregate_handler,
        super::server_http::archive::timeseries_aggregate_handler,
        super::server_http::archive::cell_aggregate_handler,
        super::server_http::archive::incidents_handler,
        super::server_http::archive::incident_handler,
        super::server_http::archive::incident_products_handler,
        super::server_http::archive::archive_product_handler,
        super::server_http::archive::archive_product_raw_handler,
        super::server_http::archive::archive_issues_handler,
        super::server_http::archive::archive_issue_handler,
        super::server_http::alerting::list_contact_points_handler,
        super::server_http::alerting::create_contact_point_handler,
        super::server_http::alerting::get_contact_point_handler,
        super::server_http::alerting::update_contact_point_handler,
        super::server_http::alerting::delete_contact_point_handler,
        super::server_http::alerting::test_contact_point_handler,
        super::server_http::alerting::list_rules_handler,
        super::server_http::alerting::create_rule_handler,
        super::server_http::alerting::get_rule_handler,
        super::server_http::alerting::update_rule_handler,
        super::server_http::alerting::delete_rule_handler,
        super::server_http::alerting::simulate_rule_handler,
        super::server_http::alerting::simulate_persisted_rule_handler,
        super::server_http::alerting::list_rule_events_handler,
        super::server_http::alerting::list_deliveries_handler,
        super::server_http::alerting::list_silences_handler,
        super::server_http::alerting::create_silence_handler,
        super::server_http::alerting::delete_silence_handler,
        super::server_http::streams::incident_events_handler,
        super::server_http::streams::events_handler,
        super::server_http::operational::files_handler,
        super::server_http::operational::file_download_handler,
        super::server_http::operational::health_handler,
        super::server_http::operational::metrics_handler,
    ),
    components(
        schemas(
            FilesResponse,
            CompletedFilePayload,
            CompletedFileEventPayload,
            ProductsResponse,
            FeaturesResponse,
            ArchivedFeaturePayload,
            FeatureCollectionResponse,
            GeoJsonFeature,
            FacetAggregateResponse,
            FacetAggregateBucket,
            TimeseriesAggregateResponse,
            TimeseriesAggregateBucket,
            CellAggregateResponse,
            CellAggregateBucket,
            IncidentsResponse,
            IncidentSummaryPayload,
            IncidentResponse,
            IncidentDetailPayload,
            IncidentProductsResponse,
            ArchiveProductSummaryPayload,
            ArchiveProductResponse,
            ArchiveProductDetailPayload,
            ArchiveIssuesResponse,
            ArchiveIssueResponse,
            ArchiveIssuePayload,
            AlertContactPointPayload,
            AlertContactPointInputPayload,
            AlertContactPointsResponse,
            AlertRuleTargetPayload,
            AlertRulePayload,
            AlertRuleInputPayload,
            AlertRulesResponse,
            AlertSilencePayload,
            AlertSilenceInputPayload,
            AlertSilencesResponse,
            AlertEventPayload,
            AlertRuleEventsResponse,
            AlertDeliveryAttemptPayload,
            AlertDeliveriesResponse,
            AlertRuleSimulationRequestPayload,
            AlertRuleSimulationWindowPayload,
            AlertSimulationSamplePayload,
            AlertSimulationResultPayload,
            AlertTestResponse,
            ArchiveStatus,
            HealthResponse,
            SseEventEnvelope,
        )
    ),
    tags(
        (name = "products", description = "Archived product resource endpoints"),
        (name = "features", description = "Archived spatial feature resource endpoints"),
        (name = "aggregates", description = "Archived aggregate resource endpoints"),
        (name = "issues", description = "Archived issue resource endpoints"),
        (name = "incidents", description = "Derived incident resource endpoints"),
        (name = "alerting", description = "Alerting control-plane, simulation, and audit endpoints"),
        (name = "streams", description = "Incremental server-sent event streams"),
        (name = "operational", description = "Operational health, metrics, and retained file endpoints")
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
    modifiers(&PublicSecurityRemover, &ArchiveFilterParamsFixup),
    paths(
        super::server_http::archive::products_handler,
        super::server_http::archive::features_handler,
        super::server_http::archive::features_geojson_handler,
        super::server_http::archive::facet_aggregate_handler,
        super::server_http::archive::timeseries_aggregate_handler,
        super::server_http::archive::cell_aggregate_handler,
        super::server_http::archive::incidents_handler,
        super::server_http::archive::incident_handler,
        super::server_http::archive::incident_products_handler,
        super::server_http::archive::archive_product_handler,
        super::server_http::archive::archive_product_raw_handler,
        super::server_http::archive::archive_issues_handler,
        super::server_http::archive::archive_issue_handler,
        super::server_http::alerting::list_contact_points_handler,
        super::server_http::alerting::create_contact_point_handler,
        super::server_http::alerting::get_contact_point_handler,
        super::server_http::alerting::update_contact_point_handler,
        super::server_http::alerting::delete_contact_point_handler,
        super::server_http::alerting::test_contact_point_handler,
        super::server_http::alerting::list_rules_handler,
        super::server_http::alerting::create_rule_handler,
        super::server_http::alerting::get_rule_handler,
        super::server_http::alerting::update_rule_handler,
        super::server_http::alerting::delete_rule_handler,
        super::server_http::alerting::simulate_rule_handler,
        super::server_http::alerting::simulate_persisted_rule_handler,
        super::server_http::alerting::list_rule_events_handler,
        super::server_http::alerting::list_deliveries_handler,
        super::server_http::alerting::list_silences_handler,
        super::server_http::alerting::create_silence_handler,
        super::server_http::alerting::delete_silence_handler,
        super::server_http::streams::incident_events_handler,
        super::server_http::streams::events_handler,
        super::server_http::operational::files_handler,
        super::server_http::operational::file_download_handler,
        super::server_http::operational::health_handler,
        super::server_http::operational::metrics_handler,
    ),
    components(
        schemas(
            FilesResponse,
            CompletedFilePayload,
            CompletedFileEventPayload,
            ProductsResponse,
            FeaturesResponse,
            ArchivedFeaturePayload,
            FeatureCollectionResponse,
            GeoJsonFeature,
            FacetAggregateResponse,
            FacetAggregateBucket,
            TimeseriesAggregateResponse,
            TimeseriesAggregateBucket,
            CellAggregateResponse,
            CellAggregateBucket,
            IncidentsResponse,
            IncidentSummaryPayload,
            IncidentResponse,
            IncidentDetailPayload,
            IncidentProductsResponse,
            ArchiveProductSummaryPayload,
            ArchiveProductResponse,
            ArchiveProductDetailPayload,
            ArchiveIssuesResponse,
            ArchiveIssueResponse,
            ArchiveIssuePayload,
            AlertContactPointPayload,
            AlertContactPointInputPayload,
            AlertContactPointsResponse,
            AlertRuleTargetPayload,
            AlertRulePayload,
            AlertRuleInputPayload,
            AlertRulesResponse,
            AlertSilencePayload,
            AlertSilenceInputPayload,
            AlertSilencesResponse,
            AlertEventPayload,
            AlertRuleEventsResponse,
            AlertDeliveryAttemptPayload,
            AlertDeliveriesResponse,
            AlertRuleSimulationRequestPayload,
            AlertRuleSimulationWindowPayload,
            AlertSimulationSamplePayload,
            AlertSimulationResultPayload,
            AlertTestResponse,
            ArchiveStatus,
            HealthResponse,
            SseEventEnvelope,
        )
    ),
    tags(
        (name = "products", description = "Archived product resource endpoints"),
        (name = "features", description = "Archived spatial feature resource endpoints"),
        (name = "aggregates", description = "Archived aggregate resource endpoints"),
        (name = "issues", description = "Archived issue resource endpoints"),
        (name = "incidents", description = "Derived incident resource endpoints"),
        (name = "alerting", description = "Alerting control-plane, simulation, and audit endpoints"),
        (name = "streams", description = "Incremental server-sent event streams"),
        (name = "operational", description = "Operational health, metrics, and retained file endpoints")
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

#[allow(dead_code)]
#[derive(Debug, ToSchema)]
pub(crate) struct SseEventEnvelope {
    #[schema(example = "42")]
    pub(crate) id: String,
    #[schema(example = "product_available")]
    pub(crate) event: String,
    #[schema(value_type = Object)]
    pub(crate) data: serde_json::Value,
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
