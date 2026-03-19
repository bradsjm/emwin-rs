# emwin-cli Server Mode API

Version: 1.1
Last Updated: 2026-03-18
Status: Authoritative for `emwin-cli server`

## 1. Purpose

This document defines the HTTP/SSE contract exposed by `emwin-cli server`.

It covers:

- Available endpoints
- Event stream contract (`/events`)
- Incident event stream contract (`/incident-events`)
- Event names and payload shapes
- Field-level definitions

## 2. Start Server Mode

```bash
cargo run -p emwin-cli -- server --email you@example.com --bind 127.0.0.1:8080
```

Common options:

- `--bind <ADDR:PORT>`: listen address (default `127.0.0.1:8080`)
- `--max-clients <N>`: max concurrent SSE clients
- `--stats-interval-secs <N>`: periodic stats logging (`0` disables); when file persistence is enabled, the log also includes persistence queue depth/capacity and cumulative enqueue, eviction, success, and failure counts
- `--file-retention-secs <N>`: retained completed-file TTL
- `--max-retained-files <N>`: retained completed-file capacity
- `--cors-origin "*"|"https://..."`: CORS policy

## 3. Endpoints

## `GET /`

Returns API index with endpoint descriptions.

Response shape:

```json
{
  "service": "emwin-cli server",
  "endpoints": [
    {"method":"GET","path":"/","description":"..."},
    {"method":"GET","path":"/events?event=file_complete&lat=41.42&lon=-96.17&distance_miles=5","description":"..."},
    {"method":"GET","path":"/incident-events?action=created,updated&office=KOAX&phenomena=FF&significance=W&etn=2001&status=active","description":"..."}
  ]
}
```

Fields:

- `service` (string): service identifier
- `endpoints` (array): documented routes
  - `method` (string): HTTP method
  - `path` (string): route path
  - `description` (string): short route description

## `GET /events`

Server-Sent Events stream.

Selected query params:

- `event` (optional string): comma-delimited event names.
- `filename` (optional string): wildcard match (`*`, case-insensitive) against completed-file filenames.
- `lat` and `lon` (optional numbers): parsed location query point. Must be supplied together.
- `distance_miles` (optional number): point-radius distance in miles. Defaults to `5.0` when `lat`/`lon` are provided.

Location matching rules:

- products match when the query point falls inside any parsed `LAT...LON` polygon
- otherwise products match when any parsed `TIME...MOT...LOC`, `UGC`, or `HVTEC` point falls within `distance_miles`
- products without parsed spatial data do not match location filters
- non-file events only match by `event=...`

SSE framing:

- `id`: monotonically increasing event id
- `event`: event name
- `data`: JSON payload

Example wire form:

```text
id: 42
event: file_complete
data: {"filename":"WARN.txt","size":2140,"timestamp_utc":1767488000,"product":{"schema_version":2,"source":"text_header","family":"nws_text_product","title":"Area Forecast Discussion","container":"raw","pil":"AFD","wmo_prefix":"FX","office":{"code":"FFC","city":"Peachtree City","state":"GA"},"header":{"kind":"afos","ttaaii":"FXUS62","cccc":"KFFC","ddhhmm":"022101","afos":"AFDFFC"},"facets":{"has_body":false,"has_artifact":false,"has_issues":false,"vtec_count":0,"ugc_count":0,"hvtec_count":0,"latlon_count":0,"time_mot_loc_count":0,"wind_hail_count":0},"keys":{},"issues":{"count":0,"codes":[]}},"download_url":"/files/WARN.txt"}
```

## `GET /incident-events`

Server-Sent Events stream for Postgres-backed incident projection changes.

Availability:

- requires `--persist-database-url`
- returns `503` when archive metadata persistence is not configured

Supported query params:

- `action` (optional string): comma-delimited incident mutation types: `created`, `updated`
- `office` (optional string): comma-delimited office filters such as `KOAX`
- `phenomena` (optional string): comma-delimited VTEC phenomena filters such as `FF`
- `significance` (optional string): comma-delimited significance filters such as `W`
- `status` (optional string): comma-delimited incident status filters such as `active`, `cancelled`, `expired`, `upgraded`
- `etn` (optional string): comma-delimited ETN filters such as `2001,2002`

SSE framing:

- `id`: monotonically increasing incident-event id
- `event`: always `incident_change`
- `data`: JSON payload

Example wire form:

```text
id: 7
event: incident_change
data: {"action":"created","trigger":"persist","incident":{"office":"KOAX","phenomena":"FF","significance":"W","etn":2001,"current_status":"active","latest_vtec_action":"NEW","issued_at":"2025-03-05T12:00:00Z","start_utc":"2025-03-05T12:00:00Z","end_utc":"2025-03-05T18:00:00Z","last_updated_at":"2025-03-05T12:00:01Z","first_product_id":10,"latest_product_id":10,"latest_product_timestamp_utc":"2025-03-05T12:00:00Z","detail_url":"/incidents/KOAX/FF/W/2001","products_url":"/incidents/KOAX/FF/W/2001/products","latest_product_url":"/archive/products/10"}}
```

## `GET /files`

Returns retained completed-file metadata.

Response shape:

```json
{
  "files": [
    {
      "filename": "nested/my file.txt",
      "size": 2140,
      "timestamp_utc": 1767488000,
      "product": {
        "schema_version": 2,
        "source": "text_header",
        "family": "nws_text_product",
        "title": "Area Forecast Discussion",
        "container": "raw",
        "pil": "AFD",
        "wmo_prefix": "FX",
        "office": {
          "code": "FFC",
          "city": "Peachtree City",
          "state": "GA"
        },
        "header": {
          "kind": "afos",
          "ttaaii": "FXUS62",
          "cccc": "KFFC",
          "ddhhmm": "022101",
          "afos": "AFDFFC"
        },
        "issues": []
      },
      "download_url": "/files/nested%2Fmy%20file.txt"
    }
  ]
}
```

Fields:

- `files` (array): retained files
  - `filename` (string): logical filename from feed
  - `size` (number): bytes
  - `timestamp_utc` (number): UNIX timestamp seconds parsed from protocol `/FD`
  - `product` (object): detail v2 metadata for the completed product
  - `download_url` (string): URL-encoded retrieval path for `GET /files/*filename`

## `GET /files/*filename`

Downloads retained file content.

Notes:

- `filename` must be URL-encoded when needed
- Returns `404` when file is not retained/expired
- Returns `400` for invalid filename path

Example:

`/files/nested%2Fmy%20file.txt`

## `GET /health`

Response shape:

```json
{
  "status": "ok",
  "connected_clients": 2,
  "retained_files": 17,
  "uptime_secs": 320,
  "upstream_endpoint": "wxmesg.upstateweather.com:2211"
}
```

Fields:

- `status` (string): health status
- `connected_clients` (number): active SSE clients
- `retained_files` (number): retained files currently available
- `uptime_secs` (number): process uptime seconds
- `upstream_endpoint` (string|null): connected upstream endpoint, if connected

## `GET /metrics`

Returns the current telemetry snapshot. When file persistence is enabled, the same persistence
queue fields emitted in the periodic stats log are also included in the JSON response.

Response shape:

```json
{
  "receiver": "qbt",
  "connection_attempts_total": 0,
  "connection_success_total": 0,
  "connection_fail_total": 0,
  "disconnect_total": 0,
  "watchdog_timeouts_total": 0,
  "watchdog_exception_events_total": 0,
  "auth_logon_sent_total": 0,
  "bytes_in_total": 0,
  "frame_events_total": 0,
  "data_blocks_emitted_total": 0,
  "server_list_updates_total": 0,
  "checksum_mismatch_total": 0,
  "decompression_failed_total": 0,
  "decoder_recovery_events_total": 0,
  "handler_failures_total": 0,
  "backpressure_warning_emitted_total": 0,
  "event_queue_drop_total": 0,
  "telemetry_events_emitted_total": 0,
  "persistence_queue_len": 0,
  "persistence_queue_capacity": 1024,
  "persistence_enqueued_total": 0,
  "persistence_evicted_total": 0,
  "persistence_persisted_total": 0,
  "persistence_failed_total": 0
}
```

Field meanings:

- `connection_attempts_total`: outbound connect attempts
- `connection_success_total`: successful connects
- `connection_fail_total`: failed connect attempts
- `disconnect_total`: disconnect events
- `watchdog_timeouts_total`: watchdog no-data timeouts
- `watchdog_exception_events_total`: watchdog exception increments
- `auth_logon_sent_total`: auth/logon writes sent upstream
- `bytes_in_total`: upstream bytes read
- `frame_events_total`: decoded frame events emitted
- `data_blocks_emitted_total`: data block events emitted
- `server_list_updates_total`: server list update events emitted
- `checksum_mismatch_total`: checksum mismatch detections
- `decompression_failed_total`: decompress failures
- `decoder_recovery_events_total`: decoder resync recoveries
- `handler_failures_total`: handler callback failures
- `backpressure_warning_emitted_total`: backpressure warning emissions
- `event_queue_drop_total`: dropped events from queue pressure
- `telemetry_events_emitted_total`: telemetry events emitted
- `persistence_queue_len`: queued persistence requests waiting to be written (only when persistence is enabled)
- `persistence_queue_capacity`: maximum in-memory persistence queue size before eviction (only when persistence is enabled)
- `persistence_enqueued_total`: accepted persistence requests (only when persistence is enabled)
- `persistence_evicted_total`: queued requests evicted to admit newer work (only when persistence is enabled)
- `persistence_persisted_total`: requests fully written to blob storage and metadata sink (only when persistence is enabled)
- `persistence_failed_total`: requests that failed during persistence (only when persistence is enabled)

## `GET /dashboard`

Serves a built-in read-only admin dashboard as a single HTML page.

The dashboard connects to `/events` as an SSE client (counts toward `--max-clients`)
and polls `/health`, `/metrics`, and `/files` for live state.

Panels:

- **Status bar** — connection indicator, upstream endpoint, client count, uptime
- **Server list** — primary and satellite endpoints with active highlight, endpoint switch history
- **Reliability** — queue drops, watchdog timeouts, watchdog exceptions, auth logons, server list updates
- **Throughput** — cumulative bytes/frames with rolling 5-minute rate sparklines
- **Files** — retained files with size, age, and download links
- **Event log** — filterable, capped at 500 rows, expandable JSON detail per event

No authentication. The dashboard is intended for local/private network use.

## 4. SSE Event Catalog

All `/events` payloads are JSON in the `data` field.

## `event: connected`

```json
{"endpoint":"wxmesg.upstateweather.com:2211"}
```

Fields:

- `endpoint` (string): current upstream endpoint

## `event: disconnected`

```json
{}
```

No fields.

## `event: data_block`

```json
{
  "type":"data_block",
  "filename":"TAFS31AS.TXT",
  "block_number":1,
  "total_blocks":1,
  "length":104,
  "version":"V1",
  "preview":"SAZS31 ..."
}
```

Fields:

- `type` (string): always `data_block`
- `filename` (string): feed filename
- `block_number` (number): 1-based block index
- `total_blocks` (number): expected blocks for full file
- `length` (number): payload byte length for this block
- `version` (string): protocol version label (for example `V1`, `V2`)
- `preview` (string, optional): text preview when enabled by formatter (not guaranteed)

## `event: server_list`

```json
{
  "type":"server_list",
  "servers":[["host1.example",2211],["host2.example",2211]],
  "sat_servers":[["sat.example",2211]]
}
```

Fields:

- `type` (string): always `server_list`
- `servers` (array): server endpoints as `[host, port]`
- `sat_servers` (array): satellite server endpoints as `[host, port]`

## `event: warning`

Two warning payload forms are currently emitted:

Frame warning form:

```json
{"type":"warning","warning":"..."}
```

SSE lag warning form:

```json
{"message":"client lagged; events dropped","dropped":12,"peer":"127.0.0.1:55555"}
```

Fields:

- Frame form:
  - `type` (string): `warning`
  - `warning` (string): warning detail
- Lag form:
  - `message` (string): warning summary
  - `dropped` (number): dropped event count
  - `peer` (string): client socket address

## `event: file_complete`

```json
{
  "filename":"nested/my file.txt",
  "size":2140,
  "timestamp_utc":1767488000,
  "product":{
    "source":"text_header",
    "family":"nws_text_product",
    "title":"Area Forecast Discussion",
    "container":"raw",
    "pil":"AFD",
    "wmo_prefix":"FX",
    "office":{
      "code":"FFC",
      "city":"Peachtree City",
      "state":"GA"
    },
    "header":{
      "ttaaii":"FXUS62",
      "cccc":"KFFC",
      "ddhhmm":"022101",
      "afos":"AFDFFC"
    },
    "issues":[]
  },
  "download_url":"/files/nested%2Fmy%20file.txt"
}
```

Fields:

- `filename` (string): completed file name
- `size` (number): file bytes
- `timestamp_utc` (number): UNIX timestamp seconds parsed from protocol `/FD`
  - `product` (object): summary v2 metadata for the completed product
- `download_url` (string): URL-encoded retrieval path for `GET /files/*filename`

## `event: incident_change`

```json
{
  "action": "created",
  "trigger": "persist",
  "incident": {
    "office": "KOAX",
    "phenomena": "FF",
    "significance": "W",
    "etn": 2001,
    "current_status": "active",
    "latest_vtec_action": "NEW",
    "issued_at": "2025-03-05T12:00:00Z",
    "start_utc": "2025-03-05T12:00:00Z",
    "end_utc": "2025-03-05T18:00:00Z",
    "last_updated_at": "2025-03-05T12:00:01Z",
    "first_product_id": 10,
    "latest_product_id": 10,
    "latest_product_timestamp_utc": "2025-03-05T12:00:00Z",
    "detail_url": "/incidents/KOAX/FF/W/2001",
    "products_url": "/incidents/KOAX/FF/W/2001/products",
    "latest_product_url": "/archive/products/10"
  }
}
```

Fields:

- `action` (string): `created` for first insert of one incident key, `updated` for later persisted or cleanup-driven changes
- `trigger` (string): `persist` for ingest-driven writes, `cleanup` for background expiry updates
- `incident` (object): incident summary payload using the same fields returned by `GET /incidents`, plus archive/detail links

## `event: telemetry`

Payload is the same telemetry object returned by `GET /metrics`.

## `event: error`

```json
{"message":"..."}
```

Fields:

- `message` (string): error message

## `event: unknown`

```json
{"type":"unknown"}
```

Emitted only when an unsupported frame variant is projected to SSE.

## 5. Filtering Rules (`/events?filter=`)

- Matching is wildcard-only with `*`.
- Matching is case-insensitive.
- Filter target is filename only.
- Non-filename events are never filtered out by filename filter.

Examples:

- `*.TXT`
- `WARN*.TXT`
- `*FORECAST*`

## 6. Retention and Availability

- Completed files are retained in memory only.
- Retention is bounded by:
  - max age (`--file-retention-secs`)
  - max entries (`--max-retained-files`)
- When a file expires/evicts, download endpoint returns `404`.
