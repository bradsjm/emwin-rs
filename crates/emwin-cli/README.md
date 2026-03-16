# emwin-cli

CLI application for EMWIN live server workflows. Built on `emwin-protocol` and `emwin-parser`.

## Commands

- `server`
  - Live command.
  - Connects to EMWIN servers, exposes HTTP and SSE endpoints, and retains recent files for `/files` downloads.
  - Optional `--output-dir <PATH|s3://bucket[/prefix]>` persists completed payloads asynchronously.

## Output formats

- `server` emits structured `tracing` diagnostics to `stderr` and serves retained payloads over HTTP.

Contract:

- command payloads are written to `stdout`
- diagnostics and warnings are written to `stderr`
- diagnostics use canonical `tracing-subscriber` formatting (configure via `RUST_LOG`; ANSI style via `RUST_LOG_STYLE=auto|always|never`)

## Live mode options

Common live ingest options:

- `--receiver <qbt|wxwire>` (optional, default `qbt`)
- `--username <EMAIL>` (required)
- `--password <PASSWORD>` (required when `--receiver wxwire`)
- `--server <host:port>` (optional, repeatable or comma-delimited)
- `--server-list-path <PATH>` (optional persisted server list path)
- `--post-process-archives <true|false>` (default `true`; extracts the first entry from completed `.ZIP` and `.ZIS` products before parsing and delivery)
- `--persist-queue-capacity <N>` (default `1024`; bounded async persistence queue, evicts oldest queued item when full)
- `--persist-database-url <URL>` (optional; writes normalized metadata into Postgres/PostGIS while still storing payload blobs under `--output-dir`)

Live command: `server`

- `--bind <ADDR>` (default `127.0.0.1:8080`)
- `--cors-origin <ORIGIN>`
- `--max-clients <N>`
- `--stats-interval-secs <SECONDS>`
- `--file-retention-secs <SECONDS>`
- `--max-retained-files <N>`
- `--quiet`
- `--output-dir <PATH|s3://bucket[/prefix]>` (optional; writes each matching completed file plus a `.JSON` metadata sidecar under canonical archival paths)

Persistence behavior when `--output-dir` is set:

- each persisted product writes the payload file and a sibling `.JSON` metadata sidecar under a canonical archival path such as `qbt/2026/03/16/BOX/nws_text_product/20260316T021530Z-4f2c9d91-AFDBOX.TXT`
- persistence runs in a background task so live ingest does not wait on filesystem or S3 I/O
- when `--persist-database-url` is set, the background task also upserts normalized product metadata and spatial child rows into Postgres/PostGIS
- when `--persist-database-url` is set, startup runs one incident cleanup pass and the server retries cleanup every 5 minutes to mark `active` incidents `expired` after their `end_utc` passes
- Postgres metadata failures do not roll back payload or sidecar files already written under `--output-dir`
- Postgres outages no longer abort server startup; the server stays online and background persistence retries with backoff until the database is reachable again
- S3 persistence attempts to auto-create the target bucket when missing; if S3 is unavailable or bucket creation/checks fail transiently, the server stays online and background persistence retries with backoff
- transient filesystem write failures, including disk-full conditions, and transient S3 request failures are retried in the background with throttled warnings so live ingest and connected clients keep running
- if the persistence queue fills, the oldest queued item is evicted so the newest product can still be accepted
- `.ZIP` and `.ZIS` products are extracted before parsing, filtering, and persistence by default; the extracted entry filename replaces the archive filename
- corrupt archives are logged as `Corrupt Zip File Received` and dropped when post-processing is enabled
- sidecar names replace the original extension within the canonical archival path, for example `qbt/.../20260316T021530Z-4f2c9d91-AFDBOX.TXT` -> `qbt/.../20260316T021530Z-4f2c9d91-AFDBOX.JSON`
- ZIP/ZIS archive entry directories are flattened for persisted storage keys; the original delivered filename, including nested archive paths, remains visible in metadata and `/files`
- `/files/*` continues to serve only the in-memory retained payload cache; persisted S3 objects are archival storage and are not proxied by the CLI

If `--server` is omitted, built-in default endpoints are used.
`--server` and `--server-list-path` are only supported for `--receiver qbt`.
When `--server` is provided for QBT live mode, the CLI now pins that explicit server set instead
of later replacing it with server-list updates.

## Environment variables and `.env`

The CLI loads `.env` from the current working directory before parsing arguments.
Precedence is:

- CLI args
- process environment
- `.env`
- built-in defaults

Supported environment variables include:

- `EMWIN_TEXT_PREVIEW_CHARS`
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

Filters are intentionally not configurable through environment variables.

When `EMWIN_OUTPUT_DIR` uses `s3://bucket[/prefix]`, object-store configuration stays env-driven: set `AWS_ENDPOINT_URL` for MinIO or another custom S3-compatible endpoint, `AWS_REGION` or `AWS_DEFAULT_REGION` for region selection, and `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`, or `AWS_PROFILE` for credentials. Persisted metadata still stores canonical `s3://bucket/key` references rather than presigned URLs.

## Examples

Live mode:

```bash
cargo run -p emwin-cli -- server --username you@example.com --bind 127.0.0.1:8080
cargo run -p emwin-cli -- server --username you@example.com --output-dir ./out
cargo run -p emwin-cli -- server --username you@example.com --output-dir ./out --persist-database-url postgres://localhost/emwin
cargo run -p emwin-cli -- server --username you@example.com --output-dir s3://my-bucket/emwin --persist-database-url postgres://localhost/emwin
cargo run -p emwin-cli -- server --receiver wxwire --username you@example.com --password your-pass
```

## Server filter examples

When running `server`, `/events` supports parsed-location filters:

- `/events?event=file_complete&lat=41.42&lon=-96.17`
- `/events?event=file_complete&lat=41.42&lon=-96.17&distance_miles=15`

`lat` and `lon` must be provided together. `distance_miles` is optional and defaults to `5.0`.
Matches use parsed `LAT...LON` polygons for containment and parsed `TIME...MOT...LOC`, `UGC`,
and `HVTEC` coordinates for radius checks.

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
