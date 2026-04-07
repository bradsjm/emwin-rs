use super::super::prepare::{
    PendingIncidentChange, PreparedProduct, ProductHvtecRow, ProductIssueRow, ProductPolygonRow,
    ProductSearchPointRow, ProductTimeMotLocRow, ProductUgcAreaRow, ProductVtecRow,
    ProductWindHailRow,
};
use super::super::{PersistResult, Postgres, QueryBuilder};
use sqlx::Transaction;

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
    let incident_changes =
        super::incidents::upsert_incidents(tx, product_id, &prepared.incident_updates).await?;
    insert_product_ugc_areas(tx, product_id, &prepared.ugc_areas).await?;
    insert_product_hvtec(tx, product_id, &prepared.hvtec).await?;
    insert_product_time_mot_loc(tx, product_id, &prepared.time_mot_loc).await?;
    insert_product_polygons(tx, product_id, &prepared.polygons).await?;
    insert_product_wind_hail(tx, product_id, &prepared.wind_hail).await?;
    insert_product_search_points(tx, product_id, &prepared.search_points).await?;
    Ok(incident_changes)
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
