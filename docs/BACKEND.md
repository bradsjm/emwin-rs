# Backend Capabilities for Interactive Severe Weather Frontend

## Purpose

This document defines the current backend capabilities available to a frontend built on top of `emwin-rs`.
It is grounded in the shipped `emwin-cli server` API and current archive/query capabilities, then calls out the smaller set of backend gaps that still remain for a more polished frontend.

The current backend already supports:

- national and regional severe-weather exploration
- live incident creation and update tracking
- drill-down from incident to product to raw payload
- historical product, issue, feature, and aggregate queries

The current backend does not yet provide every frontend-shaped convenience surface. Those remaining gaps are listed explicitly below.

## Confirmed Current Backend Surface

The current `emwin-cli server` API exposes a product-first, resource-first `/v1/*` namespace:

- `GET /`
  - Swagger UI
- `GET /openapi.json`
  - generated OpenAPI document
- `GET /v1/streams/products`
  - incremental SSE stream of completed products
  - completed-product event name is `product_available`
  - supports rich filtering over event name, filename, product metadata, header metadata, issue metadata, hazard/body presence, UGC geography, VTEC fields, HVTEC fields, wind/hail thresholds, point-radius location, bounding box, and size
- `GET /v1/streams/incidents`
  - incremental SSE stream of persisted incident projection changes
  - event name is `incident_change`
  - incident actions are `created` and `updated`
- `GET /v1/incidents`
  - paginated incident list from the persisted incident projection
- `GET /v1/incidents/{office}/{phenomena}/{significance}/{etn}`
  - incident detail
- `GET /v1/incidents/{office}/{phenomena}/{significance}/{etn}/products`
  - paginated archived product timeline for one incident
- `GET /v1/products`
  - paginated archived product list and search endpoint
- `GET /v1/products/{product_id}`
  - archived product detail, including `product_json`
- `GET /v1/products/{product_id}/raw`
  - raw archived payload bytes
- `GET /v1/features`
  - paginated archived spatial feature list
- `GET /v1/features/geojson`
  - bounded GeoJSON `FeatureCollection` over archived spatial features
- `GET /v1/aggregates/facets`
  - uncursored facet aggregation over archived products
- `GET /v1/aggregates/timeseries`
  - uncursored time-bucket aggregation over archived products and incidents
- `GET /v1/aggregates/cells`
  - uncursored geohash cell aggregation over archived spatial features
- `GET /v1/issues`
  - paginated archived issue list
- `GET /v1/issues/{issue_id}`
  - archived issue detail
- `GET /v1/files`
  - retained in-memory completed-file list
- `GET /v1/files/{*filename}`
  - retained in-memory file download
- `GET /v1/health`
  - server health summary
- `GET /v1/metrics`
  - JSON telemetry snapshot

Completed work from [`PLAN.md`](/Users/jonathan/Code/emwin-rs/docs/PLAN.md) that is now shipped:

- product-first resource API
- archive product list/search
- archive feature queries
- GeoJSON feature collection output
- generic aggregates
- resource/stream naming cleanup
- shared archive filter grammar reused across product, feature, and aggregate archive reads
- `query` command parity for archive product, feature, and aggregate reads

## Confirmed Current Data Model and Semantics

### Product Summary

The `product_available` SSE payload and retained-file payload expose a stable product summary model with:

- source
- family
- artifact kind
- title
- container
- PIL
- WMO prefix
- BBB kind
- office
- header
- facets
  - `has_body`
  - `has_artifact`
  - `has_issues`
  - `vtec_count`
  - `ugc_count`
  - `hvtec_count`
  - `latlon_count`
  - `time_mot_loc_count`
  - `wind_hail_count`
- keys
  - states
  - UGC codes
  - VTEC phenomena
  - VTEC significance
  - VTEC actions
  - VTEC offices
  - ETNs
  - HVTEC NWSLIDs
  - HVTEC causes
  - HVTEC severities
  - HVTEC records
- issue summary
  - count
  - unique issue codes

### Product Detail

The archived product detail exposed by `/v1/products/{product_id}` includes:

- all summary fields
- full parsed body when present
- full specialized artifact when present
- parse and QC issues
- payload and metadata storage locations
- raw payload download URL

### Incident Model

The persisted incident projection exposed by `/v1/incidents` and `/v1/streams/incidents` includes:

- office
- phenomena
- significance
- ETN
- current status
- latest VTEC action
- issued time
- start time
- end time
- last updated time
- first product id
- latest product id
- latest product timestamp

### Archive Query and Filter Model

`/v1/products` is a shipped archive product list and search surface.

Current archive read capabilities:

- cursor pagination on product, issue, feature, and incident-product list endpoints
- one shared archive filter grammar reused across `/v1/products`, `/v1/features`, `/v1/features/geojson`, and `/v1/aggregates/*`
- equivalent archive reads exposed through the `query` command directly against Postgres

Archive filters currently cover:

- metadata fields such as filename, source receiver, source, family, artifact kind, container, and office metadata
- header fields such as `cccc`, `ttaaii`, `afos`, `bbb`, `pil`, and WMO prefix
- issue fields such as issue kind and issue code
- hazard/body presence fields such as `has_vtec`, `has_ugc`, `has_hvtec`, `has_latlon`, `has_time_mot_loc`, and `has_wind_hail`
- geographic filters such as state, county, zone, fire zone, and marine zone
- VTEC fields
- HVTEC fields
- wind and hail thresholds
- point-radius spatial filters
- bounding-box spatial filters
- source timestamp bounds
- ingest timestamp bounds
- payload size bounds

Validation semantics already implemented:

- invalid archive booleans return `400`
- invalid size ranges where `min_size > max_size` return `400`
- archive-backed resource endpoints return `503` when Postgres-backed archive metadata is not configured

### Feature API and Geometry Semantics

The backend already exposes generic archived spatial features through `/v1/features` and `/v1/features/geojson`.

Supported feature kinds:

- `polygon`
- `time_mot_loc_path`
- `ugc_point`
- `hvtec_point`
- `search_point`

Feature responses include:

- geometry
- source timestamp
- feature properties
- product linkage via `product_url` and `product_raw_url`

Spatial filter semantics:

- spatial filters apply to each returned geometry or counted feature contribution
- they are not limited to product admission only

### Aggregate API

The backend already exposes generic archive aggregates:

- `/v1/aggregates/facets`
- `/v1/aggregates/timeseries`
- `/v1/aggregates/cells`

Aggregate responses include completeness metadata in the public schema:

- `partial`
- `approximate`
- `reason`

Currently supported facet dimensions:

- `office`
- `family`
- `artifact_kind`
- `phenomena`
- `significance`
- `status`
- `issue_kind`
- `issue_code`

Currently supported timeseries measures:

- `product_count`
- `issue_count`
- `incident_count`

Currently supported timeseries buckets:

- `hour`
- `day`
- `week`

Currently supported cell measures:

- `product_count`

Cell aggregation currently counts distinct products per intersected geohash cell across persisted polygons, paths, and representative points.

### Hazard and Parsing Semantics Already Available

The parser already supports, when present in source products:

- VTEC event segments
- UGC county, zone, fire-zone, and marine-zone geography
- HVTEC data
- `LAT...LON` polygons
- `TIME...MOT...LOC` tracks and points
- wind and hail threat tags and numeric thresholds
- specialized severe-weather products including:
  - `lsr`
  - `mcd`
  - `ero`
  - `spc_outlook`
  - `wwp`
  - `saw`
  - `sel`
  - `sigmet`
  - `cwa`

## Frontend Consumption Rules and Constraints

The current backend is sufficient for a strong frontend if clients consume it as a resource API with incremental streams layered on top.

Recommended frontend usage:

- use `/v1/incidents`, `/v1/products`, `/v1/features`, and `/v1/aggregates/*` for initial snapshots and historical reads
- use `/v1/streams/products` for live completed-product updates
- use `/v1/streams/incidents` for persisted incident lifecycle updates
- use `/v1/incidents/{...}/products` for incident timeline drill-down
- use `/v1/products/{product_id}` for lazy product detail fetches
- use `/v1/issues` and `/v1/issues/{issue_id}` for parse and QC inspection

Stream contract constraints:

- `/v1/streams/products` and `/v1/streams/incidents` are incremental streams, not durable replay logs
- clients should fetch an initial snapshot from resource endpoints before attaching SSE
- `Last-Event-ID` is best-effort only for short reconnect gaps
- lag warnings require a full resync

Rendering constraints:

- use summary payloads for list and map rendering
- fetch archived detail lazily when drilling into one product
- treat parser issues as first-class data rather than hidden diagnostics
- treat geometry as optional
- some products have polygons
- some only have points or keyed geography
- some outlook products may degrade to tokenized locations or non-geometric areal-outline mode

Server configuration constraints:

- `--persist-database-url` is required for `/v1/incidents`, `/v1/incidents/{...}`, `/v1/incidents/{...}/products`, `/v1/products`, `/v1/products/{product_id}`, `/v1/products/{product_id}/raw`, `/v1/issues`, `/v1/issues/{issue_id}`, `/v1/features`, `/v1/features/geojson`, `/v1/aggregates/*`, and `/v1/streams/incidents`
- bearer auth remains optional but should be enabled in deployed environments
- when `--openapi-auth-token` is configured, `Authorization: Bearer <token>` applies to all `/v1/*` routes
- `GET /`, `GET /openapi.json`, and Swagger UI assets remain public
- browser clients need `--cors-origin` when cross-origin access is required
- `/v1/files` serves retained in-memory payloads, not archived S3 objects

## Remaining Backend Gaps

- frontend-specific derived map layer endpoint: not present
- precomputed hotspot summary endpoint: not present
- precomputed trend summary endpoint: not present
- durable replay or event-log semantics for SSE: not present
- richer cell measures such as issue or incident counts: not present

Additional detail on those gaps:

- There is no dedicated frontend-shaped derived layer endpoint for opinionated map layers such as active warning polygons, watch polygons, or LSR-only bundles.
- There is no dedicated hotspot endpoint with frontend-specific combined measures such as active incidents, updated incidents, warning counts, and threshold exceedance counts in one response.
- There is no dedicated trend summary endpoint with precomposed national or regional summaries.
- The aggregate API is generic but intentionally limited to the supported measures and dimensions listed above.
- The backend currently answers aggregates directly from query-time reads rather than from durable rollups or replayable event-log infrastructure.

## Non-Functional Requirements

### Availability

- live ingest must remain usable if archive reads are temporarily unavailable
- archive-backed resource endpoints may return `503` when Postgres-backed metadata is unavailable
- `/v1/files` and live ingest can continue to operate while archive persistence is degraded

### Authentication

- bearer auth should be enabled in deployed environments
- frontend clients must support authenticated SSE and JSON requests when `/v1/*` auth is configured

### Pagination

- incident, product, issue, and feature list views preserve cursor-based pagination
- frontend state should keep cursors stable during drill-down and refresh

### Resume and Recovery

- SSE consumers may use `Last-Event-ID` only for short reconnect gaps
- clients must resync from resource endpoints after lag warnings or detected gaps

### Data Quality

- backend preserves issue rows and issue codes as queryable data
- aggregate responses carry completeness metadata when results are partial or approximate

### Performance

- list and map endpoints should be usable at national scope without fetching full product detail for every visible object
- summary resources, generic feature resources, and generic aggregates are the intended building blocks for frontend overview screens
