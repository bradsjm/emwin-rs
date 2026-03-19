use super::{test_state, test_state_with_auth};
use crate::live::server::build_router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn root_endpoint_serves_swagger_ui() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .method("GET")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.starts_with("text/html")),
        Some(true)
    );
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let body_text = String::from_utf8(body.to_vec()).expect("body should be utf8 html");
    assert!(body_text.contains("swagger-ui"));
}

#[tokio::test]
async fn openapi_json_lists_versioned_routes() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .method("GET")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let body_text = String::from_utf8(body.to_vec()).expect("body should be utf8 json");
    assert!(body_text.contains("\"/v1/live/events\""));
    assert!(body_text.contains("\"/v1/live/incident-events\""));
    assert!(body_text.contains("\"/v1/live/incidents\""));
    assert!(body_text.contains("\"/v1/archive/products/{product_id}\""));
    assert!(body_text.contains("\"/v1/live/files\""));
    assert!(body_text.contains("\"/v1/live/health\""));
    assert!(body_text.contains("\"/v1/live/metrics\""));
    assert!(!body_text.contains("\"/events\""));
}

#[tokio::test]
async fn auth_enabled_keeps_docs_routes_public() {
    let state = test_state_with_auth(10, "secret-token");
    let app = build_router(state, None).expect("router should build");

    for path in ["/", "/openapi.json", "/swagger-ui.css"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method("GET")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK, "path={path}");
    }
}

#[tokio::test]
async fn openapi_json_omits_bearer_security_when_auth_disabled() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .method("GET")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let body_json: serde_json::Value =
        serde_json::from_slice(&body).expect("body should be utf8 json");
    assert!(body_json["components"]["securitySchemes"]["bearer_auth"].is_null());
    assert!(body_json["paths"]["/v1/live/health"]["get"]["security"].is_null());
}

#[tokio::test]
async fn openapi_json_declares_bearer_security_when_auth_enabled() {
    let state = test_state_with_auth(10, "secret-token");
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .method("GET")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let body_json: serde_json::Value =
        serde_json::from_slice(&body).expect("body should be utf8 json");
    assert_eq!(
        body_json["components"]["securitySchemes"]["bearer_auth"]["type"],
        "http"
    );
    assert_eq!(
        body_json["components"]["securitySchemes"]["bearer_auth"]["scheme"],
        "bearer"
    );
    assert_eq!(
        body_json["paths"]["/v1/live/health"]["get"]["security"][0]["bearer_auth"],
        serde_json::json!([])
    );
}
