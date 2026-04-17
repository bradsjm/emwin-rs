**Triage Report: emwin-rs**

Status note: updated after the remediation pass completed on the current branch. Items marked `Completed` are implemented; the only remaining non-completed entries are intentionally retained or narrowed design points.

Scope: architectural, performance, reliability, maintainability, and YAGNI review across 8 crates, ~152k LoC (130k in one crate is embedded generated data).

## Critical — Performance

- `Completed` **Alert worker N+N×R fan-out with blob reads per check** — fixed by loading product metadata once per source event and reusing it across rule and silence checks in the worker loop.
- `Completed` **Spatial filter casts defeat GiST indexes** — fixed by adding functional geography GiST indexes in `crates/emwin-db/migrations/0005_spatial_geography_indexes.sql`.
- `Completed` **5-way `EXISTS … OR EXISTS …` spatial union** — fixed by routing spatial matches through a candidate CTE with `UNION ALL` and joining the distinct candidate product ids back to `products`.
- `Completed` **Double broadcast pipeline with full payload remap** — fixed by having the API subscribe to the live runtime broadcast directly and transform events at the SSE boundary.
- `Completed` **Retained file store: owned `Vec<u8>` + O(n) duplicate insert** — fixed by switching retained payloads to `Bytes` and replacing duplicate `retain` scans with generation-tracked queue entries.
- `Completed` **QBT frame resync uses `windows().position()` instead of memchr** — fixed by using `memchr`-driven sync scanning in the protocol codec.
- `Completed` **Excess payload clones on delivery** — fixed by using `Bytes` for retained payload storage and removing the duplicate API rebroadcast/remap path.
- `Completed` **2.3 MB of generated Rust slows every build** — fixed by replacing the large generated Rust NWSLID/UGC catalogs with lazy JSON-backed catalogs.

## Reliability

- `Completed` **Duplicate sqlx migration version 0003** — fixed by renumbering `0003_child_product_id_indexes.sql` to `0004_child_product_id_indexes.sql` and adding a regression test for unique migration versions.
- `Completed` **Alerting rule render builds a new `minijinja::Environment` per event** — fixed at the lower-effort end by reusing one `Environment` per worker iteration; compiled template caching by `(rule_id, updated_at)` is still not implemented.
- `Completed` **`unreachable!` in runtime config match** — fixed by switching runtime startup to strongly typed QBT/WxWire config builders.
- `Completed` **Poisoned-mutex boilerplate replicated ~70 times** — production mutex recovery paths now use crate-local `lock_unpoisoned` helpers in live, db, and protocol code. Remaining explicit poisoning handling is limited to helper definitions, tests, and non-mutex lock cases.
- `Completed` **Silence list not refreshed during event loop** — fixed by fetching active silences per source event instead of once per batch.
- `Completed` **`deliver_attempt` hard-codes retry ceiling `attempt_no >= 4`** — fixed by adding `max_delivery_attempts` to `AlertWorkerConfig` and CLI/env configuration.
- `Completed` **Expired source event "claim_lost" is `tracing::warn` + skip** — the alert worker now records a `source_claim_lost_total` counter and emits periodic structured stats. This is worker-local instead of `/v1/metrics` because the alert worker runs outside the live API process.
- `Completed` **QBT server-list frame validation trusts upstream length** — fixed by rejecting oversize server-list frames before UTF-8 allocation.

## Maintainability

- `Partially completed` **`LiveRuntime` reimplements `ArchiveQueryService` purely as forwarding** — live, retained-file, and incident-stream forwarding traits were removed from `LiveRuntime` and the API now calls the runtime and archive service directly. The archive query decorator remains intentionally because it records archive health/error metrics around the DB-backed service.
- `Completed` **`openapi.rs` carries 30 hand-written mirror DTOs** — mirror schema DTOs were removed and canonical API/service payloads now derive `utoipa::ToSchema`.
- `Completed` **Archive filter macro chain is hard to follow** — fixed in an earlier remediation pass by simplifying the generated filter structure.
- `Completed` **SQL filter string concatenation in `query/filters.rs` (380 lines)** — repeated text/boolean filter calls were consolidated behind small local macros.
- `Completed` **`emwin-parser` pipeline has two 900+ line files** — `classify/text` and `assemble` were split into focused submodules with `mod.rs` re-exports.
- `Completed` **Module naming inconsistency** — fixed in an earlier remediation pass by renaming the top-level postgres archive module to `archive_service`.

## YAGNI

- `Not actionable` **`LiveTelemetry::Unavailable` branch is never produced by concrete receivers** — retained intentionally because `IngestTelemetry` is `#[non_exhaustive]` and the runtime needs a safe initial/unknown telemetry state.
- `Completed` **`emwin-service` trait surface is single-impl forwarding** — `LiveEventService`, `RetainedFileService`, and `IncidentChangeStream` were removed. `ArchiveQueryService` remains as the storage adapter boundary used by the API and by the metrics-recording archive decorator.
- `Completed` **`new_for_tests`, `new_for_tests_with_active_servers`, `new_for_tests_with_archive_status`, `new_for_tests_with_state`** — fixed in an earlier remediation pass by replacing the overloads with test support builder configuration.
- `Completed` **`RuntimeTasks` optional fields with three possible None cases** — fixed in an earlier remediation pass by encoding runtime task shape with a mode enum.
- `Completed` **`emwin_archive_filter_fields!` drives both parsing and building; some fields unused downstream** — audited during remediation; current archive filter fields are wired into query building.
- `Completed` **`EMWIN_TEXT_PREVIEW_CHARS` mentioned in README but search shows no handler** — removed the stale env var from the docs during the remediation pass.

## Top 5 actions to prioritize

1. `Completed` Fix `event_matches_criteria` N×R blob reads — highest operational impact once alerts scale past a handful of rules.
2. `Completed` Either add a geography-typed column or a functional geography GiST index — point/distance endpoints will degrade quadratically as product volume grows.
3. `Partially completed` Collapse the double broadcast (`live → api`) + remove `LiveRuntime`'s trait forwarder layer — the duplicate broadcast and live/retained/incident forwarding are gone; the archive query decorator remains for metrics.
4. `Completed` Move `generated_nwslid.rs` / `generated_ugc.rs` out of `.rs` into embedded data files — big build-time reduction.
5. `Completed` Consolidate `openapi.rs` schema mirrors and the `new_for_tests_*` overloads — pure maintainability cleanup that makes future changes safer.

Nothing in this review is a drop-everything incident; the production-load risks identified as items #1 and #2 have been remediated.
