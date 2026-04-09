use super::super::types::{
    AppState, ArchiveFilterParams, ArchiveIssuePayload, ArchiveIssueResponse, ArchiveIssuesQuery,
    ArchiveIssuesResponse, ArchiveProductDetailPayload, ArchiveProductResponse,
    ArchiveProductSummaryPayload, ArchivedFeaturePayload, CellAggregateHttpQuery,
    CellAggregateResponse, FacetAggregateHttpQuery, FacetAggregateResponse,
    FeatureCollectionResponse, FeaturesGeoJsonQuery, FeaturesQuery, FeaturesResponse,
    IncidentDetailPayload, IncidentProductsQuery, IncidentProductsResponse, IncidentResponse,
    IncidentSummaryPayload, IncidentsQuery, IncidentsResponse, ProductsQuery, ProductsResponse,
    TimeseriesAggregateHttpQuery, TimeseriesAggregateResponse,
};
use super::support::{
    archive_service, map_archive_error, normalize_incident_key, parse_archive_filters,
};
use crate::server_support::build_bytes_download_response;
use axum::Json;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::StatusCode;
use axum::response::Response;
use emwin_service::{
    build_cell_aggregate_query, build_facet_aggregate_query, build_feature_list_query,
    build_timeseries_aggregate_query,
};
use std::sync::Arc;

#[utoipa::path(
    get,
    path = "/v1/products",
    tag = "products",
    params(ArchiveFilterParams, ProductsQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List archived products.", body = crate::server::openapi::ProductsResponseSchema),
        (status = 400, description = "Product filter query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn products_handler(
    State(state): State<Arc<AppState>>,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<ProductsQuery>,
) -> Result<Json<ProductsResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let filters = parse_archive_filters(raw_query.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let product_query = filters
        .into_product_list_query(100, query.limit, query.cursor)
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let page = archive
        .list_archived_products(product_query)
        .await
        .map_err(map_archive_error)?;

    Ok(Json(ProductsResponse {
        page: emwin_service::PaginatedResponse {
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
    params(ArchiveFilterParams, FeaturesQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "List archived spatial features.", body = crate::server::openapi::FeaturesResponseSchema),
        (status = 400, description = "Feature query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn features_handler(
    State(state): State<Arc<AppState>>,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<FeaturesQuery>,
) -> Result<Json<FeaturesResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let filters = parse_archive_filters(raw_query.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let feature_query = build_feature_list_query(
        filters
            .into_archive_filter_input()
            .map_err(|message| (StatusCode::BAD_REQUEST, message))?,
        query.kind,
        100,
        query.limit,
        query.cursor,
    )
    .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
    let page = archive
        .list_archived_features(feature_query)
        .await
        .map_err(map_archive_error)?;

    Ok(Json(FeaturesResponse {
        page: emwin_service::PaginatedResponse {
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
    params(ArchiveFilterParams, FeaturesGeoJsonQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "FeatureCollection view of archived spatial features.", body = crate::server::openapi::FeatureCollectionSchema),
        (status = 400, description = "Feature query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn features_geojson_handler(
    State(state): State<Arc<AppState>>,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<FeaturesGeoJsonQuery>,
) -> Result<Json<FeatureCollectionResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let filters = parse_archive_filters(raw_query.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let feature_query = build_feature_list_query(
        filters
            .into_archive_filter_input()
            .map_err(|message| (StatusCode::BAD_REQUEST, message))?,
        query.kind,
        100,
        query.limit,
        None,
    )
    .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
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
    params(ArchiveFilterParams, FacetAggregateHttpQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Facet aggregation over archived products.", body = crate::server::openapi::FacetAggregateResponseSchema),
        (status = 400, description = "Aggregate query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn facet_aggregate_handler(
    State(state): State<Arc<AppState>>,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<FacetAggregateHttpQuery>,
) -> Result<Json<FacetAggregateResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let filters = parse_archive_filters(raw_query.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let aggregate_query = build_facet_aggregate_query(
        filters
            .into_archive_filter_input()
            .map_err(|message| (StatusCode::BAD_REQUEST, message))?,
        &query.dimension,
        query.limit,
    )
    .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
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
    params(ArchiveFilterParams, TimeseriesAggregateHttpQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Timeseries aggregation over archived products.", body = crate::server::openapi::TimeseriesAggregateResponseSchema),
        (status = 400, description = "Aggregate query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn timeseries_aggregate_handler(
    State(state): State<Arc<AppState>>,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<TimeseriesAggregateHttpQuery>,
) -> Result<Json<TimeseriesAggregateResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let filters = parse_archive_filters(raw_query.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let aggregate_query = build_timeseries_aggregate_query(
        filters
            .into_archive_filter_input()
            .map_err(|message| (StatusCode::BAD_REQUEST, message))?,
        &query.measure,
        query.start,
        query.end,
        &query.bucket,
    )
    .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
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
    params(ArchiveFilterParams, CellAggregateHttpQuery),
    security(("bearer_auth" = [])),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Cell aggregation over archived products.", body = crate::server::openapi::CellAggregateResponseSchema),
        (status = 400, description = "Aggregate query validation failed.", body = String),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn cell_aggregate_handler(
    State(state): State<Arc<AppState>>,
    RawQuery(raw_query): RawQuery,
    Query(query): Query<CellAggregateHttpQuery>,
) -> Result<Json<CellAggregateResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let filters = parse_archive_filters(raw_query.as_deref())
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let aggregate_query = build_cell_aggregate_query(
        filters
            .into_archive_filter_input()
            .map_err(|message| (StatusCode::BAD_REQUEST, message))?,
        &query.measure,
        query.precision,
        query.limit,
    )
    .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
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
        (status = 200, description = "List live incident projection rows.", body = crate::server::openapi::IncidentsResponseSchema),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn incidents_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<IncidentsQuery>,
) -> Result<Json<IncidentsResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let page = archive
        .list_incidents(emwin_service::IncidentListQuery {
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
        page: emwin_service::PaginatedResponse {
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
        (status = 200, description = "Fetch one live incident projection row.", body = crate::server::openapi::IncidentResponseSchema),
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
        (status = 200, description = "List archived products linked to one incident.", body = crate::server::openapi::IncidentProductsResponseSchema),
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
            emwin_service::IncidentProductsQuery {
                limit: query.limit.unwrap_or(100),
                cursor: query.cursor,
            },
        )
        .await
        .map_err(map_archive_error)?;

    Ok(Json(IncidentProductsResponse {
        page: emwin_service::PaginatedResponse {
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
    path = "/v1/products/{product_id}",
    tag = "products",
    security(("bearer_auth" = [])),
    params(("product_id" = i64, Path, description = "Archived product id")),
    responses(
        (status = 401, description = "Missing or invalid bearer token.", body = String),
        (status = 200, description = "Fetch one archived product detail record.", body = crate::server::openapi::ArchiveProductResponseSchema),
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
        (status = 200, description = "List archived issue rows.", body = crate::server::openapi::ArchiveIssuesResponseSchema),
        (status = 503, description = "Archive metadata persistence is not configured.", body = String)
    )
)]
pub(super) async fn archive_issues_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ArchiveIssuesQuery>,
) -> Result<Json<ArchiveIssuesResponse>, (StatusCode, String)> {
    let archive = archive_service(&state)?;
    let page = archive
        .list_archived_issues(emwin_service::ArchivedIssueListQuery {
            product_id: query.product_id,
            kind: query.kind,
            code: query.code,
            limit: query.limit.unwrap_or(100),
            cursor: query.cursor,
        })
        .await
        .map_err(map_archive_error)?;

    Ok(Json(ArchiveIssuesResponse {
        page: emwin_service::PaginatedResponse {
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
        (status = 200, description = "Fetch one archived issue row.", body = crate::server::openapi::ArchiveIssueResponseSchema),
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
