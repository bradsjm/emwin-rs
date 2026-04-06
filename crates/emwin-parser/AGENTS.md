# AGENTS.md

Agent guide for `crates/emwin-parser`.

## Scope

- Crate: `emwin-parser` (library crate).
- Role: text-product parsing, header enrichment, body extraction, product classification, and structured bulletin projection.
- Owns the staged parser pipeline and generated catalog-driven routing used by higher-level crates.

## Before You Change Code

- Read root `AGENTS.md` first.
- Keep parser behavior stable unless the task explicitly changes it.
- Keep the staged pipeline intact: normalize -> envelope -> classify -> optional body plan -> assemble.
- Keep adapter concerns out of this crate. HTTP belongs in `emwin-api`, runtime orchestration belongs in `emwin-live`, and persistence/query behavior belongs in `emwin-db`.
- Do not edit generated catalog Rust by hand. Update the source data and generator, then regenerate.

## Build, Lint, and Test Commands

Run from repo root.

```bash
cargo test -p emwin-parser
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture Boundaries

- Keep input normalization in `src/pipeline/normalize.rs`.
- Keep envelope construction and parse-error preservation in `src/pipeline/envelope.rs`.
- Keep parser selection and candidate creation in `src/pipeline/*` and classifier helpers.
- Keep generic body extraction and QC in `src/body/*`.
- Keep specialized bulletin-family parsers under `src/specialized/*`.
- Keep generated catalog and lookup data under `src/data/*`; regenerate instead of hand-editing generated files.
- Keep public exports curated in `src/lib.rs`.

## Parser-Specific Rules

- Do not reintroduce flag-matrix routing, compatibility aliases, or ad hoc parser dispatch.
- Do not move parsing responsibility into assembly. `assemble` consumes parsed candidates; it does not decide how to parse.
- Do not bypass catalog-driven AFOS routing with scattered `if afos.starts_with(...)` rules as the primary source of truth.
- Do not let recognized supported families silently fall back to generic text when structured parsing fails; preserve the recognized family and surface issues.
- Treat extractor order and catalog body behavior as semantic, not incidental.

## Testing Expectations

- Keep unit tests near the implementation.
- Preserve fixture-backed corpus coverage for supported families and generic body extraction.
- Add regression tests when changing routing, extractor plans, malformed-family handling, or structured projections.
- Update `README.md` when adding or removing supported families or changing parser architecture.
