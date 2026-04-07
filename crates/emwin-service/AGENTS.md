# AGENTS.md

Agent guide for `crates/emwin-service`.

## Scope

- Crate: `emwin-service` (library crate).
- Role: shared live/archive service contracts, DTOs, cursors, pagination models, and service-layer errors.
- Depends on `emwin-parser` only where contract-owned metadata projection requires parser-owned domain models.

## Before You Change Code

- Read root `AGENTS.md` first.
- Keep this crate narrow. It is a contract crate, not an implementation crate.
- Keep protocol/runtime behavior in `emwin-protocol` and `emwin-live`.
- Keep persistence and Postgres behavior in `emwin-db`.
- Keep HTTP-specific serialization decisions in `emwin-api` unless the wire shape is part of the shared contract.
- Do not introduce dependencies on `emwin-db` or `emwin-protocol`.

## Build, Lint, and Test Commands

Run from repo root.

```bash
cargo test -p emwin-service
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture Boundaries

- Keep archive DTOs, queries, cursors, and service traits under `src/archive.rs`.
- Keep live DTOs and service traits under `src/live.rs`.
- Keep service error types under `src/error.rs`.
- Keep contract-owned metadata projection under `src/metadata.rs`.
- Keep public exports curated in `src/lib.rs`.

## Testing Expectations

- Keep unit tests near the implementation.
- Preserve serialization stability for contract types that cross adapter boundaries.
- Add regression tests when changing cursor encoding, query validation, or serialized enum/value shapes.
