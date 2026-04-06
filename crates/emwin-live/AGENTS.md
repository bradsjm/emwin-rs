# AGENTS.md

Agent guide for `crates/emwin-live`.

## Scope

- Crate: `emwin-live` (library crate).
- Role: headless live ingest runtime, retention, persistence wiring, runtime telemetry, and event fanout.
- Depends on `emwin-protocol` for receiver/runtime events, `emwin-parser` for completed-product enrichment, and `emwin-db` for persistence and shared metadata/query types.

## Before You Change Code

- Read root `AGENTS.md` first.
- Keep `emwin-live` narrow. It is a headless runtime crate, not a general shared-logic bucket.
- Keep HTTP concerns in `emwin-api`, CLI parsing/output in `emwin-cli`, and archive query contracts in `emwin-db`.
- Keep receiver orchestration and completed-product handling here rather than drifting back into adapter crates.
- Preserve shutdown, retry, and bounded-channel behavior unless the task explicitly changes it.

## Build, Lint, and Test Commands

Run from repo root.

```bash
cargo test -p emwin-live
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture Boundaries

- Keep runtime lifecycle and shared live state in `src/runtime.rs` and `src/types.rs`.
- Keep ingest-loop translation and event publication in `src/ingest.rs`.
- Keep archive post-processing in `src/archive_postprocess.rs`.
- Keep completed-product enrichment and persistence request construction in `src/file_pipeline.rs`.
- Keep persistence startup/shutdown wiring in `src/persistence.rs`.
- Keep in-memory retained-file behavior in `src/retained.rs`.
- Keep live file-filter logic in `src/filter/*`.
- Keep public exports curated in `src/lib.rs`.

## Testing Expectations

- Keep unit tests near the implementation.
- Preserve archive post-processing, retained-file, persistence wiring, and ingest-event regression coverage.
- Add regression tests when changing retention semantics, event shapes, persistence request building, or runtime lifecycle behavior.
