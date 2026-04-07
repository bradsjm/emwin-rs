# emwin-service

Shared service contracts for the `emwin-rs` workspace.

## What it owns

- Live-service DTOs and traits shared across adapters
- Archive query DTOs, cursors, aggregate models, and service traits
- Service-layer error types
- Contract-owned metadata models that cross crate boundaries

## What it does not own

- Receiver implementations or protocol runtime details
- Persistence runtime wiring or database access
- HTTP route shaping or CLI output formatting
- Parser internals beyond the metadata projection needed by shared contracts

## Boundary rules

- Adapter crates such as `emwin-api` and `emwin-cli` should depend on this crate for shared live/archive contracts.
- Implementation crates such as `emwin-live` and `emwin-db` should map internal types into these contracts at their public boundaries.
- Do not leak `emwin-protocol` or `emwin-db` implementation types through this crate.

## Validation

From workspace root:

```bash
cargo test -p emwin-service
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
