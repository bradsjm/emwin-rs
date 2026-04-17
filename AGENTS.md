# AGENTS.md

Agent guide for `emwin-rs`.
Use this file as the operational contract for automated coding agents.

## Scope

- Repository type: Rust workspace (Edition 2024).
- Toolchain target: stable Rust, workspace `rust-version = 1.94`.
- Workspace members:
  - `crates/emwin-api`
  - `crates/emwin-alert`
  - `crates/emwin-live`
  - `crates/emwin-protocol`
  - `crates/emwin-service`
  - `crates/emwin-cli`
  - `crates/emwin-db`
  - `crates/emwin-parser`
- Protocol behavior authority:
  - `docs/EMWIN QBT TCP Protocol.md` (QBT/EMWIN)
  - `docs/weather-wire.md` (Weather Wire/XMPP)

## Repo Rules Snapshot

- `unsafe_code` is forbidden at workspace level (`[workspace.lints.rust]`).
- Keep changes focused; avoid unrelated refactors.
- If protocol behavior changes, update all three:
  - implementation
  - tests
  - corresponding protocol spec doc (`docs/EMWIN QBT TCP Protocol.md` or `docs/weather-wire.md`)

## Build, Lint, and Test Commands

Run commands from repository root unless noted.

### Full workspace checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p emwin-protocol --features qbt
```

### Build commands

```bash
cargo build --workspace
cargo build -p emwin-api
cargo build -p emwin-live
cargo build -p emwin-protocol
cargo build -p emwin-cli
```

### Crate-specific test commands

```bash
cargo test -p emwin-protocol
cargo test -p emwin-api
cargo test -p emwin-live
cargo test -p emwin-cli
cargo test -p emwin-protocol --features qbt
```

### Running a single test (important)

Use the test name as a filter:

```bash
cargo test -p emwin-protocol checksum_fixture
cargo test -p emwin-protocol --features qbt protocol::codec::tests::v2_compressed_roundtrip
cargo test -p emwin-cli cli_output_channeling
```

Use exact matching when names are ambiguous:

```bash
cargo test -p emwin-protocol --features qbt protocol::codec::tests::checksum_strict_drop -- --exact
```

Run one integration test target file:

```bash
cargo test -p emwin-protocol --features qbt --test qbt_receiver
cargo test -p emwin-cli --test cli_contract
```

Run one integration test function from a test target:

```bash
cargo test -p emwin-protocol --features qbt --test qbt_receiver full_server_list_frame_ignores_satellite_entries -- --exact
```

List available tests before selecting one:

```bash
cargo test -p emwin-protocol -- --list
cargo test -p emwin-protocol --features qbt -- --list
cargo test -p emwin-cli -- --list
```

Debug failing tests with output:

```bash
cargo test -p emwin-protocol --features qbt <test_name> -- --nocapture
```

## Local Run Commands

```bash
cargo run -p emwin-cli -- server --username you@example.com --bind 127.0.0.1:8080
cargo run -p emwin-cli -- server --username you@example.com --output-dir ./out
```

Live mode examples:

```bash
cargo run -p emwin-cli -- server --receiver wxwire --username you@example.com --password your-pass
cargo run -p emwin-cli -- server --username you@example.com --output-dir ./out --persist-database-url postgres://localhost/emwin
```

## Code Style Guidelines

### Formatting and linting

- Always format with `cargo fmt --all`.
- Always pass clippy with `-D warnings`.
- Do not merge code that introduces warnings.

### Imports

- Prefer explicit imports; avoid wildcard imports.
- Keep imports minimal and local to module needs.
- Follow existing file style and let `rustfmt` normalize layout.
- Alias imports only when name collisions or readability require it.

### Types and APIs

- Prefer concrete domain types over loosely typed values.
- Use enums for protocol state and event variants.
- Keep public APIs intentionally small (`lib.rs` re-exports are curated).
- Validate configuration before runtime startup (builder/constructor boundary).
- Avoid introducing `unsafe` or unstable/nightly-only features.

### Naming conventions

- Types/traits/enums: `UpperCamelCase`.
- Functions/modules/variables: `snake_case`.
- Constants: `SCREAMING_SNAKE_CASE`.
- Keep names domain-specific (`ProtocolDecoder`, `ServerListManager`, etc.).

### Error handling

- Use typed errors (`thiserror`) for domain failures.
- Keep crate-local result aliases when present (`CoreResult<T>`).
- Propagate errors with `?`; avoid `unwrap()` in production code.
- Use `expect(...)` only in tests with a specific failure message.
- Preserve context when converting errors (`#[from]` or explicit mapping).
- Avoid stringly-typed catch-all errors unless no typed option exists.

### Async and concurrency

- Use Tokio primitives already used in the repo (`mpsc`, `watch`, tasks).
- Respect shutdown signals and avoid orphaned background tasks.
- Use bounded channels unless there is a clear reason not to.
- Handle reconnect/backoff deterministically; prefer saturating math.

### Logging and output boundaries

- CLI contract: payloads to `stdout`, diagnostics/logs to `stderr`.
- Preserve machine-readable JSON output stability.
- Keep human-readable text mode concise and non-ambiguous.

### Testing conventions

- Unit tests close to implementation (`#[cfg(test)] mod tests`).
- Cross-module behavior in `crates/*/tests/*.rs` integration tests.
- Add regression tests for protocol parsing edge cases and corruption.
- For new protocol behavior, test both success and failure paths.
- Prefer deterministic tests; avoid unnecessary timing flakiness.

### File and module organization

- Keep unified ingest concerns under `emwin-protocol/src/ingest`.
- Keep QBT receiver concerns under `emwin-protocol/src/qbt_receiver`.
- Keep Weather Wire runtime concerns under `emwin-protocol/src/wxwire_receiver`.
- Keep headless live ingest orchestration under `emwin-live/src`.
- Keep HTTP server and OpenAPI concerns under `emwin-api/src/server`.
- Keep CLI command handling under `emwin-cli/src/cmd`.
- Do not leak CLI-only concerns into core library modules.

## Documentation Requirements

- Update crate README examples if user-facing behavior changes.

## Cursor/Copilot Rule Files

## Agent Execution Checklist

- Read this file and `README.md` first.
- Make smallest correct change.
- Run format, clippy, and relevant tests.
- Prefer running a focused single test during iteration, then full crate/workspace tests.
- Ensure protocol changes include code + tests + spec updates.
