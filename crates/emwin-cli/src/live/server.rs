//! Primary live command for running the EMWIN HTTP server with SSE endpoints.
//!
//! This module provides an HTTP server that:
//! - Streams events via Server-Sent Events (SSE)
//! - Serves completed files for download
//! - Provides health and metrics endpoints
//! - Supports CORS for browser clients

use crate::ReceiverKind;
use crate::live::config::{LiveConfigRequest, LiveReceiverConfig, build_live_receiver_config};
use crate::live::persistence::{run_incident_cleanup_loop, start_runtime_with_postgres};
use crate::live::server_support::RetainedFiles;
use axum::http::HeaderValue;
use chrono::Utc;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

mod server_http;
mod server_ingest;
mod types;

pub use types::ServerOptions;
use types::{AppState, BroadcastEvent, EventKind, TelemetryPayload};

#[cfg(test)]
use server_http::{event_matches_filter, files_handler};

/// Capacity of the broadcast channel for events.
const EVENT_CHANNEL_CAPACITY: usize = 4096;

pub async fn run(options: ServerOptions) -> crate::error::CliResult<()> {
    let ServerOptions {
        receiver,
        username,
        password,
        raw_servers,
        server_list_path,
        output_dir,
        bind,
        cors_origin,
        max_clients,
        stats_interval_secs,
        file_retention_secs,
        max_retained_files,
        post_process_archives,
        quiet,
        persistence_queue_capacity,
        postgres_database_url,
    } = options;

    if postgres_database_url.is_some() && output_dir.is_none() {
        return Err(crate::error::CliError::invalid_argument(
            "--persist-database-url requires --output-dir for blob storage",
        ));
    }

    let bind_addr = SocketAddr::from_str(&bind).map_err(|err| {
        crate::error::CliError::invalid_argument(format!("invalid --bind value {bind}: {err}"))
    })?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let started_persistence = match output_dir {
        Some(path) => Some(
            start_runtime_with_postgres(
                path,
                persistence_queue_capacity,
                postgres_database_url.as_deref(),
                "emwin-cli-server",
            )
            .await?,
        ),
        None => None,
    };
    if let Some(postgres_sink) = started_persistence
        .as_ref()
        .and_then(|started| started.postgres_sink.as_ref())
    {
        match postgres_sink.expire_active_incidents(Utc::now()).await {
            Ok(result) if result.expired_count > 0 => {
                info!(
                    backend = "database",
                    target = %postgres_sink.describe_target(),
                    expired_count = result.expired_count,
                    "expired stale incidents during startup"
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(
                    backend = "database",
                    target = %postgres_sink.describe_target(),
                    stage = "incident_cleanup",
                    error = %err,
                    "startup incident cleanup skipped; will retry in background"
                );
            }
        }
    }
    let cleanup_sink = started_persistence
        .as_ref()
        .and_then(|started| started.postgres_sink.clone());
    let archive_sink = started_persistence
        .as_ref()
        .and_then(|started| started.postgres_sink.clone());
    let persistence_runtime = started_persistence.map(|started| started.runtime);
    let persistence_producer = persistence_runtime
        .as_ref()
        .map(|runtime| runtime.producer());

    let state = Arc::new(AppState {
        event_tx: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
        shutdown_rx: shutdown_rx.clone(),
        retained_files: Mutex::new(RetainedFiles::new(
            max_retained_files.max(1),
            Duration::from_secs(file_retention_secs.max(1)),
        )),
        telemetry: Mutex::new(TelemetryPayload::Unavailable),
        persistence: persistence_producer.clone(),
        archive: archive_sink,
        connected_clients: AtomicUsize::new(0),
        max_clients: max_clients.max(1),
        next_event_id: AtomicU64::new(1),
        data_blocks_total: AtomicU64::new(0),
        received_servers: AtomicUsize::new(0),
        received_sat_servers: AtomicUsize::new(0),
        started_at: Instant::now(),
        upstream_endpoint: Mutex::new(None),
        quiet,
    });

    let cors = build_cors_layer(cors_origin)?;
    let app = server_http::build_router(Arc::clone(&state), cors);

    let listener = TcpListener::bind(bind_addr).await?;
    log_info(quiet, &format!("server listening addr={bind_addr}"));

    let ingest_task = match receiver {
        ReceiverKind::Qbt => {
            let LiveReceiverConfig::Qbt(config) = build_live_receiver_config(LiveConfigRequest {
                receiver: ReceiverKind::Qbt,
                username: Some(username),
                password,
                raw_servers,
                server_list_path,
                idle_timeout_secs: 90,
                qbt_watchdog_timeout_secs: 20,
                username_context: "server mode",
                password_context: "server mode",
            })?
            else {
                unreachable!("qbt server mode must build qbt config");
            };
            tokio::spawn(server_ingest::run_qbt_ingest_loop(
                config,
                Arc::clone(&state),
                post_process_archives,
                persistence_producer.clone(),
                shutdown_rx.clone(),
            ))
        }
        ReceiverKind::Wxwire => {
            let LiveReceiverConfig::WxWire(config) =
                build_live_receiver_config(LiveConfigRequest {
                    receiver: ReceiverKind::Wxwire,
                    username: Some(username),
                    password,
                    raw_servers,
                    server_list_path,
                    idle_timeout_secs: 90,
                    qbt_watchdog_timeout_secs: 0,
                    username_context: "wxwire server mode",
                    password_context: "wxwire server mode",
                })?
            else {
                unreachable!("wxwire server mode must build wxwire config");
            };
            tokio::spawn(server_ingest::run_wxwire_ingest_loop(
                config,
                Arc::clone(&state),
                post_process_archives,
                persistence_producer.clone(),
                shutdown_rx.clone(),
            ))
        }
    };
    let stats_task = tokio::spawn(server_ingest::run_stats_loop(
        Arc::clone(&state),
        stats_interval_secs,
        persistence_producer.clone(),
        shutdown_rx.clone(),
    ));
    let cleanup_task =
        cleanup_sink.map(|sink| tokio::spawn(run_incident_cleanup_loop(sink, shutdown_rx.clone())));
    let mut http_shutdown_rx = shutdown_rx.clone();

    let serve = async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = http_shutdown_rx.changed().await;
        })
        .await
    };
    tokio::pin!(serve);

    let mut shutdown_requested = false;
    let serve_result = tokio::select! {
        result = &mut serve => result,
        signal = tokio::signal::ctrl_c() => {
            signal?;
            shutdown_requested = true;
            info!("shutdown signal received; waiting for server tasks to stop");
            let _ = shutdown_tx.send(true);
            (&mut serve).await
        }
    };

    if !shutdown_requested {
        let _ = shutdown_tx.send(true);
    }

    let ingest_result = ingest_task.await;
    let stats_result = stats_task.await;
    let cleanup_result = match cleanup_task {
        Some(task) => Some(task.await),
        None => None,
    };
    let _persistence_shutdown_stats = match persistence_runtime {
        Some(runtime) => Some(crate::live::persistence::shutdown_runtime(runtime).await?),
        None => None,
    };

    if let Err(err) = serve_result {
        return Err(crate::error::CliError::runtime(format!(
            "http server failed: {err}"
        )));
    }
    match ingest_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            return Err(crate::error::CliError::runtime(format!(
                "ingest task failed: {err}"
            )));
        }
        Err(err) => {
            return Err(crate::error::CliError::runtime(format!(
                "ingest task join failed: {err}"
            )));
        }
    }
    match stats_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            return Err(crate::error::CliError::runtime(format!(
                "stats task failed: {err}"
            )));
        }
        Err(err) => {
            return Err(crate::error::CliError::runtime(format!(
                "stats task join failed: {err}"
            )));
        }
    }
    match cleanup_result {
        Some(Ok(Ok(()))) | None => {}
        Some(Ok(Err(err))) => {
            return Err(crate::error::CliError::runtime(format!(
                "cleanup task failed: {err}"
            )));
        }
        Some(Err(err)) => {
            return Err(crate::error::CliError::runtime(format!(
                "cleanup task join failed: {err}"
            )));
        }
    }
    info!("server stopped");
    Ok(())
}

#[cfg(test)]
fn build_router(
    state: Arc<AppState>,
    cors_origin: Option<String>,
) -> crate::error::CliResult<axum::Router> {
    let cors = build_cors_layer(cors_origin)?;
    Ok(server_http::build_router(state, cors))
}

fn publish(state: &Arc<AppState>, kind: EventKind) {
    let id = state
        .next_event_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = state.event_tx.send(BroadcastEvent { id, kind });
}

#[cfg(test)]
fn sanitize_requested_filename(raw: &str) -> Option<String> {
    crate::live::server_support::sanitize_requested_filename(raw)
}

fn log_info(quiet: bool, msg: &str) {
    if !quiet {
        info!("{msg}");
    }
}

fn log_error(msg: &str) {
    error!("{msg}");
}

fn build_cors_layer(cors_origin: Option<String>) -> crate::error::CliResult<CorsLayer> {
    if let Some(origin) = cors_origin {
        if origin == "*" {
            return Ok(CorsLayer::new().allow_origin(Any).allow_methods(Any));
        }

        let header_value = HeaderValue::from_str(&origin).map_err(|err| {
            crate::error::CliError::invalid_argument(format!(
                "invalid --cors-origin value {origin}: {err}"
            ))
        })?;
        return Ok(CorsLayer::new()
            .allow_origin(header_value)
            .allow_methods(Any));
    }

    Ok(CorsLayer::new().allow_methods(Any))
}

#[cfg(test)]
mod tests {
    use super::{
        TelemetryPayload, build_router, event_matches_filter, files_handler,
        sanitize_requested_filename,
    };
    use crate::live::file_pipeline::build_persist_request;
    use crate::live::server::types::{AppState, CompletedFileEventPayload, EventKind, EventsQuery};
    use crate::live::server_support::RetainedFiles;
    use crate::live::server_support::wildcard_match;
    use axum::Json;
    use axum::body::{Body, to_bytes};
    use axum::extract::{ConnectInfo, Query, State};
    use axum::http::{HeaderMap, Request, StatusCode};
    use emwin_db::{
        CompletedFileMetadata, NoopMetadataSink, PersistenceConfig, PersistenceRuntime,
    };
    use emwin_protocol::ingest::ProductOrigin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::time::{Duration, Instant, SystemTime};
    use tempfile::tempdir;
    use tokio::sync::{broadcast, watch};
    use tower::ServiceExt;

    fn test_state(max_clients: usize) -> Arc<AppState> {
        let (_, shutdown_rx) = watch::channel(false);
        Arc::new(AppState {
            event_tx: broadcast::channel(32).0,
            shutdown_rx,
            retained_files: std::sync::Mutex::new(RetainedFiles::new(32, Duration::from_secs(60))),
            telemetry: std::sync::Mutex::new(TelemetryPayload::Unavailable),
            persistence: None,
            archive: None,
            connected_clients: AtomicUsize::new(0),
            max_clients,
            next_event_id: AtomicU64::new(1),
            data_blocks_total: AtomicU64::new(0),
            received_servers: AtomicUsize::new(0),
            received_sat_servers: AtomicUsize::new(0),
            started_at: Instant::now(),
            upstream_endpoint: std::sync::Mutex::new(None),
            quiet: true,
        })
    }

    fn file_complete_event(filename: &str) -> EventKind {
        let data = if filename.eq_ignore_ascii_case("TAFPDKGA.TXT") {
            b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n".as_slice()
        } else if filename.eq_ignore_ascii_case("TAFWBCFJ.TXT") {
            b"000 \nFTXX01 KWBC 070200\nTAF AMD\nWBCF 070244Z 0703/0803 18012KT P6SM SCT050\n"
                .as_slice()
        } else if filename.eq_ignore_ascii_case("TAFWMOONLY.TXT") {
            b"000 \nFTUS80 KWBC 070200\nTAF SBAF 070200Z 0703/0803 00000KT CAVOK=\n".as_slice()
        } else if filename.eq_ignore_ascii_case("BROKEN.TXT") {
            b"000 \nINVALID HEADER\nAFDBOX\nBody\n".as_slice()
        } else if filename.eq_ignore_ascii_case("SVROAXNE.TXT") {
            br#"000
WUUS53 KOAX 051200
SVROAX

URGENT - IMMEDIATE BROADCAST REQUESTED
Severe Thunderstorm Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/

Severe Thunderstorm Warning for...
East Central Cuming County in northeastern Nebraska...

This is a test product.
$$
"#
            .as_slice()
        } else if filename.eq_ignore_ascii_case("SVRWIND.TXT") {
            br#"000
WUUS53 KOAX 051200
SVROAX

URGENT - IMMEDIATE BROADCAST REQUESTED
Severe Thunderstorm Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
TIME...MOT...LOC 1200Z 300DEG 25KT 4143 9613 4140 9608
HAILTHREAT...RADARINDICATED
MAXHAILSIZE...1.00 IN
WINDTHREAT...OBSERVED
MAXWINDGUST...60 MPH
"#
            .as_slice()
        } else if filename.eq_ignore_ascii_case("SVRPOLY.TXT") {
            br#"000
WUUS53 KOAX 051200
SVROAX

URGENT - IMMEDIATE BROADCAST REQUESTED
Severe Thunderstorm Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
"#
            .as_slice()
        } else if filename.eq_ignore_ascii_case("SVRALC.TXT") {
            br#"000
WUUS54 KBMX 051200
SVRBMX

URGENT - IMMEDIATE BROADCAST REQUESTED
Severe Thunderstorm Warning
National Weather Service Birmingham AL
1200 PM CST Wed Mar 5 2025

ALC001-051300-
/O.NEW.KBMX.SV.W.0001.250305T1200Z-250305T1800Z/
"#
            .as_slice()
        } else if filename.eq_ignore_ascii_case("FFWOAXNE.TXT") {
            br#"000
WUUS53 KOAX 051200
FFWOAX

Flash Flood Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.FF.W.0001.250305T1200Z-250305T1800Z/
/MSRM1.3.ER.250305T1200Z.250305T1800Z.250306T0000Z.NO/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
TIME...MOT...LOC 1200Z 300DEG 25KT 4143 9613 4140 9608
"#
            .as_slice()
        } else if filename.eq_ignore_ascii_case("FFWCHFA2.TXT") {
            br#"000
WGUS53 PAFG 051200
FFWAFG

Flash Flood Warning
National Weather Service Fairbanks AK
1200 PM AKST Wed Mar 5 2025

AKC090-051300-
/O.NEW.PAFG.FF.W.0001.250305T1200Z-250305T1800Z/
/CHFA2.3.ER.250305T1200Z.250305T1800Z.250306T0000Z.NO/
"#
            .as_slice()
        } else {
            b"ignored".as_slice()
        };

        let product = emwin_parser::enrich_product(filename, data);
        let metadata = CompletedFileMetadata {
            filename: filename.to_string(),
            size: 11,
            timestamp_utc: 1,
            origin: ProductOrigin::Qbt,
            product_summary: emwin_parser::summarize_product_v2(&product),
            product_detail: emwin_parser::detail_product_v2(&product),
            product,
        };

        EventKind::FileComplete(Box::new(CompletedFileEventPayload::from_metadata(metadata)))
    }

    fn empty_events_query() -> EventsQuery {
        EventsQuery {
            event: None,
            filename: None,
            source: None,
            pil: None,
            family: None,
            container: None,
            wmo_prefix: None,
            office: None,
            office_city: None,
            office_state: None,
            bbb_kind: None,
            cccc: None,
            ttaaii: None,
            afos: None,
            bbb: None,
            has_issues: None,
            issue_kind: None,
            issue_code: None,
            has_vtec: None,
            has_ugc: None,
            has_hvtec: None,
            has_latlon: None,
            has_time_mot_loc: None,
            has_wind_hail: None,
            state: None,
            county: None,
            zone: None,
            fire_zone: None,
            marine_zone: None,
            vtec_phenomena: None,
            vtec_significance: None,
            vtec_action: None,
            vtec_office: None,
            etn: None,
            hvtec_nwslid: None,
            hvtec_severity: None,
            hvtec_cause: None,
            hvtec_record: None,
            wind_hail_kind: None,
            lat: None,
            lon: None,
            distance_miles: None,
            min_wind_mph: None,
            min_hail_inches: None,
            min_size: None,
            max_size: None,
        }
    }

    fn event_filter(query: EventsQuery) -> crate::live::server::types::EventFilter {
        crate::live::server::types::EventFilter::try_from_query(query)
            .expect("query should compile")
    }

    #[test]
    fn wildcard_patterns_match_case_insensitive() {
        assert!(wildcard_match("*.TXT", "warn123.txt"));
        assert!(wildcard_match("WARN*", "warn123.txt"));
        assert!(wildcard_match("*orecast*", "FORecast_report.txt"));
        assert!(!wildcard_match("*.ZIP", "warn123.txt"));
    }

    #[test]
    fn sanitize_filename_rejects_bad_paths() {
        assert_eq!(
            sanitize_requested_filename("file.txt"),
            Some("file.txt".to_string())
        );
        assert_eq!(
            sanitize_requested_filename("/nested/file.txt"),
            Some("nested/file.txt".to_string())
        );
        assert!(sanitize_requested_filename("../file.txt").is_none());
        assert!(sanitize_requested_filename(" ").is_none());
    }

    #[test]
    fn retained_files_evict_by_capacity_and_ttl() {
        let mut files = RetainedFiles::new(2, Duration::from_millis(50));
        files.insert(
            "a.txt".to_string(),
            vec![1],
            1,
            ProductOrigin::Qbt,
            SystemTime::now(),
        );
        files.insert(
            "b.txt".to_string(),
            vec![2],
            2,
            ProductOrigin::Qbt,
            SystemTime::now(),
        );
        files.insert(
            "c.txt".to_string(),
            vec![3],
            3,
            ProductOrigin::Qbt,
            SystemTime::now(),
        );

        assert!(files.get("a.txt").is_none());
        assert!(files.get("b.txt").is_some());
        assert!(files.get("c.txt").is_some());

        let old = SystemTime::now() - Duration::from_secs(1);
        files.insert("old.txt".to_string(), vec![9], 9, ProductOrigin::Qbt, old);
        assert!(files.get("old.txt").is_none());
    }

    #[tokio::test]
    async fn events_handler_rejects_when_client_limit_reached() {
        let state = test_state(1);
        state.connected_clients.store(1, Ordering::Relaxed);

        let result = crate::live::server::server_http::events_handler(
            State(state),
            ConnectInfo("127.0.0.1:4000".parse().expect("valid socket addr")),
            HeaderMap::new(),
            Query(empty_events_query()),
        )
        .await;

        assert!(matches!(result, Err((StatusCode::TOO_MANY_REQUESTS, _))));
    }

    #[tokio::test]
    async fn files_download_accepts_url_encoded_nested_filename() {
        let state = test_state(10);
        {
            let mut guard = state
                .retained_files
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.insert(
                "nested/my file.txt".to_string(),
                b"hello world".to_vec(),
                1,
                ProductOrigin::Qbt,
                SystemTime::now(),
            );
        }

        let app = build_router(Arc::clone(&state), None).expect("router should build");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/files/nested%2Fmy%20file.txt")
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
        let state = test_state(10);
        {
            let mut guard = state
                .retained_files
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.insert(
                "TAFPDKGA.TXT".to_string(),
                b"000
FTUS42 KFFC 022320
TAFPDK
Body
"
                .to_vec(),
                1,
                ProductOrigin::Qbt,
                SystemTime::now(),
            );
        }

        let Json(response) = files_handler(State(state)).await;
        let file = &response.files[0];
        assert_eq!(file.metadata.filename, "TAFPDKGA.TXT");
        assert_eq!(file.download_url, "/files/TAFPDKGA.TXT");
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
        let metadata_json =
            serde_json::to_value(&file.metadata).expect("metadata should serialize");
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

    #[tokio::test]
    async fn root_endpoint_lists_available_routes() {
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
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let body_text = String::from_utf8(body.to_vec()).expect("body should be utf8 json");
        assert!(
            body_text
                .contains("\"/events?event=file_complete&lat=41.42&lon=-96.17&distance_miles=5\"")
        );
        assert!(body_text.contains("\"/incidents\""));
        assert!(body_text.contains("\"/archive/products/{product_id}\""));
        assert!(body_text.contains("\"/files\""));
        assert!(body_text.contains("\"/health\""));
        assert!(body_text.contains("\"/metrics\""));
    }

    #[tokio::test]
    async fn incidents_endpoint_returns_service_unavailable_without_archive_database() {
        let state = test_state(10);
        let app = build_router(state, None).expect("router should build");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/incidents")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn archive_product_raw_endpoint_returns_service_unavailable_without_archive_database() {
        let state = test_state(10);
        let app = build_router(state, None).expect("router should build");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/archive/products/1/raw")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

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
            shutdown_rx,
            retained_files: std::sync::Mutex::new(RetainedFiles::new(32, Duration::from_secs(60))),
            telemetry: std::sync::Mutex::new(TelemetryPayload::Unavailable),
            persistence: Some(producer),
            archive: None,
            connected_clients: AtomicUsize::new(0),
            max_clients: 10,
            next_event_id: AtomicU64::new(1),
            data_blocks_total: AtomicU64::new(0),
            received_servers: AtomicUsize::new(0),
            received_sat_servers: AtomicUsize::new(0),
            started_at: Instant::now(),
            upstream_endpoint: std::sync::Mutex::new(None),
            quiet: true,
        });
        let app = build_router(state, None).expect("router should build");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
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

    #[test]
    fn events_filter_only_allows_matching_filenames() {
        let txt = file_complete_event("report.txt");
        let zip = file_complete_event("report.zip");
        let telemetry = EventKind::Telemetry(TelemetryPayload::Unavailable);
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            filename: Some("*.txt".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &txt));
        assert!(!event_matches_filter(&filter, &zip));
        assert!(!event_matches_filter(&filter, &telemetry));
    }

    #[test]
    fn events_filter_matches_structured_metadata_fields() {
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            event: Some("file_complete".to_string()),
            pil: Some("taf,afd".to_string()),
            office: Some("ffc".to_string()),
            office_state: Some("ga".to_string()),
            cccc: Some("kffc".to_string()),
            family: Some("NWS_TEXT_PRODUCT".to_string()),
            container: Some("raw".to_string()),
            ..empty_events_query()
        });
        let event = file_complete_event("TAFPDKGA.TXT");

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_office_city_for_wmo_only_fallbacks() {
        let event = file_complete_event("TAFWBCFJ.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            office: Some("wbc".to_string()),
            office_city: Some("national centers for environmental prediction".to_string()),
            office_state: Some("md".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_wmo_header_fallback_fields() {
        let event = file_complete_event("TAFWMOONLY.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            source: Some("wmo_bulletin".to_string()),
            cccc: Some("kwbc".to_string()),
            ttaaii: Some("ftus80".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_geographic_codes() {
        let event = file_complete_event("SVROAXNE.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            state: Some("ne".to_string()),
            county: Some("NEC002".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_requires_matching_geographic_class() {
        let event = file_complete_event("SVROAXNE.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            zone: Some("NEZ002".to_string()),
            ..empty_events_query()
        });

        assert!(!event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_vtec_codes() {
        let event = file_complete_event("SVROAXNE.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            vtec_phenomena: Some("sv".to_string()),
            vtec_significance: Some("w".to_string()),
            vtec_action: Some("new".to_string()),
            vtec_office: Some("koax".to_string()),
            etn: Some("1,99".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_rejects_non_matching_vtec_codes() {
        let event = file_complete_event("SVROAXNE.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            vtec_action: Some("CAN".to_string()),
            ..empty_events_query()
        });

        assert!(!event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_issue_fields() {
        let event = file_complete_event("BROKEN.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            has_issues: Some("true".to_string()),
            issue_kind: Some("text_product_parse".to_string()),
            issue_code: Some("invalid_wmo_header".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_body_presence_for_hvtec_product() {
        let event = file_complete_event("FFWOAXNE.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            has_vtec: Some("true".to_string()),
            has_ugc: Some("true".to_string()),
            has_hvtec: Some("true".to_string()),
            has_latlon: Some("true".to_string()),
            has_time_mot_loc: Some("true".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_body_presence_for_wind_hail_product() {
        let event = file_complete_event("SVRWIND.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            has_vtec: Some("true".to_string()),
            has_ugc: Some("true".to_string()),
            has_latlon: Some("true".to_string()),
            has_time_mot_loc: Some("true".to_string()),
            has_wind_hail: Some("true".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_hvtec_fields() {
        let event = file_complete_event("FFWOAXNE.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            hvtec_nwslid: Some("MSRM1".to_string()),
            hvtec_severity: Some("major".to_string()),
            hvtec_cause: Some("excessive_rainfall".to_string()),
            hvtec_record: Some("no_record".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_wind_hail_fields() {
        let event = file_complete_event("SVRWIND.TXT");
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            wind_hail_kind: Some("hail_threat,max_wind_gust".to_string()),
            min_wind_mph: Some(50.0),
            min_hail_inches: Some(1.0),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_requires_matching_header_metadata() {
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            ttaaii: Some("WWUS53".to_string()),
            ..empty_events_query()
        });
        let event = file_complete_event("TAFPDKGA.TXT");

        assert!(!event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_non_file_events_only_by_event_name() {
        let telemetry = EventKind::Telemetry(TelemetryPayload::Unavailable);
        let filter = crate::live::server::types::EventFilter::from_query(EventsQuery {
            event: Some("telemetry,connected".to_string()),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &telemetry));
    }

    #[tokio::test]
    async fn events_handler_rejects_invalid_size_range() {
        let state = test_state(1);

        let result = crate::live::server::server_http::events_handler(
            State(state),
            ConnectInfo("127.0.0.1:4001".parse().expect("valid socket addr")),
            HeaderMap::new(),
            Query(EventsQuery {
                min_size: Some(10),
                max_size: Some(1),
                ..empty_events_query()
            }),
        )
        .await;

        assert!(matches!(result, Err((StatusCode::BAD_REQUEST, _))));
    }

    #[test]
    fn events_filter_matches_polygon_containment_without_point_distance() {
        let event = file_complete_event("SVRPOLY.TXT");
        let filter = event_filter(EventsQuery {
            lat: Some(41.43),
            lon: Some(-96.13),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_time_mot_loc_points_within_default_radius() {
        let event = file_complete_event("SVRWIND.TXT");
        let filter = event_filter(EventsQuery {
            lat: Some(41.43),
            lon: Some(-96.13),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_ugc_representative_points_within_radius() {
        let event = file_complete_event("SVRALC.TXT");
        let filter = event_filter(EventsQuery {
            lat: Some(32.5349),
            lon: Some(-86.6428),
            distance_miles: Some(1.0),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_matches_hvtec_gauge_points_within_radius() {
        let event = file_complete_event("FFWCHFA2.TXT");
        let filter = event_filter(EventsQuery {
            lat: Some(64.8458),
            lon: Some(-147.7011),
            distance_miles: Some(1.0),
            ..empty_events_query()
        });

        assert!(event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_rejects_products_without_spatial_data() {
        let event = file_complete_event("TAFPDKGA.TXT");
        let filter = event_filter(EventsQuery {
            lat: Some(41.42),
            lon: Some(-96.17),
            ..empty_events_query()
        });

        assert!(!event_matches_filter(&filter, &event));
    }

    #[test]
    fn events_filter_does_not_match_non_file_events_with_location_constraints() {
        let telemetry = EventKind::Telemetry(TelemetryPayload::Unavailable);
        let filter = event_filter(EventsQuery {
            event: Some("telemetry".to_string()),
            lat: Some(41.42),
            lon: Some(-96.17),
            ..empty_events_query()
        });

        assert!(!event_matches_filter(&filter, &telemetry));
    }

    #[tokio::test]
    async fn events_handler_rejects_lat_without_lon() {
        let state = test_state(1);

        let result = crate::live::server::server_http::events_handler(
            State(state),
            ConnectInfo("127.0.0.1:4002".parse().expect("valid socket addr")),
            HeaderMap::new(),
            Query(EventsQuery {
                lat: Some(41.42),
                ..empty_events_query()
            }),
        )
        .await;

        assert_eq!(
            result.err(),
            Some((
                StatusCode::BAD_REQUEST,
                "lat and lon must be provided together".to_string(),
            ))
        );
    }

    #[tokio::test]
    async fn events_handler_rejects_invalid_latitude() {
        let state = test_state(1);

        let result = crate::live::server::server_http::events_handler(
            State(state),
            ConnectInfo("127.0.0.1:4003".parse().expect("valid socket addr")),
            HeaderMap::new(),
            Query(EventsQuery {
                lat: Some(95.0),
                lon: Some(-96.17),
                ..empty_events_query()
            }),
        )
        .await;

        assert_eq!(
            result.err(),
            Some((
                StatusCode::BAD_REQUEST,
                "lat must be a finite value between -90 and 90".to_string(),
            ))
        );
    }

    #[tokio::test]
    async fn events_handler_rejects_non_positive_distance() {
        let state = test_state(1);

        let result = crate::live::server::server_http::events_handler(
            State(state),
            ConnectInfo("127.0.0.1:4004".parse().expect("valid socket addr")),
            HeaderMap::new(),
            Query(EventsQuery {
                lat: Some(41.42),
                lon: Some(-96.17),
                distance_miles: Some(0.0),
                ..empty_events_query()
            }),
        )
        .await;

        assert_eq!(
            result.err(),
            Some((
                StatusCode::BAD_REQUEST,
                "distance_miles must be a finite value greater than 0".to_string(),
            ))
        );
    }

    #[test]
    fn file_complete_event_includes_download_url() {
        let value = file_complete_event("nested/my file.txt").to_json();

        assert_eq!(value["download_url"], "/files/nested%2Fmy%20file.txt");
        assert_eq!(value["timestamp_utc"], 1);
        assert_eq!(value["product"]["schema_version"], 2);
        assert!(value["product"].get("facets").is_some());
        assert!(value["product"].get("issues").is_some());
        assert!(value["product"].get("artifact").is_none());
    }
}
