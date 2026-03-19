use crate::live::file_pipeline::build_persist_request;
use crate::live::server::build_router;
use crate::live::server::types::{AppState, TelemetryPayload};
use crate::live::server_support::RetainedFiles;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use emwin_db::{CompletedFileMetadata, NoopMetadataSink, PersistenceConfig, PersistenceRuntime};
use emwin_protocol::ingest::ProductOrigin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::time::{Duration, Instant};
use tempfile::tempdir;
use tokio::sync::{broadcast, watch};
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
    let request = build_persist_request("TEST.TXT", b"payload", metadata)
        .expect("persist request should build");
    assert!(producer.enqueue(request).accepted);

    let (_, shutdown_rx) = watch::channel(false);
    let state = Arc::new(AppState {
        event_tx: broadcast::channel(32).0,
        incident_event_tx: broadcast::channel(32).0,
        shutdown_rx,
        retained_files: std::sync::Mutex::new(RetainedFiles::new(32, Duration::from_secs(60))),
        telemetry: std::sync::Mutex::new(TelemetryPayload::Unavailable),
        persistence: Some(producer),
        archive: None,
        connected_clients: AtomicUsize::new(0),
        max_clients: 10,
        next_event_id: AtomicU64::new(1),
        next_incident_event_id: AtomicU64::new(1),
        data_blocks_total: AtomicU64::new(0),
        received_servers: AtomicUsize::new(0),
        received_sat_servers: AtomicUsize::new(0),
        started_at: Instant::now(),
        upstream_endpoint: std::sync::Mutex::new(None),
        openapi_auth_token: None,
        quiet: true,
    });
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/live/metrics")
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
