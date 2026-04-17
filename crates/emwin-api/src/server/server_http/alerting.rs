use super::super::types::{
    AlertContactPointInputPayload, AlertContactPointPayload, AlertContactPointsResponse,
    AlertDeliveriesResponse, AlertDeliveryAttemptPayload, AlertRuleEventsResponse,
    AlertRuleInputPayload, AlertRulePayload, AlertRuleSimulationRequestPayload,
    AlertRuleSimulationWindowPayload, AlertRulesResponse, AlertSilenceInputPayload,
    AlertSilencePayload, AlertSilencesResponse, AlertSimulationResultPayload, AlertTestResponse,
    AppState,
};
use super::support::{alert_store, map_alert_error};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use emwin_service::{AlertMatchCriteria, AlertSourceKind};
use std::sync::Arc;

#[utoipa::path(
    get,
    path = "/v1/alerting/contact-points",
    tag = "alerting",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "List alerting contact points.", body = crate::server::types::AlertContactPointsResponse),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn list_contact_points_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AlertContactPointsResponse>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let items = store
        .list_alert_contact_points()
        .await
        .map_err(map_alert_error)?
        .into_iter()
        .map(AlertContactPointPayload::from_contact_point)
        .collect::<serde_json::Result<Vec<_>>>()
        .map_err(internal_error)?;
    Ok(Json(AlertContactPointsResponse { items }))
}

#[utoipa::path(
    post,
    path = "/v1/alerting/contact-points",
    tag = "alerting",
    security(("bearer_auth" = [])),
    request_body = crate::server::types::AlertContactPointInputPayload,
    responses(
        (status = 200, description = "Create an alerting contact point.", body = crate::server::types::AlertContactPointPayload),
        (status = 400, description = "Validation failed.", body = String),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn create_contact_point_handler(
    State(state): State<Arc<AppState>>,
    Json(input): Json<AlertContactPointInputPayload>,
) -> Result<Json<AlertContactPointPayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let contact_point = store
        .create_alert_contact_point(input.into_domain().map_err(invalid_json)?)
        .await
        .map_err(map_alert_error)?;
    Ok(Json(
        AlertContactPointPayload::from_contact_point(contact_point).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/alerting/contact-points/{id}",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert contact point id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Get one alerting contact point.", body = crate::server::types::AlertContactPointPayload),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Contact point not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn get_contact_point_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<AlertContactPointPayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let contact_point = store
        .get_alert_contact_point(id)
        .await
        .map_err(map_alert_error)?
        .ok_or_else(not_found_contact_point)?;
    Ok(Json(
        AlertContactPointPayload::from_contact_point(contact_point).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    patch,
    path = "/v1/alerting/contact-points/{id}",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert contact point id")),
    security(("bearer_auth" = [])),
    request_body = crate::server::types::AlertContactPointInputPayload,
    responses(
        (status = 200, description = "Update an alerting contact point.", body = crate::server::types::AlertContactPointPayload),
        (status = 400, description = "Validation failed.", body = String),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Contact point not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn update_contact_point_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<AlertContactPointInputPayload>,
) -> Result<Json<AlertContactPointPayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let contact_point = store
        .update_alert_contact_point(id, input.into_domain().map_err(invalid_json)?)
        .await
        .map_err(map_alert_error)?
        .ok_or_else(not_found_contact_point)?;
    Ok(Json(
        AlertContactPointPayload::from_contact_point(contact_point).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    delete,
    path = "/v1/alerting/contact-points/{id}",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert contact point id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 204, description = "Deleted."),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Contact point not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn delete_contact_point_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let store = alert_store(&state)?;
    if !store
        .delete_alert_contact_point(id)
        .await
        .map_err(map_alert_error)?
    {
        return Err(not_found_contact_point());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/v1/alerting/contact-points/{id}/test",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert contact point id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Contact point test send finished.", body = crate::server::types::AlertTestResponse),
        (status = 400, description = "Validation failed.", body = String),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Contact point not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn test_contact_point_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<AlertTestResponse>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let contact_point = store
        .get_alert_contact_point_record(id)
        .await
        .map_err(map_alert_error)?
        .ok_or_else(not_found_contact_point)?;
    if !contact_point.enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            "contact point is disabled".to_string(),
        ));
    }
    let result = emwin_alert::send_test_notification(
        &contact_point.config,
        &emwin_alert::AlertDispatchConfig {
            apprise_api_url: state.alerting_apprise_api_url.clone(),
            http_timeout: std::time::Duration::from_secs(30),
        },
        &emwin_alert::TestAlertNotification {
            title: format!("emwin-rs contact point test: {}", contact_point.name),
            body: "This is a test notification from the emwin-rs alerting control plane."
                .to_string(),
        },
    )
    .await
    .map_err(map_alert_transport_error)?;
    Ok(Json(AlertTestResponse {
        delivered: true,
        response_code: result.response_code,
        response_excerpt: result.response_excerpt,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/alerting/rules",
    tag = "alerting",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "List alert rules.", body = crate::server::types::AlertRulesResponse),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn list_rules_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AlertRulesResponse>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let items = store
        .list_alert_rules()
        .await
        .map_err(map_alert_error)?
        .into_iter()
        .map(AlertRulePayload::from_rule)
        .collect::<serde_json::Result<Vec<_>>>()
        .map_err(internal_error)?;
    Ok(Json(AlertRulesResponse { items }))
}

#[utoipa::path(
    post,
    path = "/v1/alerting/rules",
    tag = "alerting",
    security(("bearer_auth" = [])),
    request_body = crate::server::types::AlertRuleInputPayload,
    responses(
        (status = 200, description = "Create an alert rule.", body = crate::server::types::AlertRulePayload),
        (status = 400, description = "Validation failed.", body = String),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn create_rule_handler(
    State(state): State<Arc<AppState>>,
    Json(input): Json<AlertRuleInputPayload>,
) -> Result<Json<AlertRulePayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let rule = store
        .create_alert_rule(input.into_domain().map_err(invalid_json)?)
        .await
        .map_err(map_alert_error)?;
    Ok(Json(
        AlertRulePayload::from_rule(rule).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/alerting/rules/{id}",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert rule id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Get one alert rule.", body = crate::server::types::AlertRulePayload),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Alert rule not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn get_rule_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<AlertRulePayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let rule = store
        .get_alert_rule(id)
        .await
        .map_err(map_alert_error)?
        .ok_or_else(not_found_rule)?;
    Ok(Json(
        AlertRulePayload::from_rule(rule).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    patch,
    path = "/v1/alerting/rules/{id}",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert rule id")),
    security(("bearer_auth" = [])),
    request_body = crate::server::types::AlertRuleInputPayload,
    responses(
        (status = 200, description = "Update an alert rule.", body = crate::server::types::AlertRulePayload),
        (status = 400, description = "Validation failed.", body = String),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Alert rule not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn update_rule_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<AlertRuleInputPayload>,
) -> Result<Json<AlertRulePayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let rule = store
        .update_alert_rule(id, input.into_domain().map_err(invalid_json)?)
        .await
        .map_err(map_alert_error)?
        .ok_or_else(not_found_rule)?;
    Ok(Json(
        AlertRulePayload::from_rule(rule).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    delete,
    path = "/v1/alerting/rules/{id}",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert rule id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 204, description = "Deleted."),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Alert rule not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn delete_rule_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let store = alert_store(&state)?;
    if !store.delete_alert_rule(id).await.map_err(map_alert_error)? {
        return Err(not_found_rule());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/v1/alerting/rules/simulate",
    tag = "alerting",
    security(("bearer_auth" = [])),
    request_body = crate::server::types::AlertRuleSimulationRequestPayload,
    responses(
        (status = 200, description = "Simulate a draft alert rule.", body = crate::server::types::AlertSimulationResultPayload),
        (status = 400, description = "Validation failed.", body = String),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn simulate_rule_handler(
    State(state): State<Arc<AppState>>,
    Json(input): Json<AlertRuleSimulationRequestPayload>,
) -> Result<Json<AlertSimulationResultPayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let request = input.into_domain().map_err(invalid_json)?;
    validate_simulation_window(store, &request.criteria, request.start).await?;
    let result = store
        .simulate_alerts(&request)
        .await
        .map_err(map_alert_error)?;
    Ok(Json(
        AlertSimulationResultPayload::from_result(result).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    post,
    path = "/v1/alerting/rules/{id}/simulate",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert rule id")),
    security(("bearer_auth" = [])),
    request_body = crate::server::types::AlertRuleSimulationWindowPayload,
    responses(
        (status = 200, description = "Simulate a persisted alert rule.", body = crate::server::types::AlertSimulationResultPayload),
        (status = 400, description = "Validation failed.", body = String),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Alert rule not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn simulate_persisted_rule_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<AlertRuleSimulationWindowPayload>,
) -> Result<Json<AlertSimulationResultPayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let rule = store
        .get_alert_rule(id)
        .await
        .map_err(map_alert_error)?
        .ok_or_else(not_found_rule)?;
    validate_simulation_window(store, &rule.criteria, input.start).await?;
    let result = store
        .simulate_alerts(&input.with_criteria(rule.criteria))
        .await
        .map_err(map_alert_error)?;
    Ok(Json(
        AlertSimulationResultPayload::from_result(result).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    get,
    path = "/v1/alerting/rules/{id}/events",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert rule id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "List persisted alert events for a rule.", body = crate::server::types::AlertRuleEventsResponse),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn list_rule_events_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<AlertRuleEventsResponse>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let items = store
        .list_alert_rule_events(id)
        .await
        .map_err(map_alert_error)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(AlertRuleEventsResponse { items }))
}

#[utoipa::path(
    get,
    path = "/v1/alerting/deliveries",
    tag = "alerting",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "List persisted delivery attempts.", body = crate::server::types::AlertDeliveriesResponse),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn list_deliveries_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AlertDeliveriesResponse>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let items = store
        .list_alert_deliveries()
        .await
        .map_err(map_alert_error)?
        .into_iter()
        .map(AlertDeliveryAttemptPayload::from_attempt)
        .collect::<serde_json::Result<Vec<_>>>()
        .map_err(internal_error)?;
    Ok(Json(AlertDeliveriesResponse { items }))
}

#[utoipa::path(
    get,
    path = "/v1/alerting/silences",
    tag = "alerting",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "List alert silences.", body = crate::server::types::AlertSilencesResponse),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn list_silences_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AlertSilencesResponse>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let items = store
        .list_alert_silences()
        .await
        .map_err(map_alert_error)?
        .into_iter()
        .map(AlertSilencePayload::from_silence)
        .collect::<serde_json::Result<Vec<_>>>()
        .map_err(internal_error)?;
    Ok(Json(AlertSilencesResponse { items }))
}

#[utoipa::path(
    post,
    path = "/v1/alerting/silences",
    tag = "alerting",
    security(("bearer_auth" = [])),
    request_body = crate::server::types::AlertSilenceInputPayload,
    responses(
        (status = 200, description = "Create an alert silence.", body = crate::server::types::AlertSilencePayload),
        (status = 400, description = "Validation failed.", body = String),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn create_silence_handler(
    State(state): State<Arc<AppState>>,
    Json(input): Json<AlertSilenceInputPayload>,
) -> Result<Json<AlertSilencePayload>, (StatusCode, String)> {
    let store = alert_store(&state)?;
    let silence = store
        .create_alert_silence(input.into_domain().map_err(invalid_json)?)
        .await
        .map_err(map_alert_error)?;
    Ok(Json(
        AlertSilencePayload::from_silence(silence).map_err(internal_error)?,
    ))
}

#[utoipa::path(
    delete,
    path = "/v1/alerting/silences/{id}",
    tag = "alerting",
    params(("id" = i64, Path, description = "Alert silence id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 204, description = "Deleted."),
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 404, description = "Alert silence not found.", body = String),
        (status = 503, description = "Alerting persistence is not configured.", body = String)
    )
)]
pub(super) async fn delete_silence_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let store = alert_store(&state)?;
    if !store
        .delete_alert_silence(id)
        .await
        .map_err(map_alert_error)?
    {
        return Err((
            StatusCode::NOT_FOUND,
            format!("alert silence {id} was not found"),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn validate_simulation_window(
    store: &emwin_db::PostgresMetadataSink,
    criteria: &AlertMatchCriteria,
    start: chrono::DateTime<chrono::Utc>,
) -> Result<(), (StatusCode, String)> {
    if !matches!(criteria, AlertMatchCriteria::IncidentChange(_)) {
        return Ok(());
    }
    let first_retained = store
        .first_alert_source_event_timestamp(AlertSourceKind::IncidentChange)
        .await
        .map_err(map_alert_error)?;
    match first_retained {
        Some(first_retained) if start < first_retained => Err((
            StatusCode::BAD_REQUEST,
            format!(
                "incident simulation is only available from {first_retained} onward because alerting source-event retention starts at rollout"
            ),
        )),
        Some(_) => Ok(()),
        None => Err((
            StatusCode::BAD_REQUEST,
            "incident simulation is unavailable because no retained incident change source events exist yet".to_string(),
        )),
    }
}

fn invalid_json(err: serde_json::Error) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}

fn internal_error(err: serde_json::Error) -> (StatusCode, String) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("failed to encode alerting payload: {err}"),
    )
}

fn map_alert_transport_error(err: emwin_alert::AlertError) -> (StatusCode, String) {
    match err {
        emwin_alert::AlertError::InvalidConfig(message) => (StatusCode::BAD_REQUEST, message),
        other => (StatusCode::BAD_GATEWAY, other.to_string()),
    }
}

fn not_found_contact_point() -> (StatusCode, String) {
    (
        StatusCode::NOT_FOUND,
        "alert contact point was not found".to_string(),
    )
}

fn not_found_rule() -> (StatusCode, String) {
    (
        StatusCode::NOT_FOUND,
        "alert rule was not found".to_string(),
    )
}
