CREATE INDEX IF NOT EXISTS product_ugc_point_geog_gist_idx
    ON product_ugc_areas USING GIST ((point_geom::geography))
    WHERE point_geom IS NOT NULL;

CREATE INDEX IF NOT EXISTS product_hvtec_point_geog_gist_idx
    ON product_hvtec USING GIST ((point_geom::geography))
    WHERE point_geom IS NOT NULL;

CREATE INDEX IF NOT EXISTS product_time_mot_loc_path_geog_gist_idx
    ON product_time_mot_loc USING GIST ((path_geom::geography));

CREATE INDEX IF NOT EXISTS product_search_points_geog_gist_idx
    ON product_search_points USING GIST ((point_geom::geography))
    WHERE point_geom IS NOT NULL;
