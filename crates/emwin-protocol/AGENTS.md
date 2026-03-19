# AGENTS.md

Agent guide for `crates/emwin-protocol`.
This file defines crate-local expectations for automated coding agents.

## Scope

- Crate: `emwin-protocol` (library crate).
- Role: protocol parsing/encoding, client runtime orchestration, stream/file assembly.
- Normative protocol authority:
  - `docs/EMWIN QBT TCP Protocol.md` for QBT/EMWIN
  - `docs/weather-wire.md` for Weather Wire/XMPP

## Before You Change Code

- Read root `AGENTS.md` first, then this file.
- Keep changes local to `emwin-protocol` unless cross-crate updates are required.
- Prefer smallest correct fix; avoid cleanup refactors unrelated to the task.
- If behavior changes, update tests in this crate and docs/spec where required.

## Build, Lint, and Test Commands

Run from repo root.

### Fast crate-focused loop

```bash
cargo build -p emwin-protocol
cargo test -p emwin-protocol
```

### Required quality gates for this crate

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p emwin-protocol
```

### Run a single unit/integration test

Use test-name filter:

```bash
cargo test -p emwin-protocol checksum_fixture
cargo test -p emwin-protocol protocol::codec::tests::v2_compressed_roundtrip
```

Use exact match when names collide:

```bash
cargo test -p emwin-protocol protocol::codec::tests::checksum_strict_drop -- --exact
```

Run one integration target:

```bash
cargo test -p emwin-protocol --test protocol_parity
cargo test -p emwin-protocol --test protocol_prop
cargo test -p emwin-protocol --test reconnect_failover
```

Run one integration test function:

```bash
cargo test -p emwin-protocol --test protocol_parity server_update_full_format -- --exact
```

Discover available tests:

```bash
cargo test -p emwin-protocol -- --list
```

Debug with test output:

```bash
cargo test -p emwin-protocol <test_name> -- --nocapture
```

## Crate Architecture Boundaries

- Keep protocol concerns in `src/protocol/*`.
- Keep runtime/client lifecycle in `src/client/*`.
- Keep file assembly concerns in `src/file/*`.
- Keep streaming abstractions in `src/stream/*`.
- Keep public exports curated in `src/lib.rs`.
- Do not introduce CLI presentation/output concerns in this crate.

## Code Style Guidelines

### Formatting and linting

- Always run `cargo fmt --all` before finalizing.
- Keep clippy clean under `-D warnings`.
- Do not suppress warnings unless there is a strong, documented reason.

### Imports

- Prefer explicit imports over wildcard imports.
- Keep imports minimal and file-local.
- Alias imports only for readability or collision handling.

### Types and APIs

- Prefer domain-specific structs/enums over generic containers.
- Keep public APIs small and intentional.
- Validate config at constructor/builder boundaries.
- Avoid widening public surface area unless required by task.

### Naming

- Types/traits/enums: `UpperCamelCase`.
- Functions/modules/variables: `snake_case`.
- Constants: `SCREAMING_SNAKE_CASE`.
- Use protocol/runtime domain names (for example `ProtocolDecoder`, `ClientBuilder`).

### Error handling

- Use typed errors (`thiserror`) and preserve variant semantics.
- Reuse crate result alias (`CoreResult<T>`) when available.
- Propagate with `?` and conversion via `#[from]` where appropriate.
- No `unwrap()` in production code paths.
- `expect(...)` is acceptable in tests with specific messages.

### Async/concurrency

- Use Tokio primitives already established in crate (`mpsc`, `watch`, tasks).
- Prefer bounded channels.
- Respect shutdown and task lifecycle; avoid orphaned tasks.
- Use deterministic/saturating reconnect/backoff logic.

### Protocol safety

- Treat decoder behavior as compatibility-sensitive.
- For parser changes, test success and failure/corruption paths.
- If protocol interpretation changes, update the relevant protocol spec doc.

## Testing Expectations

- Keep unit tests near implementation (`#[cfg(test)] mod tests`).
- Keep cross-module behavior in `crates/emwin-protocol/tests/*.rs`.
- Add regression tests for every parser/runtime bug fix.
- Prefer deterministic tests; avoid sleep-heavy or timing-flaky patterns.

## Documentation and Spec Sync

- Update `docs/EMWIN QBT TCP Protocol.md` or `docs/weather-wire.md` for protocol behavior changes and requirement mappings.
- Keep root or crate README examples accurate when behavior changes.

## Cursor/Copilot Rules

Repository check status at time of writing:

- `.cursorrules`: not present
- `.cursor/rules/`: not present
- `.github/copilot-instructions.md`: not present

If these are added later, treat them as higher-priority local constraints.
