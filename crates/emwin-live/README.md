# emwin-live

Headless live ingest runtime for EMWIN.

## What it owns

- Receiver startup and shutdown
- Archive post-processing for completed products
- In-memory retained-file state
- Async blob and metadata persistence wiring
- Live event fanout and runtime telemetry snapshots

## Validation

From workspace root:

```bash
cargo test -p emwin-live
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
