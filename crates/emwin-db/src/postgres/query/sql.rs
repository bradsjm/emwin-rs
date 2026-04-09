use emwin_service::FacetDimension;

pub(crate) fn incident_select_sql() -> String {
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

pub(crate) fn archived_product_summary_select_sql() -> String {
    String::from(
        "SELECT
            id AS product_id,
            filename,
            source_timestamp_utc,
            ingested_at,
            source_receiver,
            source_message_id,
            size_bytes,
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

pub(crate) fn archived_product_detail_select_sql() -> String {
    let mut sql = archived_product_summary_select_sql();
    sql.push_str(", payload_location, metadata_location, product_json");
    sql
}

pub(crate) fn archived_issue_select_sql() -> String {
    String::from(
        "SELECT
            product_issues.id,
            product_issues.product_id,
            product_issues.kind,
            product_issues.code,
            product_issues.message,
            product_issues.line
         FROM product_issues
         INNER JOIN products ON products.id = product_issues.product_id",
    )
}

pub(crate) fn archived_feature_select_sql() -> String {
    let mut sql = String::from(
        "SELECT
            features.feature_kind,
            features.feature_kind_order,
            features.feature_row_id,
            products.id AS product_id,
            products.source_timestamp_utc,
            features.feature_geom,
            features.geometry,
            features.properties",
    );
    sql.push_str(&archived_feature_source_sql());
    sql
}

pub(super) fn archived_feature_source_sql() -> String {
    String::from(
        " FROM (
            SELECT
                'polygon' AS feature_kind,
                1 AS feature_kind_order,
                product_polygons.id AS feature_row_id,
                product_polygons.product_id,
                product_polygons.polygon_geom AS feature_geom,
                ST_AsGeoJSON(product_polygons.polygon_geom)::json AS geometry,
                jsonb_build_object(
                    'segment_index', product_polygons.segment_index,
                    'polygon_index', product_polygons.polygon_index
                ) AS properties
            FROM product_polygons
            UNION ALL
            SELECT
                'time_mot_loc_path' AS feature_kind,
                2 AS feature_kind_order,
                product_time_mot_loc.id AS feature_row_id,
                product_time_mot_loc.product_id,
                product_time_mot_loc.path_geom AS feature_geom,
                ST_AsGeoJSON(product_time_mot_loc.path_geom)::json AS geometry,
                jsonb_build_object(
                    'segment_index', product_time_mot_loc.segment_index,
                    'entry_index', product_time_mot_loc.entry_index,
                    'time_utc', product_time_mot_loc.time_utc,
                    'direction_degrees', product_time_mot_loc.direction_degrees,
                    'speed_kt', product_time_mot_loc.speed_kt
                ) AS properties
            FROM product_time_mot_loc
            UNION ALL
            SELECT
                'ugc_point' AS feature_kind,
                3 AS feature_kind_order,
                product_ugc_areas.id AS feature_row_id,
                product_ugc_areas.product_id,
                product_ugc_areas.point_geom AS feature_geom,
                ST_AsGeoJSON(product_ugc_areas.point_geom)::json AS geometry,
                jsonb_build_object(
                    'segment_index', product_ugc_areas.segment_index,
                    'section_index', product_ugc_areas.section_index,
                    'area_kind', product_ugc_areas.area_kind,
                    'state', product_ugc_areas.state,
                    'ugc_code', product_ugc_areas.ugc_code,
                    'name', product_ugc_areas.name,
                    'expires_utc', product_ugc_areas.expires_utc
                ) AS properties
            FROM product_ugc_areas
            WHERE product_ugc_areas.point_geom IS NOT NULL
            UNION ALL
            SELECT
                'hvtec_point' AS feature_kind,
                4 AS feature_kind_order,
                product_hvtec.id AS feature_row_id,
                product_hvtec.product_id,
                product_hvtec.point_geom AS feature_geom,
                ST_AsGeoJSON(product_hvtec.point_geom)::json AS geometry,
                jsonb_build_object(
                    'segment_index', product_hvtec.segment_index,
                    'hvtec_index', product_hvtec.hvtec_index,
                    'nwslid', product_hvtec.nwslid,
                    'location_name', product_hvtec.location_name,
                    'severity', product_hvtec.severity,
                    'cause', product_hvtec.cause,
                    'record', product_hvtec.record,
                    'begin_utc', product_hvtec.begin_utc,
                    'crest_utc', product_hvtec.crest_utc,
                    'end_utc', product_hvtec.end_utc
                ) AS properties
            FROM product_hvtec
            WHERE product_hvtec.point_geom IS NOT NULL
            UNION ALL
            SELECT
                'search_point' AS feature_kind,
                5 AS feature_kind_order,
                product_search_points.id AS feature_row_id,
                product_search_points.product_id,
                product_search_points.point_geom AS feature_geom,
                ST_AsGeoJSON(product_search_points.point_geom)::json AS geometry,
                jsonb_build_object(
                    'source_kind', product_search_points.source_kind,
                    'source_index', product_search_points.source_index
                ) AS properties
            FROM product_search_points
        ) AS features
        INNER JOIN products ON products.id = features.product_id",
    )
}

pub(super) fn geohash_alphabet_sql() -> &'static str {
    "(VALUES
        ('0'), ('1'), ('2'), ('3'), ('4'), ('5'), ('6'), ('7'),
        ('8'), ('9'), ('b'), ('c'), ('d'), ('e'), ('f'), ('g'),
        ('h'), ('j'), ('k'), ('m'), ('n'), ('p'), ('q'), ('r'),
        ('s'), ('t'), ('u'), ('v'), ('w'), ('x'), ('y'), ('z')
    )"
}

pub(super) fn facet_aggregate_select_sql(dimension: FacetDimension) -> String {
    match dimension {
        FacetDimension::Office => String::from(
            "SELECT products.office_code AS value, COUNT(DISTINCT products.id) AS count
             FROM products",
        ),
        FacetDimension::Family => String::from(
            "SELECT products.family AS value, COUNT(DISTINCT products.id) AS count
             FROM products",
        ),
        FacetDimension::ArtifactKind => String::from(
            "SELECT products.artifact_kind AS value, COUNT(DISTINCT products.id) AS count
             FROM products",
        ),
        FacetDimension::Phenomena => String::from(
            "SELECT facet.phenomena AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_vtec AS facet ON facet.product_id = products.id",
        ),
        FacetDimension::Significance => String::from(
            "SELECT facet.significance AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_vtec AS facet ON facet.product_id = products.id",
        ),
        FacetDimension::Status => String::from(
            "SELECT facet.current_status AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_vtec ON product_vtec.product_id = products.id
             INNER JOIN incidents AS facet
               ON facet.office = product_vtec.office
              AND facet.phenomena = product_vtec.phenomena
              AND facet.significance = product_vtec.significance
              AND facet.etn = product_vtec.etn",
        ),
        FacetDimension::IssueKind => String::from(
            "SELECT facet.kind AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_issues AS facet ON facet.product_id = products.id",
        ),
        FacetDimension::IssueCode => String::from(
            "SELECT facet.code AS value, COUNT(DISTINCT products.id) AS count
             FROM products
             INNER JOIN product_issues AS facet ON facet.product_id = products.id",
        ),
    }
}
