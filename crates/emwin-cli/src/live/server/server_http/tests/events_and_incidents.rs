use super::{empty_events_query, incident_event_payload, test_state, test_state_with_archive};
use crate::live::server::build_router;
use crate::live::server::server_http::{events_handler, incident_events_handler};
use crate::live::server::types::IncidentEventsQuery;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tower::ServiceExt;

#[tokio::test]
async fn events_handler_rejects_when_client_limit_reached() {
    let state = test_state(1);
    state.connected_clients.store(1, Ordering::Relaxed);

    let result = events_handler(
        State(state),
        ConnectInfo("127.0.0.1:4000".parse().expect("valid socket addr")),
        HeaderMap::new(),
        Query(empty_events_query()),
    )
    .await;

    assert!(matches!(result, Err((StatusCode::TOO_MANY_REQUESTS, _))));
}

#[tokio::test]
async fn incidents_endpoint_returns_service_unavailable_without_archive_database() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/live/incidents")
                .method("GET")
                .body(axum::body::Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn incident_events_endpoint_returns_service_unavailable_without_archive_database() {
    let state = test_state(10);
    let result = incident_events_handler(
        State(state),
        ConnectInfo("127.0.0.1:4000".parse().expect("valid socket addr")),
        HeaderMap::new(),
        Query(IncidentEventsQuery {
            action: None,
            office: None,
            phenomena: None,
            significance: None,
            status: None,
            etn: None,
        }),
    )
    .await;

    assert!(matches!(result, Err((StatusCode::SERVICE_UNAVAILABLE, _))));
}

#[tokio::test]
async fn incident_events_handler_streams_incident_change_payloads() {
    let state = test_state_with_archive(10);
    let result = incident_events_handler(
        State(Arc::clone(&state)),
        ConnectInfo("127.0.0.1:4000".parse().expect("valid socket addr")),
        HeaderMap::new(),
        Query(IncidentEventsQuery {
            action: Some("created".to_string()),
            office: Some("KOAX".to_string()),
            phenomena: None,
            significance: None,
            status: Some("active".to_string()),
            etn: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "handler should accept configured incident SSE"
    );
    let payload = incident_event_payload();
    let json = serde_json::to_value(&payload).expect("payload should serialize");
    assert_eq!(json["action"], "created");
    assert_eq!(json["trigger"], "persist");
    assert_eq!(json["incident"]["office"], "KOAX");
    assert_eq!(
        json["incident"]["detail_url"],
        "/v1/live/incidents/KOAX/FF/W/2001"
    );
}

#[tokio::test]
async fn archive_product_raw_endpoint_returns_service_unavailable_without_archive_database() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/archive/products/1/raw")
                .method("GET")
                .body(axum::body::Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
