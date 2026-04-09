CREATE EXTENSION IF NOT EXISTS postgis;

CREATE TABLE IF NOT EXISTS products (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    filename TEXT NOT NULL,
    source_timestamp_utc BIGINT NOT NULL,
    source_receiver TEXT NOT NULL,
    source_message_id TEXT,
    ingested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    size_bytes BIGINT NOT NULL,
    payload_location TEXT NOT NULL,
    metadata_location TEXT,
    source TEXT NOT NULL,
    family TEXT,
    artifact_kind TEXT,
    title TEXT,
    container TEXT NOT NULL,
    pil TEXT,
    wmo_prefix TEXT,
    bbb_kind TEXT,
    office_code TEXT,
    office_city TEXT,
    office_state TEXT,
    header_kind TEXT,
    ttaaii TEXT,
    cccc TEXT,
    ddhhmm TEXT,
    bbb TEXT,
    afos TEXT,
    has_body BOOLEAN NOT NULL,
    has_artifact BOOLEAN NOT NULL,
    has_issues BOOLEAN NOT NULL,
    has_vtec BOOLEAN NOT NULL,
    has_ugc BOOLEAN NOT NULL,
    has_hvtec BOOLEAN NOT NULL,
    has_latlon BOOLEAN NOT NULL,
    has_time_mot_loc BOOLEAN NOT NULL,
    has_wind_hail BOOLEAN NOT NULL,
    vtec_count INTEGER NOT NULL,
    ugc_count INTEGER NOT NULL,
    hvtec_count INTEGER NOT NULL,
    latlon_count INTEGER NOT NULL,
    time_mot_loc_count INTEGER NOT NULL,
    wind_hail_count INTEGER NOT NULL,
    issue_count INTEGER NOT NULL,
    states TEXT[] NOT NULL DEFAULT '{}',
    ugc_codes TEXT[] NOT NULL DEFAULT '{}',
    product_json JSONB NOT NULL,
    CONSTRAINT products_filename_source_timestamp_key UNIQUE (filename, source_timestamp_utc)
);

CREATE TABLE IF NOT EXISTS product_issues (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    line TEXT
);

CREATE TABLE IF NOT EXISTS product_vtec (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    segment_index INTEGER,
    status TEXT NOT NULL,
    action TEXT NOT NULL,
    office TEXT NOT NULL,
    phenomena TEXT NOT NULL,
    significance TEXT NOT NULL,
    etn BIGINT NOT NULL,
    begin_utc TIMESTAMPTZ,
    end_utc TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS product_ugc_areas (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    segment_index INTEGER,
    section_index INTEGER NOT NULL,
    area_kind TEXT NOT NULL,
    state TEXT NOT NULL,
    ugc_code TEXT NOT NULL,
    name TEXT,
    expires_utc TIMESTAMPTZ NOT NULL,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    point_geom geometry(Point, 4326)
);

CREATE TABLE IF NOT EXISTS product_hvtec (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    segment_index INTEGER,
    hvtec_index INTEGER NOT NULL,
    nwslid TEXT NOT NULL,
    location_name TEXT,
    severity TEXT NOT NULL,
    cause TEXT NOT NULL,
    record TEXT NOT NULL,
    begin_utc TIMESTAMPTZ,
    crest_utc TIMESTAMPTZ,
    end_utc TIMESTAMPTZ,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    point_geom geometry(Point, 4326)
);

CREATE TABLE IF NOT EXISTS product_time_mot_loc (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    segment_index INTEGER,
    entry_index INTEGER NOT NULL,
    time_utc TIMESTAMPTZ NOT NULL,
    direction_degrees INTEGER NOT NULL,
    speed_kt INTEGER NOT NULL,
    path_wkt TEXT NOT NULL,
    path_geom geometry(Geometry, 4326) NOT NULL
);

CREATE TABLE IF NOT EXISTS product_polygons (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    segment_index INTEGER,
    polygon_index INTEGER NOT NULL,
    polygon_wkt TEXT NOT NULL,
    polygon_geom geometry(Polygon, 4326) NOT NULL
);

CREATE TABLE IF NOT EXISTS product_wind_hail (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    segment_index INTEGER,
    entry_index INTEGER NOT NULL,
    kind TEXT NOT NULL,
    numeric_value DOUBLE PRECISION,
    units TEXT,
    comparison TEXT
);

CREATE TABLE IF NOT EXISTS product_search_points (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL,
    source_index INTEGER NOT NULL,
    latitude DOUBLE PRECISION NOT NULL,
    longitude DOUBLE PRECISION NOT NULL,
    point_geom geometry(Point, 4326) NOT NULL
);

CREATE INDEX IF NOT EXISTS products_source_timestamp_idx ON products (source_timestamp_utc DESC);
CREATE INDEX IF NOT EXISTS products_filename_idx ON products (filename);
CREATE INDEX IF NOT EXISTS products_source_receiver_idx ON products (source_receiver);
CREATE INDEX IF NOT EXISTS products_source_message_id_idx ON products (source_message_id) WHERE source_message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS products_ingested_at_idx ON products (ingested_at DESC);
CREATE INDEX IF NOT EXISTS products_family_pil_idx ON products (family, pil);
CREATE INDEX IF NOT EXISTS products_office_idx ON products (office_code, office_state);
CREATE INDEX IF NOT EXISTS products_wmo_prefix_idx ON products (wmo_prefix);
CREATE INDEX IF NOT EXISTS products_artifact_kind_idx ON products (artifact_kind);
CREATE INDEX IF NOT EXISTS products_source_idx ON products (source);
CREATE INDEX IF NOT EXISTS products_states_gin_idx ON products USING GIN (states);
CREATE INDEX IF NOT EXISTS products_ugc_codes_gin_idx ON products USING GIN (ugc_codes);

CREATE INDEX IF NOT EXISTS product_issues_code_kind_idx ON product_issues (code, kind, product_id);
CREATE INDEX IF NOT EXISTS product_vtec_lookup_idx ON product_vtec (office, phenomena, significance, action);
CREATE INDEX IF NOT EXISTS product_vtec_etn_idx ON product_vtec (etn);
CREATE INDEX IF NOT EXISTS product_ugc_code_idx ON product_ugc_areas (ugc_code);
CREATE INDEX IF NOT EXISTS product_ugc_state_kind_idx ON product_ugc_areas (state, area_kind);
CREATE INDEX IF NOT EXISTS product_ugc_point_gist_idx ON product_ugc_areas USING GIST (point_geom);
CREATE INDEX IF NOT EXISTS product_hvtec_lookup_idx ON product_hvtec (nwslid, severity, cause, record);
CREATE INDEX IF NOT EXISTS product_hvtec_point_gist_idx ON product_hvtec USING GIST (point_geom);
CREATE INDEX IF NOT EXISTS product_time_mot_loc_time_idx ON product_time_mot_loc (time_utc);
CREATE INDEX IF NOT EXISTS product_time_mot_loc_path_gist_idx ON product_time_mot_loc USING GIST (path_geom);
CREATE INDEX IF NOT EXISTS product_polygons_geom_gist_idx ON product_polygons USING GIST (polygon_geom);
CREATE INDEX IF NOT EXISTS product_wind_hail_kind_idx ON product_wind_hail (kind);
CREATE INDEX IF NOT EXISTS product_search_points_source_idx ON product_search_points (source_kind, product_id);
CREATE INDEX IF NOT EXISTS product_search_points_geom_gist_idx ON product_search_points USING GIST (point_geom);
