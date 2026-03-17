# AGENTS.md

Agent guide for `crates/emwin-parser`.
This file extends the repository-level `AGENTS.md` and is the architectural contract for future parser work in this crate.

## Purpose

`emwin-parser` has already been refactored away from ad hoc parser dispatch, flag matrices, and probe-then-reparse flows.

**Future changes must preserve that architecture.**

Do not treat this crate as a place for opportunistic one-off parsing logic.
If a change does not fit the architecture below, fix the architecture or stop.

## Architecture Authority

The current architecture is defined by these files:

- [README.md](./README.md)
- [src/pipeline/normalize.rs](./src/pipeline/normalize.rs)
- [src/pipeline/envelope.rs](./src/pipeline/envelope.rs)
- [src/pipeline/classify/mod.rs](./src/pipeline/classify/mod.rs)
- [src/pipeline/classify/context.rs](./src/pipeline/classify/context.rs)
- [src/pipeline/classify/common.rs](./src/pipeline/classify/common.rs)
- [src/pipeline/classify/text.rs](./src/pipeline/classify/text.rs)
- [src/pipeline/classify/wmo.rs](./src/pipeline/classify/wmo.rs)
- [src/pipeline/candidate.rs](./src/pipeline/candidate.rs)
- [src/pipeline/assemble.rs](./src/pipeline/assemble.rs)
- [src/data/mod.rs](./src/data/mod.rs)
- [src/body/enrich.rs](./src/body/enrich.rs)
- [scripts/generate_product_data.py](../../scripts/generate_product_data.py)

If a parser update conflicts with those files, those files win unless you are intentionally performing an architecture change and updating all affected layers.

## Non-Negotiable Rules

1. Do not reintroduce `ProductMetadataFlags`, compatibility aliases, or any equivalent boolean execution matrix.
2. Do not route AFOS products with scattered `if afos.starts_with(...)` rules as the primary source of truth.
3. Do not make assembly parse products. Parsing belongs in classification.
4. Do not add `expect(...)`-based invariants to production classification or assembly flow.
5. Do not let parser modules bypass the staged pipeline.
6. Do not add legacy/deprecated symbols. This repo explicitly allows breaking changes.
7. Do not edit generated catalog Rust by hand. Update the JSON source and generator, then regenerate.

## Required Pipeline Shape

All text-product handling must continue to follow this staged model:

```text
bytes + filename
    -> normalize
    -> envelope build
    -> classify to parsed candidate
    -> optional body plan/body request
    -> assemble ProductEnrichment
```

### Stage ownership

- `normalize.rs`
  - owns container detection and single-buffer text normalization
- `envelope.rs`
  - owns AFOS/WMO envelope construction and parse error preservation
- `classify/mod.rs` and its submodules
  - owns parser selection and creation of fully parsed candidates
- `assemble.rs`
  - owns conversion from candidate to public `ProductEnrichment`
- `body/enrich.rs`
  - owns generic body extraction and QC

Do not move responsibilities across these boundaries without updating the architecture docs and tests.

## AFOS Routing Rules

AFOS routing is metadata-driven.
The authoritative source is `TextProductCatalogEntry` from [src/data/mod.rs](./src/data/mod.rs) and generated catalog data.

### What the catalog must define

Every text-product catalog entry must define:

- `pil`
- `wmo_prefix`
- `title`
- `routing`
- `body_behavior`
- `extractors`

### What classification must do

For AFOS products:

1. derive PIL from the header
2. look up `TextProductCatalogEntry`
3. derive optional `BodyExtractionPlan`
4. allow a specialized strategy only when `metadata.routing` matches that strategy
5. keep parser-specific body-shape heuristics as guards, not as primary routing truth

Current routing values:

- `generic`
- `fd`
- `pirep`
- `sigmet`
- `lsr`
- `cwa`
- `wwp`
- `saw`
- `sel`
- `cf6`
- `dsm`
- `hml`
- `mos`
- `cli`
- `mcd`
- `ero`
- `spc_outlook`

If you add a new specialized AFOS family, do all of the following together:

1. extend `TextProductRouting`
2. extend the JSON schema and generator
3. update the generated catalog
4. add a new strategy in `src/pipeline/classify/text.rs` or `src/pipeline/classify/wmo.rs`
5. add candidate and assembly support if required
6. add regression tests
7. update `README.md`

Do not add a new specialized parser family by heuristics alone.

## Generic Body Extraction Rules

Generic body extraction is plan-driven.
The authoritative source is `BodyExtractionPlan` in [src/body/enrich.rs](./src/body/enrich.rs).

### Required model

```text
TextProductCatalogEntry
    -> body_behavior
    -> ordered extractors
    -> BodyExtractionPlan
    -> enrich_body_from_plan(...)
    -> ProductBody + issues
```

### Rules

1. `assemble.rs` may consume a `BodyContributionRequest`, but it must not decide extractor policy itself.
2. `classify/mod.rs` and its helpers may build a `BodyContributionRequest`, but they must not parse body content themselves.
3. `body/enrich.rs` is the only place allowed to map extractor lists to QC rules.
4. Extractor order is semantically significant. Preserve it unless intentionally changing output behavior.

### Adding a new generic body extractor

If you add a new extractor, update all of these in one change:

1. `BodyExtractorId`
2. `ProductBody`
3. extractor application in `body/enrich.rs`
4. QC mapping if needed
5. generator canonical order in `scripts/generate_product_data.py`
6. source JSON entries that use it
7. tests
8. `README.md`

Do not add a new extractor only in parser code and “wire it later.”

## Specialized/Generic Coexistence Rules

The architecture supports candidates carrying both:

- a specialized artifact (`fd`, `pirep`, `sigmet`, etc.)
- a generic `body`

This coexistence is policy-driven, not hardcoded.

### Current truth

At the time of writing:

- `FD*` routes to specialized parsing and `body_behavior = never`
- `PIR` routes to specialized parsing and `body_behavior = never`
- `SIG` routes to specialized parsing and `body_behavior = never`
- `LSR` routes to specialized parsing and `body_behavior = never`
- `CWA` routes to specialized parsing and `body_behavior = never`
- `WWP` routes to specialized parsing and `body_behavior = never`
- `SAW` routes to specialized parsing and `body_behavior = never`
- `SEL` routes to specialized parsing and `body_behavior = never`
- `CF6` routes to specialized parsing and `body_behavior = never`
- `DSM` routes to specialized parsing and `body_behavior = never`
- `HML` routes to specialized parsing and `body_behavior = never`
- `MET`, `MAV`, `MEX`, `FRH`, `FTP`, `ECS`, `LAV`, `LEV`, `NBE`, and `NBS` route to specialized parsing and `body_behavior = never`
- `CLI` routes to specialized parsing and `body_behavior = never`

That means current specialized AFOS families remain bodyless by catalog policy.

Exact-AFOS overrides also route:

- `PRCUS` to specialized PIREP parsing
- `NBXUSA` to specialized MOS parsing
- `SWOMCD` and `FFGMPD` to specialized MCD/MPD parsing
- `RBG94E`, `RBG98E`, and `RBG99E` to specialized ERO parsing
- `PTSDY1`, `PTSDY2`, `PTSDY3`, `PTSD48`, `PFWFD1`, `PFWFD2`, and `PFWF38` to specialized SPC outlook parsing

WMO-only strategies currently support structured parsing for:

- `FD`
- `PIREP`
- `METAR`
- `TAF`
- `SIGMET`
- `DCP`
- WMO-routed `CWA`

If you add or remove a supported family, update this section, [README.md](./README.md), and the corpus fixtures/tests together.

### If you want to enable coexistence for a family

Do not just set `body_request: Some(...)` in classification logic.
You must:

1. update the catalog entry to `body_behavior = "catalog"`
2. confirm the extractor list is actually meaningful for that family
3. add fixture-backed tests proving both specialized and generic outputs are correct
4. add regression tests proving issue behavior is still acceptable
5. update docs

If you cannot prove coexistence with fixtures, do not enable it.

## Recognized-Malformed Family Rule

Supported-family recognition is not allowed to silently fall back to generic text just because structured parsing failed.

### Required behavior

- if a supported family is positively recognized, classification must stop on that family
- structured success produces that family candidate
- structured failure produces `MalformedFamily`
- `assemble.rs` must preserve the recognized family and surfaced issues

### What this forbids

- falling through to `TextGeneric` for a recognized supported AFOS or WMO family
- falling through to `UnsupportedWmo` for a recognized supported WMO family
- using corpus filename exceptions to hide classifier gaps

If a real fixture in a supported-family corpus still needs a per-file exception, treat that as a parser bug unless the fixture was imported into the wrong family directory.

## Parser Module Guidance

Parser modules under `src/` and `src/body/` should continue following the modernized style already established in this crate.

## Performance Library Policy

This crate already chose a small set of parsing/performance libraries intentionally.
Future updates must use them consistently instead of drifting back to full-string rebuilds or regex-first parsing.

### `bstr`

Use `bstr` when operating on incoming text-like bytes that may contain control characters, mixed framing, or imperfect UTF-8 assumptions.

Current architectural role:

- normalization and byte-to-text boundaries
- header conditioning paths
- text handling where bytes should remain authoritative until a narrower parse boundary

Use `bstr` for:

- byte-oriented trimming and splitting before committing to owned `String`
- handling `\0`, `\r`, SOH/ETX, and similar transport artifacts
- keeping the normalized backing buffer authoritative

Do not:

- eagerly convert payloads into multiple owned `String`s at the start of parsing
- use lossy string conversion as a general preprocessing step across the whole pipeline

### `memchr`

Use `memchr` for delimiter scanning in hot paths.

Current architectural role:

- newline scanning
- slash-delimited candidate scanning
- line/block offset discovery

Use `memchr` for:

- finding `\n` boundaries
- scanning candidate delimiters such as `/`
- computing borrowed body slices or ranges

Prefer `memchr` over:

- `lines().collect::<Vec<_>>()` when you only need offsets or sequential scanning
- repeated `find(...)` loops over the same byte buffer
- rebuilding text solely to discover section boundaries

### `winnow`

`winnow` is allowed, but only where the grammar is fixed enough that explicit combinators improve clarity.

Current architectural role:

- fixed structured preludes such as TAF and parts of SIGMET

Use `winnow` when:

- the format is grammar-like and stable
- token order matters
- parser combinators are clearer than hand-written stateful string walking

Do not use `winnow` for:

- every parser by default
- loosely structured multiline sections that are easier to scan line-by-line
- simple token splits where standard parsing is more readable

### `regex`

Regex is permitted only as a narrow tool, not as the default parser model.

Regex is still acceptable when:

- the matched shape is narrow, fixed, and isolated
- the regex is genuinely simpler than the manual parser
- the regex is not being used to flatten or “parse everything”

Regex is not acceptable for:

- primary product routing
- whole-bulletin parsing when token/line parsing is clearer
- replacing structured parser stages with one large capture expression

### Library Selection Patterns

Choose libraries by parse shape:

- byte buffer normalization or delimiter scanning:
  - `bstr` + `memchr`
- line/block parser with narrow repair heuristics:
  - standard library iteration, optionally `memchr`
- fixed grammar prelude:
  - selective `winnow`
- narrow legacy pattern that is simpler as a single matcher:
  - `regex`

If a proposed parser change needs a new library, justify why the existing set is insufficient.
“Because it is easier to write quickly” is not a valid reason.

### Preferred implementation style

- explicit token parsing
- explicit line/block parsing
- borrowed internal helpers where useful
- owned public output types
- narrow repair heuristics with tests and comments

### Discouraged implementation style

- flattening full multiline inputs with `replace('\n', " ")`
- `collect::<Vec<_>>().join(...)` on hot parsing paths
- full-regex parsing when the grammar is simple and tokenizable
- parser selection in `assemble.rs`
- probe-then-reparse flows

Regex is allowed when it is clearly the simplest correct solution, but it is not the default.

## Data/Codegen Rules

The source of truth for text-product metadata is:

- [data/text_product_catalog.json](./data/text_product_catalog.json)

Generated Rust must be produced by:

- [scripts/generate_product_data.py](../../scripts/generate_product_data.py)

Generated output lives at:

- [src/data/generated_text_products.rs](./src/data/generated_text_products.rs)

If you change the catalog schema:

1. update the generator
2. regenerate output
3. update Rust types and lookup APIs
4. update tests
5. update docs

Do not keep compatibility aliases for removed schema or API names.

## Testing Requirements

Every architecture-affecting parser change must add or update tests close to the changed layer.

Integration tests should use real product bulletins as fixtures wherever possible. The current corpus is intentionally pyIEM-heavy and family-scoped; Mesonet remains useful for provenance and targeted acquisition, but it is no longer the dominant fixture source.

Unit tests under `src/**` must not depend on files under `tests/**`.

Archived real-bulletin fixtures belong under `tests/fixtures/**` and must be consumed only by integration tests under `tests/**`.

Unit tests in `src/**` should use inline or module-local data and focus on parser mechanics, candidate logic, and output-shape behavior.

### `pyIEM` sanity checks

Use `pyIEM` as a behavior and fixture sanity-check source when working on weather product parsers.
It is useful because it has a large corpus of real-world weather product handling and sample data.

Helpful references:

- Repository: `https://github.com/akrherz/pyIEM`
- Example corpus: `pyiem/data/product_examples/`

Use `pyIEM` to:

- find real bulletin samples for regression tests
- compare edge-case parsing behavior for UGC, VTEC, and HVTEC paths
- sanity-check malformed or awkward real-world product formatting before inventing synthetic fixtures

Check current local examples of this pattern before adding new tests:

- `tests/vtec_hvtec_regressions.rs`
- `src/body/ugc.rs`

Do not treat `pyIEM` as an architecture authority.
Do not copy its module structure, parser layering, or public model into `emwin-parser`.
Do not change `emwin-parser` outputs solely to match `pyIEM` unless a real bulletin fixture proves the discrepancy and the change still fits this crate's public model and issue semantics.

### Minimum expectations

- unit tests for the parser or data model you changed
- regression tests for output-shape or routing changes
- corpus integration updates when the change affects supported-family routing, malformed-family behavior, or real-product fixture handling
- crate tests:
  - `cargo test -p emwin-parser`
- workspace tests:
  - `cargo test --workspace`

### Corpus policy

- fixture-backed `enrich_product()` behavior belongs in `tests/corpus_*.rs`
- direct low-level parser API regressions belong in narrow non-corpus files such as `tests/body_parser_integration.rs` and `tests/vtec_hvtec_regressions.rs`
- do not create standalone fixture-backed regression files when the assertion belongs to an existing corpus family suite
- supported-family corpus suites should not depend on filename allowlists to permit generic fallback

### Validation commands

Run from repo root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p emwin-parser
cargo test --workspace
```

If you change the generator or catalog validation logic, also run:

```bash
python3 -m unittest scripts.tests.test_generate_product_data
```

## Documentation Requirements

If you change any of these, update docs in the same change:

- pipeline stage ownership
- routing policy
- body extraction policy
- catalog schema
- public lookup names
- specialized/generic coexistence behavior

Required docs to keep aligned:

- [README.md](./README.md)
- this file
- inline Rust docs in touched modules

## Anti-Patterns To Reject

Reject changes that do any of the following:

- add one-off parser branches in `product.rs`
- add new catalog-derived booleans instead of richer metadata
- make `assemble.rs` responsible for parser selection
- add new specialized families without catalog routing support
- add generic body parsing directly from header enrichment
- bypass generator validation by editing generated files manually
- preserve obsolete names “for compatibility” during development
- adding `src/**` tests that read fixtures from `tests/**`

## Practical Update Checklists

### Adding a specialized AFOS parser family

1. Add parser module or extend an existing specialized parser.
2. Add routing enum value.
3. Update JSON schema and generator validation.
4. Update catalog entries.
5. Extend the `classify/mod.rs` strategy registry and the appropriate classifier submodule.
6. Add candidate + assembly support if needed.
7. Add tests.
8. Update docs.

### Adding a generic extractor

1. Extend `BodyExtractorId`.
2. Extend `ProductBody`.
3. Implement parser/extractor application.
4. Update QC plan logic if needed.
5. Update generator canonical order.
6. Update catalog entries.
7. Add tests.
8. Update docs.

### Enabling coexistence for an existing specialized family

1. Change catalog `body_behavior`.
2. Ensure extractor list is meaningful.
3. Prove output correctness with fixtures.
4. Add regression tests for `body` presence and issue behavior.
5. Update docs.

If a proposed change does not fit one of these checklists, it probably does not fit the architecture.
