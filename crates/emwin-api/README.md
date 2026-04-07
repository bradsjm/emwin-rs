# emwin-api

HTTP/SSE/OpenAPI adapter crate for EMWIN live ingest.

## What it owns

- Axum HTTP routes and SSE streams
- OpenAPI generation and Swagger UI
- HTTP auth, CORS, and response shaping
- Retained-file downloads backed by `emwin-live`
- Archive read endpoints backed by the service surface exposed through `emwin-live`

## What it does not own

- Live ingest orchestration
- Persistence runtime wiring
- Archive filter/query contracts and service DTO definitions
- CLI parsing or output formatting

`emwin-cli server` starts `emwin-live::LiveRuntime`, then serves it through `emwin-api`.

## Validation

From workspace root:

```bash
cargo test -p emwin-api
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```
