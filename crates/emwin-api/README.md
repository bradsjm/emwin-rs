# emwin-api

Reusable HTTP/SSE/OpenAPI server crate for EMWIN live ingest.

## What it owns

- Live ingest orchestration for QBT and Weather Wire server mode
- Axum HTTP routes and SSE streams
- OpenAPI generation and Swagger UI
- Retained-file downloads and in-memory file cache
- Background blob/database persistence wiring
- API filter grammar shared by the HTTP surface and CLI archive query mode

`emwin-cli server` remains the primary process entrypoint and delegates into this crate.

## Validation

From workspace root:

```bash
cargo test -p emwin-api
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
