# emwin-rs

Rust monorepo for EMWIN protocol decoding, client runtime, and CLI tooling.

## Install

Install latest release via script:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/bradsjm/emwin-rs/releases/latest/download/emwin-cli-installer.sh | sh
```

Run via Docker (no local Rust toolchain required):

```bash
docker run --rm ghcr.io/bradsjm/emwin-rs/emwin-cli:latest --help
```

Development compose stack (ephemeral Postgres + MinIO + `emwin-cli server`):

```bash
cp .env.compose.example .env.compose
docker compose up --build
```

- `compose.yml` provisions `postgis/postgis`, MinIO, and `emwin-cli server`.
- Postgres data and MinIO object storage use `tmpfs`, so the stack is intentionally non-persistent.
- `emwin-cli` runs with `EMWIN_OUTPUT_DIR=s3://emwin/emwin`, `AWS_ENDPOINT_URL=http://minio:9000`, and `EMWIN_PERSIST_DATABASE_URL=postgresql://emwin:emwin@postgres:5432/emwin?sslmode=disable` by default.
- Set `EMWIN_USERNAME` in `.env.compose`; set `EMWIN_RECEIVER=wxwire` and `EMWIN_PASSWORD` only when using Weather Wire.
- To point compose at a different S3-compatible target, set `EMWIN_OUTPUT_DIR=s3://bucket[/prefix]`; use `AWS_ENDPOINT_URL` to target MinIO or another custom endpoint with path-style addressing, set `AWS_REGION` or `AWS_DEFAULT_REGION` for region selection, and provide credentials with `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`, or `AWS_PROFILE` when needed.
- The HTTP server is exposed on `http://127.0.0.1:8080`, Postgres on `127.0.0.1:5432`, MinIO S3 on `http://127.0.0.1:9000`, and the MinIO console on `http://127.0.0.1:9001` by default.

## Use `emwin-protocol` in your app

Add the crate from this monorepo:

```toml
[dependencies]
emwin-protocol = { git = "https://github.com/bradsjm/emwin-rs", tag = "v0.1.0", package = "emwin-protocol" }
```

Use the unified ingest API from the crate root:

```rust
use emwin_protocol::ingest::{IngestConfig, IngestReceiver};
use emwin_protocol::qbt_receiver::{QbtDecodeConfig, QbtReceiverConfig, default_qbt_upstream_servers};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut receiver = IngestReceiver::build(IngestConfig::Qbt(QbtReceiverConfig {
        email: "you@example.com".to_string(),
        servers: default_qbt_upstream_servers(),
        server_list_path: None,
        follow_server_list_updates: true,
        reconnect_delay_secs: 5,
        connection_timeout_secs: 5,
        watchdog_timeout_secs: 49,
        max_exceptions: 10,
        decode: QbtDecodeConfig::default(),
    }))?;
    receiver.start()?;
    receiver.stop().await?;
    Ok(())
}
```

For active development against local changes, use a path dependency instead:

```toml
[dependencies]
emwin-protocol = { path = "../emwin-rs/crates/emwin-protocol" }
```

`emwin-protocol` protocol feature flags:

```toml
[dependencies]
emwin-protocol = { path = "../emwin-rs/crates/emwin-protocol", default-features = false, features = ["qbt"] }
```

## Quick start

Live server mode:

```bash
cargo run -p emwin-cli -- server --username you@example.com --bind 127.0.0.1:8080
cargo run -p emwin-cli -- server --username you@example.com --output-dir ./out
cargo run -p emwin-cli -- server --username you@example.com --output-dir ./out --post-process-archives false
cargo run -p emwin-cli -- server --username you@example.com --output-dir ./out --persist-database-url postgres://localhost/emwin
cargo run -p emwin-cli -- server --username you@example.com --output-dir s3://my-bucket/emwin --persist-database-url postgres://localhost/emwin
cargo run -p emwin-cli -- server --receiver wxwire --username you@example.com --password 'secret'
```

Optional file persistence:

- `server --output-dir <PATH|s3://bucket[/prefix]>` writes each completed assembled file and a sibling `.JSON` metadata sidecar under canonical archival paths such as `qbt/2026/03/16/BOX/nws_text_product/20260316T021530Z-4f2c9d91-AFDBOX.TXT`.
- When `--persist-database-url` is also set, blob writes still succeed even if the Postgres metadata upsert fails.
- When Postgres is unavailable at startup or during runtime, the server stays up, retries metadata persistence in the background with backoff, and resumes writing after connectivity returns.
- When filesystem writes fail transiently, including `ENOSPC`, or S3 returns transient service/network failures, including bucket auto-create checks during startup/runtime, the background persistence worker retries with throttled warning logs while live ingest and connected clients remain online.
- ZIP/ZIS archive entry directories are not preserved in persisted blob paths; the original delivered filename remains available in metadata and `/files` responses.
- `server` defaults to `--post-process-archives true`, which extracts the first entry from completed `.ZIP` and `.ZIS` products before parsing and downstream delivery.
- Corrupt `.ZIP` and `.ZIS` payloads are logged as `Corrupt Zip File Received` and dropped when archive post-processing is enabled.
- `server` serves retained payloads over HTTP from the in-memory retention cache while optionally persisting payloads and metadata asynchronously in the background.

CLI logging format:

- Diagnostics/logging use canonical `tracing-subscriber` formatting and `RUST_LOG` filtering.
- Process startup logs include the `emwin-cli` crate version and selected subcommand.
- Command payloads remain on `stdout`; diagnostics/logging remain on `stderr`.
- This `stdout`/`stderr` split applies to all modes, including `relay`.

Live server mode (SSE + JSON endpoints):

```bash
cargo run -p emwin-cli -- server --username you@example.com --bind 127.0.0.1:8080
cargo run -p emwin-cli -- server --receiver wxwire --username you@example.com --password 'secret' --bind 127.0.0.1:8080
cargo run -p emwin-cli -- server --post-process-archives false --username you@example.com --bind 127.0.0.1:8080
```

Useful server flags:

- `--stats-interval-secs 30` (set `0` to disable periodic stats logging)
- `--quiet` (suppress non-error logs)
- `--max-clients 100` (cap concurrent SSE clients)
- `--file-retention-secs 300` (in-memory completed-file TTL)
- `--max-retained-files 1000` (in-memory completed-file capacity)
- `--cors-origin "*"` or `--cors-origin "https://your-ui.example"`

Server endpoints:

- `GET /events?event=file_complete&lat=41.42&lon=-96.17&distance_miles=5` - SSE event stream with optional live filters over event, file, product, header, and parsed location metadata
- `GET /files` - retained completed-file payloads using the same shape as `file_complete` events, including parsed `product` metadata and `download_url`
- `GET /files/{*filename}` - retained file download (URL-encoded path segment)
- `GET /health` - server health summary
- `GET /metrics` - JSON telemetry snapshot

`/events` filter parameters:

- `event` - comma-delimited event names such as `file_complete`, `telemetry`, or `connected`
- `filename` - wildcard filename match such as `*.TXT` or `A_*`
- `source`, `pil`, `family`, `container`, `wmo_prefix`, `office`, `office_city`, `office_state`, `bbb_kind` - product metadata filters (`source` uses parsed enrichment sources such as `text_header` or `wmo_taf_bulletin`; `office` matches the normalized 3-letter office code; `container` reflects parsed container values such as `raw` or `zip`)
- `cccc`, `ttaaii`, `afos`, `bbb` - header filters (`cccc`, `ttaaii`, and `bbb` match both AFOS-backed headers and WMO-only bulletin headers when present)
- `has_issues`, `issue_kind`, `issue_code` - parse/QC issue filters
- `has_vtec`, `has_ugc`, `has_hvtec`, `has_latlon`, `has_time_mot_loc`, `has_wind_hail` - parsed body presence filters using `true`/`false` or `1`/`0`
- `state`, `county`, `zone`, `fire_zone`, `marine_zone` - UGC geographic filters using canonical codes such as `NE`, `IAC001`, `CAZ041`, `COF214`, `AMZ250`
- `vtec_phenomena`, `vtec_significance`, `vtec_action`, `vtec_office`, `etn` - VTEC filters using canonical codes such as `TO`, `W`, `NEW`, `KDMX`, and `123`
- `hvtec_nwslid`, `hvtec_severity`, `hvtec_cause`, `hvtec_record` - HVTEC filters using values such as `MSRM1`, `major`, `excessive_rainfall`, and `no_record`
- `wind_hail_kind`, `min_wind_mph`, `min_hail_inches` - severe-tag filters using kinds such as `max_wind_gust`, `hail_threat`, `legacy_hail`
- `lat`, `lon`, `distance_miles` - parsed location filters; `lat`/`lon` are required together, `distance_miles` defaults to `5.0`, products match if the point falls inside any parsed `LAT...LON` polygon or within range of any parsed `TIME...MOT...LOC`, `UGC`, or `HVTEC` point
- `min_size`, `max_size` - completed file size bounds in bytes

Examples:

- `GET /events?event=file_complete&pil=TAF,AFD`
- `GET /events?event=file_complete&family=nws_text_product&container=raw`
- `GET /events?event=file_complete&source=wmo_taf_bulletin&cccc=KWBC`
- `GET /events?event=file_complete&office=FFC&office_state=GA`
- `GET /events?event=file_complete&has_issues=true&issue_code=invalid_wmo_header`
- `GET /events?event=file_complete&cccc=KBOX&ttaaii=FXUS61`
- `GET /events?event=file_complete&county=IAC001&vtec_phenomena=TO&vtec_significance=W`
- `GET /events?event=file_complete&has_hvtec=true&hvtec_cause=excessive_rainfall`
- `GET /events?event=file_complete&has_wind_hail=true&min_wind_mph=50&min_hail_inches=1.00`
- `GET /events?event=file_complete&state=NE&vtec_office=KOAX&vtec_action=NEW`
- `GET /events?event=file_complete&lat=41.42&lon=-96.17`
- `GET /events?event=file_complete&lat=41.42&lon=-96.17&distance_miles=15`

Optional live-mode endpoint/persistence overrides:

- `--server host:port` (repeatable or comma-delimited)
- `--server-list-path ./servers.json`

Environment and `.env` support:

- `.env` from the current working directory is loaded before CLI parsing.
- CLI args override process env; process env overrides `.env`.
- Useful variables include `EMWIN_RECEIVER`, `EMWIN_USERNAME`, `EMWIN_PASSWORD`, `EMWIN_SERVER`, `EMWIN_SERVER_LIST_PATH`, `EMWIN_OUTPUT_DIR`, `EMWIN_PERSIST_DATABASE_URL`, `EMWIN_MAX_EVENTS`, `EMWIN_IDLE_TIMEOUT_SECS`, `EMWIN_BIND`, `EMWIN_CORS_ORIGIN`, `EMWIN_MAX_CLIENTS`, `EMWIN_STATS_INTERVAL_SECS`, `EMWIN_FILE_RETENTION_SECS`, `EMWIN_MAX_RETAINED_FILES`, `EMWIN_QUIET`, `EMWIN_TEXT_PREVIEW_CHARS`, and `EMWIN_POST_PROCESS_ARCHIVES`.
- When `EMWIN_OUTPUT_DIR` uses `s3://bucket[/prefix]`, `emwin-db` resolves object-store settings from AWS-compatible environment variables: `AWS_ENDPOINT_URL` switches to a custom endpoint with path-style access, `AWS_REGION` or `AWS_DEFAULT_REGION` selects the region, and credentials come from `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`, or `AWS_PROFILE` plus the compatible metadata providers exposed by `rust-s3`.
- Filters are CLI-only and are not loaded from environment variables.

Relay mode (raw TCP passthrough + metrics):

```bash
cargo run -p emwin-cli -- relay --username you@example.com
```

Useful relay flags:

- `--bind 0.0.0.0:2211` (downstream client listener)
- `--max-clients 100` (connection cap; over-capacity clients receive server-list frame then disconnect)
- `--auth-timeout-secs 720` (downstream re-authentication window)
- `--client-buffer-bytes 65536` (per-client backpressure budget)
- `--metrics-bind 127.0.0.1:9090` (metrics listener)

Relay endpoints:

- `GET /health` - relay health summary
- `GET /metrics` - relay telemetry snapshot (connections, auth, buffering, and quality state)
