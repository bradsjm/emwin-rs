//! HTTP and SSE handlers for live server mode.
//!
//! The handlers stay thin: request parsing and response shaping live here, while retention,
//! filtering, and ingest coordination live in neighboring modules.

use super::types::{
    API_PREFIX, AppState, ArchiveIssuePayload, ArchiveIssueResponse, ArchiveIssuesQuery,
    ArchiveIssuesResponse, ArchiveProductDetailPayload, ArchiveProductResponse,
    ArchiveProductSummaryPayload, ArchivedFeaturePayload, BroadcastEvent, CellAggregateHttpQuery,
    CellAggregateResponse, ClientGuard, EventFilter, EventKind, EventsQuery,
    FacetAggregateHttpQuery, FacetAggregateResponse, FeatureCollectionResponse,
    FeaturesGeoJsonQuery, FeaturesQuery, FeaturesResponse, FilesResponse, HealthResponse,
    IncidentBroadcastEvent, IncidentDetailPayload, IncidentEventFilter, IncidentEventPayload,
    IncidentEventsQuery, IncidentProductsQuery, IncidentProductsResponse, IncidentResponse,
    IncidentSummaryPayload, IncidentsQuery, IncidentsResponse, ProductsQuery, ProductsResponse,
    TimeseriesAggregateHttpQuery, TimeseriesAggregateResponse,
};
use crate::server_support::{
    build_bytes_download_response, build_file_download_response, filename_request_or_400,
};
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
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
    let auth_enabled = state.openapi_auth_token.is_some();
    let api_router = Router::new()
        .route("/products", get(products_handler))
        .route("/products/{product_id}", get(archive_product_handler))
        .route(
            "/products/{product_id}/raw",
            get(archive_product_raw_handler),
        )
        .route("/features", get(features_handler))
        .route("/features/geojson", get(features_geojson_handler))
        .route("/aggregates/facets", get(facet_aggregate_handler))
        .route("/aggregates/timeseries", get(timeseries_aggregate_handler))
        .route("/aggregates/cells", get(cell_aggregate_handler))
        .route("/issues", get(archive_issues_handler))
        .route("/issues/{issue_id}", get(archive_issue_handler))
        .route("/incidents", get(incidents_handler))
        .route(
            "/incidents/{office}/{phenomena}/{significance}/{etn}",
            get(incident_handler),
        )
        .route(
            "/incidents/{office}/{phenomena}/{significance}/{etn}/products",
            get(incident_products_handler),
        )
        .route("/streams/incidents", get(incident_events_handler))
        .route("/streams/products", get(events_handler))
        .route("/files", get(files_handler))
        .route("/files/{*filename}", get(file_download_handler))
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            require_bearer_auth,
        ));

    Router::new()
        .merge(super::openapi::swagger_ui_mount(auth_enabled))
        .nest(API_PREFIX, api_router)
        .layer(cors)
        .with_state(state)
}

async fn require_bearer_auth(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected_token) = state.openapi_auth_token.as_deref() else {
        return next.run(request).await;
    };

    let Some(header_value) = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return unauthorized_response();
    };

    let Some(provided_token) = parse_bearer_token(header_value) else {
        return unauthorized_response();
    };

    if provided_token != expected_token {
        return unauthorized_response();
    }

    next.run(request).await
}

fn unauthorized_response() -> Response {
    (StatusCode::UNAUTHORIZED, "missing or invalid bearer token").into_response()
}

fn parse_bearer_token(header_value: &str) -> Option<&str> {
    let mut parts = header_value.split_ascii_whitespace();
    let scheme = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some(token)
}

#[utoipa::path(
    get,
    path = "/v1/products",
    tag = "products",
    params(ProductsQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List archived products.", body = super::openapi::ProductsResponseSchema),
        (status = 400, description = "Product filter query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn products_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ProductsQuery>,
) -> Result<Json<ProductsResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let product_query = query
        .into_product_list_query()
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let page = archive
        .list_archived_products(product_query)
        .await
        .map_err(map_archive_error)?;

    Ok(Json(ProductsResponse {
        page: emwin_db::PaginatedResponse {
            items: page
                .items
                .into_iter()
                .map(ArchiveProductSummaryPayload::from_product)
                .collect(),
            next_cursor: page.next_cursor,
        },
    }))
}

#[utoipa::path(
    get,
    path = "/v1/features",
    tag = "features",
    params(FeaturesQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List archived spatial features.", body = super::openapi::FeaturesResponseSchema),
        (status = 400, description = "Feature query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn features_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FeaturesQuery>,
) -> Result<Json<FeaturesResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let feature_query = query
        .into_feature_list_query()
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let page = archive
        .list_archived_features(feature_query)
        .await
        .map_err(map_archive_error)?;

    Ok(Json(FeaturesResponse {
        page: emwin_db::PaginatedResponse {
            items: page
                .items
                .into_iter()
                .map(ArchivedFeaturePayload::from_feature)
                .collect(),
            next_cursor: page.next_cursor,
        },
    }))
}

#[utoipa::path(
    get,
    path = "/v1/features/geojson",
    tag = "features",
    params(FeaturesGeoJsonQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "FeatureCollection view of archived spatial features.", body = super::openapi::FeatureCollectionSchema),
        (status = 400, description = "Feature query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn features_geojson_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FeaturesGeoJsonQuery>,
) -> Result<Json<FeatureCollectionResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let feature_query = query
        .into_feature_list_query()
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let page = archive
        .list_archived_features(feature_query)
        .await
        .map_err(map_archive_error)?;

    Ok(Json(FeatureCollectionResponse {
        kind: "FeatureCollection",
        features: page
            .items
            .into_iter()
            .map(ArchivedFeaturePayload::from_feature)
            .map(ArchivedFeaturePayload::into_geojson_feature)
            .collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/aggregates/facets",
    tag = "aggregates",
    params(FacetAggregateHttpQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Facet aggregation over archived products.", body = super::openapi::FacetAggregateResponseSchema),
        (status = 400, description = "Aggregate query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn facet_aggregate_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FacetAggregateHttpQuery>,
) -> Result<Json<FacetAggregateResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let aggregate_query = query
        .into_facet_aggregate_query()
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let items = archive
        .list_facet_aggregate(aggregate_query.clone())
        .await
        .map_err(map_archive_error)?;

    Ok(Json(FacetAggregateResponse {
        dimension: aggregate_query.dimension.as_str().to_string(),
        completeness: items.completeness,
        items: items.items,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/aggregates/timeseries",
    tag = "aggregates",
    params(TimeseriesAggregateHttpQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Timeseries aggregation over archived products.", body = super::openapi::TimeseriesAggregateResponseSchema),
        (status = 400, description = "Aggregate query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn timeseries_aggregate_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TimeseriesAggregateHttpQuery>,
) -> Result<Json<TimeseriesAggregateResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let aggregate_query = query
        .into_timeseries_aggregate_query()
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let items = archive
        .list_timeseries_aggregate(aggregate_query.clone())
        .await
        .map_err(map_archive_error)?;

    Ok(Json(TimeseriesAggregateResponse {
        measure: aggregate_query.measure.as_str().to_string(),
        bucket: aggregate_query.bucket.as_str().to_string(),
        start: aggregate_query.start,
        end: aggregate_query.end,
        completeness: items.completeness,
        items: items.items,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/aggregates/cells",
    tag = "aggregates",
    description = "Returns uncursored geohash cell buckets for `product_count`. Each bucket counts distinct products per intersected geohash cell across persisted polygons, paths, and representative points after applying the requested spatial filters.",
    params(CellAggregateHttpQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Cell aggregation over archived products.", body = super::openapi::CellAggregateResponseSchema),
        (status = 400, description = "Aggregate query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn cell_aggregate_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CellAggregateHttpQuery>,
) -> Result<Json<CellAggregateResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let aggregate_query = query
        .into_cell_aggregate_query()
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let items = archive
        .list_cell_aggregate(aggregate_query.clone())
        .await
        .map_err(map_archive_error)?;

    Ok(Json(CellAggregateResponse {
        measure: aggregate_query.measure.as_str().to_string(),
        precision: aggregate_query.precision,
        completeness: items.completeness,
        items: items.items,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/incidents",
    tag = "incidents",
    params(IncidentsQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List live incident projection rows.", body = super::openapi::IncidentsResponseSchema),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
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

#[utoipa::path(
    get,
    path = "/v1/incidents/{office}/{phenomena}/{significance}/{etn}",
    tag = "incidents",
    security(("bearer_auth" = [])),
    params(
        ("office" = String, Path, description = "NWS office code"),
        ("phenomena" = String, Path, description = "VTEC phenomena code"),
        ("significance" = String, Path, description = "VTEC significance code"),
        ("etn" = i64, Path, description = "Event tracking number")
    ),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Fetch one live incident projection row.", body = super::openapi::IncidentResponseSchema),
        (status = 404, description = "Incident was not found.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
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

#[utoipa::path(
    get,
    path = "/v1/incidents/{office}/{phenomena}/{significance}/{etn}/products",
    tag = "incidents",
    security(("bearer_auth" = [])),
    params(
        ("office" = String, Path, description = "NWS office code"),
        ("phenomena" = String, Path, description = "VTEC phenomena code"),
        ("significance" = String, Path, description = "VTEC significance code"),
        ("etn" = i64, Path, description = "Event tracking number"),
        IncidentProductsQuery
    ),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List archived products linked to one incident.", body = super::openapi::IncidentProductsResponseSchema),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
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

#[utoipa::path(
    get,
    path = "/v1/products/{product_id}",
    tag = "products",
    security(("bearer_auth" = [])),
    params(("product_id" = i64, Path, description = "Archived product id")),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Fetch one archived product detail record.", body = super::openapi::ArchiveProductResponseSchema),
        (status = 404, description = "Archived product was not found.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
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

#[utoipa::path(
    get,
    path = "/v1/issues",
    tag = "issues",
    params(ArchiveIssuesQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List archived issue rows.", body = super::openapi::ArchiveIssuesResponseSchema),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn archive_issues_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ArchiveIssuesQuery>,
) -> Result<Json<ArchiveIssuesResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let page = archive
        .list_archived_issues(emwin_db::ArchivedIssueListQuery {
            product_id: query.product_id,
            kind: query.kind,
            code: query.code,
            limit: query.limit.unwrap_or(100),
            cursor: query.cursor,
        })
        .await
        .map_err(map_archive_error)?;

    Ok(Json(ArchiveIssuesResponse {
        page: emwin_db::PaginatedResponse {
            items: page
                .items
                .into_iter()
                .map(ArchiveIssuePayload::from_issue)
                .collect(),
            next_cursor: page.next_cursor,
        },
    }))
}

#[utoipa::path(
    get,
    path = "/v1/issues/{issue_id}",
    tag = "issues",
    security(("bearer_auth" = [])),
    params(("issue_id" = i64, Path, description = "Archived issue id")),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Fetch one archived issue row.", body = super::openapi::ArchiveIssueResponseSchema),
        (status = 404, description = "Archived issue was not found.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn archive_issue_handler(
    State(state): State<Arc<AppState>>,
    Path(issue_id): Path<i64>,
) -> Result<Json<ArchiveIssueResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let issue = archive
        .get_archived_issue(issue_id)
        .await
        .map_err(map_archive_error)?
        .ok_or((
            StatusCode::NOT_FOUND,
            "archived issue not found".to_string(),
        ))?;

    Ok(Json(ArchiveIssueResponse {
        issue: ArchiveIssuePayload::from_issue(issue),
    }))
}

#[utoipa::path(
    get,
    path = "/v1/products/{product_id}/raw",
    tag = "products",
    security(("bearer_auth" = [])),
    params(("product_id" = i64, Path, description = "Archived product id")),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Download archived raw payload bytes.", content_type = "application/octet-stream"),
        (status = 404, description = "Archived payload was not found.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
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

#[utoipa::path(
    get,
    path = "/v1/streams/products",
    tag = "streams",
    description = "Incremental SSE stream of completed products. Clients should fetch an initial snapshot from the resource endpoints, then attach the stream. `Last-Event-ID` is best-effort for short reconnect gaps only; lag warnings require a full resync.",
    params(EventsQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Server-sent events stream of live feed activity.", body = super::openapi::SseEventEnvelope, content_type = "text/event-stream"),
        (status = 400, description = "Event filter query validation failed.", body = String),
        (status = 429, description = "Concurrent SSE client limit reached.", body = String)
    )
)]
pub(super) async fn events_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
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
    let guard = acquire_client_guard(&state, peer)?;
    let rx = state.event_tx.subscribe();
    let shutdown_rx = state.shutdown_rx.clone();

    let stream = futures::stream::unfold(
        StreamState {
            state: Arc::clone(&state),
            rx: Some(rx),
            last_id,
            filter,
            shutdown_rx,
            peer,
            _guard: Some(guard),
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
                        if !matches!(event.kind, EventKind::FileComplete(_)) {
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

#[utoipa::path(
    get,
    path = "/v1/streams/incidents",
    tag = "streams",
    description = "Incremental SSE stream of persisted incident projection changes. Clients should fetch an initial snapshot from the incident resource endpoints, then attach the stream. `Last-Event-ID` is best-effort for short reconnect gaps only; lag warnings require a full resync.",
    params(IncidentEventsQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Server-sent events stream of persisted incident projection changes.", body = super::openapi::SseEventEnvelope, content_type = "text/event-stream"),
        (status = 429, description = "Concurrent SSE client limit reached.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn incident_events_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<IncidentEventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let _ = archive_service(&state)?;
    let guard = acquire_client_guard(&state, peer)?;

    let rx = state.incident_event_tx.subscribe();
    let shutdown_rx = state.shutdown_rx.clone();
    let last_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let filter = IncidentEventFilter::from_query(query);

    let stream = futures::stream::unfold(
        IncidentStreamState {
            state: Arc::clone(&state),
            rx: Some(rx),
            last_id,
            filter,
            shutdown_rx,
            peer,
            _guard: Some(guard),
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
                        if !st.filter.matches(&event.payload) {
                            continue;
                        }

                        st.last_id = event.id;
                        let payload = match serde_json::to_string(&event.payload) {
                            Ok(payload) => payload,
                            Err(_) => "{}".to_string(),
                        };
                        let sse = Event::default()
                            .id(event.id.to_string())
                            .event(IncidentEventPayload::EVENT_NAME)
                            .data(payload);
                        return Some((Ok(sse), st));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        super::log_info(
                            st.state.quiet,
                            &format!("incident sse client lagged peer={} dropped={}", st.peer, dropped),
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

#[utoipa::path(
    get,
    path = "/v1/files",
    tag = "operational",
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List retained completed files.", body = super::openapi::FilesResponseSchema)
    )
)]
pub(super) async fn files_handler(State(state): State<Arc<AppState>>) -> Json<FilesResponse> {
    let files = state
        .live
        .list_retained_files()
        .into_iter()
        .map(super::types::CompletedFilePayload::from_metadata)
        .collect();
    Json(FilesResponse { files })
}

#[utoipa::path(
    get,
    path = "/v1/files/{filename}",
    tag = "operational",
    security(("bearer_auth" = [])),
    params(("filename" = String, Path, description = "URL-encoded retained file path")),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Download retained file contents.", content_type = "application/octet-stream"),
        (status = 400, description = "Requested filename is invalid.", body = String),
        (status = 404, description = "Retained file was not found.", body = String)
    )
)]
pub(super) async fn file_download_handler(
    State(state): State<Arc<AppState>>,
    Path(filename): Path<String>,
) -> Result<Response, StatusCode> {
    let normalized = filename_request_or_400(&filename)?;

    let file = state
        .live
        .get_retained_file(&normalized)
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(build_file_download_response(file))
}

#[utoipa::path(
    get,
    path = "/v1/health",
    tag = "operational",
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Live server health summary.", body = super::openapi::HealthResponseSchema)
    )
)]
pub(super) async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let connected_clients = state.connected_clients.load(Ordering::Relaxed);
    let snapshot = state.live.stats_snapshot();

    Json(HealthResponse {
        status: "ok",
        connected_clients,
        retained_files: snapshot.retained_files,
        uptime_secs: snapshot.uptime_secs,
        upstream_endpoint: snapshot.upstream_endpoint,
    })
}

#[utoipa::path(
    get,
    path = "/v1/metrics",
    tag = "operational",
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Live server telemetry snapshot.", body = serde_json::Value)
    )
)]
pub(super) async fn metrics_handler(
    State(state): State<Arc<AppState>>,
) -> Json<super::types::MetricsPayload> {
    let telemetry = state.live.telemetry_snapshot();
    let persistence = state.live.stats_snapshot().persistence;
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

pub(super) struct IncidentStreamState {
    pub(super) state: Arc<AppState>,
    pub(super) rx: Option<tokio::sync::broadcast::Receiver<IncidentBroadcastEvent>>,
    pub(super) last_id: u64,
    pub(super) filter: IncidentEventFilter,
    pub(super) shutdown_rx: tokio::sync::watch::Receiver<bool>,
    pub(super) peer: SocketAddr,
    pub(super) _guard: Option<ClientGuard>,
}

fn reserve_client_slot(
    state: &Arc<AppState>,
    peer: SocketAddr,
) -> Result<(), (StatusCode, String)> {
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
    Ok(())
}

fn acquire_client_guard(
    state: &Arc<AppState>,
    peer: SocketAddr,
) -> Result<ClientGuard, (StatusCode, String)> {
    reserve_client_slot(state, peer)?;
    Ok(ClientGuard {
        state: Arc::clone(state),
        peer,
    })
}

fn archive_service(
    state: &Arc<AppState>,
) -> Result<emwin_db::PostgresMetadataSink, (StatusCode, String)> {
    state.live.archive_sink().ok_or((
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

#[cfg(test)]
mod tests;
