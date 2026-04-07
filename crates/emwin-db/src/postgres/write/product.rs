use super::PreparedProduct;
use super::{PersistResult, Postgres};
use sqlx::Row;
use sqlx::Transaction;

pub(super) async fn upsert_product(
    tx: &mut Transaction<'_, Postgres>,
    prepared: &PreparedProduct,
) -> PersistResult<i64> {
    let row = &prepared.row;
    let product_id = sqlx::query(
        "INSERT INTO products (
            filename,
            source_timestamp_utc,
            source_receiver,
            source_message_id,
            size_bytes,
            payload_storage_kind,
            payload_location,
            metadata_storage_kind,
            metadata_location,
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
            issue_count,
            states,
            ugc_codes,
            product_json
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
            $11, $12, $13, $14, $15, $16, $17, $18, $19, $20,
            $21, $22, $23, $24, $25, $26, $27, $28, $29, $30,
            $31, $32, $33, $34, $35, $36, $37, $38, $39, $40,
            $41, $42, $43, $44, $45
        ) ON CONFLICT (filename, source_timestamp_utc) DO UPDATE SET
            source_receiver = EXCLUDED.source_receiver,
            source_message_id = EXCLUDED.source_message_id,
            ingested_at = now(),
            size_bytes = EXCLUDED.size_bytes,
            payload_storage_kind = EXCLUDED.payload_storage_kind,
            payload_location = EXCLUDED.payload_location,
            metadata_storage_kind = EXCLUDED.metadata_storage_kind,
            metadata_location = EXCLUDED.metadata_location,
            source = EXCLUDED.source,
            family = EXCLUDED.family,
            artifact_kind = EXCLUDED.artifact_kind,
            title = EXCLUDED.title,
            container = EXCLUDED.container,
            pil = EXCLUDED.pil,
            wmo_prefix = EXCLUDED.wmo_prefix,
            bbb_kind = EXCLUDED.bbb_kind,
            office_code = EXCLUDED.office_code,
            office_city = EXCLUDED.office_city,
            office_state = EXCLUDED.office_state,
            header_kind = EXCLUDED.header_kind,
            ttaaii = EXCLUDED.ttaaii,
            cccc = EXCLUDED.cccc,
            ddhhmm = EXCLUDED.ddhhmm,
            bbb = EXCLUDED.bbb,
            afos = EXCLUDED.afos,
            has_body = EXCLUDED.has_body,
            has_artifact = EXCLUDED.has_artifact,
            has_issues = EXCLUDED.has_issues,
            has_vtec = EXCLUDED.has_vtec,
            has_ugc = EXCLUDED.has_ugc,
            has_hvtec = EXCLUDED.has_hvtec,
            has_latlon = EXCLUDED.has_latlon,
            has_time_mot_loc = EXCLUDED.has_time_mot_loc,
            has_wind_hail = EXCLUDED.has_wind_hail,
            vtec_count = EXCLUDED.vtec_count,
            ugc_count = EXCLUDED.ugc_count,
            hvtec_count = EXCLUDED.hvtec_count,
            latlon_count = EXCLUDED.latlon_count,
            time_mot_loc_count = EXCLUDED.time_mot_loc_count,
            wind_hail_count = EXCLUDED.wind_hail_count,
            issue_count = EXCLUDED.issue_count,
            states = EXCLUDED.states,
            ugc_codes = EXCLUDED.ugc_codes,
            product_json = EXCLUDED.product_json
        RETURNING id",
    )
    .bind(&row.filename)
    .bind(row.source_timestamp_utc)
    .bind(&row.source_receiver)
    .bind(&row.source_message_id)
    .bind(row.size_bytes)
    .bind(&row.payload_storage_kind)
    .bind(&row.payload_location)
    .bind(&row.metadata_storage_kind)
    .bind(&row.metadata_location)
    .bind(&row.source)
    .bind(&row.family)
    .bind(&row.artifact_kind)
    .bind(&row.title)
    .bind(&row.container)
    .bind(&row.pil)
    .bind(&row.wmo_prefix)
    .bind(&row.bbb_kind)
    .bind(&row.office_code)
    .bind(&row.office_city)
    .bind(&row.office_state)
    .bind(&row.header_kind)
    .bind(&row.ttaaii)
    .bind(&row.cccc)
    .bind(&row.ddhhmm)
    .bind(&row.bbb)
    .bind(&row.afos)
    .bind(row.has_body)
    .bind(row.has_artifact)
    .bind(row.has_issues)
    .bind(row.has_vtec)
    .bind(row.has_ugc)
    .bind(row.has_hvtec)
    .bind(row.has_latlon)
    .bind(row.has_time_mot_loc)
    .bind(row.has_wind_hail)
    .bind(row.vtec_count)
    .bind(row.ugc_count)
    .bind(row.hvtec_count)
    .bind(row.latlon_count)
    .bind(row.time_mot_loc_count)
    .bind(row.wind_hail_count)
    .bind(row.issue_count)
    .bind(&row.states)
    .bind(&row.ugc_codes)
    .bind(&row.product_json)
    .fetch_one(&mut **tx)
    .await?
    .get::<i64, _>("id");

    Ok(product_id)
}
