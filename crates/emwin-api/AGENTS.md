# AGENTS.md

Agent guide for `crates/emwin-api`.

## Scope

- Crate: `emwin-api` (library crate).
- Role: HTTP API, SSE streams, OpenAPI surface, auth/CORS, and retained-file downloads.
- Depends on `emwin-live` for live runtime state and `emwin-db` for archive query access.

## Before You Change Code

- Read root `AGENTS.md` first.
- Keep HTTP behavior and payload shapes stable unless the task explicitly changes them.
- Keep command-line parsing concerns in `emwin-cli`.
- Keep live runtime orchestration in `emwin-live`; this crate adapts it at the HTTP boundary.
- Keep archive query contracts in `emwin-db`, not in this crate.

## Build, Lint, and Test Commands

Run from repo root.

```bash
cargo test -p emwin-api
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture Boundaries

- Keep routing/handler concerns under `src/server/*`.
- Keep persistence wiring and runtime fanout out of this crate.
- Keep HTTP-local serialization and auth middleware in this crate.
- Keep public exports curated in `src/lib.rs`.

## Testing Expectations

- Keep unit tests near the implementation.
- Preserve existing HTTP route, auth, OpenAPI, and SSE coverage.
- Add regression tests if refactoring reveals a behavior break.
