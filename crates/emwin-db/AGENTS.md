# AGENTS.md

Agent guide for `crates/emwin-db`.

## Scope

- Crate: `emwin-db` (library crate).
- Role: persistence runtime, blob storage backends, Postgres metadata/archive access, and shared archive query contracts.
- Depends on `emwin-parser` and `emwin-protocol` only where persisted metadata construction or archive models require it.

## Before You Change Code

- Read root `AGENTS.md` first.
- Keep persistence and archive query behavior stable unless the task explicitly changes it.
- Keep live runtime orchestration in `emwin-live`, not in this crate.
- Keep HTTP route shaping in `emwin-api`, not in this crate.
- Keep shared archive filter and query contracts here, not in adapter crates.

## Build, Lint, and Test Commands

Run from repo root.

```bash
cargo test -p emwin-db
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture Boundaries

- Keep persistence queueing and background worker concerns under `src/runtime.rs`.
- Keep blob writer implementations under `src/writer.rs`.
- Keep Postgres persistence, read models, and archive queries under `src/postgres.rs`.
- Keep shared archive filter/query construction in `src/archive_filter.rs`.
- Keep public exports curated in `src/lib.rs`.

## Testing Expectations

- Keep unit tests near the implementation.
- Preserve blob writer, retry, and queue-behavior coverage.
- Preserve archive query and incident projection integration coverage.
- Add regression tests when changing archive filters, persistence semantics, or Postgres query behavior.
