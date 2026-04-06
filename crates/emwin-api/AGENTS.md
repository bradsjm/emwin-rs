# AGENTS.md

Agent guide for `crates/emwin-api`.

## Scope

- Crate: `emwin-api` (library crate).
- Role: HTTP API, SSE streams, OpenAPI surface, retained-file handling, and live server runtime orchestration.
- Depends on `emwin-protocol` for ingest and `emwin-db` for persistence/query access.

## Before You Change Code

- Read root `AGENTS.md` first.
- Keep HTTP behavior and payload shapes stable unless the task explicitly changes them.
- Keep command-line parsing concerns in `emwin-cli`; this crate owns the server implementation.
- Keep protocol/runtime logic in `emwin-protocol`; this crate should adapt it at the HTTP boundary.

## Build, Lint, and Test Commands

Run from repo root.

```bash
cargo test -p emwin-api
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture Boundaries

- Keep routing/handler concerns under `src/live/server/*`.
- Keep archive/API filter grammar in `src/archive_filter.rs`.
- Keep persistence wiring and file retention helpers out of HTTP handlers.
- Keep public exports curated in `src/lib.rs`.

## Testing Expectations

- Keep unit tests near the implementation.
- Preserve existing HTTP route, auth, OpenAPI, and SSE coverage.
- Add regression tests if refactoring reveals a behavior break.
