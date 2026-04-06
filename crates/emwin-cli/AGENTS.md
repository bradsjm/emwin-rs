# AGENTS.md

Agent guide for `crates/emwin-cli`.
This file defines crate-local expectations for automated coding agents.

## Scope

- Crate: `emwin-cli` (binary crate).
- Role: command-line UX, argument parsing, command dispatch, stdout/stderr contract.
- Depends on `emwin-api` for server mode and `emwin-protocol` for relay/runtime functionality.

## Before You Change Code

- Read root `AGENTS.md` first, then this file.
- Keep CLI behavior changes explicit and test-covered.
- Preserve output contract stability (especially JSON output fields).
- Keep server implementation in `emwin-api`, not in this crate.
- Keep business/protocol logic in core crates, not in CLI command handlers.

## Build, Lint, and Test Commands

Run from repo root.

### Fast crate-focused loop

```bash
cargo build -p emwin-cli
cargo test -p emwin-cli
```

### Required quality gates for this crate

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p emwin-cli
```

### Run a single test (important)

Use test-name filter:

```bash
cargo test -p emwin-cli cli_output_channeling
cargo test -p emwin-cli cli_stream_json_fixture
```

Use exact match when needed:

```bash
cargo test -p emwin-cli cli_output_channeling -- --exact
```

Run integration target:

```bash
cargo test -p emwin-cli --test cli_contract
```

Discover tests before selecting one:

```bash
cargo test -p emwin-cli -- --list
```

Debug failing tests with captured output:

```bash
cargo test -p emwin-cli <test_name> -- --nocapture
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

## Crate Architecture Boundaries

- Keep command implementations in `src/cmd/*`.
- Keep CLI argument parsing and command wiring in `src/main.rs`.
- Keep archive query serialization and presentation concerns in this crate.
- Delegate server mode orchestration to `emwin-api`.
- Do not duplicate protocol parsing/runtime logic from `emwin-protocol`.

## Code Style Guidelines

### Formatting and linting

- Always run `cargo fmt --all` before finalizing.
- Keep clippy clean under `-D warnings`.

### Imports

- Prefer explicit imports over wildcard imports.
- Keep imports minimal and local to file/module usage.
- Alias only when needed for readability.

### Types and APIs

- Use `clap` derive patterns consistently for args/subcommands.
- Keep command option names stable unless change is intentional and documented.
- Use strongly typed enums (`ValueEnum`) for user-facing mode choices.

### Naming

- Types/traits/enums: `UpperCamelCase`.
- Functions/modules/variables: `snake_case`.
- Constants: `SCREAMING_SNAKE_CASE`.
- Use names that reflect command semantics (`server`, `relay`).

### Error handling

- Return typed/structured errors from command functions when possible.
- In top-level CLI flow, use `anyhow::Result<()>` for command orchestration.
- Propagate with `?`; avoid `unwrap()` in production paths.
- Use `expect(...)` only inside tests with precise failure messages.

### Output and logging contract

- Machine-readable command payloads go to `stdout` when a command defines one.
- Diagnostics/logs/warnings go to `stderr`.
- Keep text output concise and unambiguous.

## Testing Expectations

- Prefer unit tests in the defining module and add integration tests only when command-process coverage is required.
- Validate stdout/stderr channel behavior for contract-sensitive changes.
- For `server --output-dir`, assert filesystem side effects deterministically.
- Keep tests deterministic and independent from external network timing.

## Documentation Requirements

- Update `crates/emwin-cli/README.md` when command behavior or flags change.
- If protocol-facing behavior changed via CLI integration, sync with root docs as needed.

## Cursor/Copilot Rules

Repository check status at time of writing:

- `.cursorrules`: not present
- `.cursor/rules/`: not present
- `.github/copilot-instructions.md`: not present

If these are added later, treat them as higher-priority local constraints.
