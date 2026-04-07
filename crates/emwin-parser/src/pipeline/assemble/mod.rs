//! Assembly of public `ProductEnrichment` values from parsed candidates.
//!
//! Phase 2 removes parser selection from assembly. The classification stage now
//! owns all specialized parsing, and assembly performs a pure conversion from
//! candidate to the public output model.
//!
//! Module ownership is split by responsibility:
//! - `specialized`: structured bulletin assembly for specialized parsed candidates
//! - `fallback`: malformed, unsupported, non-text, and unknown fallback assembly
//! - `mod.rs`: shared enrichment builders and common body/header helpers

#![allow(dead_code)]

mod fallback;
mod specialized;

use crate::data::{NonTextProductMeta, container_from_filename, wmo_office_entry};
use crate::{
    ParserError, ProductArtifact, ProductEnrichment, ProductEnrichmentSource, ProductParseIssue,
    TextProductHeader, WmoHeader, wmo_prefix_for_pil,
};
use crate::{
    ProductBody,
    body::{BodyInputFormat, enrich_body_from_plan, enrich_body_from_plan_with_format},
};

use super::ClassificationCandidate;
use super::candidate::{
    BodyContributionRequest, Cf6Candidate, CliCandidate, CwaCandidate, DcpCandidate, DsmCandidate,
    EroCandidate, FdCandidate, HmlCandidate, LsrCandidate, MalformedFamilyCandidate, McdCandidate,
    MetarCandidate, MosCandidate, PirepCandidate, SawCandidate, SelCandidate, SigmetCandidate,
    SpcOutlookCandidate, TafCandidate, TextGenericCandidate, UnsupportedWmoCandidate, WwpCandidate,
};
use super::normalize::detected_container;

/// Assembles the public enrichment result from a parsed classification candidate.
///
/// The filename and raw bytes remain inputs so the unknown-product path can
/// preserve the existing container detection semantics.
pub(crate) fn assemble_product_enrichment(
    candidate: ClassificationCandidate,
    filename: &str,
    raw_bytes: &[u8],
) -> ProductEnrichment {
    match candidate {
        ClassificationCandidate::TextGeneric(candidate) => {
            specialized::assemble_from_text_generic(candidate, filename)
        }
        ClassificationCandidate::Fd(candidate) => {
            specialized::assemble_from_fd(candidate, filename)
        }
        ClassificationCandidate::Pirep(candidate) => {
            specialized::assemble_from_pirep(candidate, filename)
        }
        ClassificationCandidate::Sigmet(candidate) => {
            specialized::assemble_from_sigmet(candidate, filename)
        }
        ClassificationCandidate::Lsr(candidate) => {
            specialized::assemble_from_lsr(candidate, filename)
        }
        ClassificationCandidate::Cli(candidate) => {
            specialized::assemble_from_cli(candidate, filename)
        }
        ClassificationCandidate::Cwa(candidate) => {
            specialized::assemble_from_cwa(candidate, filename)
        }
        ClassificationCandidate::Wwp(candidate) => {
            specialized::assemble_from_wwp(candidate, filename)
        }
        ClassificationCandidate::Saw(candidate) => {
            specialized::assemble_from_saw(candidate, filename)
        }
        ClassificationCandidate::Sel(candidate) => {
            specialized::assemble_from_sel(candidate, filename)
        }
        ClassificationCandidate::Cf6(candidate) => {
            specialized::assemble_from_cf6(candidate, filename)
        }
        ClassificationCandidate::Dsm(candidate) => {
            specialized::assemble_from_dsm(candidate, filename)
        }
        ClassificationCandidate::Hml(candidate) => {
            specialized::assemble_from_hml(candidate, filename)
        }
        ClassificationCandidate::Mos(candidate) => {
            specialized::assemble_from_mos(candidate, filename)
        }
        ClassificationCandidate::Mcd(candidate) => {
            specialized::assemble_from_mcd(candidate, filename)
        }
        ClassificationCandidate::Ero(candidate) => {
            specialized::assemble_from_ero(candidate, filename)
        }
        ClassificationCandidate::SpcOutlook(candidate) => {
            specialized::assemble_from_spc_outlook(candidate, filename)
        }
        ClassificationCandidate::Metar(candidate) => {
            specialized::assemble_from_metar(candidate, filename)
        }
        ClassificationCandidate::Taf(candidate) => {
            specialized::assemble_from_taf(candidate, filename)
        }
        ClassificationCandidate::Dcp(candidate) => {
            specialized::assemble_from_dcp(candidate, filename)
        }
        ClassificationCandidate::MalformedFamily(candidate) => {
            fallback::assemble_from_malformed_family(candidate, filename)
        }
        ClassificationCandidate::NonText(candidate) => fallback::assemble_from_non_text(candidate),
        ClassificationCandidate::UnsupportedWmo(candidate) => {
            fallback::assemble_from_unsupported_wmo(candidate, filename)
        }
        ClassificationCandidate::TextParseFailure(error) => {
            fallback::assemble_from_text_parse_failure(filename, error)
        }
        ClassificationCandidate::Unknown => fallback::assemble_unknown(filename, raw_bytes),
    }
}

struct EnrichmentBase {
    source: ProductEnrichmentSource,
    family: Option<&'static str>,
    title: Option<&'static str>,
    container: &'static str,
    pil: Option<String>,
    wmo_prefix: Option<&'static str>,
    office: Option<crate::WmoOfficeEntry>,
    header: Option<TextProductHeader>,
    wmo_header: Option<WmoHeader>,
    bbb_kind: Option<crate::BbbKind>,
    body: Option<ProductBody>,
    parsed: Option<ProductArtifact>,
    issues: Vec<ProductParseIssue>,
}

fn build_enrichment(base: EnrichmentBase) -> ProductEnrichment {
    ProductEnrichment {
        source: base.source,
        family: base.family,
        title: base.title,
        container: base.container,
        wmo_prefix: base
            .wmo_prefix
            .or_else(|| base.pil.as_deref().and_then(wmo_prefix_for_pil)),
        pil: base.pil,
        office: base.office,
        header: base.header,
        wmo_header: base.wmo_header,
        bbb_kind: base.bbb_kind,
        body: base.body,
        parsed: base.parsed,
        issues: base.issues,
    }
}

fn office_for_headers(
    header: Option<&TextProductHeader>,
    wmo_header: Option<&WmoHeader>,
) -> Option<crate::WmoOfficeEntry> {
    header
        .and_then(|header| wmo_office_entry(&header.cccc).copied())
        .or_else(|| wmo_header.and_then(|header| wmo_office_entry(&header.cccc).copied()))
}

struct SpecializedAssemblyInput {
    source: ProductEnrichmentSource,
    family: &'static str,
    title: &'static str,
    filename: String,
    pil: Option<String>,
    header: Option<TextProductHeader>,
    wmo_header: Option<WmoHeader>,
    bbb_kind: Option<crate::BbbKind>,
    body_request: Option<BodyContributionRequest>,
    issues: Vec<ProductParseIssue>,
    parsed: ProductArtifact,
}

fn assemble_specialized_enrichment(input: SpecializedAssemblyInput) -> ProductEnrichment {
    let (body, mut body_issues) = assemble_optional_body(input.body_request);
    body_issues.extend(input.issues);

    build_enrichment(EnrichmentBase {
        source: input.source,
        family: Some(input.family),
        title: Some(input.title),
        container: container_from_filename(&input.filename),
        pil: input.pil,
        wmo_prefix: None,
        office: office_for_headers(input.header.as_ref(), input.wmo_header.as_ref()),
        header: input.header,
        wmo_header: input.wmo_header,
        bbb_kind: input.bbb_kind,
        body,
        parsed: Some(input.parsed),
        issues: body_issues,
    })
}

/// Assembles a generic AFOS text product and runs body enrichment.
fn assemble_from_text_generic(
    candidate: TextGenericCandidate,
    filename: &str,
) -> ProductEnrichment {
    let TextGenericCandidate {
        header,
        pil,
        title,
        body_request,
        bbb_kind,
        reference_time: _reference_time,
    } = candidate;
    let (body, issues) = assemble_optional_body(body_request);

    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::TextHeader,
        family: Some("nws_text_product"),
        title,
        container: container_from_filename(filename),
        pil,
        wmo_prefix: None,
        office: wmo_office_entry(&header.cccc).copied(),
        header: Some(header),
        wmo_header: None,
        bbb_kind,
        body,
        parsed: None,
        issues,
    })
}

/// Assembles an FD bulletin candidate without reparsing it.
fn assemble_from_fd(candidate: FdCandidate, filename: &str) -> ProductEnrichment {
    let FdCandidate {
        source,
        family,
        title,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request,
        bulletin,
    } = candidate;
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source,
        family,
        title,
        filename: filename.to_string(),
        pil,
        header,
        wmo_header,
        bbb_kind,
        body_request,
        issues: Vec::new(),
        parsed: ProductArtifact::Fd(bulletin),
    })
}

/// Assembles a PIREP bulletin candidate without reparsing it.
fn assemble_from_pirep(candidate: PirepCandidate, filename: &str) -> ProductEnrichment {
    let PirepCandidate {
        source,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request,
        bulletin,
    } = candidate;
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source,
        family: "pirep_bulletin",
        title: "Pilot report bulletin",
        filename: filename.to_string(),
        pil,
        header,
        wmo_header,
        bbb_kind,
        body_request,
        issues: Vec::new(),
        parsed: ProductArtifact::Pirep(bulletin),
    })
}

/// Assembles a SIGMET candidate without reparsing it.
fn assemble_from_sigmet(candidate: SigmetCandidate, filename: &str) -> ProductEnrichment {
    let SigmetCandidate {
        source,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request,
        bulletin,
        issues,
    } = candidate;
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source,
        family: "sigmet_bulletin",
        title: "SIGMET bulletin",
        filename: filename.to_string(),
        pil,
        header,
        wmo_header,
        bbb_kind,
        body_request,
        issues,
        parsed: ProductArtifact::Sigmet(bulletin),
    })
}

fn assemble_from_lsr(candidate: LsrCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "lsr_bulletin",
        title: "Local Storm Report",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Lsr(candidate.bulletin),
    })
}

fn assemble_from_cli(candidate: CliCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "cli_bulletin",
        title: "Daily climate report",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Cli(candidate.bulletin),
    })
}

fn assemble_from_cwa(candidate: CwaCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: if candidate.header.is_some() {
            ProductEnrichmentSource::TextHeader
        } else {
            ProductEnrichmentSource::WmoBulletin
        },
        family: "cwa_bulletin",
        title: "Center Weather Advisory",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: candidate.header,
        wmo_header: candidate.wmo_header,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Cwa(candidate.bulletin),
    })
}

fn assemble_from_wwp(candidate: WwpCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "wwp_bulletin",
        title: "Severe Thunderstorm Watch Probabilities",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Wwp(candidate.bulletin),
    })
}

fn assemble_from_saw(candidate: SawCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "saw_bulletin",
        title: "SPC preliminary notice of watch",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Saw(candidate.bulletin),
    })
}

fn assemble_from_sel(candidate: SelCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "sel_bulletin",
        title: "SPC watch bulletin",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Sel(candidate.bulletin),
    })
}

fn assemble_from_cf6(candidate: Cf6Candidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "cf6_bulletin",
        title: "Climate F-6 products",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Cf6(candidate.bulletin),
    })
}

fn assemble_from_dsm(candidate: DsmCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: candidate.source,
        family: "dsm_bulletin",
        title: "Asos Daily Summary",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: candidate.header,
        wmo_header: candidate.wmo_header,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Dsm(candidate.bulletin),
    })
}

fn assemble_from_hml(candidate: HmlCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "hml_bulletin",
        title: "Hyrdo Obs/Forecasts XML",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Hml(candidate.bulletin),
    })
}

fn assemble_from_mos(candidate: MosCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "mos_bulletin",
        title: "MOS guidance bulletin",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Mos(candidate.bulletin),
    })
}

fn assemble_from_mcd(candidate: McdCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "mcd_bulletin",
        title: "Mesoscale discussion bulletin",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Mcd(candidate.bulletin),
    })
}

fn assemble_from_ero(candidate: EroCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "ero_bulletin",
        title: "Excessive rainfall outlook",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Ero(candidate.bulletin),
    })
}

fn assemble_from_spc_outlook(candidate: SpcOutlookCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "spc_outlook_bulletin",
        title: "SPC outlook bulletin",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::SpcOutlook(candidate.bulletin),
    })
}

fn assemble_optional_body(
    request: Option<BodyContributionRequest>,
) -> (Option<ProductBody>, Vec<ProductParseIssue>) {
    match request {
        Some(request) => {
            let outcome = match request.input_format {
                BodyInputFormat::PlainText => {
                    enrich_body_from_plan(&request.text, &request.plan, request.reference_time)
                }
                BodyInputFormat::CapXml => enrich_body_from_plan_with_format(
                    &request.text,
                    &request.plan,
                    request.reference_time,
                    request.input_format,
                ),
            };
            (outcome.body, outcome.issues)
        }
        None => (None, Vec::new()),
    }
}

/// Assembles a parsed METAR candidate.
fn assemble_from_metar(candidate: MetarCandidate, filename: &str) -> ProductEnrichment {
    let MetarCandidate {
        source,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request: _body_request,
        bulletin,
        issues,
    } = candidate;

    build_enrichment(EnrichmentBase {
        source,
        family: Some("metar_collective"),
        title: Some("METAR bulletin"),
        container: container_from_filename(filename),
        pil,
        wmo_prefix: None,
        office: office_for_headers(header.as_ref(), wmo_header.as_ref()),
        header,
        wmo_header,
        bbb_kind,
        body: None,
        parsed: Some(ProductArtifact::Metar(bulletin)),
        issues,
    })
}

/// Assembles a parsed TAF candidate.
fn assemble_from_taf(candidate: TafCandidate, filename: &str) -> ProductEnrichment {
    let TafCandidate {
        source,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request: _body_request,
        bulletin,
    } = candidate;

    build_enrichment(EnrichmentBase {
        source,
        family: Some("taf_bulletin"),
        title: Some("Terminal Aerodrome Forecast"),
        container: container_from_filename(filename),
        pil,
        wmo_prefix: None,
        office: office_for_headers(header.as_ref(), wmo_header.as_ref()),
        header,
        wmo_header,
        bbb_kind,
        body: None,
        parsed: Some(ProductArtifact::Taf(bulletin)),
        issues: Vec::new(),
    })
}

/// Assembles a parsed DCP candidate.
fn assemble_from_dcp(candidate: DcpCandidate, filename: &str) -> ProductEnrichment {
    let DcpCandidate { header, bulletin } = candidate;

    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::WmoBulletin,
        family: Some("dcp_telemetry_bulletin"),
        title: Some("GOES DCP telemetry bulletin"),
        container: container_from_filename(filename),
        pil: None,
        wmo_prefix: None,
        office: wmo_office_entry(&header.cccc).copied(),
        header: None,
        wmo_header: Some(header),
        bbb_kind: None,
        body: None,
        parsed: Some(ProductArtifact::Dcp(bulletin)),
        issues: Vec::new(),
    })
}

/// Assembles a recognized supported family that could not produce a structured artifact.
fn assemble_from_malformed_family(
    candidate: MalformedFamilyCandidate,
    filename: &str,
) -> ProductEnrichment {
    let MalformedFamilyCandidate {
        source,
        family,
        title,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request,
        issues,
    } = candidate;
    let (body, mut body_issues) = assemble_optional_body(body_request);
    body_issues.extend(issues);

    build_enrichment(EnrichmentBase {
        source,
        family: Some(family),
        title: Some(title),
        container: container_from_filename(filename),
        pil,
        wmo_prefix: None,
        office: office_for_headers(header.as_ref(), wmo_header.as_ref()),
        header,
        wmo_header,
        bbb_kind,
        body,
        parsed: None,
        issues: body_issues,
    })
}

/// Assembles a non-text filename-classified candidate.
fn assemble_from_non_text(candidate: NonTextProductMeta) -> ProductEnrichment {
    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::FilenameNonText,
        family: Some(candidate.family),
        title: Some(candidate.title),
        container: candidate.container,
        pil: candidate.pil.map(str::to_string),
        wmo_prefix: candidate.wmo_prefix,
        office: None,
        header: None,
        wmo_header: None,
        bbb_kind: None,
        body: None,
        parsed: None,
        issues: Vec::new(),
    })
}

/// Assembles a recognized unsupported WMO bulletin candidate.
fn assemble_from_unsupported_wmo(
    candidate: UnsupportedWmoCandidate,
    filename: &str,
) -> ProductEnrichment {
    let UnsupportedWmoCandidate {
        family,
        title,
        header,
        code,
        message,
        line,
    } = candidate;

    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::WmoBulletin,
        family: Some(family),
        title,
        container: container_from_filename(filename),
        pil: None,
        wmo_prefix: None,
        office: wmo_office_entry(&header.cccc).copied(),
        header: None,
        wmo_header: Some(header),
        bbb_kind: None,
        body: None,
        parsed: None,
        issues: vec![ProductParseIssue::new(
            "wmo_bulletin_parse",
            code,
            message,
            line,
        )],
    })
}

/// Preserves the legacy issue shape for AFOS text parse failures.
fn assemble_from_text_parse_failure(filename: &str, error: ParserError) -> ProductEnrichment {
    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::TextHeader,
        family: Some("nws_text_product"),
        title: None,
        container: container_from_filename(filename),
        pil: None,
        wmo_prefix: None,
        office: None,
        header: None,
        wmo_header: None,
        bbb_kind: None,
        body: None,
        parsed: None,
        issues: vec![ProductParseIssue::new(
            "text_product_parse",
            parser_error_code(&error),
            error.to_string(),
            parser_error_line(&error).map(str::to_string),
        )],
    })
}

/// Builds the catch-all unknown product result.
fn assemble_unknown(filename: &str, raw_bytes: &[u8]) -> ProductEnrichment {
    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::Unknown,
        family: None,
        title: None,
        container: detected_container(filename, raw_bytes),
        pil: None,
        wmo_prefix: None,
        office: None,
        header: None,
        wmo_header: None,
        bbb_kind: None,
        body: None,
        parsed: None,
        issues: Vec::new(),
    })
}

fn parser_error_code(error: &ParserError) -> &'static str {
    match error {
        ParserError::EmptyInput => "empty_input",
        ParserError::MissingWmoLine => "missing_wmo_line",
        ParserError::InvalidWmoHeader { .. } => "invalid_wmo_header",
        ParserError::MissingAfosLine => "missing_afos_line",
        ParserError::MissingAfos { .. } => "missing_afos",
    }
}

fn parser_error_line(error: &ParserError) -> Option<&str> {
    match error {
        ParserError::InvalidWmoHeader { line } | ParserError::MissingAfos { line } => Some(line),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
