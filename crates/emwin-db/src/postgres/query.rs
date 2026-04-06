use super::prepare::PendingIncidentChange;
use super::{
    AggregateCompleteness, ArchivedFeature, ArchivedIssue, ArchivedIssueCursor,
    ArchivedIssueListQuery, ArchivedProductDetail, ArchivedProductSummary, CellAggregateBucket,
    CellAggregateQuery, CellAggregateResult, CellMeasure, FacetAggregateBucket,
    FacetAggregateQuery, FacetAggregateResult, FacetDimension, FeatureCursor, FeatureKind,
    FeatureListQuery, IncidentChange, IncidentChangeTrigger, IncidentCursor, IncidentKey,
    IncidentListQuery, IncidentProductsCursor, IncidentProductsQuery, IncidentSummary,
    PaginatedResponse, PersistError, PersistResult, ProductCursor, ProductListQuery,
    TimeseriesAggregateBucket, TimeseriesAggregateQuery, TimeseriesAggregateResult,
    TimeseriesMeasure,
};
use crate::writer::BlobStorageKind;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::types::Json as SqlxJson;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

pub(super) async fn load_incident_changes(
    pool: &PgPool,
    changes: Vec<PendingIncidentChange>,
    trigger: IncidentChangeTrigger,
) -> PersistResult<Vec<IncidentChange>> {
    let mut loaded = Vec::with_capacity(changes.len());
    for change in changes {
        let Some(incident) = fetch_incident_summary(pool, &change.key).await? else {
            continue;
        };
        loaded.push(IncidentChange {
            action: change.action,
            trigger,
            incident,
        });
    }
    Ok(loaded)
}

async fn fetch_incident_summary(
    pool: &PgPool,
    key: &IncidentKey,
) -> PersistResult<Option<IncidentSummary>> {
    let mut builder = QueryBuilder::<Postgres>::new(incident_select_sql());
    builder
        .push(" WHERE office = ")
        .push_bind(&key.office)
        .push(" AND phenomena = ")
        .push_bind(&key.phenomena)
        .push(" AND significance = ")
        .push_bind(&key.significance)
        .push(" AND etn = ")
        .push_bind(key.etn);
    builder
        .build_query_as::<IncidentSummary>()
        .fetch_optional(pool)
        .await
        .map_err(PersistError::from)
}

pub(super) async fn list_incidents_query(
    pool: &PgPool,
    query: IncidentListQuery,
) -> PersistResult<PaginatedResponse<IncidentSummary>> {
    let limit = normalize_page_limit(query.limit);
    let cursor = decode_optional_cursor::<IncidentCursor>(query.cursor.as_deref())?;
    let mut builder = QueryBuilder::<Postgres>::new(incident_select_sql());
    builder.push(" WHERE 1 = 1");

    if let Some(office) = query.office.as_deref() {
        builder
            .push(" AND office = ")
            .push_bind(office.trim().to_ascii_uppercase());
    }
    if let Some(phenomena) = query.phenomena.as_deref() {
        builder
            .push(" AND phenomena = ")
            .push_bind(phenomena.trim().to_ascii_uppercase());
    }
    if let Some(significance) = query.significance.as_deref() {
        builder
            .push(" AND significance = ")
            .push_bind(significance.trim().to_ascii_uppercase());
    }
    if let Some(etn) = query.etn {
        builder.push(" AND etn = ").push_bind(etn);
    }
    if let Some(status) = query.status.as_deref() {
        builder
            .push(" AND current_status = ")
            .push_bind(status.trim().to_ascii_lowercase());
    }
    if let Some(updated_after) = query.updated_after {
        builder
            .push(" AND last_updated_at >= ")
            .push_bind(updated_after);
    }
    if let Some(updated_before) = query.updated_before {
        builder
            .push(" AND last_updated_at <= ")
            .push_bind(updated_before);
    }
    if let Some(active_at) = query.active_at {
        builder
            .push(" AND issued_at <= ")
            .push_bind(active_at)
            .push(" AND (start_utc IS NULL OR start_utc <= ")
            .push_bind(active_at)
            .push(")")
            .push(" AND (end_utc IS NULL OR end_utc >= ")
            .push_bind(active_at)
            .push(")");
    }
    if let Some(cursor) = cursor.as_ref() {
        builder
            .push(
                " AND (
                    latest_product_timestamp_utc < ",
            )
            .push_bind(cursor.latest_product_timestamp_utc)
            .push(
                " OR (
                    latest_product_timestamp_utc = ",
            )
            .push_bind(cursor.latest_product_timestamp_utc)
            .push(
                " AND (
                    office > ",
            )
            .push_bind(&cursor.office)
            .push(
                " OR (
                    office = ",
            )
            .push_bind(&cursor.office)
            .push(" AND phenomena > ")
            .push_bind(&cursor.phenomena)
            .push(
                ") OR (
                    office = ",
            )
            .push_bind(&cursor.office)
            .push(" AND phenomena = ")
            .push_bind(&cursor.phenomena)
            .push(" AND significance > ")
            .push_bind(&cursor.significance)
            .push(
                ") OR (
                    office = ",
            )
            .push_bind(&cursor.office)
            .push(" AND phenomena = ")
            .push_bind(&cursor.phenomena)
            .push(" AND significance = ")
            .push_bind(&cursor.significance)
            .push(" AND etn > ")
            .push_bind(cursor.etn)
            .push(")))");
    }

    builder.push(
        " ORDER BY latest_product_timestamp_utc DESC, office ASC, phenomena ASC, significance ASC, etn ASC LIMIT ",
    );
    builder.push_bind(i64::try_from(limit + 1).expect("limit should fit in i64"));

    let mut items = builder
        .build_query_as::<IncidentSummary>()
        .fetch_all(pool)
        .await?;

    let next_cursor = if items.len() > limit {
        items.pop().expect("overflow item should exist");
        let tail = items
            .last()
            .expect("page with next cursor should retain at least one item");
        Some(encode_cursor(&IncidentCursor {
            latest_product_timestamp_utc: tail.latest_product_timestamp_utc,
            office: tail.office.clone(),
            phenomena: tail.phenomena.clone(),
            significance: tail.significance.clone(),
            etn: tail.etn,
        })?)
    } else {
        None
    };

    Ok(PaginatedResponse { items, next_cursor })
}

pub(super) async fn list_incident_products_query(
    pool: &PgPool,
    key: &IncidentKey,
    query: IncidentProductsQuery,
) -> PersistResult<PaginatedResponse<ArchivedProductSummary>> {
    let limit = normalize_page_limit(query.limit);
    let cursor = decode_optional_cursor::<IncidentProductsCursor>(query.cursor.as_deref())?;
    let mut builder = QueryBuilder::<Postgres>::new(archived_product_summary_select_sql());
    builder.push(
        " WHERE EXISTS (
            SELECT 1
            FROM product_vtec
            WHERE product_vtec.product_id = products.id
              AND office = ",
    );
    builder
        .push_bind(&key.office)
        .push(" AND phenomena = ")
        .push_bind(&key.phenomena)
        .push(" AND significance = ")
        .push_bind(&key.significance)
        .push(" AND etn = ")
        .push_bind(key.etn)
        .push(")");

    if let Some(cursor) = cursor.as_ref() {
        builder
            .push(" AND (source_timestamp_utc > ")
            .push_bind(cursor.source_timestamp_utc)
            .push(" OR (source_timestamp_utc = ")
            .push_bind(cursor.source_timestamp_utc)
            .push(" AND id > ")
            .push_bind(cursor.product_id)
            .push("))");
    }

    builder.push(" ORDER BY source_timestamp_utc ASC, id ASC LIMIT ");
    builder.push_bind(i64::try_from(limit + 1).expect("limit should fit in i64"));

    let mut items = builder
        .build_query_as::<ArchivedProductSummary>()
        .fetch_all(pool)
        .await?;

    let next_cursor = if items.len() > limit {
        items.pop().expect("overflow item should exist");
        let tail = items
            .last()
            .expect("page with next cursor should retain at least one item");
        Some(encode_cursor(&IncidentProductsCursor {
            source_timestamp_utc: tail.source_timestamp_utc,
            product_id: tail.product_id,
        })?)
    } else {
        None
    };

    Ok(PaginatedResponse { items, next_cursor })
}

pub(super) async fn list_archived_products_query(
    pool: &PgPool,
    query: ProductListQuery,
) -> PersistResult<PaginatedResponse<ArchivedProductSummary>> {
    let limit = normalize_page_limit(query.limit);
    let cursor = decode_optional_cursor::<ProductCursor>(query.cursor.as_deref())?;
    let mut builder = QueryBuilder::<Postgres>::new(archived_product_summary_select_sql());
    builder.push(" WHERE 1 = 1");

    append_product_filters(&mut builder, &query)?;

    if let Some(cursor) = cursor.as_ref() {
        builder
            .push(" AND (products.source_timestamp_utc < ")
            .push_bind(cursor.source_timestamp_utc)
            .push(" OR (products.source_timestamp_utc = ")
            .push_bind(cursor.source_timestamp_utc)
            .push(" AND products.id < ")
            .push_bind(cursor.product_id)
            .push("))");
    }

    builder.push(" ORDER BY products.source_timestamp_utc DESC, products.id DESC LIMIT ");
    builder.push_bind(i64::try_from(limit + 1).expect("limit should fit in i64"));

    let mut items = builder
        .build_query_as::<ArchivedProductSummary>()
        .fetch_all(pool)
        .await?;

    let next_cursor = if items.len() > limit {
        items.pop().expect("overflow item should exist");
        let tail = items
            .last()
            .expect("page with next cursor should retain at least one item");
        Some(encode_cursor(&ProductCursor {
            source_timestamp_utc: tail.source_timestamp_utc,
            product_id: tail.product_id,
        })?)
    } else {
        None
    };

    Ok(PaginatedResponse { items, next_cursor })
}

pub(super) async fn list_archived_features_query(
    pool: &PgPool,
    query: FeatureListQuery,
) -> PersistResult<PaginatedResponse<ArchivedFeature>> {
    let limit = normalize_page_limit(query.filters.limit);
    let cursor = decode_optional_cursor::<FeatureCursor>(query.filters.cursor.as_deref())?;
    let mut builder = QueryBuilder::<Postgres>::new(archived_feature_select_sql());
    builder.push(" WHERE 1 = 1");
    append_product_filters(&mut builder, &query.filters)?;
    append_feature_spatial_filter(&mut builder, &query.filters)?;

    if let Some(kind) = query.kind {
        builder
            .push(" AND features.feature_kind = ")
            .push_bind(kind.as_str());
    }
    if let Some(cursor) = cursor.as_ref() {
        builder
            .push(" AND (products.source_timestamp_utc < ")
            .push_bind(cursor.source_timestamp_utc)
            .push(" OR (products.source_timestamp_utc = ")
            .push_bind(cursor.source_timestamp_utc)
            .push(" AND (products.id < ")
            .push_bind(cursor.product_id)
            .push(" OR (products.id = ")
            .push_bind(cursor.product_id)
            .push(" AND (features.feature_kind_order > ")
            .push_bind(cursor.feature_kind.ordinal())
            .push(" OR (features.feature_kind_order = ")
            .push_bind(cursor.feature_kind.ordinal())
            .push(" AND features.feature_row_id < ")
            .push_bind(cursor.feature_row_id)
            .push(")))))");
    }

    builder.push(
        " ORDER BY products.source_timestamp_utc DESC, products.id DESC, features.feature_kind_order ASC, features.feature_row_id DESC LIMIT ",
    );
    builder.push_bind(i64::try_from(limit + 1).expect("limit should fit in i64"));

    let mut items = builder
        .build()
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(archived_feature_from_row)
        .collect::<PersistResult<Vec<_>>>()?;

    let next_cursor = if items.len() > limit {
        items.pop().expect("overflow item should exist");
        let tail = items
            .last()
            .expect("page with next cursor should retain at least one item");
        let feature_row_id = tail
            .feature_id
            .rsplit_once(':')
            .and_then(|(_, suffix)| suffix.parse::<i64>().ok())
            .expect("feature id should end with row id");
        Some(encode_cursor(&FeatureCursor {
            source_timestamp_utc: tail.source_timestamp_utc,
            product_id: tail.product_id,
            feature_kind: tail.feature_kind,
            feature_row_id,
        })?)
    } else {
        None
    };

    Ok(PaginatedResponse { items, next_cursor })
}

pub(super) async fn list_facet_aggregate_query(
    pool: &PgPool,
    query: FacetAggregateQuery,
) -> PersistResult<FacetAggregateResult> {
    let limit = query.limit.clamp(1, 100);
    let mut builder = QueryBuilder::<Postgres>::new(facet_aggregate_select_sql(query.dimension));
    builder.push(" WHERE 1 = 1");
    append_product_filters(&mut builder, &query.filters)?;
    match query.dimension {
        FacetDimension::Phenomena | FacetDimension::Significance => {
            append_vtec_alias_filters(&mut builder, "facet", &query.filters)?
        }
        FacetDimension::Status => {
            append_vtec_alias_filters(&mut builder, "product_vtec", &query.filters)?
        }
        FacetDimension::IssueKind | FacetDimension::IssueCode => {
            append_issue_alias_filters(&mut builder, "facet", &query.filters)
        }
        FacetDimension::Office | FacetDimension::Family | FacetDimension::ArtifactKind => {}
    }
    append_facet_non_null_filter(&mut builder, query.dimension);
    builder.push(" GROUP BY value ORDER BY count DESC, value ASC LIMIT ");
    builder.push_bind(i64::try_from(limit).expect("limit should fit in i64"));
    let items = builder
        .build_query_as::<FacetAggregateBucket>()
        .fetch_all(pool)
        .await
        .map_err(PersistError::from)?;
    Ok(FacetAggregateResult {
        completeness: AggregateCompleteness::exact(),
        items,
    })
}

pub(super) async fn list_timeseries_aggregate_query(
    pool: &PgPool,
    query: TimeseriesAggregateQuery,
) -> PersistResult<TimeseriesAggregateResult> {
    if query.end <= query.start {
        return Err(PersistError::InvalidRequest(
            "end must be after start".to_string(),
        ));
    }

    let bucket_duration = query.bucket.duration();
    let bucket_seconds = bucket_duration.num_seconds();
    let span_seconds = (query.end - query.start).num_seconds();
    let bucket_count = (span_seconds + bucket_seconds - 1) / bucket_seconds;
    if bucket_count > 1_000 {
        return Err(PersistError::InvalidRequest(
            "timeseries request would produce more than 1000 buckets".to_string(),
        ));
    }

    let mut builder = QueryBuilder::<Postgres>::new(
        "WITH matching_products AS (
            SELECT
                products.id,
                to_timestamp(products.source_timestamp_utc) AS source_time
            FROM products
            WHERE 1 = 1",
    );
    append_product_filters(&mut builder, &query.filters)?;
    builder.push("), buckets AS (SELECT bucket_start, LEAST(bucket_start + interval '");
    builder
        .push(query.bucket.postgres_interval())
        .push("', ")
        .push_bind(query.end)
        .push(") AS bucket_end FROM generate_series(")
        .push_bind(query.start)
        .push(", ")
        .push_bind(query.end)
        .push(", interval '")
        .push(query.bucket.postgres_interval())
        .push("') AS bucket_start WHERE bucket_start < ")
        .push_bind(query.end)
        .push(") SELECT buckets.bucket_start, buckets.bucket_end, ");
    match query.measure {
        TimeseriesMeasure::ProductCount => {
            builder.push("COUNT(DISTINCT matching_products.id) AS count FROM buckets LEFT JOIN matching_products ON matching_products.source_time >= buckets.bucket_start AND matching_products.source_time < buckets.bucket_end");
        }
        TimeseriesMeasure::IssueCount => {
            builder.push("COUNT(product_issues.id) AS count FROM buckets LEFT JOIN matching_products ON matching_products.source_time >= buckets.bucket_start AND matching_products.source_time < buckets.bucket_end LEFT JOIN product_issues ON product_issues.product_id = matching_products.id");
            append_issue_alias_join_filters(&mut builder, "product_issues", &query.filters);
        }
        TimeseriesMeasure::IncidentCount => {
            builder.push("COUNT(DISTINCT (product_vtec.office, product_vtec.phenomena, product_vtec.significance, product_vtec.etn)) AS count FROM buckets LEFT JOIN matching_products ON matching_products.source_time >= buckets.bucket_start AND matching_products.source_time < buckets.bucket_end LEFT JOIN product_vtec ON product_vtec.product_id = matching_products.id");
            append_vtec_alias_join_filters(&mut builder, "product_vtec", &query.filters)?;
        }
    }
    builder.push(
        " GROUP BY buckets.bucket_start, buckets.bucket_end ORDER BY buckets.bucket_start ASC",
    );

    let items = builder
        .build_query_as::<TimeseriesAggregateBucket>()
        .fetch_all(pool)
        .await
        .map_err(PersistError::from)?;
    Ok(TimeseriesAggregateResult {
        completeness: AggregateCompleteness::exact(),
        items,
    })
}

pub(super) async fn list_cell_aggregate_query(
    pool: &PgPool,
    query: CellAggregateQuery,
) -> PersistResult<CellAggregateResult> {
    if query.measure != CellMeasure::ProductCount {
        return Err(PersistError::InvalidRequest(
            "unsupported cell aggregate measure".to_string(),
        ));
    }
    if !(1..=12).contains(&query.precision) {
        return Err(PersistError::InvalidRequest(
            "precision must be between 1 and 12".to_string(),
        ));
    }

    let limit = query.limit.clamp(1, 1000);
    let precision = i32::from(query.precision);
    let mut builder = QueryBuilder::<Postgres>::new(
        "WITH RECURSIVE matching_features AS (
            SELECT DISTINCT
                features.product_id,
                features.feature_geom",
    );
    builder.push(archived_feature_source_sql());
    builder.push(" WHERE 1 = 1");
    append_product_filters(&mut builder, &query.filters)?;
    append_feature_spatial_filter(&mut builder, &query.filters)?;
    builder.push(
        "), geohash_prefixes(cell) AS (
            SELECT chars.ch
            FROM ",
    );
    builder.push(geohash_alphabet_sql());
    builder.push(
        " AS chars
            WHERE EXISTS (
                SELECT 1
                FROM matching_features
                WHERE ST_Intersects(
                    matching_features.feature_geom,
                    ST_SetSRID(ST_GeomFromGeoHash(chars.ch), 4326)
                )
            )
            UNION ALL
            SELECT geohash_prefixes.cell || chars.ch
            FROM geohash_prefixes
            CROSS JOIN ",
    );
    builder.push(geohash_alphabet_sql());
    builder.push(" AS chars WHERE char_length(geohash_prefixes.cell) < ");
    builder.push_bind(precision);
    builder.push(
        " AND EXISTS (
                SELECT 1
                FROM matching_features
                WHERE ST_Intersects(
                    matching_features.feature_geom,
                    ST_SetSRID(ST_GeomFromGeoHash(geohash_prefixes.cell || chars.ch), 4326)
                )
            )
        ), candidate_cells AS (
            SELECT cell
            FROM geohash_prefixes
            WHERE char_length(cell) = ",
    );
    builder.push_bind(precision);
    builder.push(
        ")
        SELECT
            candidate_cells.cell,
            COUNT(DISTINCT matching_features.product_id) AS count
        FROM candidate_cells
        INNER JOIN matching_features
            ON ST_Intersects(
                matching_features.feature_geom,
                ST_SetSRID(ST_GeomFromGeoHash(candidate_cells.cell), 4326)
            )",
    );
    builder
        .push(" GROUP BY candidate_cells.cell ORDER BY count DESC, candidate_cells.cell ASC LIMIT ")
        .push_bind(i64::try_from(limit).expect("limit should fit in i64"));

    let items = builder
        .build_query_as::<CellAggregateBucket>()
        .fetch_all(pool)
        .await
        .map_err(PersistError::from)?;
    Ok(CellAggregateResult {
        completeness: AggregateCompleteness::exact(),
        items,
    })
}

pub(super) async fn list_archived_issues_query(
    pool: &PgPool,
    query: ArchivedIssueListQuery,
) -> PersistResult<PaginatedResponse<ArchivedIssue>> {
    let limit = normalize_page_limit(query.limit);
    let cursor = decode_optional_cursor::<ArchivedIssueCursor>(query.cursor.as_deref())?;
    let mut builder = QueryBuilder::<Postgres>::new(archived_issue_select_sql());
    builder.push(" WHERE 1 = 1");

    if let Some(product_id) = query.product_id {
        builder
            .push(" AND product_issues.product_id = ")
            .push_bind(product_id);
    }
    if let Some(kind) = query.kind.as_deref() {
        builder
            .push(" AND product_issues.kind = ")
            .push_bind(kind.trim().to_ascii_lowercase());
    }
    if let Some(code) = query.code.as_deref() {
        builder
            .push(" AND product_issues.code = ")
            .push_bind(code.trim().to_ascii_lowercase());
    }
    if let Some(cursor) = cursor.as_ref() {
        builder
            .push(" AND (products.source_timestamp_utc < ")
            .push_bind(cursor.source_timestamp_utc)
            .push(" OR (products.source_timestamp_utc = ")
            .push_bind(cursor.source_timestamp_utc)
            .push(" AND (product_issues.product_id < ")
            .push_bind(cursor.product_id)
            .push(" OR (product_issues.product_id = ")
            .push_bind(cursor.product_id)
            .push(" AND product_issues.id < ")
            .push_bind(cursor.issue_id)
            .push("))))");
    }

    builder.push(
        " ORDER BY products.source_timestamp_utc DESC, product_issues.product_id DESC, product_issues.id DESC LIMIT ",
    );
    builder.push_bind(i64::try_from(limit + 1).expect("limit should fit in i64"));

    let mut items = builder
        .build_query_as::<ArchivedIssue>()
        .fetch_all(pool)
        .await?;

    let next_cursor = if items.len() > limit {
        items.pop().expect("overflow item should exist");
        let tail = items
            .last()
            .expect("page with next cursor should retain at least one item");
        let source_timestamp_utc = fetch_issue_cursor_timestamp(pool, tail.id).await?;
        Some(encode_cursor(&ArchivedIssueCursor {
            source_timestamp_utc,
            product_id: tail.product_id,
            issue_id: tail.id,
        })?)
    } else {
        None
    };

    Ok(PaginatedResponse { items, next_cursor })
}

pub(super) async fn get_archived_issue_query(
    pool: &PgPool,
    issue_id: i64,
) -> PersistResult<Option<ArchivedIssue>> {
    let mut builder = QueryBuilder::<Postgres>::new(archived_issue_select_sql());
    builder
        .push(" WHERE product_issues.id = ")
        .push_bind(issue_id)
        .push(" ORDER BY products.source_timestamp_utc DESC, product_issues.product_id DESC, product_issues.id DESC");
    builder
        .build_query_as::<ArchivedIssue>()
        .fetch_optional(pool)
        .await
        .map_err(PersistError::from)
}

pub(super) fn normalize_page_limit(limit: usize) -> usize {
    limit.clamp(1, 500)
}

pub(super) fn incident_select_sql() -> String {
    String::from(
        "SELECT
            office,
            phenomena,
            significance,
            etn,
            current_status,
            latest_vtec_action,
            issued_at,
            start_utc,
            end_utc,
            last_updated_at,
            first_product_id,
            latest_product_id,
            latest_product_timestamp_utc
         FROM incidents",
    )
}

pub(super) fn archived_product_summary_select_sql() -> String {
    String::from(
        "SELECT
            id AS product_id,
            filename,
            source_timestamp_utc,
            ingested_at,
            source_receiver,
            source_message_id,
            size_bytes,
            payload_storage_kind,
            (metadata_location IS NOT NULL) AS has_metadata_sidecar,
            source,
            family,
            artifact_kind,
            title,
            container,
            pil,
            wmo_prefix,
            bbb_kind,
            office_code,
            office_city,
            office_state,
            header_kind,
            ttaaii,
            cccc,
            ddhhmm,
            bbb,
            afos,
            has_body,
            has_artifact,
            has_issues,
            has_vtec,
            has_ugc,
            has_hvtec,
            has_latlon,
            has_time_mot_loc,
            has_wind_hail,
            vtec_count,
            ugc_count,
            hvtec_count,
            latlon_count,
            time_mot_loc_count,
            wind_hail_count,
            issue_count
         FROM products",
    )
}

pub(super) fn archived_product_detail_select_sql() -> String {
    let mut sql = archived_product_summary_select_sql();
    sql.push_str(", payload_location, metadata_location, product_json");
    sql
}

pub(super) fn archived_issue_select_sql() -> String {
    String::from(
        "SELECT
            product_issues.id,
            product_issues.product_id,
            product_issues.kind,
            product_issues.code,
            product_issues.message,
            product_issues.line
         FROM product_issues
         INNER JOIN products ON products.id = product_issues.product_id",
    )
}

pub(super) fn archived_feature_select_sql() -> String {
    let mut sql = String::from(
        "SELECT
            features.feature_kind,
            features.feature_kind_order,
            features.feature_row_id,
            products.id AS product_id,
            products.source_timestamp_utc,
            features.feature_geom,
            features.geometry,
            features.properties",
    );
    sql.push_str(&archived_feature_source_sql());
    sql
}

fn archived_feature_source_sql() -> String {
    String::from(
        " FROM (
            SELECT
                'polygon' AS feature_kind,
                1 AS feature_kind_order,
                product_polygons.id AS feature_row_id,
                product_polygons.product_id,
                product_polygons.polygon_geom AS feature_geom,
                ST_AsGeoJSON(product_polygons.polygon_geom)::json AS geometry,
                jsonb_build_object(
                    'segment_index', product_polygons.segment_index,
                    'polygon_index', product_polygons.polygon_index
                ) AS properties
            FROM product_polygons
            UNION ALL
            SELECT
                'time_mot_loc_path' AS feature_kind,
                2 AS feature_kind_order,
                product_time_mot_loc.id AS feature_row_id,
                product_time_mot_loc.product_id,
                product_time_mot_loc.path_geom AS feature_geom,
                ST_AsGeoJSON(product_time_mot_loc.path_geom)::json AS geometry,
                jsonb_build_object(
                    'segment_index', product_time_mot_loc.segment_index,
                    'entry_index', product_time_mot_loc.entry_index,
                    'time_utc', product_time_mot_loc.time_utc,
                    'direction_degrees', product_time_mot_loc.direction_degrees,
                    'speed_kt', product_time_mot_loc.speed_kt
                ) AS properties
            FROM product_time_mot_loc
            UNION ALL
            SELECT
                'ugc_point' AS feature_kind,
                3 AS feature_kind_order,
                product_ugc_areas.id AS feature_row_id,
                product_ugc_areas.product_id,
                product_ugc_areas.point_geom AS feature_geom,
                ST_AsGeoJSON(product_ugc_areas.point_geom)::json AS geometry,
                jsonb_build_object(
                    'segment_index', product_ugc_areas.segment_index,
                    'section_index', product_ugc_areas.section_index,
                    'area_kind', product_ugc_areas.area_kind,
                    'state', product_ugc_areas.state,
                    'ugc_code', product_ugc_areas.ugc_code,
                    'name', product_ugc_areas.name,
                    'expires_utc', product_ugc_areas.expires_utc
                ) AS properties
            FROM product_ugc_areas
            WHERE product_ugc_areas.point_geom IS NOT NULL
            UNION ALL
            SELECT
                'hvtec_point' AS feature_kind,
                4 AS feature_kind_order,
                product_hvtec.id AS feature_row_id,
                product_hvtec.product_id,
                product_hvtec.point_geom AS feature_geom,
                ST_AsGeoJSON(product_hvtec.point_geom)::json AS geometry,
                jsonb_build_object(
                    'segment_index', product_hvtec.segment_index,
                    'hvtec_index', product_hvtec.hvtec_index,
                    'nwslid', product_hvtec.nwslid,
                    'location_name', product_hvtec.location_name,
                    'severity', product_hvtec.severity,
                    'cause', product_hvtec.cause,
                    'record', product_hvtec.record,
                    'begin_utc', product_hvtec.begin_utc,
                    'crest_utc', product_hvtec.crest_utc,
                    'end_utc', product_hvtec.end_utc
                ) AS properties
            FROM product_hvtec
            WHERE product_hvtec.point_geom IS NOT NULL
            UNION ALL
            SELECT
                'search_point' AS feature_kind,
                5 AS feature_kind_order,
                product_search_points.id AS feature_row_id,
                product_search_points.product_id,
                product_search_points.point_geom AS feature_geom,
                ST_AsGeoJSON(product_search_points.point_geom)::json AS geometry,
                jsonb_build_object(
                    'source_kind', product_search_points.source_kind,
                    'source_index', product_search_points.source_index
                ) AS properties
            FROM product_search_points
        ) AS features
        INNER JOIN products ON products.id = features.product_id",
    )
}

fn geohash_alphabet_sql() -> &'static str {
    "(VALUES
        ('0'), ('1'), ('2'), ('3'), ('4'), ('5'), ('6'), ('7'),
        ('8'), ('9'), ('b'), ('c'), ('d'), ('e'), ('f'), ('g'),
        ('h'), ('j'), ('k'), ('m'), ('n'), ('p'), ('q'), ('r'),
        ('s'), ('t'), ('u'), ('v'), ('w'), ('x'), ('y'), ('z')
    )"
}

fn facet_aggregate_select_sql(dimension: FacetDimension) -> String {
    match dimension {
        FacetDimension::Office => String::from(
            "SELECT products.office_code AS value, COUNT(DISTINCT products.id) AS count
             FROM products",
        ),
        FacetDimension::Family => String::from(
            "SELECT products.family AS value, COUNT(DISTINCT products.id) AS count
             FROM products",
        ),
        FacetDimension::ArtifactKind => String::from(
            "SELECT products.artifact_kind AS value, COUNT(DISTINCT products.id) AS count
             FROM products",
        ),
        FacetDimension::Phenomena => String::from(
            "SELECT facet.phenomena AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_vtec AS facet ON facet.product_id = products.id",
        ),
        FacetDimension::Significance => String::from(
            "SELECT facet.significance AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_vtec AS facet ON facet.product_id = products.id",
        ),
        FacetDimension::Status => String::from(
            "SELECT facet.current_status AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_vtec ON product_vtec.product_id = products.id
             INNER JOIN incidents AS facet
               ON facet.office = product_vtec.office
              AND facet.phenomena = product_vtec.phenomena
              AND facet.significance = product_vtec.significance
              AND facet.etn = product_vtec.etn",
        ),
        FacetDimension::IssueKind => String::from(
            "SELECT facet.kind AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_issues AS facet ON facet.product_id = products.id",
        ),
        FacetDimension::IssueCode => String::from(
            "SELECT facet.code AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_issues AS facet ON facet.product_id = products.id",
        ),
    }
}

fn append_facet_non_null_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    dimension: FacetDimension,
) {
    match dimension {
        FacetDimension::Office => builder.push(" AND products.office_code IS NOT NULL"),
        FacetDimension::Family => builder.push(" AND products.family IS NOT NULL"),
        FacetDimension::ArtifactKind => builder.push(" AND products.artifact_kind IS NOT NULL"),
        FacetDimension::Phenomena => builder.push(" AND facet.phenomena IS NOT NULL"),
        FacetDimension::Significance => builder.push(" AND facet.significance IS NOT NULL"),
        FacetDimension::Status => builder.push(" AND facet.current_status IS NOT NULL"),
        FacetDimension::IssueKind => builder.push(" AND facet.kind IS NOT NULL"),
        FacetDimension::IssueCode => builder.push(" AND facet.code IS NOT NULL"),
    };
}

async fn fetch_issue_cursor_timestamp(pool: &PgPool, issue_id: i64) -> PersistResult<i64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT products.source_timestamp_utc
         FROM product_issues
         INNER JOIN products ON products.id = product_issues.product_id
         WHERE product_issues.id = $1",
    )
    .bind(issue_id)
    .fetch_one(pool)
    .await
    .map_err(PersistError::from)
}

fn encode_cursor<T: Serialize>(cursor: &T) -> PersistResult<String> {
    let bytes = serde_json::to_vec(cursor)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_optional_cursor<T>(cursor: Option<&str>) -> PersistResult<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    cursor.map(decode_cursor).transpose()
}

fn decode_cursor<T>(cursor: &str) -> PersistResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|err| PersistError::InvalidRequest(format!("invalid cursor: {err}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| PersistError::InvalidRequest(format!("invalid cursor payload: {err}")))
}

fn append_product_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    query: &ProductListQuery,
) -> PersistResult<()> {
    append_like_filter(builder, "products.filename", query.filename.as_deref());
    append_text_set_filter(
        builder,
        "products.source_receiver",
        query.source_receiver.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.source",
        query.source.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.pil",
        query.pil.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.family",
        query.family.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.artifact_kind",
        query.artifact_kind.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.container",
        query.container.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.wmo_prefix",
        query.wmo_prefix.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.office_code",
        query.office.as_deref(),
        normalize_upper,
    );
    append_case_insensitive_text_set_filter(
        builder,
        "products.office_city",
        query.office_city.as_deref(),
    );
    append_text_set_filter(
        builder,
        "products.office_state",
        query.office_state.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.bbb_kind",
        query.bbb_kind.as_deref(),
        normalize_lower,
    );
    append_text_set_filter(
        builder,
        "products.cccc",
        query.cccc.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.ttaaii",
        query.ttaaii.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.afos",
        query.afos.as_deref(),
        normalize_upper,
    );
    append_text_set_filter(
        builder,
        "products.bbb",
        query.bbb.as_deref(),
        normalize_upper,
    );
    append_bool_filter(builder, "products.has_issues", query.has_issues);
    append_bool_filter(builder, "products.has_vtec", query.has_vtec);
    append_bool_filter(builder, "products.has_ugc", query.has_ugc);
    append_bool_filter(builder, "products.has_hvtec", query.has_hvtec);
    append_bool_filter(builder, "products.has_latlon", query.has_latlon);
    append_bool_filter(builder, "products.has_time_mot_loc", query.has_time_mot_loc);
    append_bool_filter(builder, "products.has_wind_hail", query.has_wind_hail);

    if let Some(min_size) = query.min_size {
        builder
            .push(" AND products.size_bytes >= ")
            .push_bind(i64::try_from(min_size).expect("size should fit in i64"));
    }
    if let Some(max_size) = query.max_size {
        builder
            .push(" AND products.size_bytes <= ")
            .push_bind(i64::try_from(max_size).expect("size should fit in i64"));
    }
    if let Some(after) = query.source_timestamp_after {
        builder
            .push(" AND products.source_timestamp_utc >= ")
            .push_bind(after);
    }
    if let Some(before) = query.source_timestamp_before {
        builder
            .push(" AND products.source_timestamp_utc <= ")
            .push_bind(before);
    }
    if let Some(after) = query.ingested_after {
        builder
            .push(" AND products.ingested_at >= ")
            .push_bind(after);
    }
    if let Some(before) = query.ingested_before {
        builder
            .push(" AND products.ingested_at <= ")
            .push_bind(before);
    }

    if let Some(states) = split_csv_values(query.state.as_deref(), normalize_upper) {
        builder.push(" AND EXISTS (SELECT 1 FROM product_ugc_areas WHERE product_ugc_areas.product_id = products.id AND ");
        append_in_clause(builder, "product_ugc_areas.state", states);
        builder.push(")");
    }

    append_ugc_exists(builder, query.county.as_deref(), "county");
    append_ugc_exists(builder, query.zone.as_deref(), "zone");
    append_ugc_exists(builder, query.fire_zone.as_deref(), "fire_zone");
    append_ugc_exists(builder, query.marine_zone.as_deref(), "marine_zone");

    if query.issue_kind.is_some() || query.issue_code.is_some() {
        builder.push(
            " AND EXISTS (SELECT 1 FROM product_issues WHERE product_issues.product_id = products.id",
        );
        if let Some(kinds) = split_csv_values(query.issue_kind.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_issues.kind", kinds);
        }
        if let Some(codes) = split_csv_values(query.issue_code.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_issues.code", codes);
        }
        builder.push(")");
    }

    if query.vtec_phenomena.is_some()
        || query.vtec_significance.is_some()
        || query.vtec_action.is_some()
        || query.vtec_office.is_some()
        || query.etn.is_some()
    {
        builder.push(
            " AND EXISTS (SELECT 1 FROM product_vtec WHERE product_vtec.product_id = products.id",
        );
        if let Some(values) = split_csv_values(query.vtec_phenomena.as_deref(), normalize_upper) {
            builder.push(" AND ");
            append_in_clause(builder, "product_vtec.phenomena", values);
        }
        if let Some(values) = split_csv_values(query.vtec_significance.as_deref(), normalize_upper)
        {
            builder.push(" AND ");
            append_in_clause(builder, "product_vtec.significance", values);
        }
        if let Some(values) = split_csv_values(query.vtec_action.as_deref(), normalize_upper) {
            builder.push(" AND ");
            append_in_clause(builder, "product_vtec.action", values);
        }
        if let Some(values) = split_csv_values(query.vtec_office.as_deref(), normalize_upper) {
            builder.push(" AND ");
            append_in_clause(builder, "product_vtec.office", values);
        }
        if let Some(values) = split_csv_i64(query.etn.as_deref())? {
            builder.push(" AND ");
            append_in_clause_i64(builder, "product_vtec.etn", values);
        }
        builder.push(")");
    }

    if query.hvtec_nwslid.is_some()
        || query.hvtec_severity.is_some()
        || query.hvtec_cause.is_some()
        || query.hvtec_record.is_some()
    {
        builder.push(
            " AND EXISTS (SELECT 1 FROM product_hvtec WHERE product_hvtec.product_id = products.id",
        );
        if let Some(values) = split_csv_values(query.hvtec_nwslid.as_deref(), normalize_upper) {
            builder.push(" AND ");
            append_in_clause(builder, "product_hvtec.nwslid", values);
        }
        if let Some(values) = split_csv_values(query.hvtec_severity.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_hvtec.severity", values);
        }
        if let Some(values) = split_csv_values(query.hvtec_cause.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_hvtec.cause", values);
        }
        if let Some(values) = split_csv_values(query.hvtec_record.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_hvtec.record", values);
        }
        builder.push(")");
    }

    if query.wind_hail_kind.is_some()
        || query.min_wind_mph.is_some()
        || query.min_hail_inches.is_some()
    {
        builder.push(
            " AND EXISTS (SELECT 1 FROM product_wind_hail WHERE product_wind_hail.product_id = products.id",
        );
        if let Some(values) = split_csv_values(query.wind_hail_kind.as_deref(), normalize_lower) {
            builder.push(" AND ");
            append_in_clause(builder, "product_wind_hail.kind", values);
        }
        if let Some(min_wind_mph) = query.min_wind_mph {
            builder.push(
                " AND product_wind_hail.kind IN ('legacy_wind', 'max_wind_gust') AND CASE WHEN UPPER(COALESCE(product_wind_hail.units, '')) IN ('KTS', 'KT') THEN product_wind_hail.numeric_value * 1.15078 ELSE product_wind_hail.numeric_value END >= ",
            )
            .push_bind(min_wind_mph);
        }
        if let Some(min_hail_inches) = query.min_hail_inches {
            builder.push(
                " AND product_wind_hail.kind IN ('legacy_hail', 'max_hail_size') AND product_wind_hail.numeric_value >= ",
            )
            .push_bind(min_hail_inches);
        }
        builder.push(")");
    }

    append_spatial_filter(builder, query)?;
    Ok(())
}

fn append_issue_alias_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    alias: &str,
    query: &ProductListQuery,
) {
    if let Some(kinds) = split_csv_values(query.issue_kind.as_deref(), normalize_lower) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.kind"), kinds);
    }
    if let Some(codes) = split_csv_values(query.issue_code.as_deref(), normalize_lower) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.code"), codes);
    }
}

fn append_issue_alias_join_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    alias: &str,
    query: &ProductListQuery,
) {
    if let Some(kinds) = split_csv_values(query.issue_kind.as_deref(), normalize_lower) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.kind"), kinds);
    }
    if let Some(codes) = split_csv_values(query.issue_code.as_deref(), normalize_lower) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.code"), codes);
    }
}

fn append_vtec_alias_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    alias: &str,
    query: &ProductListQuery,
) -> PersistResult<()> {
    if let Some(values) = split_csv_values(query.vtec_phenomena.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.phenomena"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_significance.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.significance"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_action.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.action"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_office.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.office"), values);
    }
    if let Some(values) = split_csv_i64(query.etn.as_deref())? {
        builder.push(" AND ");
        append_in_clause_i64(builder, &format!("{alias}.etn"), values);
    }
    Ok(())
}

fn append_vtec_alias_join_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    alias: &str,
    query: &ProductListQuery,
) -> PersistResult<()> {
    if let Some(values) = split_csv_values(query.vtec_phenomena.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.phenomena"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_significance.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.significance"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_action.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.action"), values);
    }
    if let Some(values) = split_csv_values(query.vtec_office.as_deref(), normalize_upper) {
        builder.push(" AND ");
        append_in_clause(builder, &format!("{alias}.office"), values);
    }
    if let Some(values) = split_csv_i64(query.etn.as_deref())? {
        builder.push(" AND ");
        append_in_clause_i64(builder, &format!("{alias}.etn"), values);
    }
    Ok(())
}

fn append_spatial_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    query: &ProductListQuery,
) -> PersistResult<()> {
    match (query.min_lat, query.max_lat, query.min_lon, query.max_lon) {
        (None, None, None, None) => {}
        (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) => {
            validate_lat("min_lat", min_lat)?;
            validate_lat("max_lat", max_lat)?;
            validate_lon("min_lon", min_lon)?;
            validate_lon("max_lon", max_lon)?;
            if min_lat > max_lat {
                return Err(PersistError::InvalidRequest(
                    "min_lat must be less than or equal to max_lat".to_string(),
                ));
            }
            if min_lon > max_lon {
                return Err(PersistError::InvalidRequest(
                    "min_lon must be less than or equal to max_lon".to_string(),
                ));
            }
            builder.push(" AND (");
            builder.push(
                "EXISTS (SELECT 1 FROM product_polygons WHERE product_polygons.product_id = products.id AND ST_Intersects(product_polygons.polygon_geom, ST_MakeEnvelope(",
            );
            builder
                .push_bind(min_lon)
                .push(", ")
                .push_bind(min_lat)
                .push(", ")
                .push_bind(max_lon)
                .push(", ")
                .push_bind(max_lat)
                .push(", 4326)))");
            builder.push(
                " OR EXISTS (SELECT 1 FROM product_time_mot_loc WHERE product_time_mot_loc.product_id = products.id AND ST_Intersects(product_time_mot_loc.path_geom, ST_MakeEnvelope(",
            );
            builder
                .push_bind(min_lon)
                .push(", ")
                .push_bind(min_lat)
                .push(", ")
                .push_bind(max_lon)
                .push(", ")
                .push_bind(max_lat)
                .push(", 4326)))");
            for table in [
                "product_ugc_areas",
                "product_hvtec",
                "product_search_points",
            ] {
                builder
                    .push(" OR EXISTS (SELECT 1 FROM ")
                    .push(table)
                    .push(" WHERE ")
                    .push(table)
                    .push(".product_id = products.id AND ")
                    .push(table)
                    .push(".point_geom IS NOT NULL AND ST_Covers(ST_MakeEnvelope(")
                    .push_bind(min_lon)
                    .push(", ")
                    .push_bind(min_lat)
                    .push(", ")
                    .push_bind(max_lon)
                    .push(", ")
                    .push_bind(max_lat)
                    .push(", 4326), ")
                    .push(table)
                    .push(".point_geom))");
            }
            builder.push(")");
        }
        _ => {
            return Err(PersistError::InvalidRequest(
                "min_lat, max_lat, min_lon, and max_lon must be provided together".to_string(),
            ));
        }
    }

    match (query.lat, query.lon) {
        (Some(lat), Some(lon)) => {
            validate_lat("lat", lat)?;
            validate_lon("lon", lon)?;
            let distance_meters = query.distance_miles.unwrap_or(5.0) * 1_609.344;
            builder.push(" AND (");
            builder.push(
                "EXISTS (SELECT 1 FROM product_polygons WHERE product_polygons.product_id = products.id AND ST_Covers(product_polygons.polygon_geom, ST_SetSRID(ST_MakePoint(",
            );
            builder
                .push_bind(lon)
                .push(", ")
                .push_bind(lat)
                .push("), 4326))");
            builder.push(" OR EXISTS (SELECT 1 FROM product_time_mot_loc WHERE product_time_mot_loc.product_id = products.id AND ST_DWithin(product_time_mot_loc.path_geom::geography, ST_SetSRID(ST_MakePoint(");
            builder
                .push_bind(lon)
                .push(", ")
                .push_bind(lat)
                .push("), 4326)::geography, ")
                .push_bind(distance_meters)
                .push("))");
            for table in [
                "product_ugc_areas",
                "product_hvtec",
                "product_search_points",
            ] {
                builder
                    .push(" OR EXISTS (SELECT 1 FROM ")
                    .push(table)
                    .push(" WHERE ")
                    .push(table)
                    .push(".product_id = products.id AND ")
                    .push(table)
                    .push(".point_geom IS NOT NULL AND ST_DWithin(")
                    .push(table)
                    .push(".point_geom::geography, ST_SetSRID(ST_MakePoint(")
                    .push_bind(lon)
                    .push(", ")
                    .push_bind(lat)
                    .push("), 4326)::geography, ")
                    .push_bind(distance_meters)
                    .push("))");
            }
            builder.push(")");
        }
        (None, None) => {}
        _ => {
            return Err(PersistError::InvalidRequest(
                "lat and lon must be provided together".to_string(),
            ));
        }
    }
    if query.distance_miles.is_some() && (query.lat.is_none() || query.lon.is_none()) {
        return Err(PersistError::InvalidRequest(
            "distance_miles requires both lat and lon".to_string(),
        ));
    }
    if let Some(distance_miles) = query.distance_miles
        && (!distance_miles.is_finite() || distance_miles <= 0.0)
    {
        return Err(PersistError::InvalidRequest(
            "distance_miles must be a finite value greater than 0".to_string(),
        ));
    }
    Ok(())
}

fn append_feature_spatial_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    query: &ProductListQuery,
) -> PersistResult<()> {
    match (query.min_lat, query.max_lat, query.min_lon, query.max_lon) {
        (None, None, None, None) => {}
        (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) => {
            validate_lat("min_lat", min_lat)?;
            validate_lat("max_lat", max_lat)?;
            validate_lon("min_lon", min_lon)?;
            validate_lon("max_lon", max_lon)?;
            if min_lat > max_lat {
                return Err(PersistError::InvalidRequest(
                    "min_lat must be less than or equal to max_lat".to_string(),
                ));
            }
            if min_lon > max_lon {
                return Err(PersistError::InvalidRequest(
                    "min_lon must be less than or equal to max_lon".to_string(),
                ));
            }
            builder
                .push(" AND ST_Intersects(features.feature_geom, ST_MakeEnvelope(")
                .push_bind(min_lon)
                .push(", ")
                .push_bind(min_lat)
                .push(", ")
                .push_bind(max_lon)
                .push(", ")
                .push_bind(max_lat)
                .push(", 4326))");
        }
        _ => {
            return Err(PersistError::InvalidRequest(
                "min_lat, max_lat, min_lon, and max_lon must be provided together".to_string(),
            ));
        }
    }

    match (query.lat, query.lon) {
        (Some(lat), Some(lon)) => {
            validate_lat("lat", lat)?;
            validate_lon("lon", lon)?;
            let distance_meters = query.distance_miles.unwrap_or(5.0) * 1_609.344;
            builder.push(
                " AND ((features.feature_kind = 'polygon' AND ST_Covers(features.feature_geom, ST_SetSRID(ST_MakePoint(",
            );
            builder
                .push_bind(lon)
                .push(", ")
                .push_bind(lat)
                .push("), 4326))) OR (features.feature_kind <> 'polygon' AND ST_DWithin(features.feature_geom::geography, ST_SetSRID(ST_MakePoint(")
                .push_bind(lon)
                .push(", ")
                .push_bind(lat)
                .push("), 4326)::geography, ")
                .push_bind(distance_meters)
                .push(")))");
        }
        (None, None) => {}
        _ => {
            return Err(PersistError::InvalidRequest(
                "lat and lon must be provided together".to_string(),
            ));
        }
    }
    if query.distance_miles.is_some() && (query.lat.is_none() || query.lon.is_none()) {
        return Err(PersistError::InvalidRequest(
            "distance_miles requires both lat and lon".to_string(),
        ));
    }
    if let Some(distance_miles) = query.distance_miles
        && (!distance_miles.is_finite() || distance_miles <= 0.0)
    {
        return Err(PersistError::InvalidRequest(
            "distance_miles must be a finite value greater than 0".to_string(),
        ));
    }
    Ok(())
}

fn validate_lat(name: &str, value: f64) -> PersistResult<()> {
    if !value.is_finite() || !(-90.0..=90.0).contains(&value) {
        return Err(PersistError::InvalidRequest(format!(
            "{name} must be a finite value between -90 and 90"
        )));
    }
    Ok(())
}

fn validate_lon(name: &str, value: f64) -> PersistResult<()> {
    if !value.is_finite() || !(-180.0..=180.0).contains(&value) {
        return Err(PersistError::InvalidRequest(format!(
            "{name} must be a finite value between -180 and 180"
        )));
    }
    Ok(())
}

fn append_ugc_exists(
    builder: &mut QueryBuilder<'_, Postgres>,
    raw_values: Option<&str>,
    normalized_kind: &'static str,
) {
    let Some(values) = split_csv_values(raw_values, normalize_upper) else {
        return;
    };
    builder.push(
        " AND EXISTS (SELECT 1 FROM product_ugc_areas WHERE product_ugc_areas.product_id = products.id AND product_ugc_areas.area_kind = ",
    );
    builder.push_bind(normalized_kind);
    builder.push(" AND ");
    append_in_clause(builder, "product_ugc_areas.ugc_code", values);
    builder.push(")");
}

fn append_like_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    column: &str,
    raw_value: Option<&str>,
) {
    let Some(raw_value) = raw_value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let pattern = raw_value.replace('*', "%");
    builder
        .push(" AND ")
        .push(column)
        .push(" ILIKE ")
        .push_bind(pattern);
}

fn append_bool_filter(builder: &mut QueryBuilder<'_, Postgres>, column: &str, value: Option<bool>) {
    if let Some(value) = value {
        builder
            .push(" AND ")
            .push(column)
            .push(" = ")
            .push_bind(value);
    }
}

fn append_text_set_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    column: &str,
    raw_values: Option<&str>,
    normalize: fn(&str) -> String,
) {
    let Some(values) = split_csv_values(raw_values, normalize) else {
        return;
    };
    builder.push(" AND ");
    append_in_clause(builder, column, values);
}

fn append_case_insensitive_text_set_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    column: &str,
    raw_values: Option<&str>,
) {
    let Some(values) = split_csv_values(raw_values, normalize_lower) else {
        return;
    };
    builder.push(" AND LOWER(").push(column).push(") IN (");
    let mut separated = builder.separated(", ");
    for value in values {
        separated.push_bind(value);
    }
    separated.push_unseparated(")");
}

fn append_in_clause(builder: &mut QueryBuilder<'_, Postgres>, column: &str, values: Vec<String>) {
    builder.push(column).push(" IN (");
    let mut separated = builder.separated(", ");
    for value in values {
        separated.push_bind(value);
    }
    separated.push_unseparated(")");
}

fn append_in_clause_i64(builder: &mut QueryBuilder<'_, Postgres>, column: &str, values: Vec<i64>) {
    builder.push(column).push(" IN (");
    let mut separated = builder.separated(", ");
    for value in values {
        separated.push_bind(value);
    }
    separated.push_unseparated(")");
}

fn split_csv_values(
    raw_values: Option<&str>,
    normalize: fn(&str) -> String,
) -> Option<Vec<String>> {
    let values = raw_values
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize)
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

fn split_csv_i64(raw_values: Option<&str>) -> PersistResult<Option<Vec<i64>>> {
    let mut values = Vec::new();
    for raw_value in raw_values.into_iter().flat_map(|value| value.split(',')) {
        let raw_value = raw_value.trim();
        if raw_value.is_empty() {
            continue;
        }
        let value = raw_value.parse::<i64>().map_err(|err| {
            PersistError::InvalidRequest(format!("invalid etn value `{raw_value}`: {err}"))
        })?;
        values.push(value);
    }
    Ok((!values.is_empty()).then_some(values))
}

fn normalize_upper(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn normalize_lower(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn archived_feature_from_row(row: PgRow) -> PersistResult<ArchivedFeature> {
    let kind_raw = row.get::<String, _>("feature_kind");
    let feature_kind = kind_raw
        .parse::<FeatureKind>()
        .map_err(PersistError::InvalidRequest)?;
    let feature_row_id = row.get::<i64, _>("feature_row_id");
    let geometry = row.get::<SqlxJson<Value>, _>("geometry").0;
    let properties = row.get::<SqlxJson<Value>, _>("properties").0;

    Ok(ArchivedFeature {
        feature_id: format!("{}:{feature_row_id}", feature_kind.as_str()),
        feature_kind,
        product_id: row.get("product_id"),
        source_timestamp_utc: row.get("source_timestamp_utc"),
        geometry,
        properties,
    })
}

pub(super) fn parse_blob_storage_kind(storage_kind: &str) -> PersistResult<BlobStorageKind> {
    match storage_kind {
        "filesystem" => Ok(BlobStorageKind::Filesystem),
        "s3" => Ok(BlobStorageKind::S3),
        other => Err(PersistError::InvalidRequest(format!(
            "unsupported payload storage kind `{other}`"
        ))),
    }
}

pub(super) fn archived_product_detail_from_row(row: PgRow) -> ArchivedProductDetail {
    ArchivedProductDetail {
        summary: ArchivedProductSummary {
            product_id: row.get("product_id"),
            filename: row.get("filename"),
            source_timestamp_utc: row.get("source_timestamp_utc"),
            ingested_at: row.get("ingested_at"),
            source_receiver: row.get("source_receiver"),
            source_message_id: row.get("source_message_id"),
            size_bytes: row.get("size_bytes"),
            payload_storage_kind: row.get("payload_storage_kind"),
            has_metadata_sidecar: row.get("has_metadata_sidecar"),
            source: row.get("source"),
            family: row.get("family"),
            artifact_kind: row.get("artifact_kind"),
            title: row.get("title"),
            container: row.get("container"),
            pil: row.get("pil"),
            wmo_prefix: row.get("wmo_prefix"),
            bbb_kind: row.get("bbb_kind"),
            office_code: row.get("office_code"),
            office_city: row.get("office_city"),
            office_state: row.get("office_state"),
            header_kind: row.get("header_kind"),
            ttaaii: row.get("ttaaii"),
            cccc: row.get("cccc"),
            ddhhmm: row.get("ddhhmm"),
            bbb: row.get("bbb"),
            afos: row.get("afos"),
            has_body: row.get("has_body"),
            has_artifact: row.get("has_artifact"),
            has_issues: row.get("has_issues"),
            has_vtec: row.get("has_vtec"),
            has_ugc: row.get("has_ugc"),
            has_hvtec: row.get("has_hvtec"),
            has_latlon: row.get("has_latlon"),
            has_time_mot_loc: row.get("has_time_mot_loc"),
            has_wind_hail: row.get("has_wind_hail"),
            vtec_count: row.get("vtec_count"),
            ugc_count: row.get("ugc_count"),
            hvtec_count: row.get("hvtec_count"),
            latlon_count: row.get("latlon_count"),
            time_mot_loc_count: row.get("time_mot_loc_count"),
            wind_hail_count: row.get("wind_hail_count"),
            issue_count: row.get("issue_count"),
        },
        payload_location: row.get("payload_location"),
        metadata_location: row.get("metadata_location"),
        product_json: row.get("product_json"),
    }
}
