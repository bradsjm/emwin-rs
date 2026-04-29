use super::build_state;
use crate::server::build_router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use emwin_db::{
    BlobEntry, BlobRole, NoopMetadataSink, PersistRequest, PersistenceConfig, PersistenceRuntime,
    build_completed_file_metadata,
};
use emwin_service::SourceKind;
use tempfile::tempdir;
use tower::ServiceExt;
use url::Url;

#[tokio::test]
async fn metrics_endpoint_includes_persistence_fields_when_enabled() {
    let temp = tempdir().expect("tempdir should succeed");
    let runtime = PersistenceRuntime::spawn(
        PersistenceConfig::new(4),
        emwin_db::ObjectStoreBlobWriter::new(
            Url::from_directory_path(temp.path()).expect("directory url should build"),
        )
        .expect("writer should build"),
        NoopMetadataSink,
    );
    let producer = runtime.producer();
    let metadata = build_completed_file_metadata(
        "TEST.TXT",
        1,
        SourceKind::Qbt,
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
        emwin_live::test_support::runtime()
            .telemetry(emwin_live::LiveTelemetry::Unavailable)
            .persistence(producer)
            .build(),
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
    assert_eq!(
        value.get("persistence_retry_exhausted_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("persistence_stale_dropped_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("processing_queue_len"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("processing_queue_capacity"),
        Some(&serde_json::json!(32))
    );
    assert_eq!(
        value.get("processing_enqueued_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("processing_evicted_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("processing_completed_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("processing_failed_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("archive_configured"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(value.get("archive_healthy"), Some(&serde_json::json!(true)));
    assert_eq!(
        value.get("archive_errors_total"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        value.get("archive_pool_timeouts_total"),
        Some(&serde_json::json!(0))
    );

    runtime.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn metrics_endpoint_flattens_qbt_active_servers() {
    let state = build_state(
        10,
        emwin_live::test_support::runtime()
            .telemetry(emwin_live::LiveTelemetry::Qbt(serde_json::json!({
                "receiver": "qbt",
                "active_servers": 4,
                "server_list_updates_total": 2
            })))
            .active_servers(4)
            .build(),
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
    assert_eq!(value.get("active_servers"), Some(&serde_json::json!(4)));
    assert_eq!(
        value.get("server_list_updates_total"),
        Some(&serde_json::json!(2))
    );
}

#[tokio::test]
async fn health_endpoint_reports_archive_degraded_when_archive_error_present() {
    let state = build_state(
        10,
        emwin_live::test_support::runtime()
            .telemetry(emwin_live::LiveTelemetry::Unavailable)
            .archive(emwin_db::PostgresMetadataSink::new(
                emwin_db::PostgresConfig::new("postgres://example.invalid/emwin"),
            ))
            .archive_status("pool timed out while waiting for an open connection", 1, 1)
            .build(),
        None,
    );

    let app = build_router(state, None).expect("router should build");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/health")
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
    assert_eq!(value["status"], "degraded");
    assert_eq!(value["archive"]["configured"], true);
    assert_eq!(value["archive"]["healthy"], false);
    assert_eq!(
        value["archive"]["last_error"],
        "pool timed out while waiting for an open connection"
    );
}
