use super::{test_state, test_state_with_auth};
use crate::server::build_router;
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
    assert!(body_text.contains("\"/v1/streams/products\""));
    assert!(body_text.contains("\"/v1/streams/incidents\""));
    assert!(body_text.contains("\"/v1/features\""));
    assert!(body_text.contains("\"/v1/features/geojson\""));
    assert!(body_text.contains("\"/v1/aggregates/facets\""));
    assert!(body_text.contains("\"/v1/aggregates/timeseries\""));
    assert!(body_text.contains("\"/v1/aggregates/cells\""));
    assert!(body_text.contains("\"/v1/incidents\""));
    assert!(body_text.contains("\"/v1/issues\""));
    assert!(body_text.contains("\"/v1/issues/{issue_id}\""));
    assert!(body_text.contains("\"/v1/products/{product_id}\""));
    assert!(body_text.contains("\"/v1/files\""));
    assert!(body_text.contains("\"/v1/health\""));
    assert!(body_text.contains("\"/v1/metrics\""));
    assert!(body_text.contains("\"partial\""));
    assert!(body_text.contains("\"approximate\""));
    assert!(body_text.contains("\"reason\""));
    assert!(body_text.contains("\"artifact_kind\""));
    assert!(body_text.contains("\"min_lat\""));
    assert!(body_text.contains("\"max_lat\""));
    assert!(body_text.contains("\"min_lon\""));
    assert!(body_text.contains("\"max_lon\""));
    assert!(!body_text.contains("\"/events\""));

    let body_json: serde_json::Value =
        serde_json::from_str(&body_text).expect("body should parse as json");
    let product_params = body_json["paths"]["/v1/products"]["get"]["parameters"]
        .as_array()
        .expect("products parameters should be an array");
    assert!(
        product_params
            .iter()
            .all(|param| param["name"] != "filters")
    );
    assert!(!body_text.contains("\"name\":\"filters\""));
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
    assert!(body_json["paths"]["/v1/health"]["get"]["security"].is_null());
    assert!(
        body_json["components"]["schemas"]["FacetAggregateResponseSchema"]["properties"]
            ["completeness"]
            .is_null()
    );
    assert_eq!(
        body_json["components"]["schemas"]["FacetAggregateResponseSchema"]["properties"]["partial"]
            ["type"],
        "boolean"
    );
    assert_eq!(
        body_json["components"]["schemas"]["FacetAggregateResponseSchema"]["properties"]["approximate"]
            ["type"],
        "boolean"
    );
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
        body_json["paths"]["/v1/health"]["get"]["security"][0]["bearer_auth"],
        serde_json::json!([])
    );
}
