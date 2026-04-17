**Triage Report: emwin-rs**

Status note: updated after the remediation pass completed on the current branch. Items marked `Completed` are already implemented; the rest remain open.

Scope: architectural, performance, reliability, maintainability, and YAGNI review across 8 crates, ~152k LoC (130k in one crate is embedded generated data).

## Critical — Performance

- `Completed` **Alert worker N+N×R fan-out with blob reads per check** — fixed by loading product metadata once per source event and reusing it across rule and silence checks in the worker loop.
- `Completed` **Spatial filter casts defeat GiST indexes** — fixed by adding functional geography GiST indexes in `crates/emwin-db/migrations/0005_spatial_geography_indexes.sql`.
- **5-way `EXISTS … OR EXISTS …` spatial union** — same file, per-query five correlated subqueries ORed against `products`. For large corpora, planner cannot use semi-join strategies cleanly. Consider `UNION ALL` + `DISTINCT` over a spatial candidate CTE, then join back to `products`.
- **Double broadcast pipeline with full payload remap** — `crates/emwin-live/src/ingest.rs:153-196` publishes `LiveBroadcastEvent`, `crates/emwin-api/src/server/runtime.rs:125-137` relays into a second 4096-cap `broadcast::channel` after `map_live_event` at runtime.rs:222. Each event is cloned, remapped, rebroadcast to every SSE consumer (another clone). One of the two broadcasts is redundant; the API can subscribe to the live channel directly and transform at send time, or share a single event model.
- `Completed` **Retained file store: owned `Vec<u8>` + O(n) duplicate insert** — fixed by switching retained payloads to `Bytes` and replacing duplicate `retain` scans with generation-tracked queue entries.
- `Completed` **QBT frame resync uses `windows().position()` instead of memchr** — fixed by using `memchr`-driven sync scanning in the protocol codec.
- `Partially completed` **Excess payload clones on delivery** — the hot-path `delivered.data.to_vec()` copy into retained storage is removed via `Bytes`; broader clone reduction across the API/live broadcast path is still open until the double-broadcast refactor lands.
- `Completed` **2.3 MB of generated Rust slows every build** — fixed by replacing the large generated Rust NWSLID/UGC catalogs with lazy JSON-backed catalogs.

## Reliability

- `Completed` **Duplicate sqlx migration version 0003** — fixed by renumbering `0003_child_product_id_indexes.sql` to `0004_child_product_id_indexes.sql` and adding a regression test for unique migration versions.
- `Completed` **Alerting rule render builds a new `minijinja::Environment` per event** — fixed at the lower-effort end by reusing one `Environment` per worker iteration; compiled template caching by `(rule_id, updated_at)` is still not implemented.
- `Completed` **`unreachable!` in runtime config match** — fixed by switching runtime startup to strongly typed QBT/WxWire config builders.
- **Poisoned-mutex boilerplate replicated ~70 times** — `.lock().unwrap_or_else(|poisoned| poisoned.into_inner())` across 13 files (`crates/emwin-live/src/runtime.rs` alone uses it 13×). This normalizes "continue past poisoning" everywhere. Consolidate into a small helper (`with_state(&self.mutex, |s| …)`) or switch these to `parking_lot::Mutex` which has no poison concept — fewer lines, clearer intent, same or better perf.
- `Completed` **Silence list not refreshed during event loop** — fixed by fetching active silences per source event instead of once per batch.
- `Completed` **`deliver_attempt` hard-codes retry ceiling `attempt_no >= 4`** — fixed by adding `max_delivery_attempts` to `AlertWorkerConfig` and CLI/env configuration.
- **Expired source event "claim_lost" is `tracing::warn` + skip** — `crates/emwin-alert/src/worker.rs:185-192`. The event will not be re-processed until the lease expires. Fine for the happy path, but there is no metric or counter exposed, so operators cannot detect chronic claim-lease exhaustion. Add a counter to `LiveStatsSnapshot` / `/v1/metrics`.
- `Completed` **QBT server-list frame validation trusts upstream length** — fixed by rejecting oversize server-list frames before UTF-8 allocation.

## Maintainability

- **`LiveRuntime` reimplements `ArchiveQueryService` purely as forwarding** — `crates/emwin-live/src/runtime.rs:560-711` is ~150 lines of identical `Box::pin(async move { … archive.method(query).await })` wrappers that just delegate to the same `PostgresMetadataSink` already behind the trait. The API could accept `Arc<dyn ArchiveQueryService>` populated directly from `sink` and skip the forwarder. The only value added is `record_archive_result` error accounting; extract that to a `tower::Service` or a thin decorator. Cuts ~150 lines and removes a mechanical duplication point.
- **`openapi.rs` carries 30 hand-written mirror DTOs** — `crates/emwin-api/src/server/openapi.rs` (776 lines) defines `…Schema` types alongside the real payload types in `types/payloads.rs` (451 lines). Every DTO change has to be made in two places, which defeats utoipa's `ToSchema` derive. Consolidate by deriving `ToSchema` on the canonical types; the `ArchiveFilterParamsFixup` modifier already proves the rest is mechanical.
- **Archive filter macro chain is hard to follow** — `crates/emwin-service/src/archive/query.rs:10-83` + `archive_filter_fields.rs` drive three macros through one `emwin_archive_filter_fields!` token tree. Works, but readability suffers; current test tooling can't easily inspect the expanded `ArchiveFilterInput`. If the field set is stable (~40 fields), hand-writing the struct may be clearer than three layers of macro indirection.
- **SQL filter string concatenation in `query/filters.rs` (380 lines)** — dozens of repeated `append_text_set_filter(builder, "products.x", query.x.as_deref(), normalize_lower)` lines. A single macro `text_filter!(builder, query, source => products.source, lower)` could halve this file.
- **`emwin-parser` pipeline has two 900+ line files** — `pipeline/classify/text.rs` (989) and `pipeline/assemble/mod.rs` (749). Either split by concern (classify by container kind; assemble by receiver) or add a `mod.rs` that re-exports smaller sub-files.
- **Module naming inconsistency** — `emwin-db/src/postgres/query/archive/` exists alongside `emwin-db/src/postgres/archive.rs`. Two "archive" modules at different depths in the same crate is a maintainability trap; pick one layout.

## YAGNI

- **`LiveTelemetry::Unavailable` branch is never produced by concrete receivers** — `crates/emwin-live/src/ingest.rs:139-144` hits it for "unknown" `IngestTelemetry`, but the only `IngestTelemetry` variants in use are `Qbt` and `WxWire`. Trace it: if no producer ever emits a third variant, remove the match arm and the `Unavailable` case.
- **`emwin-service` trait surface is single-impl forwarding** — `LiveEventService`, `RetainedFileService`, `IncidentChangeStream`, `ArchiveQueryService` each have exactly two impls (the real one + the `LiveRuntime` forwarder). The API uses them via `Arc<dyn …>` for testability, which is fine — but the `LiveRuntime` impl is pure delegation (see above). Either (a) make `LiveRuntime` expose the underlying `PostgresMetadataSink` trait object directly, or (b) delete the traits and inject the DB sink plus retained store directly into the API. Current shape costs ~200 lines with no runtime benefit.
- **`new_for_tests`, `new_for_tests_with_active_servers`, `new_for_tests_with_archive_status`, `new_for_tests_with_state`** — `crates/emwin-live/src/runtime.rs:321-419`, four overloads of the same test constructor in `pub` surface. Collapse into one builder: `LiveRuntime::test_builder().archive_status(..).build()`. Test ergonomics pollute the production public API today.
- **`RuntimeTasks` optional fields with three possible None cases** — `crates/emwin-live/src/runtime.rs:27-32`. `persistence_runtime` and `incident_relay` are `Option`, but `incident_relay` exists iff `archive_sink.is_some()` and `persistence_runtime` iff `output_dir.is_some()`. The implication is structural; encode it with a `Mode` enum or drop the `Option`s by always constructing the task (no-op if unused). Current state requires callers to understand four valid combinations.
- **Relay mode in `emwin-cli`** — quick scan of `crates/emwin-cli/src/relay.rs`/`main.rs` shows full `--bind`, `--max-clients`, `--auth-timeout-secs`, `--client-buffer-bytes`, `--metrics-bind` with its own metrics HTTP listener. Verify external consumers exist; if not, this is a large surface for a single internal use case.
- **`emwin_archive_filter_fields!` drives both parsing and building; some fields unused downstream** — e.g. check whether every field in `ArchiveFilterInput` reaches a real `WHERE` in `query/filters.rs`. Likely a handful (e.g., nested boolean coverage flags) are accepted but do nothing.
- **`EMWIN_TEXT_PREVIEW_CHARS` mentioned in README but search shows no handler** — worth auditing all `EMWIN_*` env vars against `clap` attrs in `emwin-cli/src/main.rs`; orphaned env vars are YAGNI debris.
- `Completed` **`EMWIN_TEXT_PREVIEW_CHARS` mentioned in README but search shows no handler** — removed the stale env var from the docs during the remediation pass.

## Top 5 actions to prioritize

1. `Completed` Fix `event_matches_criteria` N×R blob reads — highest operational impact once alerts scale past a handful of rules.
2. `Completed` Either add a geography-typed column or a functional geography GiST index — point/distance endpoints will degrade quadratically as product volume grows.
3. Collapse the double broadcast (`live → api`) + remove `LiveRuntime`'s trait forwarder layer — single largest code-reduction win.
4. `Completed` Move `generated_nwslid.rs` / `generated_ugc.rs` out of `.rs` into embedded data files — big build-time reduction.
5. Consolidate `openapi.rs` schema mirrors and the `new_for_tests_*` overloads — pure maintainability cleanup that makes future changes safer.

Nothing in this review is a drop-everything incident; items #1 and #2 are the ones that will bite under production load.
