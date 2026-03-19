use super::prepare::PendingIncidentChange;
use super::{
    ArchivedProductDetail, ArchivedProductSummary, IncidentChange, IncidentChangeTrigger,
    IncidentCursor, IncidentKey, IncidentListQuery, IncidentProductsCursor, IncidentProductsQuery,
    IncidentSummary, PaginatedResponse, PersistError, PersistResult,
};
use crate::writer::BlobStorageKind;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgRow;
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
