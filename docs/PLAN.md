# Backend Recommendation Plan

## Goal

Define a backend expansion plan that fits the current `emwin-rs` architecture, preserves existing strengths, and supports multiple client types instead of optimizing for one interactive severe-weather frontend.

## Source Constraints

- `emwin-protocol` owns ingest/runtime concerns.
- `emwin-parser` owns product enrichment and parsed weather semantics.
- `emwin-db` owns persisted metadata, spatial child tables, and query logic.
- `emwin-cli` owns HTTP, SSE, OpenAPI, CLI wiring, and response shaping.
- The current server already exposes:
  - live SSE over `/v1/live/events`
  - incident SSE over `/v1/live/incident-events`
  - incident/archive/product/issue HTTP reads
  - retained live file downloads
- Archive-backed HTTP reads require `--persist-database-url`, and that in turn requires `--output-dir`.
- The persisted schema already stores normalized products plus issues, VTEC, UGC areas, HVTEC, polygons, motion paths, wind/hail rows, and search points.

## Architectural Position

The backend should be product-first, not incident-first.

Reason:

- products are the canonical persisted unit
- incidents are already a derived VTEC projection
- many useful clients are not warning-incident centric
- the existing schema and parser output are much richer than the incident model alone

Implication:

- keep `Incident` as an important derived read model
- promote `Product`, `Issue`, `Feature`, and `Aggregate` to first-class backend concepts

## Recommended API Shape

### 1. Resource APIs

Add stable resource-oriented APIs instead of more frontend-specific endpoints.

Recommended families:

- `/v1/products`
- `/v1/products/{product_id}`
- `/v1/products/{product_id}/raw`
- `/v1/issues`
- `/v1/issues/{issue_id}`
- `/v1/incidents`
- `/v1/incidents/{office}/{phenomena}/{significance}/{etn}`
- `/v1/incidents/{office}/{phenomena}/{significance}/{etn}/products`

Design rules:

- keep cursor pagination everywhere large result sets exist
- keep filter semantics consistent across list endpoints
- keep detail responses lazy and heavier than summary responses
- preserve current raw-payload retrieval behavior

### 2. Feature APIs

Add a generic spatial query surface rather than a frontend-only “derived map layer” endpoint.

Recommended families:

- `/v1/features`
- `/v1/features/geojson`

Supported feature kinds should include:

- product polygons
- motion paths
- point features from UGC/HVTEC/search points

Design rules:

- return normalized geometry with stable properties
- keep feature generation in query/read-model logic, not in parser or HTTP handlers
- use the same filter grammar as product search where practical

### 3. Aggregate APIs

Generalize “hotspots” and “trends” into reusable aggregate services.

Recommended families:

- `/v1/aggregates/cells`
- `/v1/aggregates/timeseries`
- `/v1/aggregates/facets`

Candidate measures:

- product count
- incident count
- issue count
- warning count by phenomena/significance
- LSR count
- wind/hail threshold count

Candidate dimensions:

- time bucket
- spatial cell
- office
- family
- artifact kind
- phenomena
- significance
- status

Design rules:

- aggregation responses must report when inputs are partial or approximate
- keep aggregate definitions generic enough for dashboards, search UIs, and batch consumers
- do not hardcode one frontend’s map language into the API contract

### 4. Stream APIs

Keep SSE, but document it as an incremental stream, not a durable event log.

Recommended families:

- `/v1/streams/products`
- `/v1/streams/incidents`

If the existing `/v1/live/*` naming must remain for now:

- keep it as compatibility naming during transition
- define the long-term contract in resource/stream terms

Design rules:

- clients should fetch an initial snapshot, then attach a stream
- clients must handle lag warnings and reconnect/resync
- `Last-Event-ID` is useful for short gaps but not sufficient as a full replay mechanism

## Query Model Plan

### Shared Filter Grammar

Build one reusable query model in `emwin-db` and expose subsets through `emwin-cli`.

Core filter areas:

- time range
- source receiver
- source/family/artifact/container
- office/header fields
- issue fields
- VTEC fields
- HVTEC fields
- UGC/state fields
- wind/hail thresholds
- point-radius and bounding-box spatial filters

Reason:

- current live filters are already broad
- archive search should not invent a second incompatible filter language

### Read-Model Ownership

Keep ownership aligned with existing crate boundaries:

- `emwin-db`
  - SQL builders
  - pagination
  - resource read models
  - aggregate queries
  - feature query outputs
- `emwin-cli`
  - DTO translation
  - HTTP parsing/validation
  - auth/cors/OpenAPI

Do not put aggregate or search business logic into HTTP handlers.

## Delivery Plan

### Phase 1: Product Search

Deliver first:

- archive product list/search endpoint
- consistent cursor pagination
- shared filter validation
- product summaries only in list responses

Why first:

- highest reuse across clients
- directly supported by the current persisted schema
- unblocks historical exploration without forcing incident-first navigation

### Phase 2: Spatial Features

Deliver next:

- feature query endpoints backed by existing spatial tables
- GeoJSON output option
- filters shared with product search

Why second:

- turns current parser/persistence strengths into reusable map capability
- removes geometry normalization burden from clients

### Phase 3: Aggregates

Deliver next:

- cell aggregates
- timeseries aggregates
- facet counts

Why third:

- depends on settled search/filter semantics
- likely needs profiling before deciding whether raw SQL is enough or rollups/materialized views are required

### Phase 4: Naming Cleanup

Evaluate whether to:

- keep `live/archive` naming as-is
- add parallel product-first endpoints
- or break the API and move fully to resource-first naming

Preferred direction:

- move toward resource-first naming during development

Reason:

- `live/archive` mixes freshness, transport, and resource identity
- resource-first naming is easier to reuse across CLI, browser, services, and batch consumers

## Non-Functional Requirements

- live ingest must remain available when archive persistence is degraded
- archive-backed reads may return `503` when persistence is unavailable
- bearer auth remains the deployment default for protected endpoints
- CORS must continue to allow browser authorization headers
- summary endpoints must avoid forcing clients to fetch full product detail at scale
- all new list/search/aggregate endpoints must be paginated or explicitly bounded
- SSE contracts must document lag/drop behavior clearly

## Implementation Order by Crate

### `crates/emwin-db`

- add reusable archive product search query types
- add spatial feature query/read-model types
- add aggregate query/read-model types
- add tests for filters, pagination, and spatial predicates

### `crates/emwin-cli`

- add HTTP DTOs and handlers for new resource, feature, and aggregate endpoints
- keep handlers thin
- update OpenAPI and README docs
- add handler/integration tests

### `crates/emwin-parser`

- no architecture change should be required for the initial plan
- only extend parser outputs if a genuinely missing semantic blocks a backend capability

### `crates/emwin-protocol`

- no backend design change should require protocol/runtime changes

## Risks

### Risk 1: Two filter grammars

If live and archive queries diverge, the backend becomes harder to use and maintain.

Mitigation:

- define shared filter types early

### Risk 2: Frontend-shaped endpoints

If aggregate and feature APIs are tailored to one UI, the backend will be brittle and harder to reuse.

Mitigation:

- expose generic dimensions, measures, and feature kinds

### Risk 3: Premature analytics complexity

If rollups/materialized views are introduced before profiling, the design will get heavier than needed.

Mitigation:

- start with direct Postgres/PostGIS queries
- optimize only after national-scope testing proves it necessary

## Recommendation Summary

Proceed with a product-first backend expansion plan:

1. add archive product search
2. add generic spatial feature queries
3. add reusable aggregate endpoints
4. keep incidents as a derived warning-focused read model
5. move gradually toward resource-first API naming

This plan complements the current architecture, uses the persisted schema the repo already has, and supports a wide range of clients without binding the backend to one frontend design.
