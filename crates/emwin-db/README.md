# emwin-db

Persistence and archive query crate for EMWIN.

## What it owns

- Async persistence runtime and queueing
- Object-store blob persistence and archive readers, including local filesystem via `file://` URIs
- Postgres metadata persistence and archive service implementation
- Incident projection persistence and notification plumbing
- Mapping persistence records onto `emwin-service` contracts

## Internal layout

`src/postgres/` is organized by responsibility:

- `mod.rs`: config, pool lifecycle, and module wiring
- `connection.rs`: pool creation and target description
- `prepare.rs`: metadata-to-row normalization before persistence
- `sink.rs`: metadata sink transaction orchestration
- `write/`: write-side product, child-row, and incident projection persistence
- `query/`: archive reads, incident reads, SQL fragments, validation, and filter construction
- `query/archive/`: product, feature, aggregate, and issue archive queries
- `query/validation.rs`: cursor parsing, CSV normalization, and spatial input validation
- `query/spatial.rs`: PostGIS predicate assembly for bbox and point-distance filters
- `archive.rs`: retained payload reads and incident cleanup helpers
- `service.rs`: `emwin-service` trait implementations and error mapping

The public crate surface stays narrow. These modules are internal implementation details.

## Validation

From workspace root:

```bash
cargo test -p emwin-db
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
