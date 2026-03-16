# emwin-parser

Parser and enrichment library for EMWIN weather products, WMO/AFOS text bulletins, and related structured weather metadata.

## What This Crate Does

`emwin-parser` handles two related jobs:

1. Parse WMO/AFOS text headers and enrich them with catalog metadata.
2. Classify whole products and, when supported, decode them into structured bulletin families or parsed body features.

The crate is intentionally split between a small public API and an internal staged pipeline.

## Scope

The crate currently supports:

- WMO header parsing
- AFOS PIL parsing
- Header enrichment from the generated text-product catalog
- Generic body enrichment for products that carry:
  - event-oriented VTEC segments that correlate:
    - VTEC
    - UGC
    - HVTEC
    - `LAT...LON`
    - `TIME...MOT...LOC`
    - wind/hail tags
  - non-VTEC generic body fields such as UGC, `LAT...LON`, `TIME...MOT...LOC`, and wind/hail tags
  - CAP XML body projection into those same existing body shapes when CAP parameters and areas carry matching data
- Structured specialized parsing for:
  - FD winds and temperatures aloft bulletins
  - PIREP bulletins
  - SIGMET bulletins
  - LSR bulletins
  - CWA bulletins
  - WWP bulletins
  - SAW bulletins
  - SEL bulletins
  - CF6 climate bulletins
  - DSM collectives
  - HML bulletins
  - MOS guidance bulletins for `MET`, `MAV`, `MEX`, `FRH`, and `FTP`
  - CLI daily climate bulletins
  - MCD/MPD bulletins
  - ERO bulletins
  - SPC outlook points bulletins
  - METAR collectives
  - TAF bulletins
  - GOES DCP telemetry bulletins
- Filename-based classification for supported non-text products
- WMO-only fallback handling for valid bulletins that do not carry AFOS lines

## Architecture

### High-Level Flow

The public `enrich_product()` entrypoint is a thin facade over an internal parsing pipeline:

```text
raw bytes + filename
        |
        v
+------------------+
| Normalization    |
| - container kind |
| - text detection |
| - single buffer  |
+------------------+
        |
        v
+------------------+
| Envelope Build   |
| - condition text |
| - parse AFOS/WMO |
| - body range     |
+------------------+
        |
        v
+------------------+
| Classification   |
| - strategy order |
| - parsed         |
|   candidates     |
+------------------+
        |
        v
+------------------+
| Catalog Policy   |
| - routing        |
| - body behavior  |
| - extractors     |
| - QC rules       |
+------------------+
        |
        v
+------------------+
| Assembly         |
| - ProductEnrich. |
| - issues/output  |
+------------------+
```

### Internal Module Layout

```text
crates/emwin-parser/src
|
+-- header/
|   +-- parser.rs    # WMO/AFOS parsing and conditioning
|   +-- enrich.rs    # PIL/BBB metadata enrichment
|
+-- body/
|   +-- enrich.rs    # plan-driven generic body extraction and QC
|   +-- cap.rs       # CAP XML projection into generic body shapes
|   +-- ugc.rs
|   +-- vtec.rs
|   +-- hvtec.rs
|   +-- latlon.rs
|   +-- time_mot_loc.rs
|   +-- vtec_events.rs
|   +-- wind_hail.rs
|
+-- pipeline/
|   +-- normalize.rs # single-buffer input normalization
|   +-- envelope.rs  # parseable envelope construction
|   +-- classify.rs  # ordered strategy registry
|   +-- candidate.rs # parsed intermediate candidates
|   +-- assemble.rs  # ProductEnrichment conversion
|
+-- specialized/
|   +-- fd.rs
|   +-- pirep.rs
|   +-- sigmet.rs
|   +-- lsr.rs
|   +-- cwa.rs
|   +-- wwp.rs
|   +-- cf6.rs
|   +-- dsm.rs
|   +-- hml.rs
|   +-- mos.rs
|   +-- mcd.rs
|   +-- ero.rs
|   +-- spc_outlook.rs
|   +-- metar.rs
|   +-- taf.rs
|   +-- dcp.rs
+-- data/
    +-- generated_*  # compiled lookup tables
```

### Ownership Model

The internal parser path is optimized around a single normalized backing buffer per text payload.

```text
incoming bytes
    |
    v
[ normalized owned Vec<u8> ]
    |            |
    |            +--> text range
    |
    +--> conditioned text
             |
             +--> header refs
             +--> body range
             +--> strategy input
```

That design cuts the highest-value shared-path allocation costs without exposing borrowed lifetime-heavy types in the public API. Public result types such as `TextProductHeader`, `WmoHeader`, and `ProductEnrichment` remain owned and stable.

### Generic Body Enrichment

Header enrichment now exposes only semantic header data. The richer
text-product catalog drives both AFOS routing and generic body policy:

```text
AFOS header
   |
   +--> PIL
          |
          v
+-----------------------------+
| TextProductCatalogEntry     |
| - title                     |
| - wmo_prefix                |
| - routing                   |
| - body_behavior             |
| - extractors                |
+-----------------------------+
      |                 |
      |                 +--> BodyExtractionPlan?
      |
      +--> text strategy registry
                |
                +--> Generic candidate
                +--> FD candidate
                +--> PIREP candidate
                +--> SIGMET candidate
```

If `body_behavior` is `catalog`, the ordered extractor list becomes a
`BodyExtractionPlan` and feeds generic `ProductBody` parsing. VTEC-bearing
generic products now use the `vtec_events` extractor and emit a tagged
`ProductBody` variant with ordered source segments. Non-VTEC generic products
emit the `generic` body variant. If `body_behavior` is `never`, the candidate
remains bodyless.

CAP products remain on generic routing. Their wrapper AFOS/WMO identity stays
authoritative, while the body pipeline detects CAP XML and projects supported
fields such as `VTEC`, UGC geocodes, polygons, event motion, and wind/hail
parameters into the existing generic body models.

VTEC segment QC now emits event-oriented issue codes such as
`vtec_segment_missing_required_polygon` and `vtec_segment_missing_ugc`. The
marine-only UGC exception still applies, but now at the segment level instead
of the whole-product level. When UGC recovery is blocked only because the
header timestamp could not be resolved, the parser emits
`missing_reference_time` and does not also misreport the segment as missing
UGC in the source text.

## Product Routing Model

Classification is ordered and explicit.

### AFOS-backed text products

```text
Text AFOS envelope
    |
    +--> catalog metadata lookup
    |      |
    |      +--> routing = fd     -> FD strategy guard
    |      +--> routing = pirep  -> PIREP strategy guard
    |      +--> routing = sigmet -> SIGMET strategy guard
    |      +--> routing = lsr    -> LSR strategy guard
    |      +--> routing = cwa    -> CWA strategy guard
    |      +--> routing = wwp    -> WWP strategy guard
    |      +--> routing = cf6    -> CF6 strategy guard
    |      +--> routing = dsm    -> DSM strategy guard
    |      +--> routing = hml    -> HML strategy guard
    |      +--> routing = mos    -> MOS strategy guard
    |      +--> routing = generic
    |
    +--> generic text fallback
```

Current repo truth is encoded directly in the catalog:

- `FD*`, `PIR`, `SIG`, `LSR`, `CWA`, `WWP`, `CF6`, `DSM`, `HML`, `MET`, `MAV`, `MEX`, `FRH`, `FTP`, and `CLI` route to specialized parsers and use `body_behavior = never`
- exact-AFOS overrides also route `SWOMCD`, `FFGMPD`, `RBG94E`, `RBG98E`, `RBG99E`, `PTSDY1`, `PTSDY2`, `PTSDY3`, `PTSD48`, `PFWFD1`, `PFWFD2`, and `PFWF38` to specialized parsers
- generic warning products such as `SVR`, `TOR`, and `FFW` route as `generic`
  and use `body_behavior = catalog`

The coexistence mechanism is active in the pipeline, but current specialized
AFOS families remain specialized-only because that is what the catalog says.

### WMO-only text bulletins

```text
WMO-only envelope
    |
    +--> FD
    +--> METAR
    +--> TAF
    +--> DCP
    +--> SIGMET
    +--> unsupported AIRMET
    +--> unsupported surface observation
    +--> unsupported Canadian text
    +--> unsupported valid WMO fallback
```

The classifier produces parsed candidates, not just product kinds. Assembly then converts those candidates into the public `ProductEnrichment` shape without reparsing.

## Public API

The core entrypoints are:

- `parse_text_product(bytes) -> Result<TextProductHeader, ParserError>`
- `enrich_header(&TextProductHeader) -> TextProductEnrichment`
- `enrich_product(filename, bytes) -> ProductEnrichment`
- body parsers such as:
  - `parse_ugc_sections`
  - `parse_vtec_codes`
  - `parse_hvtec_codes`
  - `parse_latlon_polygons`
  - `parse_time_mot_loc_entries`
  - `parse_wind_hail_entries`

`ProductEnrichment` now also exposes specialized bulletin fields for:

- `cli`
- `fd`
- `metar`
- `taf`
- `pirep`
- `sigmet`
- `lsr`
- `cwa`
- `wwp`
- `cf6`
- `dsm`
- `hml`
- `mos`
- `mcd`
- `ero`
- `spc_outlook`

## Installation

```toml
[dependencies]
emwin-parser = { git = "https://github.com/bradsjm/emwin-rs", tag = "v0.3.0", package = "emwin-parser" }
```

For local development:

```toml
[dependencies]
emwin-parser = { path = "../emwin-rs/crates/emwin-parser" }
```

## Usage

### Parse a Text Product Header

```rust
use emwin_parser::{TextProductHeader, parse_text_product};

let raw_text = b"000 \nFXUS61 KBOX 022101\nAFDBOX\nAREA FORECAST DISCUSSION\n";
let header: TextProductHeader = parse_text_product(raw_text)?;

assert_eq!(header.ttaaii, "FXUS61");
assert_eq!(header.cccc, "KBOX");
assert_eq!(header.ddhhmm, "022101");
assert_eq!(header.afos, "AFDBOX");
# Ok::<(), emwin_parser::ParserError>(())
```

### Enrich a Header

```rust
use emwin_parser::{enrich_header, parse_text_product};

let header = parse_text_product(b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n")?;
let enriched = enrich_header(&header);

assert_eq!(enriched.pil_nnn, Some("TAF"));
assert_eq!(enriched.pil_description, Some("Terminal Aerodrome Forecast"));
# Ok::<(), emwin_parser::ParserError>(())
```

### Enrich a Whole Product

```rust
use emwin_parser::{ProductArtifact, ProductEnrichmentSource, enrich_product};

let enrichment = enrich_product(
    "SAGL31.TXT",
    b"000 \nSAGL31 BGGH 070200\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n",
);

assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
assert_eq!(enrichment.family, Some("metar_collective"));
assert!(enrichment.parsed.as_ref().and_then(ProductArtifact::as_metar).is_some());
```

### Parse Generic Body Features

```rust
use emwin_parser::parse_vtec_codes;

let vtec = parse_vtec_codes("/O.NEW.KOAX.TO.W.0021.250601T2300Z-250602T0000Z/\n");
assert_eq!(vtec.len(), 1);
```

## Output Shape

`enrich_product()` still returns the canonical `ProductEnrichment` parse result used inside the
workspace, but external JSON callers should prefer the v2 projection helpers:

- `summarize_product_v2(&ProductEnrichment) -> ProductSummaryV2`
- `detail_product_v2(&ProductEnrichment) -> ProductDetailV2`

`ProductSummaryV2` is the smaller wire/index shape intended for streaming and model-facing use.
`ProductDetailV2` is the richer retrieval/archive shape intended for persisted metadata and
detail-oriented APIs.

Conceptually:

```text
ProductEnrichment (canonical internal parse result)
|
+-- summarize_product_v2 -> ProductSummaryV2
|
+-- detail_product_v2    -> ProductDetailV2
```

The v2 shapes intentionally collapse `header` and `wmo_header` into one discriminated header
object and replace the old parser-specific `parsed` wrapper with `artifact_kind` plus an optional
detail artifact payload.

## Text Conditioning Behavior

Header parsing and product enrichment both account for common EMWIN text artifacts:

- strips `\0`
- strips `\r`
- strips SOH/ETX framing
- inserts a synthetic LDM line when missing
- accepts 4-character `TTAAII` and normalizes it to 6 characters by appending `00`

Example cases:

```rust
use emwin_parser::parse_text_product;

let with_controls = b"\x01123\n000 \nFXUS61 KBOX 022101\nAFDBOX\nbody\x03";
let with_nulls = b"000 \nFXUS61 KBOX 022101\nAFD\0BOX\nbody";
let missing_ldm = b"FXUS61 KBOX 022101\nAFDBOX\nbody\n";

assert_eq!(parse_text_product(with_controls)?.afos, "AFDBOX");
assert_eq!(parse_text_product(with_nulls)?.afos, "AFDBOX");
assert_eq!(parse_text_product(missing_ldm)?.afos, "AFDBOX");
# Ok::<(), emwin_parser::ParserError>(())
```

## Metadata Catalogs

The crate ships generated lookup tables for:

- text-product catalog entries
- WMO office metadata
- UGC county and zone metadata
- NWSLID metadata

Relevant helpers include:

- `pil_description`
- `text_product_catalog_entry`
- `wmo_prefix_for_pil`
- `wmo_office_entry`
- `ugc_county_entry`
- `ugc_zone_entry`
- `nwslid_entry`

The generated PIL table also drives generic body extraction plans and header title lookup.

## Error Handling

Header parsing uses typed `ParserError` values:

- `EmptyInput`
- `MissingWmoLine`
- `InvalidWmoHeader`
- `MissingAfosLine`
- `MissingAfos`

Whole-product parsing reports issues through `ProductEnrichment.issues`.

That separation is deliberate:

- `parse_text_product()` is a strict parser API
- `enrich_product()` is a resilient enrichment API

## Fixtures

Archived exact bulletin fixtures live under `tests/fixtures/products/` and are
organized first by parser domain (`generic`, `specialized`, `wmo`) and then by
product family. AFOS-backed fixtures should be pulled from
`https://mesonet.agron.iastate.edu/api/1/nwstext/{product_id}` where possible.
Use `scripts/fetch_nwstext_fixture.py` to archive new AFOS-backed fixtures from
Mesonet instead of copying bulletin text by hand.
The broader formatting-variation corpus is imported from `akrherz/pyIEM`
`data/product_examples` with `scripts/import_pyiem_product_examples.py`.
These fixtures are consumed only by integration tests under `tests/` to ground
real-product corpus coverage. Unit tests under `src/**` should use inline or
module-local samples and must not import files from `tests/**`. See
`tests/README.md` for the test-corpus organization rules.

## Current Limitations

- `ProductEnrichment` remains the canonical parser result and is richer than the default external
  summary JSON.
- Only selected specialized bulletin families are parsed structurally.
- Some valid WMO bulletin families are recognized but intentionally reported as unsupported.
- Internal performance work is focused on the shared normalization/header/classification path; not every specialized parser has been rewritten around borrowed parsing yet.

## Development

**Required** Follow instructions in [`agents`](AGENTS.md) when working on this crate.

## Related Crates

- [`emwin-protocol`](../emwin-protocol/README.md): ingest, transport, and receiver runtime
- [`emwin-cli`](../emwin-cli/README.md): CLI and server surfaces built on top of parser and protocol crates
