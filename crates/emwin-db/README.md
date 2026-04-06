# emwin-db

Persistence and archive query crate for EMWIN.

## What it owns

- Async persistence runtime and queueing
- Filesystem and S3 blob storage writers
- Postgres metadata persistence and archive read models
- Incident projection persistence and notification types
- Shared archive filter and query contract types

## Validation

From workspace root:

```bash
cargo test -p emwin-db
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
