use super::test_state_with_auth;
use crate::live::server::build_router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_REQUEST_HEADERS, ACCESS_CONTROL_REQUEST_METHOD,
    ORIGIN,
};
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn auth_enabled_rejects_missing_or_invalid_bearer_tokens() {
    let state = test_state_with_auth(10, "secret-token");
    let app = build_router(state, None).expect("router should build");

    for header in [
        None,
        Some("Basic secret-token"),
        Some("Bearer"),
        Some("Bearer wrong-token"),
    ] {
        let mut request = Request::builder().uri("/v1/health").method("GET");
        if let Some(value) = header {
            request = request.header("authorization", value);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).expect("request should build"))
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn auth_enabled_allows_protected_routes_with_valid_bearer_token() {
    let state = test_state_with_auth(10, "secret-token");
    let app = build_router(state, None)
        .expect("router should build")
        .layer(MockConnectInfo(
            "127.0.0.1:4000"
                .parse::<std::net::SocketAddr>()
                .expect("valid socket addr"),
        ));

    for path in ["/v1/health", "/v1/streams/products"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method("GET")
                    .header("authorization", "Bearer secret-token")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK, "path={path}");
    }
}

#[tokio::test]
async fn auth_enabled_accepts_case_insensitive_bearer_scheme_and_ascii_whitespace() {
    let state = test_state_with_auth(10, "secret-token");
    let app = build_router(state, None)
        .expect("router should build")
        .layer(MockConnectInfo(
            "127.0.0.1:4000"
                .parse::<std::net::SocketAddr>()
                .expect("valid socket addr"),
        ));

    for header in [
        "bearer secret-token",
        "Bearer\tsecret-token",
        "BEARER   secret-token",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .method("GET")
                    .header("authorization", header)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK, "header={header}");
    }
}

#[tokio::test]
async fn cors_preflight_allows_authorization_header() {
    let state = test_state_with_auth(10, "secret-token");
    let app = build_router(state, Some("*".to_string())).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .method(Method::OPTIONS)
                .header(ORIGIN, "https://example.com")
                .header(ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let allow_headers = response
        .headers()
        .get(ACCESS_CONTROL_ALLOW_HEADERS)
        .and_then(|value| value.to_str().ok())
        .expect("cors should advertise allowed headers");
    assert!(allow_headers.to_ascii_lowercase().contains("authorization"));
}
