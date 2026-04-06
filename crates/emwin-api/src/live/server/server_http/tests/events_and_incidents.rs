use super::{
    archive_issue_payload, empty_events_query, incident_event_payload, test_state,
    test_state_with_archive,
};
use crate::live::server::build_router;
use crate::live::server::server_http::{
    archive_issue_handler, events_handler, incident_events_handler,
};
use crate::live::server::types::IncidentEventsQuery;
use axum::body::to_bytes;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use emwin_db::MetadataSink;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tempfile::tempdir;
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
async fn events_handler_invalid_query_does_not_leak_client_slot() {
    let state = test_state(1);

    let result = events_handler(
        State(Arc::clone(&state)),
        ConnectInfo("127.0.0.1:4000".parse().expect("valid socket addr")),
        HeaderMap::new(),
        Query(crate::live::server::types::EventsQuery {
            min_size: Some(10),
            max_size: Some(1),
            ..empty_events_query()
        }),
    )
    .await;

    assert!(matches!(result, Err((StatusCode::BAD_REQUEST, _))));
    assert_eq!(state.connected_clients.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn events_handler_releases_client_slot_when_stream_dropped() {
    let state = test_state(1);

    let stream = events_handler(
        State(Arc::clone(&state)),
        ConnectInfo("127.0.0.1:4000".parse().expect("valid socket addr")),
        HeaderMap::new(),
        Query(empty_events_query()),
    )
    .await
    .expect("stream should build");

    assert_eq!(state.connected_clients.load(Ordering::Relaxed), 1);
    drop(stream);
    assert_eq!(state.connected_clients.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn incidents_endpoint_returns_service_unavailable_without_archive_database() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/incidents")
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
        "/v1/incidents/KOAX/FF/W/2001"
    );
}

#[tokio::test]
async fn archive_product_raw_endpoint_returns_service_unavailable_without_archive_database() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/products/1/raw")
                .method("GET")
                .body(axum::body::Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn archive_issues_endpoint_returns_service_unavailable_without_archive_database() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/issues")
                .method("GET")
                .body(axum::body::Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn archive_issue_endpoint_returns_service_unavailable_without_archive_database() {
    let state = test_state(10);
    let result = archive_issue_handler(State(state), axum::extract::Path(7)).await;

    assert!(matches!(result, Err((StatusCode::SERVICE_UNAVAILABLE, _))));
}

#[tokio::test]
async fn feature_and_aggregate_endpoints_return_service_unavailable_without_archive_database() {
    let state = test_state(10);
    let app = build_router(state, None).expect("router should build");

    for path in [
        "/v1/features",
        "/v1/features/geojson",
        "/v1/aggregates/facets?dimension=office",
        "/v1/aggregates/timeseries?measure=product_count&start=2025-03-05T12:00:00Z&end=2025-03-05T13:00:00Z&bucket=hour",
        "/v1/aggregates/cells?measure=product_count&precision=5",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method("GET")
                    .body(axum::body::Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "path={path}"
        );
    }
}

#[tokio::test]
async fn feature_and_aggregate_endpoints_reject_invalid_bbox_queries() {
    let state = test_state_with_archive(10);
    let app = build_router(state, None).expect("router should build");

    for path in [
        "/v1/features?min_lat=41.0&max_lat=42.0&min_lon=-97.0",
        "/v1/aggregates/facets?dimension=office&min_lat=42.0&max_lat=41.0&min_lon=-97.0&max_lon=-95.0",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method("GET")
                    .body(axum::body::Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "path={path}");
    }
}

#[tokio::test]
async fn archive_routes_reject_invalid_boolean_filters() {
    let state = test_state_with_archive(10);
    let app = build_router(state, None).expect("router should build");

    for path in [
        "/v1/products?has_issues=maybe",
        "/v1/features?has_vtec=perhaps",
        "/v1/aggregates/facets?dimension=office&has_hvtec=sometimes",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method("GET")
                    .body(axum::body::Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "path={path}");
    }
}

#[tokio::test]
async fn archive_product_route_rejects_invalid_size_range() {
    let state = test_state_with_archive(10);
    let app = build_router(state, None).expect("router should build");

    let path = "/v1/products?min_size=10&max_size=1";
    let response = app
        .oneshot(
            Request::builder()
                .uri(path)
                .method("GET")
                .body(axum::body::Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "path={path}");
}

#[tokio::test]
async fn archive_routes_reject_invalid_enum_filters() {
    let state = test_state_with_archive(10);
    let app = build_router(state, None).expect("router should build");

    for path in [
        "/v1/features?kind=bogus",
        "/v1/aggregates/facets?dimension=bogus",
        "/v1/aggregates/timeseries?measure=bogus&start=2025-03-05T12:00:00Z&end=2025-03-05T13:00:00Z&bucket=hour",
        "/v1/aggregates/cells?measure=bogus&precision=5",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method("GET")
                    .body(axum::body::Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "path={path}");
    }
}

#[tokio::test]
async fn archive_product_and_issue_routes_return_success_when_test_database_is_configured() {
    let Some(database_url) = std::env::var("EMWIN_PG_TEST_DATABASE_URL").ok() else {
        return;
    };

    let mut config = emwin_db::PostgresConfig::new(database_url);
    config.application_name = "emwin-cli-http-test".to_string();
    let sink = emwin_db::PostgresMetadataSink::connect(config)
        .await
        .expect("postgres sink should connect");

    let filename = "HTTP-ARCHIVE-KOAX-1.TXT";
    sqlx::query("DELETE FROM products WHERE filename = $1")
        .bind(filename)
        .execute(&sink.pool())
        .await
        .expect("product cleanup should succeed");

    let temp = tempdir().expect("tempdir should exist");
    let payload_path = temp.path().join("http-test.txt");
    let metadata_path = temp.path().join("http-test.json");
    let payload_bytes = br#"000
WUUS53 KOAX 051200
SVROAX

Severe Thunderstorm Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
TIME...MOT...LOC 1200Z 300DEG 25KT 4143 9613 4140 9608
"#;
    std::fs::write(&payload_path, payload_bytes).expect("payload file should write");
    std::fs::write(&metadata_path, b"{\"ok\":true}").expect("metadata file should write");

    let metadata = emwin_db::CompletedFileMetadata::build(
        filename,
        1_741_182_000,
        emwin_protocol::ingest::ProductOrigin::Qbt,
        payload_bytes,
    );
    let filename_key = metadata.filename.clone();
    let timestamp = i64::try_from(metadata.timestamp_utc).expect("timestamp should fit");
    sink.persist(emwin_db::PersistedRequest {
        request_key: filename_key.clone(),
        metadata,
        blobs: vec![
            emwin_db::StoredBlob {
                kind: emwin_db::BlobStorageKind::Filesystem,
                role: emwin_db::BlobRole::Payload,
                location: payload_path.display().to_string(),
                size_bytes: payload_bytes.len(),
                content_type: Some("text/plain".to_string()),
            },
            emwin_db::StoredBlob {
                kind: emwin_db::BlobStorageKind::Filesystem,
                role: emwin_db::BlobRole::MetadataSidecar,
                location: metadata_path.display().to_string(),
                size_bytes: 11,
                content_type: Some("application/json".to_string()),
            },
        ],
    })
    .await
    .expect("persist should succeed");

    let product_id: i64 = sqlx::query_scalar(
        "SELECT id FROM products WHERE filename = $1 AND source_timestamp_utc = $2",
    )
    .bind(&filename_key)
    .bind(timestamp)
    .fetch_one(&sink.pool())
    .await
    .expect("product should exist");

    sqlx::query(
        "INSERT INTO product_search_points (product_id, source_kind, source_index, latitude, longitude, point_geom)
         VALUES ($1, 'manual', 0, 41.5, -95.5, ST_SetSRID(ST_MakePoint(-95.5, 41.5), 4326))
         ON CONFLICT DO NOTHING",
    )
    .bind(product_id)
    .execute(&sink.pool())
    .await
    .expect("search point insert should succeed");
    let issue_id: i64 = sqlx::query_scalar(
        "INSERT INTO product_issues (product_id, kind, code, message, line)
         VALUES ($1, 'text_product_parse', 'invalid_wmo_header', 'failed', NULL)
         RETURNING id",
    )
    .bind(product_id)
    .fetch_one(&sink.pool())
    .await
    .expect("issue insert should succeed");

    let state = super::build_state(
        10,
        emwin_live::LiveRuntime::new_for_tests(
            Vec::new(),
            emwin_live::LiveTelemetry::Unavailable,
            Some(sink.clone()),
            None,
            None,
        ),
        None,
    );
    let app = build_router(state, None).expect("router should build");

    let paths = vec![
        "/v1/products?office=KOAX&artifact_kind=nws_text_product".to_string(),
        format!("/v1/products/{product_id}"),
        format!("/v1/products/{product_id}/raw"),
        format!("/v1/issues?product_id={product_id}"),
        format!("/v1/issues/{issue_id}"),
    ];
    for path in paths {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&path)
                    .method("GET")
                    .body(axum::body::Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(response.status(), StatusCode::OK, "path={path}");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        if path.ends_with("/raw") {
            assert_eq!(body.as_ref(), payload_bytes, "path={path}");
            continue;
        }

        let json: serde_json::Value = serde_json::from_slice(&body).expect("json should parse");
        if path == "/v1/products?office=KOAX&artifact_kind=nws_text_product" {
            assert_eq!(json["items"][0]["product_id"], product_id, "path={path}");
            assert_eq!(
                json["items"][0]["detail_url"],
                format!("/v1/products/{product_id}"),
                "path={path}"
            );
        } else if path == format!("/v1/products/{product_id}") {
            assert_eq!(json["product"]["product_id"], product_id, "path={path}");
            assert_eq!(
                json["product"]["raw_url"],
                format!("/v1/products/{product_id}/raw"),
                "path={path}"
            );
        } else if path == format!("/v1/issues?product_id={product_id}") {
            assert_eq!(json["items"][0]["id"], issue_id, "path={path}");
            assert_eq!(
                json["items"][0]["product_url"],
                format!("/v1/products/{product_id}"),
                "path={path}"
            );
        } else if path == format!("/v1/issues/{issue_id}") {
            assert_eq!(json["issue"]["id"], issue_id, "path={path}");
            assert_eq!(
                json["issue"]["detail_url"],
                format!("/v1/issues/{issue_id}"),
                "path={path}"
            );
        }
    }

    sqlx::query("DELETE FROM products WHERE filename = $1")
        .bind(filename)
        .execute(&sink.pool())
        .await
        .expect("product cleanup should succeed");
}

#[tokio::test]
async fn archive_issue_payload_serializes_related_urls() {
    let payload = archive_issue_payload();
    let json = serde_json::to_value(&payload).expect("payload should serialize");
    assert_eq!(json["id"], 7);
    assert_eq!(json["code"], "invalid_wmo_header");
    assert_eq!(json["detail_url"], "/v1/issues/7");
    assert_eq!(json["product_url"], "/v1/products/42");
}
