//! Incident query entrypoints and incident-change loading helpers.

use super::super::prepare::PendingIncidentChange;
use super::{
    IncidentChange, IncidentChangeTrigger, IncidentCursor, IncidentKey, IncidentListQuery,
    IncidentProductsCursor, IncidentProductsQuery, IncidentSummary, PaginatedResponse,
    PersistError, PersistResult, archived_product_summary_select_sql, decode_optional_cursor,
    encode_cursor, incident_select_sql, normalize_page_limit,
};
use emwin_service::ArchivedProductSummary;
use sqlx::{PgPool, Postgres, QueryBuilder};

pub(crate) async fn load_incident_changes(
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

pub(crate) async fn list_incidents_query(
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

pub(crate) async fn list_incident_products_query(
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
