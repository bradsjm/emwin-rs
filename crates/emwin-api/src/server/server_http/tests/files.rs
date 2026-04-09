use super::build_state;
use crate::server::build_router;
use crate::server::server_http::operational::files_handler;
use axum::Json;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{Request, StatusCode};
use emwin_service::SourceKind;
use tower::ServiceExt;

#[tokio::test]
async fn files_download_accepts_url_encoded_nested_filename() {
    let state = build_state(
        10,
        emwin_live::LiveRuntime::new_for_tests(
            vec![(
                "nested/my file.txt".to_string(),
                b"hello world".to_vec(),
                1,
                SourceKind::Qbt,
            )],
            emwin_live::LiveTelemetry::Unavailable,
            None,
            None,
            None,
        ),
        None,
    );

    let app = build_router(state.clone(), None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/files/nested%2Fmy%20file.txt")
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
    assert_eq!(&body[..], b"hello world");
}

#[tokio::test]
async fn files_endpoint_serializes_enriched_metadata() {
    let state = build_state(
        10,
        emwin_live::LiveRuntime::new_for_tests(
            vec![(
                "TAFPDKGA.TXT".to_string(),
                b"000
FTUS42 KFFC 022320
TAFPDK
Body
"
                .to_vec(),
                1,
                SourceKind::Qbt,
            )],
            emwin_live::LiveTelemetry::Unavailable,
            None,
            None,
            None,
        ),
        None,
    );

    let Json(response) = files_handler(State(state)).await;
    let file = &response.files[0];
    assert_eq!(file.metadata.filename, "TAFPDKGA.TXT");
    assert_eq!(file.download_url, "/v1/files/TAFPDKGA.TXT");
    assert_eq!(file.metadata.product.pil.as_deref(), Some("TAF"));
    assert!(
        file.metadata
            .product
            .title
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    );
    assert_eq!(
        file.metadata
            .product
            .header
            .as_ref()
            .map(|value| value.ttaaii.as_str()),
        Some("FTUS42")
    );
    assert_eq!(file.metadata.product.pil.as_deref(), Some("TAF"));
    assert!(file.metadata.product.issues.is_empty());
    let metadata_json = serde_json::to_value(&file.metadata).expect("metadata should serialize");
    assert_eq!(metadata_json["product"]["schema_version"], 2);
    assert_eq!(metadata_json["product"]["header"]["kind"], "afos");
    assert!(metadata_json["product"].get("parsed").is_none());
    assert!(metadata_json["product"].get("wmo_header").is_none());
    assert!(
        metadata_json["product"]["office"]
            .get("office_name")
            .is_none()
    );
}
