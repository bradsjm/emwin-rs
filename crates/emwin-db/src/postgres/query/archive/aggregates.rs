//! Aggregate archive queries for facets, timeseries, and cells.

use super::super::filters::{
    append_facet_non_null_filter, append_issue_alias_filters, append_product_filters,
    append_vtec_alias_filters,
};
use super::super::mappers::{
    cell_aggregate_bucket_from_row, facet_aggregate_bucket_from_row,
    timeseries_aggregate_bucket_from_row,
};
use super::super::spatial::append_feature_spatial_filter;
use super::super::sql::{
    archived_feature_source_sql, facet_aggregate_select_sql, geohash_alphabet_sql,
};
use crate::error::{PersistError, PersistResult};
use emwin_service::{
    AggregateCompleteness, CellAggregateQuery, CellAggregateResult, CellMeasure,
    FacetAggregateQuery, FacetAggregateResult, FacetDimension, TimeseriesAggregateQuery,
    TimeseriesAggregateResult, TimeseriesMeasure,
};
use sqlx::{PgPool, Postgres, QueryBuilder};

pub(crate) async fn list_facet_aggregate_query(
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
        .build()
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| facet_aggregate_bucket_from_row(&row))
                .collect::<Vec<_>>()
        })
        .map_err(PersistError::from)?;
    Ok(FacetAggregateResult {
        completeness: AggregateCompleteness::exact(),
        items,
    })
}

pub(crate) async fn list_timeseries_aggregate_query(
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
            append_issue_alias_filters(&mut builder, "product_issues", &query.filters);
        }
        TimeseriesMeasure::IncidentCount => {
            builder.push("COUNT(DISTINCT (product_vtec.office, product_vtec.phenomena, product_vtec.significance, product_vtec.etn)) AS count FROM buckets LEFT JOIN matching_products ON matching_products.source_time >= buckets.bucket_start AND matching_products.source_time < buckets.bucket_end LEFT JOIN product_vtec ON product_vtec.product_id = matching_products.id");
            append_vtec_alias_filters(&mut builder, "product_vtec", &query.filters)?;
        }
    }
    builder.push(
        " GROUP BY buckets.bucket_start, buckets.bucket_end ORDER BY buckets.bucket_start ASC",
    );

    let items = builder
        .build()
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| timeseries_aggregate_bucket_from_row(&row))
                .collect::<Vec<_>>()
        })
        .map_err(PersistError::from)?;
    Ok(TimeseriesAggregateResult {
        completeness: AggregateCompleteness::exact(),
        items,
    })
}

pub(crate) async fn list_cell_aggregate_query(
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
        .build()
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| cell_aggregate_bucket_from_row(&row))
                .collect::<Vec<_>>()
        })
        .map_err(PersistError::from)?;
    Ok(CellAggregateResult {
        completeness: AggregateCompleteness::exact(),
        items,
    })
}
