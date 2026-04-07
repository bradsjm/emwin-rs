//! Feature archive queries and feature row mapping.

use super::super::filters::append_product_filters;
use super::super::spatial::append_feature_spatial_filter;
use super::super::{
    archived_feature_select_sql, decode_optional_cursor, encode_cursor, normalize_page_limit,
};
use crate::error::{PersistError, PersistResult};
use emwin_service::{
    ArchivedFeature, FeatureCursor, FeatureKind, FeatureListQuery, PaginatedResponse,
};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::types::Json as SqlxJson;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

pub(crate) async fn list_archived_features_query(
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
