# Backend Requirements for Interactive Severe Weather Frontend

## Purpose

This document defines the backend requirements for a future frontend built on top of `emwin-rs`.
It distinguishes between:

- capabilities already present in the current API
- backend additions required for a high-quality interactive product

The target outcome is a frontend that can show:

- the national and regional severe-weather picture
- live incident creation and updates
- hotspot concentration and trend changes
- drill-down from map to incident to product to raw payload

## Confirmed Current Backend Surface

The current `emwin-cli server` API already exposes:

- `GET /v1/live/events`
  - live SSE feed
  - supports rich filtering over event name, filename, product metadata, header metadata, issue metadata, hazard/body presence, UGC geography, VTEC fields, HVTEC fields, wind/hail thresholds, point-radius location, and size
- `GET /v1/live/incident-events`
  - SSE feed for persisted incident projection changes
- `GET /v1/live/incidents`
  - paginated incident list
- `GET /v1/live/incidents/{office}/{phenomena}/{significance}/{etn}`
  - incident detail
- `GET /v1/live/incidents/{office}/{phenomena}/{significance}/{etn}/products`
  - archived product timeline for one incident
- `GET /v1/archive/products/{product_id}`
  - archived product detail, including `product_json`
- `GET /v1/archive/products/{product_id}/raw`
  - raw payload bytes
- `GET /v1/archive/issues`
  - archived issue list
- `GET /v1/archive/issues/{issue_id}`
  - archived issue detail
- `GET /v1/live/files`
  - retained live completed-file list
- `GET /v1/live/files/{*filename}`
  - retained live file download
- `GET /v1/live/health`
- `GET /v1/live/metrics`

## Confirmed Current Data Model

### Product Summary

The live SSE `file_complete` payload and retained-file payload already expose a stable product summary model with:

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

The archived product detail already exposes:

- all summary fields
- full parsed body when present
- full specialized artifact when present
- parse/QC issues
- payload and metadata storage locations
- raw payload download URL

### Incident Model

The incident projection already exposes:

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

### Hazard and Geometry Semantics Already Available

The parser already supports, when present in source products:

- VTEC event segments
- UGC county, zone, fire-zone, and marine-zone geography
- HVTEC data
- `LAT...LON` polygons
- `TIME...MOT...LOC` tracks and points
- wind/hail threat tags and numeric thresholds
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

## Backend Requirements for Frontend MVP

The current API is enough for an MVP if the frontend is scoped correctly.

### Required Existing Endpoints

- Use `/v1/live/events` as the primary live data bus for map updates and stream views.
- Use `/v1/live/incident-events` for incident lifecycle updates.
- Use `/v1/live/incidents` for initial active incident load and filtered incident lists.
- Use `/v1/live/incidents/{...}/products` for incident timeline drill-down.
- Use `/v1/archive/products/{product_id}` for product detail inspection.
- Use `/v1/archive/issues` and `/v1/archive/issues/{issue_id}` for data quality inspection.

### Required Server Configuration

- `--persist-database-url` must be enabled.
  - Without it, incidents, incident events, archived products, and archived issues are unavailable.
- `--openapi-auth-token` should be enabled in deployed environments.
- `--cors-origin` must be configured for browser clients.

### Required Frontend Consumption Rules

- Use summary payloads for list/map rendering.
- Fetch archive detail lazily only when the user drills into a specific product.
- Treat parser issues as first-class data, not as hidden logs.
- Treat geometry as optional.
  - Some products have polygons.
  - Some only have points or keyed geography.
  - Some outlook products may degrade to tokenized locations or non-geometric areal-outline mode.

## Backend Additions Required for Full Product Quality

The current API is not sufficient for the complete target product.

### 1. Aggregated Hotspot Endpoint

Required because the current API is event- and incident-oriented, not hotspot-oriented.

Proposed capability:

- aggregate recent products and incidents into map cells or clusters
- return counts by:
  - active incidents
  - new incidents in time window
  - updated incidents in time window
  - LSR count
  - wind/hail exceedance count
  - warning count by significance
  - watch count
- support:
  - geographic bounds
  - zoom/grid size
  - rolling time window
  - hazard/family filters

Current status from source:

- not present in the current API

### 2. Trend Summary Endpoint

Required because the frontend should not compute all national trend summaries from raw SSE alone.

Proposed capability:

- rolling national/regional summaries for:
  - incident creation rate
  - incident update rate
  - active incidents by VTEC phenomena/significance
  - LSR density over time
  - severe wind/hail threshold counts over time
  - outlook area counts by category

Current status from source:

- not present in the current API

### 3. General Archive Product Search Endpoint

Required for historical exploration outside one incident timeline.

Proposed capability:

- paginated archive product search by:
  - time range
  - family
  - artifact kind
  - office
  - state
  - UGC code
  - VTEC fields
  - HVTEC fields
  - issue code
  - wind/hail thresholds
  - point-radius or bounding box

Current status from source:

- not confirmed from source
- current archive HTTP surface is incident-first plus direct product lookup by id

### 4. Derived Map Layer Endpoint

Optional for MVP, required for a polished product.

Proposed capability:

- return frontend-ready map layers for:
  - active warning polygons
  - watch polygons
  - SPC outlook polygons
  - ERO outlook areas
  - MCD polygons
  - LSR points

Reasoning:

- this reduces frontend data-massaging and ensures consistent geometry normalization

Current status from source:

- not present in the current API

## Non-Functional Requirements

### Availability

- live ingest must remain usable if archive reads are temporarily unavailable
- frontend must handle `503` for archive-backed endpoints explicitly

### Authentication

- bearer auth required in deployed environments
- frontend must support authenticated SSE and JSON requests

### Pagination

- incident and issue/product list views must preserve cursor-based pagination
- frontend state should keep cursors stable during drill-down

### Resume and Recovery

- SSE consumers should use `Last-Event-ID`
- frontend should surface lag/drop conditions reported by SSE warning frames

### Data Quality

- backend must preserve issue rows and issue codes
- new derived analytics endpoints must carry quality metadata where aggregation is partial or degraded

### Performance

- list and map endpoints must be usable at national scope
- do not require the frontend to fetch full product detail for every visible object

## Delivery Phases

### Phase 1: Build Against Existing API

Allowed scope:

- live map
- live incident list
- incident drill-down
- product drill-down
- issue visibility
- basic client-side trend summaries from currently loaded data

### Phase 2: Backend Expansion

Required before calling the product complete:

- hotspot endpoint
- trend endpoint
- general archive product search
- optionally derived map layers

## Approval

Approve backend work for the frontend initiative with this constraint:

- the current API is sufficient for a strong incident-first MVP
- the current API is not sufficient for full hotspot and historical trend exploration without additional backend work
