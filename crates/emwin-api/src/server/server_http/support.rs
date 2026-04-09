use super::super::types::{AppState, ArchiveFilterParams, ArchiveStatus};
use axum::http::StatusCode;
use emwin_service::ServiceError;
use std::sync::Arc;

pub(super) fn parse_archive_filters(
    raw_query: Option<&str>,
) -> Result<ArchiveFilterParams, String> {
    if let Some(raw_query) = raw_query {
        for (key, _) in url::form_urlencoded::parse(raw_query.as_bytes()) {
            let key = key.as_ref();
            if key == "filters" || key.starts_with("filters.") || key.starts_with("filters[") {
                return Err(format!(
                    "unsupported nested archive filter parameter `{key}`"
                ));
            }
        }
        serde_urlencoded::from_str(raw_query)
            .map_err(|err| format!("Failed to deserialize query string: {err}"))
    } else {
        Ok(ArchiveFilterParams::default())
    }
}

pub(super) fn archive_status(state: &Arc<AppState>) -> ArchiveStatus {
    let snapshot = state.services.archive_status_snapshot();
    ArchiveStatus {
        configured: snapshot.configured,
        healthy: snapshot.healthy,
        errors_total: snapshot.errors_total,
        pool_timeouts_total: snapshot.pool_timeouts_total,
        last_error: snapshot.last_error,
    }
}

pub(super) fn archive_service(
    state: &Arc<AppState>,
) -> Result<&dyn emwin_service::ArchiveQueryService, (StatusCode, String)> {
    Ok(state.services.archive.as_ref())
}

pub(super) fn normalize_incident_key(
    office: String,
    phenomena: String,
    significance: String,
    etn: i64,
) -> emwin_service::IncidentKey {
    emwin_service::IncidentKey {
        office: office.trim().to_ascii_uppercase(),
        phenomena: phenomena.trim().to_ascii_uppercase(),
        significance: significance.trim().to_ascii_uppercase(),
        etn,
    }
}

pub(super) fn map_archive_error(err: ServiceError) -> (StatusCode, String) {
    match err {
        ServiceError::InvalidRequest(message) | ServiceError::InvalidConfig(message) => {
            (StatusCode::BAD_REQUEST, message)
        }
        ServiceError::NotConfigured(message) => (StatusCode::SERVICE_UNAVAILABLE, message),
        ServiceError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, io.to_string())
        }
        other => (StatusCode::BAD_GATEWAY, other.to_string()),
    }
}
