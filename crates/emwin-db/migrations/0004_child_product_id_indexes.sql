CREATE INDEX IF NOT EXISTS product_issues_product_id_idx ON product_issues (product_id);
CREATE INDEX IF NOT EXISTS product_vtec_product_id_idx ON product_vtec (product_id);
CREATE INDEX IF NOT EXISTS product_ugc_areas_product_id_idx ON product_ugc_areas (product_id);
CREATE INDEX IF NOT EXISTS product_hvtec_product_id_idx ON product_hvtec (product_id);
CREATE INDEX IF NOT EXISTS product_time_mot_loc_product_id_idx ON product_time_mot_loc (product_id);
CREATE INDEX IF NOT EXISTS product_polygons_product_id_idx ON product_polygons (product_id);
CREATE INDEX IF NOT EXISTS product_wind_hail_product_id_idx ON product_wind_hail (product_id);
CREATE INDEX IF NOT EXISTS product_search_points_product_id_idx ON product_search_points (product_id);
