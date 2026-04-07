//! Spatial SQL predicate construction for archive queries.

use super::super::PersistResult;
use super::ProductListQuery;
use super::validation::{validate_bbox, validate_point_distance};
use sqlx::{Postgres, QueryBuilder};

pub(super) fn append_spatial_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    query: &ProductListQuery,
) -> PersistResult<()> {
    if let Some((min_lat, max_lat, min_lon, max_lon)) = validate_bbox(query)? {
        builder.push(" AND (");
        builder.push(
            "EXISTS (SELECT 1 FROM product_polygons WHERE product_polygons.product_id = products.id AND ST_Intersects(product_polygons.polygon_geom, ST_MakeEnvelope(",
        );
        builder
            .push_bind(min_lon)
            .push(", ")
            .push_bind(min_lat)
            .push(", ")
            .push_bind(max_lon)
            .push(", ")
            .push_bind(max_lat)
            .push(", 4326)))");
        builder.push(
            " OR EXISTS (SELECT 1 FROM product_time_mot_loc WHERE product_time_mot_loc.product_id = products.id AND ST_Intersects(product_time_mot_loc.path_geom, ST_MakeEnvelope(",
        );
        builder
            .push_bind(min_lon)
            .push(", ")
            .push_bind(min_lat)
            .push(", ")
            .push_bind(max_lon)
            .push(", ")
            .push_bind(max_lat)
            .push(", 4326)))");
        for table in [
            "product_ugc_areas",
            "product_hvtec",
            "product_search_points",
        ] {
            builder
                .push(" OR EXISTS (SELECT 1 FROM ")
                .push(table)
                .push(" WHERE ")
                .push(table)
                .push(".product_id = products.id AND ")
                .push(table)
                .push(".point_geom IS NOT NULL AND ST_Covers(ST_MakeEnvelope(")
                .push_bind(min_lon)
                .push(", ")
                .push_bind(min_lat)
                .push(", ")
                .push_bind(max_lon)
                .push(", ")
                .push_bind(max_lat)
                .push(", 4326), ")
                .push(table)
                .push(".point_geom))");
        }
        builder.push(")");
    }

    if let Some((lat, lon, distance_meters)) = validate_point_distance(query)? {
        builder.push(" AND (");
        builder.push(
            "EXISTS (SELECT 1 FROM product_polygons WHERE product_polygons.product_id = products.id AND ST_Covers(product_polygons.polygon_geom, ST_SetSRID(ST_MakePoint(",
        );
        builder
            .push_bind(lon)
            .push(", ")
            .push_bind(lat)
            .push("), 4326))");
        builder.push(" OR EXISTS (SELECT 1 FROM product_time_mot_loc WHERE product_time_mot_loc.product_id = products.id AND ST_DWithin(product_time_mot_loc.path_geom::geography, ST_SetSRID(ST_MakePoint(");
        builder
            .push_bind(lon)
            .push(", ")
            .push_bind(lat)
            .push("), 4326)::geography, ")
            .push_bind(distance_meters)
            .push("))");
        for table in [
            "product_ugc_areas",
            "product_hvtec",
            "product_search_points",
        ] {
            builder
                .push(" OR EXISTS (SELECT 1 FROM ")
                .push(table)
                .push(" WHERE ")
                .push(table)
                .push(".product_id = products.id AND ")
                .push(table)
                .push(".point_geom IS NOT NULL AND ST_DWithin(")
                .push(table)
                .push(".point_geom::geography, ST_SetSRID(ST_MakePoint(")
                .push_bind(lon)
                .push(", ")
                .push_bind(lat)
                .push("), 4326)::geography, ")
                .push_bind(distance_meters)
                .push("))");
        }
        builder.push(")");
    }

    Ok(())
}

pub(super) fn append_feature_spatial_filter(
    builder: &mut QueryBuilder<'_, Postgres>,
    query: &ProductListQuery,
) -> PersistResult<()> {
    if let Some((min_lat, max_lat, min_lon, max_lon)) = validate_bbox(query)? {
        builder
            .push(" AND ST_Intersects(features.feature_geom, ST_MakeEnvelope(")
            .push_bind(min_lon)
            .push(", ")
            .push_bind(min_lat)
            .push(", ")
            .push_bind(max_lon)
            .push(", ")
            .push_bind(max_lat)
            .push(", 4326))");
    }

    if let Some((lat, lon, distance_meters)) = validate_point_distance(query)? {
        builder.push(
            " AND ((features.feature_kind = 'polygon' AND ST_Covers(features.feature_geom, ST_SetSRID(ST_MakePoint(",
        );
        builder
            .push_bind(lon)
            .push(", ")
            .push_bind(lat)
            .push("), 4326))) OR (features.feature_kind <> 'polygon' AND ST_DWithin(features.feature_geom::geography, ST_SetSRID(ST_MakePoint(")
            .push_bind(lon)
            .push(", ")
            .push_bind(lat)
            .push("), 4326)::geography, ")
            .push_bind(distance_meters)
            .push(")))");
    }

    Ok(())
}
