use super::super::types::{AppState, FilesResponse, HealthResponse, MetricsPayload};
use super::support::archive_status;
use crate::server_support::{build_file_download_response, filename_request_or_400};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[utoipa::path(
    get,
    path = "/v1/files",
    tag = "operational",
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List retained completed files.", body = crate::server::openapi::FilesResponseSchema)
    )
)]
pub(super) async fn files_handler(State(state): State<Arc<AppState>>) -> Json<FilesResponse> {
    let files = state
        .services
        .list_retained_files()
        .into_iter()
        .map(super::super::types::CompletedFilePayload::from_metadata)
        .collect();
    Json(FilesResponse { files })
}

#[utoipa::path(
    get,
    path = "/v1/files/{filename}",
    tag = "operational",
    security(("bearer_auth" = [])),
    params(("filename" = String, Path, description = "URL-encoded retained file path")),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Download retained file contents.", content_type = "application/octet-stream"),
        (status = 400, description = "Requested filename is invalid.", body = String),
        (status = 404, description = "Retained file was not found.", body = String)
    )
)]
pub(super) async fn file_download_handler(
    State(state): State<Arc<AppState>>,
    Path(filename): Path<String>,
) -> Result<Response, StatusCode> {
    let normalized = filename_request_or_400(&filename)?;

    let file = state
        .services
        .get_retained_file(&normalized)
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(build_file_download_response(file))
}

#[utoipa::path(
    get,
    path = "/v1/health",
    tag = "operational",
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Live server health summary.", body = crate::server::openapi::HealthResponseSchema)
    )
)]
pub(super) async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let connected_clients = state.connected_clients.load(Ordering::Relaxed);
    let snapshot = state.services.stats_snapshot();
    let archive = archive_status(&state);

    Json(HealthResponse {
        status: if archive.healthy { "ok" } else { "degraded" },
        archive,
        connected_clients,
        retained_files: snapshot.retained_files,
        uptime_secs: snapshot.uptime_secs,
        upstream_endpoint: snapshot.upstream_endpoint,
    })
}

#[utoipa::path(
    get,
    path = "/v1/metrics",
    tag = "operational",
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Live server telemetry snapshot.", body = serde_json::Value)
    )
)]
pub(super) async fn metrics_handler(State(state): State<Arc<AppState>>) -> Json<MetricsPayload> {
    let telemetry = state.services.telemetry_snapshot();
    let persistence = state.services.stats_snapshot().persistence;
    Json(MetricsPayload {
        telemetry,
        persistence,
        archive: archive_status(&state),
    })
}
