# emwin-cli Server Mode API

Version: 2.0
Last Updated: 2026-03-19
Status: Authoritative for `emwin-cli server`

## Overview

`emwin-cli server` exposes a versioned HTTP and SSE API.

Contract rules:

- `GET /` serves Swagger UI
- `GET /openapi.json` serves the generated OpenAPI document
- all live endpoints are under `/v1/live`
- all archive endpoints are under `/v1/archive`
- no unversioned compatibility routes are provided
- when `--openapi-auth-token` or `EMWIN_OPENAPI_AUTH_TOKEN` is set, `/v1/live/*` and `/v1/archive/*` require `Authorization: Bearer <token>`
- `GET /`, `GET /openapi.json`, and Swagger UI asset routes remain public when auth is enabled

## Endpoints

### Documentation

- `GET /`
- `GET /openapi.json`

### Live

- `GET /v1/live/events`
- `GET /v1/live/incident-events`
- `GET /v1/live/files`
- `GET /v1/live/files/{*filename}`
- `GET /v1/live/incidents`
- `GET /v1/live/incidents/{office}/{phenomena}/{significance}/{etn}`
- `GET /v1/live/incidents/{office}/{phenomena}/{significance}/{etn}/products`
- `GET /v1/live/health`
- `GET /v1/live/metrics`

### Archive

- `GET /v1/archive/issues`
- `GET /v1/archive/issues/{issue_id}`
- `GET /v1/archive/products/{product_id}`
- `GET /v1/archive/products/{product_id}/raw`

## Archive and Incident Availability

- `/v1/live/incidents`, `/v1/live/incident-events`, and `/v1/archive/products/*` require `--persist-database-url`
- `/v1/archive/issues*` also requires `--persist-database-url`
- those endpoints return `503` when Postgres-backed archive metadata is not configured
- `/v1/live/files/*` serves only the in-memory retained payload cache
- persisted S3 or filesystem blobs remain archive storage and are not proxied through `/v1/live/files/*`

## SSE Streams

### `GET /v1/live/events`

This is the live feed SSE stream.

Behavior:

- `id` is a monotonically increasing event id
- `event` is the event name such as `connected`, `file_complete`, `telemetry`, or `error`
- `data` is a JSON object

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
- `min_size`, `max_size`

Example:

`GET /v1/live/events?event=file_complete&lat=41.42&lon=-96.17&distance_miles=15`

### `GET /v1/live/incident-events`

This is the persisted incident projection SSE stream.

Behavior:

- `event` is always `incident_change`
- `data` is a JSON object with `action`, `trigger`, and `incident`

Supported query parameters:

- `action`
- `office`
- `phenomena`
- `significance`
- `status`
- `etn`

Example:

`GET /v1/live/incident-events?action=created,updated&office=KOAX&phenomena=FF&significance=W&etn=2001&status=active`

## Response Notes

- completed-file payloads include `download_url` values under `/v1/live/files/...`
- incident payloads include `detail_url` and `products_url` values under `/v1/live/incidents/...`
- incident and archive payloads include product links under `/v1/archive/products/...`
- archive issue payloads include `detail_url` and `product_url`
- `/v1/live/metrics` returns a flat telemetry object; when persistence is enabled it also includes `persistence_*` queue fields

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
- `--openapi-auth-token <TOKEN>`
- `--cors-origin "*"|"https://..."`

## Source of Truth

The generated OpenAPI document at `/openapi.json` is the machine-readable contract.
This document is the human-readable summary. If they disagree, fix the code and regenerate the
OpenAPI surface rather than adding parallel route documentation.
