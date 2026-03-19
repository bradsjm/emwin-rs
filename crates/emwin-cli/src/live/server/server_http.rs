//! HTTP and SSE handlers for live server mode.
//!
//! The handlers stay thin: request parsing and response shaping live here, while retention,
//! filtering, and ingest coordination live in neighboring modules.

use super::types::{
    AppState, ArchiveProductDetailPayload, ArchiveProductResponse, BroadcastEvent, ClientGuard,
    EndpointDoc, EventFilter, EventKind, EventsQuery, FilesResponse, HealthResponse,
    IncidentDetailPayload, IncidentProductsQuery, IncidentProductsResponse, IncidentResponse,
    IncidentSummaryPayload, IncidentsQuery, IncidentsResponse, RootResponse,
};
use crate::live::server_support::{
    build_bytes_download_response, build_file_download_response, filename_request_or_400,
};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::Response;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use futures::Stream;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Builds the Axum router for live server mode.
pub(super) fn build_router(state: Arc<AppState>, cors: tower_http::cors::CorsLayer) -> Router {
    Router::new()
        .route("/", get(root_handler))
        .route("/incidents", get(incidents_handler))
        .route(
            "/incidents/{office}/{phenomena}/{significance}/{etn}",
            get(incident_handler),
        )
        .route(
            "/incidents/{office}/{phenomena}/{significance}/{etn}/products",
            get(incident_products_handler),
        )
        .route(
            "/archive/products/{product_id}",
            get(archive_product_handler),
        )
        .route(
            "/archive/products/{product_id}/raw",
            get(archive_product_raw_handler),
        )
        .route("/events", get(events_handler))
        .route("/files", get(files_handler))
        .route("/files/{*filename}", get(file_download_handler))
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .layer(cors)
        .with_state(state)
}

pub(super) async fn root_handler() -> Json<RootResponse> {
    Json(RootResponse {
        service: "emwin-cli server",
        endpoints: vec![
            EndpointDoc {
                method: "GET",
                path: "/",
                description: "API index with endpoint descriptions",
            },
            EndpointDoc {
                method: "GET",
                path: "/incidents",
                description: "List live incident projection rows backed by persisted Postgres metadata",
            },
            EndpointDoc {
                method: "GET",
                path: "/incidents/{office}/{phenomena}/{significance}/{etn}",
                description: "Fetch one live incident projection row and its archive links",
            },
            EndpointDoc {
                method: "GET",
                path: "/incidents/{office}/{phenomena}/{significance}/{etn}/products",
                description: "List archived products linked to one incident",
            },
            EndpointDoc {
                method: "GET",
                path: "/archive/products/{product_id}",
                description: "Fetch one archived product detail record",
            },
            EndpointDoc {
                method: "GET",
                path: "/archive/products/{product_id}/raw",
                description: "Download archived raw payload bytes for one product",
            },
            EndpointDoc {
                method: "GET",
                path: "/events?event=file_complete&lat=41.42&lon=-96.17&distance_miles=5",
                description: "SSE stream with optional structured live filters over event, file, product, header, geography, VTEC, and location metadata",
            },
            EndpointDoc {
                method: "GET",
                path: "/files",
                description: "List retained completed files",
            },
            EndpointDoc {
                method: "GET",
                path: "/files/{*filename}",
                description: "Download retained file by URL-encoded filename path",
            },
            EndpointDoc {
                method: "GET",
                path: "/health",
                description: "Server health summary",
            },
            EndpointDoc {
                method: "GET",
                path: "/metrics",
                description: "JSON telemetry and persistence snapshot",
            },
        ],
    })
}

pub(super) async fn incidents_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<IncidentsQuery>,
) -> Result<Json<IncidentsResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let page = archive
        .list_incidents(emwin_db::IncidentListQuery {
            office: query.office,
            phenomena: query.phenomena,
            significance: query.significance,
            etn: query.etn,
            status: query.status,
            updated_after: query.updated_after,
            updated_before: query.updated_before,
            active_at: query.active_at,
            limit: query.limit.unwrap_or(100),
            cursor: query.cursor,
        })
        .await
        .map_err(map_archive_error)?;

    Ok(Json(IncidentsResponse {
        page: emwin_db::PaginatedResponse {
            items: page
                .items
                .into_iter()
                .map(IncidentSummaryPayload::from_incident)
                .collect(),
            next_cursor: page.next_cursor,
        },
    }))
}

pub(super) async fn incident_handler(
    State(state): State<Arc<AppState>>,
    Path((office, phenomena, significance, etn)): Path<(String, String, String, i64)>,
) -> Result<Json<IncidentResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let key = normalize_incident_key(office, phenomena, significance, etn);
    let incident = archive
        .get_incident(&key)
        .await
        .map_err(map_archive_error)?
        .ok_or((StatusCode::NOT_FOUND, "incident not found".to_string()))?;

    Ok(Json(IncidentResponse {
        incident: IncidentDetailPayload::from_incident(incident),
    }))
}

pub(super) async fn incident_products_handler(
    State(state): State<Arc<AppState>>,
    Path((office, phenomena, significance, etn)): Path<(String, String, String, i64)>,
    Query(query): Query<IncidentProductsQuery>,
) -> Result<Json<IncidentProductsResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let key = normalize_incident_key(office, phenomena, significance, etn);
    let page = archive
        .list_incident_products(
            &key,
            emwin_db::IncidentProductsQuery {
                limit: query.limit.unwrap_or(100),
                cursor: query.cursor,
            },
        )
        .await
        .map_err(map_archive_error)?;

    Ok(Json(IncidentProductsResponse {
        page: emwin_db::PaginatedResponse {
            items: page
                .items
                .into_iter()
                .map(super::types::ArchiveProductSummaryPayload::from_product)
                .collect(),
            next_cursor: page.next_cursor,
        },
    }))
}

pub(super) async fn archive_product_handler(
    State(state): State<Arc<AppState>>,
    Path(product_id): Path<i64>,
) -> Result<Json<ArchiveProductResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let product = archive
        .get_archived_product(product_id)
        .await
        .map_err(map_archive_error)?
        .ok_or((
            StatusCode::NOT_FOUND,
            "archived product not found".to_string(),
        ))?;

    Ok(Json(ArchiveProductResponse {
        product: ArchiveProductDetailPayload::from_product(product),
    }))
}

pub(super) async fn archive_product_raw_handler(
    State(state): State<Arc<AppState>>,
    Path(product_id): Path<i64>,
) -> Result<Response, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let payload = archive
        .read_archived_payload(product_id)
        .await
        .map_err(map_archive_error)?
        .ok_or((
            StatusCode::NOT_FOUND,
            "archived payload not found".to_string(),
        ))?;

    Ok(build_bytes_download_response(
        &payload.filename,
        payload.bytes,
    ))
}

pub(super) async fn events_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    if state
        .connected_clients
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < state.max_clients).then_some(current + 1)
        })
        .is_err()
    {
        super::log_info(
            state.quiet,
            &format!("rejecting client; limit reached peer={peer}"),
        );
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "client limit reached".to_string(),
        ));
    }

    super::log_info(state.quiet, &format!("sse client connected peer={peer}"));

    let rx = state.event_tx.subscribe();
    let shutdown_rx = state.shutdown_rx.clone();
    let last_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    if query
        .min_size
        .zip(query.max_size)
        .is_some_and(|(min, max)| min > max)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "min_size must be less than or equal to max_size".to_string(),
        ));
    }
    let filter =
        EventFilter::try_from_query(query).map_err(|err| (StatusCode::BAD_REQUEST, err.message))?;

    let stream = futures::stream::unfold(
        StreamState {
            state: Arc::clone(&state),
            rx: Some(rx),
            last_id,
            filter,
            shutdown_rx,
            peer,
            _guard: Some(ClientGuard {
                state: Arc::clone(&state),
                peer,
            }),
        },
        move |mut st| async move {
            let rx = st.rx.as_mut()?;
            loop {
                tokio::select! {
                    _ = st.shutdown_rx.changed() => return None,
                    received = rx.recv() => match received {
                    Ok(event) => {
                        if event.id <= st.last_id {
                            continue;
                        }
                        if !event_matches_filter(&st.filter, &event.kind) {
                            continue;
                        }

                        st.last_id = event.id;
                        let payload = match serde_json::to_string(&event.kind.to_json()) {
                            Ok(payload) => payload,
                            Err(_) => "{}".to_string(),
                        };
                        let sse = Event::default()
                            .id(event.id.to_string())
                            .event(event.kind.event_name())
                            .data(payload);
                        return Some((Ok(sse), st));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        super::log_info(
                            st.state.quiet,
                            &format!("sse client lagged peer={} dropped={}", st.peer, dropped),
                        );
                        let warning = Event::default().event("warning").data(
                            serde_json::json!({
                                "message": "client lagged; events dropped",
                                "dropped": dropped,
                                "peer": st.peer,
                            })
                            .to_string(),
                        );
                        return Some((Ok(warning), st));
                    }
                    }
                }
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

pub(super) async fn files_handler(State(state): State<Arc<AppState>>) -> Json<FilesResponse> {
    let files = state
        .retained_files
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .list()
        .into_iter()
        .map(super::types::CompletedFilePayload::from_metadata)
        .collect();
    Json(FilesResponse { files })
}

pub(super) async fn file_download_handler(
    State(state): State<Arc<AppState>>,
    Path(filename): Path<String>,
) -> Result<Response, StatusCode> {
    let normalized = filename_request_or_400(&filename)?;

    let file = state
        .retained_files
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&normalized)
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(build_file_download_response(file))
}

pub(super) async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let connected_clients = state.connected_clients.load(Ordering::Relaxed);
    let retained_files = state
        .retained_files
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .len();
    let upstream_endpoint = state
        .upstream_endpoint
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    Json(HealthResponse {
        status: "ok",
        connected_clients,
        retained_files,
        uptime_secs: state.started_at.elapsed().as_secs(),
        upstream_endpoint,
    })
}

pub(super) async fn metrics_handler(
    State(state): State<Arc<AppState>>,
) -> Json<super::types::MetricsPayload> {
    let telemetry = state
        .telemetry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let persistence = state
        .persistence
        .as_ref()
        .map(|producer| producer.stats_snapshot());
    Json(super::types::MetricsPayload {
        telemetry,
        persistence,
    })
}

pub(super) fn event_matches_filter(filter: &EventFilter, event: &EventKind) -> bool {
    filter.matches(event)
}

pub(super) struct StreamState {
    pub(super) state: Arc<AppState>,
    pub(super) rx: Option<tokio::sync::broadcast::Receiver<BroadcastEvent>>,
    pub(super) last_id: u64,
    pub(super) filter: EventFilter,
    pub(super) shutdown_rx: tokio::sync::watch::Receiver<bool>,
    pub(super) peer: SocketAddr,
    pub(super) _guard: Option<ClientGuard>,
}

fn archive_service(
    state: &Arc<AppState>,
) -> Result<&emwin_db::PostgresMetadataSink, (StatusCode, String)> {
    state.archive.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "archive database is not configured".to_string(),
    ))
}

fn normalize_incident_key(
    office: String,
    phenomena: String,
    significance: String,
    etn: i64,
) -> emwin_db::IncidentKey {
    emwin_db::IncidentKey {
        office: office.trim().to_ascii_uppercase(),
        phenomena: phenomena.trim().to_ascii_uppercase(),
        significance: significance.trim().to_ascii_uppercase(),
        etn,
    }
}

fn map_archive_error(err: emwin_db::PersistError) -> (StatusCode, String) {
    match err {
        emwin_db::PersistError::InvalidRequest(message)
        | emwin_db::PersistError::InvalidConfig(message) => (StatusCode::BAD_REQUEST, message),
        emwin_db::PersistError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
            (StatusCode::NOT_FOUND, io.to_string())
        }
        other => (StatusCode::BAD_GATEWAY, other.to_string()),
    }
}
