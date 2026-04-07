//! Issue archive queries and cursor helpers.

use super::super::{
    archived_issue_select_sql, decode_optional_cursor, encode_cursor, normalize_page_limit,
};
use crate::error::{PersistError, PersistResult};
use emwin_service::{
    ArchivedIssue, ArchivedIssueCursor, ArchivedIssueListQuery, PaginatedResponse,
};
use sqlx::{PgPool, Postgres, QueryBuilder};

pub(crate) async fn list_archived_issues_query(
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

pub(crate) async fn get_archived_issue_query(
    pool: &PgPool,
    issue_id: i64,
) -> PersistResult<Option<ArchivedIssue>> {
    let mut builder = QueryBuilder::<Postgres>::new(archived_issue_select_sql());
    builder
        .push(" WHERE product_issues.id = ")
        .push_bind(issue_id)
        .push(
            " ORDER BY products.source_timestamp_utc DESC, product_issues.product_id DESC, product_issues.id DESC",
        );
    builder
        .build_query_as::<ArchivedIssue>()
        .fetch_optional(pool)
        .await
        .map_err(PersistError::from)
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
