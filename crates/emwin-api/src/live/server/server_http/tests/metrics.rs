use super::build_state;
use crate::live::server::build_router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use emwin_db::{
    BlobEntry, BlobRole, CompletedFileMetadata, NoopMetadataSink, PersistRequest,
    PersistenceConfig, PersistenceRuntime,
};
use emwin_protocol::ingest::ProductOrigin;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn metrics_endpoint_includes_persistence_fields_when_enabled() {
    let temp = tempdir().expect("tempdir should succeed");
    let runtime = PersistenceRuntime::spawn(
        PersistenceConfig::new(4),
        emwin_db::FilesystemBlobWriter::new(temp.path().to_path_buf()),
        NoopMetadataSink,
    );
    let producer = runtime.producer();
    let metadata = CompletedFileMetadata::build(
        "TEST.TXT",
        1,
        ProductOrigin::Qbt,
        b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n",
    );
    let request = PersistRequest {
        request_key: "TEST.TXT".to_string(),
        metadata,
        blobs: vec![BlobEntry::new(
            BlobRole::Payload,
            "TEST.TXT",
            b"payload".to_vec(),
            Some("application/octet-stream"),
        )],
    };
    assert!(producer.enqueue(request).accepted);

    let state = build_state(
        10,
        emwin_live::LiveRuntime::new_for_tests(
            Vec::new(),
            emwin_live::LiveTelemetry::Unavailable,
            None,
            Some(producer),
            None,
        ),
        None,
    );
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/metrics")
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
    let value: serde_json::Value =
        serde_json::from_slice(&body).expect("body should be json object");
    assert_eq!(
        value.get("receiver"),
        Some(&serde_json::json!("unavailable"))
    );
    assert_eq!(
        value.get("persistence_queue_len"),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        value.get("persistence_queue_capacity"),
        Some(&serde_json::json!(4))
    );
    assert_eq!(
        value.get("persistence_enqueued_total"),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        value.get("persistence_evicted_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("persistence_persisted_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("persistence_failed_total"),
        Some(&serde_json::json!(0))
    );

    runtime.shutdown().await.expect("shutdown should succeed");
}
