use super::prepare::{
    PendingIncidentChange, PreparedProduct, ProductHvtecRow, ProductIssueRow, ProductPolygonRow,
    ProductSearchPointRow, ProductTimeMotLocRow, ProductUgcAreaRow, ProductVtecRow,
    ProductWindHailRow,
};
use super::{IncidentChangeAction, PersistResult, Postgres, QueryBuilder};
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

pub(super) async fn replace_children(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    prepared: &PreparedProduct,
) -> PersistResult<Vec<PendingIncidentChange>> {
    for table in [
        "product_issues",
        "product_vtec",
        "product_ugc_areas",
        "product_hvtec",
        "product_time_mot_loc",
        "product_polygons",
        "product_wind_hail",
        "product_search_points",
    ] {
        let query = format!("DELETE FROM {table} WHERE product_id = $1");
        sqlx::query(&query)
            .bind(product_id)
            .execute(&mut **tx)
            .await?;
    }

    insert_product_issues(tx, product_id, &prepared.issues).await?;
    insert_product_vtec(tx, product_id, &prepared.vtec).await?;
    let incident_changes = upsert_incidents(tx, product_id, &prepared.incident_updates).await?;
    insert_product_ugc_areas(tx, product_id, &prepared.ugc_areas).await?;
    insert_product_hvtec(tx, product_id, &prepared.hvtec).await?;
    insert_product_time_mot_loc(tx, product_id, &prepared.time_mot_loc).await?;
    insert_product_polygons(tx, product_id, &prepared.polygons).await?;
    insert_product_wind_hail(tx, product_id, &prepared.wind_hail).await?;
    insert_product_search_points(tx, product_id, &prepared.search_points).await?;
    Ok(incident_changes)
}

async fn upsert_incidents(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[super::prepare::PreparedIncidentUpdate],
) -> PersistResult<Vec<PendingIncidentChange>> {
    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        let existed = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1
                FROM incidents
                WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4
            )",
        )
        .bind(&row.key.office)
        .bind(&row.key.phenomena)
        .bind(&row.key.significance)
        .bind(row.key.etn)
        .fetch_one(&mut **tx)
        .await?;

        let done = sqlx::query(
            "WITH existing AS (
                SELECT current_status
                FROM incidents
                WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4
            )
            INSERT INTO incidents (
                office,
                phenomena,
                significance,
                etn,
                current_status,
                latest_vtec_action,
                issued_at,
                start_utc,
                end_utc,
                first_product_id,
                latest_product_id,
                latest_product_timestamp_utc
            )
            SELECT
                $1,
                $2,
                $3,
                $4,
                COALESCE($5, existing.current_status),
                $6,
                $7,
                $8,
                $9,
                $10,
                $11,
                $12
            FROM (SELECT 1) seed
            LEFT JOIN existing ON TRUE
            WHERE $5 IS NOT NULL OR existing.current_status IS NOT NULL
            ON CONFLICT (office, phenomena, significance, etn) DO UPDATE SET
                current_status = COALESCE(EXCLUDED.current_status, incidents.current_status),
                latest_vtec_action = EXCLUDED.latest_vtec_action,
                issued_at = EXCLUDED.issued_at,
                start_utc = EXCLUDED.start_utc,
                end_utc = EXCLUDED.end_utc,
                last_updated_at = now(),
                first_product_id = incidents.first_product_id,
                latest_product_id = EXCLUDED.latest_product_id,
                latest_product_timestamp_utc = EXCLUDED.latest_product_timestamp_utc
            WHERE EXCLUDED.latest_product_timestamp_utc >= incidents.latest_product_timestamp_utc",
        )
        .bind(&row.key.office)
        .bind(&row.key.phenomena)
        .bind(&row.key.significance)
        .bind(row.key.etn)
        .bind(&row.current_status)
        .bind(&row.latest_vtec_action)
        .bind(row.issued_at)
        .bind(row.start_utc)
        .bind(row.end_utc)
        .bind(product_id)
        .bind(product_id)
        .bind(row.latest_product_timestamp_utc)
        .execute(&mut **tx)
        .await?;

        if done.rows_affected() > 0 {
            changes.push(PendingIncidentChange {
                key: row.key.clone(),
                action: if existed {
                    IncidentChangeAction::Updated
                } else {
                    IncidentChangeAction::Created
                },
            });
        }
    }

    Ok(changes)
}

async fn insert_product_issues(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[ProductIssueRow],
) -> PersistResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO product_issues (product_id, kind, code, message, line) ",
    );
    builder.push_values(rows, |mut row, issue| {
        row.push_bind(product_id)
            .push_bind(&issue.kind)
            .push_bind(&issue.code)
            .push_bind(&issue.message)
            .push_bind(&issue.line);
    });
    builder.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_product_vtec(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[ProductVtecRow],
) -> PersistResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO product_vtec (product_id, segment_index, status, action, office, phenomena, significance, etn, begin_utc, end_utc) ",
    );
    builder.push_values(rows, |mut row, vtec| {
        row.push_bind(product_id)
            .push_bind(vtec.segment_index)
            .push_bind(&vtec.status)
            .push_bind(&vtec.action)
            .push_bind(&vtec.office)
            .push_bind(&vtec.phenomena)
            .push_bind(&vtec.significance)
            .push_bind(vtec.etn)
            .push_bind(vtec.begin_utc)
            .push_bind(vtec.end_utc);
    });
    builder.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_product_ugc_areas(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[ProductUgcAreaRow],
) -> PersistResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO product_ugc_areas (product_id, segment_index, section_index, area_kind, state, ugc_code, name, expires_utc, latitude, longitude, point_geom) ",
    );
    builder.push_values(rows, |mut row, area| {
        row.push_bind(product_id)
            .push_bind(area.segment_index)
            .push_bind(area.section_index)
            .push_bind(&area.area_kind)
            .push_bind(&area.state)
            .push_bind(&area.ugc_code)
            .push_bind(&area.name)
            .push_bind(area.expires_utc)
            .push_bind(area.latitude)
            .push_bind(area.longitude)
            .push(if area.latitude.is_some() && area.longitude.is_some() {
                "ST_SetSRID(ST_MakePoint("
            } else {
                "NULL"
            });
        if let (Some(latitude), Some(longitude)) = (area.latitude, area.longitude) {
            row.push_bind_unseparated(longitude)
                .push_unseparated(", ")
                .push_bind_unseparated(latitude)
                .push_unseparated("), 4326)");
        }
    });
    builder.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_product_hvtec(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[ProductHvtecRow],
) -> PersistResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO product_hvtec (product_id, segment_index, hvtec_index, nwslid, location_name, severity, cause, record, begin_utc, crest_utc, end_utc, latitude, longitude, point_geom) ",
    );
    builder.push_values(rows, |mut row, hvtec| {
        row.push_bind(product_id)
            .push_bind(hvtec.segment_index)
            .push_bind(hvtec.hvtec_index)
            .push_bind(&hvtec.nwslid)
            .push_bind(&hvtec.location_name)
            .push_bind(&hvtec.severity)
            .push_bind(&hvtec.cause)
            .push_bind(&hvtec.record)
            .push_bind(hvtec.begin_utc)
            .push_bind(hvtec.crest_utc)
            .push_bind(hvtec.end_utc)
            .push_bind(hvtec.latitude)
            .push_bind(hvtec.longitude)
            .push(if hvtec.latitude.is_some() && hvtec.longitude.is_some() {
                "ST_SetSRID(ST_MakePoint("
            } else {
                "NULL"
            });
        if let (Some(latitude), Some(longitude)) = (hvtec.latitude, hvtec.longitude) {
            row.push_bind_unseparated(longitude)
                .push_unseparated(", ")
                .push_bind_unseparated(latitude)
                .push_unseparated("), 4326)");
        }
    });
    builder.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_product_time_mot_loc(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[ProductTimeMotLocRow],
) -> PersistResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO product_time_mot_loc (product_id, segment_index, entry_index, time_utc, direction_degrees, speed_kt, path_wkt, path_geom) ",
    );
    builder.push_values(rows, |mut row, entry| {
        row.push_bind(product_id)
            .push_bind(entry.segment_index)
            .push_bind(entry.entry_index)
            .push_bind(entry.time_utc)
            .push_bind(entry.direction_degrees)
            .push_bind(entry.speed_kt)
            .push_bind(&entry.path_wkt)
            .push("ST_GeomFromText(")
            .push_bind_unseparated(&entry.path_wkt)
            .push_unseparated(", 4326)");
    });
    builder.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_product_polygons(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[ProductPolygonRow],
) -> PersistResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO product_polygons (product_id, segment_index, polygon_index, polygon_wkt, polygon_geom) ",
    );
    builder.push_values(rows, |mut row, polygon| {
        row.push_bind(product_id)
            .push_bind(polygon.segment_index)
            .push_bind(polygon.polygon_index)
            .push_bind(&polygon.polygon_wkt)
            .push("ST_GeomFromText(")
            .push_bind_unseparated(&polygon.polygon_wkt)
            .push_unseparated(", 4326)");
    });
    builder.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_product_wind_hail(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[ProductWindHailRow],
) -> PersistResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO product_wind_hail (product_id, segment_index, entry_index, kind, numeric_value, units, comparison) ",
    );
    builder.push_values(rows, |mut row, entry| {
        row.push_bind(product_id)
            .push_bind(entry.segment_index)
            .push_bind(entry.entry_index)
            .push_bind(&entry.kind)
            .push_bind(entry.numeric_value)
            .push_bind(&entry.units)
            .push_bind(&entry.comparison);
    });
    builder.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_product_search_points(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[ProductSearchPointRow],
) -> PersistResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO product_search_points (product_id, source_kind, source_index, latitude, longitude, point_geom) ",
    );
    builder.push_values(rows, |mut row, point| {
        row.push_bind(product_id)
            .push_bind(&point.source_kind)
            .push_bind(point.source_index)
            .push_bind(point.latitude)
            .push_bind(point.longitude)
            .push("ST_SetSRID(ST_MakePoint(")
            .push_bind_unseparated(point.longitude)
            .push_unseparated(", ")
            .push_bind_unseparated(point.latitude)
            .push_unseparated("), 4326)");
    });
    builder.build().execute(&mut **tx).await?;
    Ok(())
}
