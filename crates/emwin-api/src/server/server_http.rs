//! HTTP and SSE handlers for live server mode.
//!
//! Router wiring stays here; feature handlers live in focused submodules.

pub(super) mod alerting;
pub(super) mod archive;
mod auth;
pub(super) mod operational;
pub(super) mod streams;
mod support;

use super::types::AppState;
use axum::Router;
use axum::middleware::{self};
use axum::routing::get;
use std::sync::Arc;

/// Builds the Axum router for live server mode.
pub(super) fn build_router(state: Arc<AppState>, cors: tower_http::cors::CorsLayer) -> Router {
    let auth_enabled = state.openapi_auth_token.is_some();
    let api_router = Router::new()
        .route("/products", get(archive::products_handler))
        .route(
            "/products/{product_id}",
            get(archive::archive_product_handler),
        )
        .route(
            "/products/{product_id}/raw",
            get(archive::archive_product_raw_handler),
        )
        .route("/features", get(archive::features_handler))
        .route("/features/geojson", get(archive::features_geojson_handler))
        .route("/aggregates/facets", get(archive::facet_aggregate_handler))
        .route(
            "/aggregates/timeseries",
            get(archive::timeseries_aggregate_handler),
        )
        .route("/aggregates/cells", get(archive::cell_aggregate_handler))
        .route("/issues", get(archive::archive_issues_handler))
        .route("/issues/{issue_id}", get(archive::archive_issue_handler))
        .route("/incidents", get(archive::incidents_handler))
        .route(
            "/incidents/{office}/{phenomena}/{significance}/{etn}",
            get(archive::incident_handler),
        )
        .route(
            "/incidents/{office}/{phenomena}/{significance}/{etn}/products",
            get(archive::incident_products_handler),
        )
        .route("/streams/incidents", get(streams::incident_events_handler))
        .route("/streams/products", get(streams::events_handler))
        .route(
            "/alerting/contact-points",
            get(alerting::list_contact_points_handler).post(alerting::create_contact_point_handler),
        )
        .route(
            "/alerting/contact-points/{id}",
            get(alerting::get_contact_point_handler)
                .patch(alerting::update_contact_point_handler)
                .delete(alerting::delete_contact_point_handler),
        )
        .route(
            "/alerting/contact-points/{id}/test",
            axum::routing::post(alerting::test_contact_point_handler),
        )
        .route(
            "/alerting/rules",
            get(alerting::list_rules_handler).post(alerting::create_rule_handler),
        )
        .route(
            "/alerting/rules/simulate",
            axum::routing::post(alerting::simulate_rule_handler),
        )
        .route(
            "/alerting/rules/{id}",
            get(alerting::get_rule_handler)
                .patch(alerting::update_rule_handler)
                .delete(alerting::delete_rule_handler),
        )
        .route(
            "/alerting/rules/{id}/simulate",
            axum::routing::post(alerting::simulate_persisted_rule_handler),
        )
        .route(
            "/alerting/rules/{id}/events",
            get(alerting::list_rule_events_handler),
        )
        .route(
            "/alerting/deliveries",
            get(alerting::list_deliveries_handler),
        )
        .route(
            "/alerting/silences",
            get(alerting::list_silences_handler).post(alerting::create_silence_handler),
        )
        .route(
            "/alerting/silences/{id}",
            axum::routing::delete(alerting::delete_silence_handler),
        )
        .route("/files", get(operational::files_handler))
        .route(
            "/files/{*filename}",
            get(operational::file_download_handler),
        )
        .route("/health", get(operational::health_handler))
        .route("/metrics", get(operational::metrics_handler))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth::require_bearer_auth,
        ));

    Router::new()
        .merge(super::openapi::swagger_ui_mount(auth_enabled))
        .nest(super::types::API_PREFIX, api_router)
        .layer(cors)
        .with_state(state)
}

#[cfg(test)]
mod tests;
