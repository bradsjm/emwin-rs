# emwin-cli Server Mode API

Version: 3.0
Last Updated: 2026-04-17
Status: Human-readable summary for `emwin-cli server`

## Overview

`emwin-cli server` exposes a versioned HTTP and SSE API under a flat `/v1/*` namespace.

Contract rules:

- `GET /` serves Swagger UI.
- `GET /openapi.json` serves the generated OpenAPI document.
- There are no `/v1/live/*` or `/v1/archive/*` route prefixes.
- No unversioned compatibility routes are provided.
- When `--openapi-auth-token` or `EMWIN_OPENAPI_AUTH_TOKEN` is set, all `/v1/*` routes require `Authorization: Bearer <token>`.
- `GET /`, `GET /openapi.json`, and Swagger UI asset routes remain public when auth is enabled.

## Endpoints

### Documentation

- `GET /`
- `GET /openapi.json`

### Archive and Incident Resources

- `GET /v1/incidents`
- `GET /v1/incidents/{office}/{phenomena}/{significance}/{etn}`
- `GET /v1/incidents/{office}/{phenomena}/{significance}/{etn}/products`
- `GET /v1/products`
- `GET /v1/products/{product_id}`
- `GET /v1/products/{product_id}/raw`
- `GET /v1/features`
- `GET /v1/features/geojson`
- `GET /v1/issues`
- `GET /v1/issues/{issue_id}`

### Archive Aggregates

- `GET /v1/aggregates/facets`
- `GET /v1/aggregates/timeseries`
- `GET /v1/aggregates/cells`

### Streams

- `GET /v1/streams/products`
- `GET /v1/streams/incidents`

### Alerting

- `GET|POST /v1/alerting/contact-points`
- `GET|PATCH|DELETE /v1/alerting/contact-points/{id}`
- `POST /v1/alerting/contact-points/{id}/test`
- `GET|POST /v1/alerting/rules`
- `POST /v1/alerting/rules/simulate`
- `GET|PATCH|DELETE /v1/alerting/rules/{id}`
- `POST /v1/alerting/rules/{id}/simulate`
- `GET /v1/alerting/rules/{id}/events`
- `GET /v1/alerting/deliveries`
- `GET|POST /v1/alerting/silences`
- `DELETE /v1/alerting/silences/{id}`

### Operational

- `GET /v1/files`
- `GET /v1/files/{*filename}`
- `GET /v1/health`
- `GET /v1/metrics`

## Persistence-Backed Availability

- `/v1/incidents`, `/v1/incidents/{...}`, `/v1/incidents/{...}/products`, `/v1/products`, `/v1/products/{product_id}`, `/v1/products/{product_id}/raw`, `/v1/issues`, `/v1/issues/{issue_id}`, `/v1/features`, `/v1/features/geojson`, `/v1/aggregates/*`, and `/v1/streams/incidents` require `--persist-database-url`.
- `/v1/alerting/*` also requires `--persist-database-url`; the alerting control plane is backed by the same Postgres deployment.
- `POST /v1/alerting/contact-points/{id}/test` can send Apprise tests only when `--alerting-apprise-api-url` or `EMWIN_APPRISE_API_URL` is configured.
- Archive-backed routes return `503` when Postgres-backed archive metadata is not configured.
- `/v1/files/*` serves only the in-memory retained payload cache.
- Persisted S3 or filesystem blobs remain archive storage and are not proxied through `/v1/files/*`.
- `/v1/health` returns `status: "degraded"` and includes archive health details when archive persistence is configured but archive access is failing.

## Resource and Query Notes

- `/v1/incidents` exposes the mutable incident projection from the `incidents` table.
- `/v1/incidents/{office}/{phenomena}/{significance}/{etn}/products` returns the archived product timeline for one incident.
- `/v1/products` lists archived products with cursor pagination and the shared archive filter grammar.
- `/v1/products/{product_id}` returns persisted product detail, including `product_json`.
- `/v1/products/{product_id}/raw` returns persisted archived payload bytes.
- `/v1/features` lists archived spatial features with cursor pagination.
- `/v1/features/geojson` emits a bounded GeoJSON `FeatureCollection` over archived spatial features.
- `/v1/aggregates/facets` returns uncursored facet buckets.
- `/v1/aggregates/timeseries` returns uncursored time buckets.
- `/v1/aggregates/cells` returns uncursored geohash cell buckets for `product_count`.
- `/v1/issues` lists archived issue rows.
- `/v1/issues/{issue_id}` fetches one archived issue row.
- Archive resource endpoints accept flat query parameters such as `office=MKX`, `lat=41.42`, and `source_timestamp_after=1775586000`.
- Nested query forms such as `filters.office=...` and `filters[office]=...` are rejected with `400`.
- Archive boolean filters accept `true`, `false`, `1`, or `0`; other non-empty values return `400`.
- Archive size ranges where `min_size > max_size` return `400`.
- `/v1/features`, `/v1/features/geojson`, and `/v1/aggregates/cells` apply spatial filters to each returned geometry or counted feature contribution, not just to product admission.
- `/v1/aggregates/cells` requires a complete bbox (`min_lat`, `max_lat`, `min_lon`, `max_lon`) and caps precision at `6`.

## SSE Streams

### `GET /v1/streams/products`

This is the completed-product SSE stream.

Behavior:

- `id` is a monotonically increasing event id from the live runtime event stream.
- `event` is `product_available` for completed products.
- `data` is a JSON object containing completed file metadata, parsed product summary, and `download_url`.
- Clients should fetch an initial snapshot from resource endpoints before attaching the stream.
- `Last-Event-ID` is best-effort for short reconnect gaps only.
- If the server emits a lag warning or the client detects a gap, the client must resync from resource endpoints.

Selected query parameters:

- `event`
- `filename`
- `source`, `pil`, `family`, `container`, `wmo_prefix`
- `office`, `office_city`, `office_state`
- `cccc`, `ttaaii`, `afos`, `bbb`, `bbb_kind`
- `has_issues`, `issue_kind`, `issue_code`
- `has_vtec`, `has_ugc`, `has_hvtec`, `has_latlon`, `has_time_mot_loc`, `has_wind_hail`
- `state`, `county`, `zone`, `fire_zone`, `marine_zone`
- `vtec_phenomena`, `vtec_significance`, `vtec_action`, `vtec_office`, `etn`
- `hvtec_nwslid`, `hvtec_severity`, `hvtec_cause`, `hvtec_record`
- `wind_hail_kind`, `min_wind_mph`, `min_hail_inches`
- `lat`, `lon`, `distance_miles`
- `min_lat`, `max_lat`, `min_lon`, `max_lon`
- `min_size`, `max_size`

Examples:

- `GET /v1/streams/products?event=product_available&lat=41.42&lon=-96.17&distance_miles=15`
- `GET /v1/streams/products?event=product_available&min_lat=41.0&max_lat=42.0&min_lon=-97.0&max_lon=-95.0`
- `GET /v1/streams/products?event=product_available&has_wind_hail=true&min_wind_mph=50&min_hail_inches=1.00`

### `GET /v1/streams/incidents`

This is the persisted incident projection SSE stream.

Behavior:

- `event` is always `incident_change`.
- `data` is a JSON object with `action`, `trigger`, and `incident`.
- Incident actions are `created` and `updated`.
- Clients should fetch an initial snapshot from `/v1/incidents`, then attach the stream.
- `Last-Event-ID` is best-effort for short reconnect gaps only.
- Lag warnings require a full resync from incident resource endpoints.

Supported query parameters:

- `action`
- `office`
- `phenomena`
- `significance`
- `status`
- `etn`

Example:

- `GET /v1/streams/incidents?action=created,updated&office=KOAX&phenomena=FF&significance=W&etn=2001&status=active`

## Alerting Notes

- Contact-point routes manage delivery targets.
- Rule routes manage alert rules and support ad hoc and persisted rule simulation.
- `GET /v1/alerting/rules/{id}/events` returns alert rule event audit rows.
- `GET /v1/alerting/deliveries` returns delivery audit rows.
- Silence routes manage alert suppression windows.
- Incident alert simulations use retained `alerting.source_events`; requests earlier than the first retained incident source event are rejected instead of fabricating history.

## Response Notes

- Completed-file payloads include `download_url` values under `/v1/files/...`.
- Incident payloads include `detail_url`, `products_url`, and product links under `/v1/products/...`.
- Archive product payloads include `detail_url` and `raw_url`.
- Archive issue payloads include `detail_url` and `product_url`.
- `/v1/metrics` returns a flat telemetry object; when persistence is enabled it also includes `persistence_*` queue fields.
- Aggregate responses include completeness metadata: `partial`, `approximate`, and `reason`.

## Start Server Mode

```bash
cargo run -p emwin-cli -- server --username you@example.com --bind 127.0.0.1:8080
```

Common options:

- `--bind <ADDR:PORT>`
- `--max-clients <N>`
- `--stats-interval-secs <N>`
- `--file-retention-secs <N>`
- `--max-retained-files <N>`
- `--persist-database-url <URL>`
- `--max-db-connections <N>`
- `--openapi-auth-token <TOKEN>`
- `--alerting-apprise-api-url <URL>`
- `--cors-origin "*"|"https://..."`

## Source of Truth

The generated OpenAPI document at `/openapi.json` is the machine-readable contract.
This document is the human-readable summary. If they disagree, fix the code and regenerate the OpenAPI surface rather than adding parallel route documentation.
