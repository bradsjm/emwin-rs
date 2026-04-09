use super::super::types::AppState;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

pub(super) async fn require_bearer_auth(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected_token) = state.openapi_auth_token.as_deref() else {
        return next.run(request).await;
    };

    let Some(header_value) = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return unauthorized_response();
    };

    let Some(provided_token) = parse_bearer_token(header_value) else {
        return unauthorized_response();
    };

    if provided_token != expected_token {
        return unauthorized_response();
    }

    next.run(request).await
}

fn unauthorized_response() -> Response {
    (StatusCode::UNAUTHORIZED, "missing or invalid bearer token").into_response()
}

fn parse_bearer_token(header_value: &str) -> Option<&str> {
    let mut parts = header_value.split_ascii_whitespace();
    let scheme = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some(token)
}
