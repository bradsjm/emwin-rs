//! Product archive queries and row mappers.

use super::super::filters::append_product_filters;
use super::super::sql::archived_product_summary_select_sql;
use super::super::{decode_optional_cursor, encode_cursor, normalize_page_limit};
use crate::error::{PersistError, PersistResult};
use crate::writer::BlobStorageKind;
use emwin_service::{
    ArchivedProductDetail, ArchivedProductSummary, PaginatedResponse, ProductCursor,
    ProductListQuery,
};
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

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

pub(crate) fn parse_blob_storage_kind(storage_kind: &str) -> PersistResult<BlobStorageKind> {
    match storage_kind {
        "filesystem" => Ok(BlobStorageKind::Filesystem),
        "s3" => Ok(BlobStorageKind::S3),
        other => Err(PersistError::InvalidRequest(format!(
            "unsupported payload storage kind `{other}`"
        ))),
    }
}

pub(crate) fn archived_product_detail_from_row(row: PgRow) -> ArchivedProductDetail {
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
