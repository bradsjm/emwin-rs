//! Product archive queries and row mappers.

use super::super::filters::append_product_filters;
use super::super::mappers::archived_product_summary_from_row;
use super::super::sql::archived_product_summary_select_sql;
use super::super::{decode_optional_cursor, encode_cursor, normalize_page_limit};
use crate::error::PersistResult;
use emwin_service::{ArchivedProductSummary, PaginatedResponse, ProductCursor, ProductListQuery};
use sqlx::{PgPool, Postgres, QueryBuilder};

pub(crate) async fn list_archived_products_query(
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
        .build()
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| archived_product_summary_from_row(&row))
        .collect::<Vec<_>>();

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
