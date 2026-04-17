# emwin-cli

CLI application for EMWIN live server workflows. Built on `emwin-live`, `emwin-api`, `emwin-protocol`, and `emwin-db`.

## Commands

- `query`
  - Archive query command.
  - Connects directly to persisted Postgres metadata and reads archived payloads through stored object-store locations.
  - Supports archived products, issues, features, and aggregate reads.
- `server`
  - Live command.
  - Starts `emwin-live` for headless ingest/runtime coordination and serves it through `emwin-api`.
  - Optional `--output-dir <OBJECT_STORE_ROOT_URI>` persists completed payloads asynchronously.

## Output formats

- `query` emits JSON payloads to `stdout` for structured archive reads. `query product-raw` writes bytes to `stdout` only with `--stdout`, otherwise it requires `--output <PATH>`.
- `server` emits structured `tracing` diagnostics to `stderr` and serves retained payloads over HTTP.

Contract:

- command payloads are written to `stdout`
- diagnostics and warnings are written to `stderr`
- diagnostics use canonical `tracing-subscriber` formatting (configure via `RUST_LOG`; ANSI style via `RUST_LOG_STYLE=auto|always|never`)
- startup diagnostics include the crate version and selected subcommand

## Live mode options

Common live ingest options:

- `--receiver <qbt|wxwire>` (optional, default `qbt`)
- `--username <EMAIL>` (required)
- `--password <PASSWORD>` (required when `--receiver wxwire`)
- `--server <host:port>` (optional, repeatable or comma-delimited; pins QBT to that list)
- `--server-list-path <PATH>` (optional persisted automatic QBT server list path; rejected when `--server` is set)
- `--post-process-archives <true|false>` (default `true`; extracts the first entry from completed `.ZIP` and `.ZIS` products before parsing and delivery)
- `--persist-queue-capacity <N>` (default `1024`; bounded async persistence queue, evicts oldest queued item when full)
- `--persist-database-url <URL>` (optional; writes normalized metadata into Postgres/PostGIS while still storing payload blobs under `--output-dir`)
- `--max-db-connections <N>` (default `10`; Postgres pool size used for archive reads and persistence writes)

Live command: `server`

- `--bind <ADDR>` (default `127.0.0.1:8080`)
- `--cors-origin <ORIGIN>`
- `--max-clients <N>`
- `--stats-interval-secs <SECONDS>`
- `--file-retention-secs <SECONDS>`
- `--max-retained-files <N>`
- `--quiet`
- `--openapi-auth-token <TOKEN>` (optional; requires `Authorization: Bearer <token>` on `/v1/*`)
- `--output-dir <OBJECT_STORE_ROOT_URI>` (optional; writes each matching completed file plus a `.JSON` metadata sidecar under canonical archival paths)

Persistence behavior when `--output-dir` is set:

- each persisted product writes the payload file and a sibling `.JSON` metadata sidecar under a canonical archival path such as `qbt/2026/03/16/BOX/nws_text_product/20260316T021530Z-4f2c9d91-AFDBOX.TXT`
- persistence runs in a background task so live ingest does not wait on filesystem or object-store I/O
- when `--persist-database-url` is set, the background task also upserts normalized product metadata and spatial child rows into Postgres/PostGIS
- when `--persist-database-url` is set, startup runs one incident cleanup pass and the server retries cleanup every 5 minutes to mark `active` incidents `expired` after their `end_utc` passes
- Postgres metadata failures do not roll back payload or sidecar files already written under `--output-dir`
- Postgres outages no longer abort server startup; the server stays online and background persistence retries with backoff until the database is reachable again
- local filesystem persistence must use `file://` URIs such as `file:///tmp/emwin`; plain paths are rejected
- the configured object-store target must already exist; the server no longer tries to create buckets or containers
- transient filesystem write failures, including disk-full conditions, and transient object-store request failures are retried in the background with throttled warnings so live ingest and connected clients keep running
- persistence failure logs identify the failing backend and target, such as filesystem root, object-store URI root, or database target
- if the persistence queue fills, the oldest queued item is evicted so the newest product can still be accepted
- `.ZIP` and `.ZIS` products are extracted before parsing, filtering, and persistence by default; the extracted entry filename replaces the archive filename
- corrupt archives are logged as `Corrupt Zip File Received` and dropped when post-processing is enabled
- sidecar names replace the original extension within the canonical archival path, for example `qbt/.../20260316T021530Z-4f2c9d91-AFDBOX.TXT` -> `qbt/.../20260316T021530Z-4f2c9d91-AFDBOX.JSON`
- ZIP/ZIS archive entry directories are flattened for persisted storage keys; the original delivered filename, including nested archive paths, remains visible in metadata and `/v1/files`
- `/v1/files/*` continues to serve only the in-memory retained payload cache; persisted S3 objects are archival storage and are not proxied by the CLI
- when `--persist-database-url` is configured, the server also exposes `/v1/incidents`, `/v1/products/*`, `/v1/issues/*`, and `/v1/streams/incidents`
- `/v1/incidents`, `/v1/products/*`, and `/v1/issues/*` return `503` when Postgres-backed archive metadata is not configured

If `--server` is omitted, built-in default EMWIN endpoints are used and automatic server-list updates remain enabled.
`--server` and `--server-list-path` are only supported for `--receiver qbt`.
When `--server` is provided for QBT live mode, the CLI pins that explicit server set and disables
automatic server-list load/save/update behavior.

## Environment variables and `.env`

The CLI loads `.env` from the current working directory before parsing arguments.
Precedence is:

- CLI args
- process environment
- `.env`
- built-in defaults

Supported environment variables include:

- `EMWIN_DATABASE_URL`
- `EMWIN_RECEIVER`
- `EMWIN_USERNAME`
- `EMWIN_PASSWORD`
- `EMWIN_SERVER`
- `EMWIN_SERVER_LIST_PATH`
- `EMWIN_BIND`
- `EMWIN_CORS_ORIGIN`
- `EMWIN_MAX_CLIENTS`
- `EMWIN_STATS_INTERVAL_SECS`
- `EMWIN_FILE_RETENTION_SECS`
- `EMWIN_MAX_RETAINED_FILES`
- `EMWIN_QUIET`
- `EMWIN_POST_PROCESS_ARCHIVES`
- `EMWIN_OUTPUT_DIR`
- `EMWIN_PERSIST_QUEUE_CAPACITY`
- `EMWIN_PERSIST_DATABASE_URL`
- `EMWIN_MAX_DB_CONNECTIONS`
- `EMWIN_OPENAPI_AUTH_TOKEN`
- `EMWIN_APPRISE_API_URL`
- `EMWIN_ALERT_SOURCE_BATCH_SIZE`
- `EMWIN_ALERT_DELIVERY_BATCH_SIZE`
- `EMWIN_ALERT_IDLE_POLL_SECS`
- `EMWIN_ALERT_SOURCE_CLAIM_LEASE_SECS`
- `EMWIN_ALERT_DELIVERY_CLAIM_LEASE_SECS`
- `EMWIN_ALERT_HTTP_TIMEOUT_SECS`
- `EMWIN_ALERT_MAX_DELIVERY_ATTEMPTS`

Filters are intentionally not configurable through environment variables.

When `EMWIN_OUTPUT_DIR` uses an object-store URL such as `s3://bucket/prefix` or `https://example.com/path`, object-store configuration stays env-driven for that backend. S3-compatible targets still use `AWS_ENDPOINT_URL`, `AWS_REGION` or `AWS_DEFAULT_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`, and `AWS_PROFILE`.

## Examples

Archive query mode:

```bash
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin incidents --office KOAX
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin incident KOAX FF W 2001
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin incident-products KOAX FF W 2001 --limit 50
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin products --office KOAX --artifact-kind nws_text_product --min-lat 41 --max-lat 42 --min-lon -97 --max-lon -95 --limit 25
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin product 42
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin features --kind polygon --artifact-kind nws_text_product --limit 25
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin features-geojson --kind search_point --limit 100
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin aggregate-facets office --artifact-kind nws_text_product --limit 20
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin aggregate-timeseries product_count --start 2025-03-05T12:00:00Z --end 2025-03-05T15:00:00Z --bucket hour
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin aggregate-cells product_count --precision 5 --limit 100
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin issues --product-id 42 --kind text_product_parse
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin issue 7
cargo run -p emwin-cli -- query --database-url postgres://localhost/emwin product-raw 42 --output ./product.bin
```

Live mode:

```bash
cargo run -p emwin-cli -- server --username you@example.com --bind 127.0.0.1:8080
cargo run -p emwin-cli -- server --username you@example.com --output-dir file:///tmp/emwin
cargo run -p emwin-cli -- server --username you@example.com --output-dir file:///tmp/emwin --persist-database-url postgres://localhost/emwin
cargo run -p emwin-cli -- server --username you@example.com --output-dir file:///tmp/emwin --persist-database-url postgres://localhost/emwin --max-db-connections 16
cargo run -p emwin-cli -- server --username you@example.com --output-dir s3://my-bucket/emwin --persist-database-url postgres://localhost/emwin
cargo run -p emwin-cli -- server --receiver wxwire --username you@example.com --password your-pass
```

## Server filter examples

When running `server`, `/v1/streams/products` supports parsed-location filters:

- `/v1/streams/products?event=product_available&lat=41.42&lon=-96.17`
- `/v1/streams/products?event=product_available&lat=41.42&lon=-96.17&distance_miles=15`
- `/v1/streams/products?event=product_available&min_lat=41.0&max_lat=42.0&min_lon=-97.0&max_lon=-95.0`

`lat` and `lon` must be provided together. `distance_miles` is optional and defaults to `5.0`.
Matches use parsed `LAT...LON` polygons for containment and parsed `TIME...MOT...LOC`, `UGC`,
and `HVTEC` coordinates for radius checks.
Bounding boxes require all four of `min_lat`, `max_lat`, `min_lon`, and `max_lon`, and match any
parsed polygon, motion path, or parsed point that intersects the box.
Archive boolean filters accept only `true`, `false`, `1`, or `0`; any other non-empty value fails
request validation instead of being ignored.
Archive size ranges also validate at request-build time; `min_size` must be less than or equal to
`max_size`.
Archive HTTP resource endpoints use the same flat query grammar as the README examples. Nested
forms such as `filters.office=...` and `filters[office]=...` are rejected with `400`.
`/v1/features`, `/v1/features/geojson`, and `/v1/aggregates/cells` apply spatial filters to each
returned geometry or counted point, not just to product admission.

## Resource endpoints

- `GET /` serves Swagger UI and `GET /openapi.json` serves the generated OpenAPI document
- when `--openapi-auth-token` or `EMWIN_OPENAPI_AUTH_TOKEN` is set, all `/v1/*` requests require `Authorization: Bearer <token>`
- `/openapi.json` advertises bearer auth only when `--openapi-auth-token` or `EMWIN_OPENAPI_AUTH_TOKEN` is set
- `GET /`, `GET /openapi.json`, and Swagger UI asset routes remain public when auth is enabled
- `/v1/health` returns `status: "degraded"` and includes archive health details when archive persistence is configured but archive access is failing
- `/v1/streams/incidents` streams `incident_change` SSE payloads for persisted incident projection changes; supported filters are `action`, `office`, `phenomena`, `significance`, `status`, and `etn`
- `/v1/streams/products` streams `product_available` SSE payloads for completed products; supported filters match the parsed product metadata and spatial filter set documented below
- Both SSE endpoints are incremental streams, not durable replay logs. Clients should fetch an initial snapshot from the resource endpoints, then attach the stream.
- `Last-Event-ID` is best-effort for short reconnect gaps only. If the server emits a lag warning or the client detects a gap, the client must resync from the resource endpoints.
- `/v1/incidents` lists live incident projection rows from persisted Postgres metadata
- `/v1/incidents/{office}/{phenomena}/{significance}/{etn}` fetches one incident plus related product links
- `/v1/incidents/{office}/{phenomena}/{significance}/{etn}/products` returns the archived product timeline for one incident
- `/v1/products` lists archived products with cursor pagination and the shared product filter grammar
- archive product/feature/aggregate filters include `artifact_kind` alongside the existing source, family, and container metadata filters
- `/v1/products/{product_id}` returns persisted product detail including `product_json`
- `/v1/products/{product_id}/raw` proxies archived payload bytes for one product
- `/v1/features` lists archived spatial features with cursor pagination and GeoJSON geometry per item
- `/v1/features/geojson` emits a bounded GeoJSON `FeatureCollection` view over the same archived feature set
- `/v1/aggregates/facets` returns uncursored facet buckets for supported archive dimensions
- `/v1/aggregates/timeseries` returns uncursored time buckets for `product_count`, `issue_count`, or `incident_count`
- `/v1/aggregates/cells` returns uncursored geohash cell buckets for `product_count`, counting each product once per intersected cell across persisted polygons, paths, and representative points
- `/v1/issues` lists archived issue rows with optional exact filters `product_id`, `kind`, and `code`
- `/v1/issues/{issue_id}` fetches one archived issue row

The `query` command mirrors those archive read capabilities locally, including features and aggregate responses. Postgres remains the query backend; remote payload reads are resolved through `object_store` when an archived payload location points at a supported object-store URL.

Authenticated example:

```bash
curl -H 'Authorization: Bearer secret-token' http://127.0.0.1:8080/v1/health
```

Cross-origin browser clients can combine `--cors-origin` with `--openapi-auth-token`; CORS preflights now allow the `Authorization` request header.

## Text product parsing

The CLI leverages `emwin-parser` to parse WMO/AFOS formatted text products:

**Automatic parsing:**
- WMO header extraction (TTAAII, CCCC, DDHHMM, BBB indicators)
- AFOS PIL (Product Identifier Line) parsing
- Text conditioning (SOH/ETX stripping, null byte removal)
- PIL lookup with product type descriptions

**Supported products:**
- Area Forecast Discussions (AFD)
- Severe Thunderstorm Warnings (SVR)
- Tornado Warnings (TOR)
- Flash Flood Warnings (FFW)
- Terminal Aerodrome Forecasts (TAF/FTM)
- And hundreds more meteorological product types

**Parsing handles:**
- BBB indicator classification (Amendment, Correction, Delayed Repeat)
- Missing LDM sequence numbers
- Various text encoding issues
- Correction and amendment flags in WMO headers

## Development checks

From workspace root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p emwin-cli
```

For faster command-specific compile checks, disable default command features and enable only the command family under active development:

```bash
cargo check -p emwin-cli --no-default-features --features query
cargo check -p emwin-cli --no-default-features --features relay
cargo check -p emwin-cli --no-default-features --features server
cargo check -p emwin-cli --no-default-features --features alert-worker
```
