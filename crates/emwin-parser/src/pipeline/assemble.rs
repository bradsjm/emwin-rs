//! Assembly of public `ProductEnrichment` values from parsed candidates.
//!
//! Phase 2 removes parser selection from assembly. The classification stage now
//! owns all specialized parsing, and assembly performs a pure conversion from
//! candidate to the public output model.

use crate::data::{container_from_filename, wmo_office_entry, NonTextProductMeta};
use crate::{
    body::{enrich_body_from_plan, enrich_body_from_plan_with_format, BodyInputFormat},
    ProductBody,
};
use crate::{
    wmo_prefix_for_pil, ParserError, ProductArtifact, ProductEnrichment, ProductEnrichmentSource,
    ProductParseIssue, TextProductHeader, WmoHeader,
};

use super::candidate::{
    BodyContributionRequest, Cf6Candidate, CliCandidate, CwaCandidate, DcpCandidate, DsmCandidate,
    EroCandidate, FdCandidate, HmlCandidate, LsrCandidate, MalformedFamilyCandidate, McdCandidate,
    MetarCandidate, MosCandidate, PirepCandidate, SawCandidate, SelCandidate, SigmetCandidate,
    SpcOutlookCandidate, TafCandidate, TextGenericCandidate, UnsupportedWmoCandidate, WwpCandidate,
};
use super::normalize::detected_container;
use super::ClassificationCandidate;

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
            assemble_from_text_generic(candidate, filename)
        }
        ClassificationCandidate::Fd(candidate) => assemble_from_fd(candidate, filename),
        ClassificationCandidate::Pirep(candidate) => assemble_from_pirep(candidate, filename),
        ClassificationCandidate::Sigmet(candidate) => assemble_from_sigmet(candidate, filename),
        ClassificationCandidate::Lsr(candidate) => assemble_from_lsr(candidate, filename),
        ClassificationCandidate::Cli(candidate) => assemble_from_cli(candidate, filename),
        ClassificationCandidate::Cwa(candidate) => assemble_from_cwa(candidate, filename),
        ClassificationCandidate::Wwp(candidate) => assemble_from_wwp(candidate, filename),
        ClassificationCandidate::Saw(candidate) => assemble_from_saw(candidate, filename),
        ClassificationCandidate::Sel(candidate) => assemble_from_sel(candidate, filename),
        ClassificationCandidate::Cf6(candidate) => assemble_from_cf6(candidate, filename),
        ClassificationCandidate::Dsm(candidate) => assemble_from_dsm(candidate, filename),
        ClassificationCandidate::Hml(candidate) => assemble_from_hml(candidate, filename),
        ClassificationCandidate::Mos(candidate) => assemble_from_mos(candidate, filename),
        ClassificationCandidate::Mcd(candidate) => assemble_from_mcd(candidate, filename),
        ClassificationCandidate::Ero(candidate) => assemble_from_ero(candidate, filename),
        ClassificationCandidate::SpcOutlook(candidate) => {
            assemble_from_spc_outlook(candidate, filename)
        }
        ClassificationCandidate::Metar(candidate) => assemble_from_metar(candidate, filename),
        ClassificationCandidate::Taf(candidate) => assemble_from_taf(candidate, filename),
        ClassificationCandidate::Dcp(candidate) => assemble_from_dcp(candidate, filename),
        ClassificationCandidate::MalformedFamily(candidate) => {
            assemble_from_malformed_family(candidate, filename)
        }
        ClassificationCandidate::NonText(candidate) => assemble_from_non_text(candidate),
        ClassificationCandidate::UnsupportedWmo(candidate) => {
            assemble_from_unsupported_wmo(candidate, filename)
        }
        ClassificationCandidate::TextParseFailure(error) => {
            assemble_from_text_parse_failure(filename, error)
        }
        ClassificationCandidate::Unknown => assemble_unknown(filename, raw_bytes),
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
        header,
        code,
        message,
        line,
    } = candidate;

    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::WmoBulletin,
        family: Some("unsupported_wmo_bulletin"),
        title: None,
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
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};

    use crate::body::BodyInputFormat;
    use crate::specialized::dcp::parse_dcp_bulletin;
    use crate::specialized::fd::parse_fd_bulletin;
    use crate::specialized::metar::{parse_metar_bulletin, MetarBulletin};
    use crate::specialized::pirep::parse_pirep_bulletin;
    use crate::specialized::sigmet::parse_sigmet_bulletin;
    use crate::specialized::taf::parse_taf_bulletin;
    use crate::ParserError;
    use crate::{
        Cf6Bulletin, Cf6DayRow, CwaBulletin, CwaGeometry, CwaGeometryKind, DsmBulletin, DsmSummary,
        GeoPoint, HmlBulletin, HmlDatum, HmlDocument, HmlSeries, LsrBulletin, LsrReport,
        MosBulletin, MosForecastRow, MosSection, ProductArtifact, ProductEnrichmentSource,
        ProductParseIssue, SawAction, SawBulletin, SelBulletin, SpcWatchType, WwpBulletin,
    };

    use super::assemble_product_enrichment;
    use crate::pipeline::candidate::{
        BodyContributionRequest, Cf6Candidate, ClassificationCandidate, CwaCandidate, DcpCandidate,
        DsmCandidate, FdCandidate, HmlCandidate, LsrCandidate, MetarCandidate, MosCandidate,
        PirepCandidate, SawCandidate, SelCandidate, SigmetCandidate, TafCandidate,
        TextGenericCandidate, UnsupportedWmoCandidate, WwpCandidate,
    };

    fn text_header(afos: &str) -> crate::TextProductHeader {
        crate::TextProductHeader {
            ttaaii: "FTUS42".to_string(),
            cccc: "KFFC".to_string(),
            ddhhmm: "022320".to_string(),
            bbb: None,
            afos: afos.to_string(),
        }
    }

    fn wmo_header(ttaaii: &str, cccc: &str) -> crate::WmoHeader {
        crate::WmoHeader {
            ttaaii: ttaaii.to_string(),
            cccc: cccc.to_string(),
            ddhhmm: "070200".to_string(),
            bbb: None,
        }
    }

    #[test]
    fn assembles_text_generic_product_shape() {
        let candidate = ClassificationCandidate::TextGeneric(TextGenericCandidate {
            header: text_header("TAFPDK"),
            pil: Some("TAF".to_string()),
            title: Some("Terminal Aerodrome Forecast"),
            body_request: None,
            bbb_kind: None,
            reference_time: Some(Utc::now()),
        });

        let enrichment = assemble_product_enrichment(candidate, "TAFPDKGA.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.pil.as_deref(), Some("TAF"));
        assert_eq!(enrichment.family, Some("nws_text_product"));
    }

    #[test]
    fn assembles_fd_candidate_shape() {
        let reference_time = Utc
            .with_ymd_and_hms(2025, 3, 7, 0, 0, 0)
            .single()
            .expect("valid reference time");
        let bulletin = parse_fd_bulletin(
            "DATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
            Some("FD1US1"),
            reference_time,
        )
        .expect("fd bulletin should parse");
        let candidate = ClassificationCandidate::Fd(FdCandidate {
            source: crate::ProductEnrichmentSource::TextHeader,
            family: "fd_bulletin",
            title: "Winds and temperatures aloft",
            header: Some(text_header("FD1US1")),
            wmo_header: None,
            pil: Some("FD1".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
        });

        let enrichment = assemble_product_enrichment(candidate, "FD1US1.TXT", b"ignored");

        assert_eq!(enrichment.family, Some("fd_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_fd)
            .is_some());
    }

    #[test]
    fn assembles_pirep_candidate_shape() {
        let bulletin = parse_pirep_bulletin("DEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n")
            .expect("pirep bulletin should parse");
        let candidate = ClassificationCandidate::Pirep(PirepCandidate {
            source: ProductEnrichmentSource::TextHeader,
            header: Some(text_header("PIRBOU")),
            wmo_header: None,
            pil: Some("PIR".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
        });

        let enrichment = assemble_product_enrichment(candidate, "PIRBOU.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_pirep)
            .is_some());
    }

    #[test]
    fn assembles_sigmet_candidate_shape() {
        let bulletin = parse_sigmet_bulletin(
            "CONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
        )
        .expect("sigmet bulletin should parse");
        let candidate = ClassificationCandidate::Sigmet(SigmetCandidate {
            source: crate::ProductEnrichmentSource::TextHeader,
            header: Some(text_header("SIGABC")),
            wmo_header: None,
            pil: Some("SIG".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "SIGABC.TXT", b"ignored");

        assert_eq!(enrichment.family, Some("sigmet_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_sigmet)
            .is_some());
    }

    #[test]
    fn assembles_lsr_candidate_shape() {
        let issues = vec![ProductParseIssue::new(
            "lsr_parse",
            "invalid_lsr_report",
            "could not parse LSR report block",
            Some("bad chunk".to_string()),
        )];
        let bulletin = LsrBulletin {
            reports: vec![LsrReport {
                valid: "2026-03-10T01:50:00+00:00".to_string(),
                event_text: "HAIL".to_string(),
                city: "BROOKSVILLE".to_string(),
                county: Some("WINSTON".to_string()),
                state: Some("AL".to_string()),
                latitude: 34.40,
                longitude: -87.70,
                source: Some("PUBLIC".to_string()),
                remark: Some("QUARTER SIZE HAIL".to_string()),
                magnitude_value: Some(1.0),
                magnitude_units: Some("IN".to_string()),
                magnitude_qualifier: None,
            }],
            is_summary: false,
        };
        let candidate = ClassificationCandidate::Lsr(LsrCandidate {
            header: text_header("LSRBMX"),
            pil: Some("LSR".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues,
        });

        let enrichment = assemble_product_enrichment(candidate, "LSRBMX.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.family, Some("lsr_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_lsr)
            .is_some());
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_cwa)
            .is_none());
        assert!(enrichment.body.is_none());
        assert_eq!(enrichment.issues.len(), 1);
    }

    #[test]
    fn assembles_cwa_candidate_shape() {
        let bulletin = CwaBulletin {
            center: "ZLC".to_string(),
            number: 202,
            issue_time: "2026-03-10T02:29:00+00:00".to_string(),
            expire_time: "2026-03-10T04:30:00+00:00".to_string(),
            is_corrected: false,
            is_cancelled: false,
            narrative: Some("AREA TS.".to_string()),
            geometry: Some(CwaGeometry {
                kind: CwaGeometryKind::Polygon,
                points: vec![
                    GeoPoint {
                        lat: 40.7884,
                        lon: -111.9778,
                    },
                    GeoPoint {
                        lat: 44.7692,
                        lon: -106.9803,
                    },
                ],
            }),
        };
        let candidate = ClassificationCandidate::Cwa(CwaCandidate {
            header: None,
            wmo_header: Some(wmo_header("FAUS22", "KZLC")),
            pil: Some("CWA".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "CWAZLC.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::WmoBulletin
        );
        assert_eq!(enrichment.family, Some("cwa_bulletin"));
        assert!(enrichment.header.is_none());
        assert!(enrichment.wmo_header.is_some());
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_cwa)
            .is_some());
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_wwp)
            .is_none());
        assert!(enrichment.body.is_none());
    }

    #[test]
    fn assembles_wwp_candidate_shape() {
        let bulletin = WwpBulletin {
            watch_type: SpcWatchType::Tornado,
            watch_number: 31,
            prob_tornadoes_2_or_more: 20,
            prob_tornadoes_1_or_more_strong: 10,
            prob_severe_wind_10_or_more: 70,
            prob_wind_1_or_more_65kt: 40,
            prob_severe_hail_10_or_more: 60,
            prob_hail_1_or_more_2inch: 30,
            prob_combined_hail_wind_6_or_more: 95,
            max_hail_inches: 2.0,
            max_wind_gust_knots: 70,
            max_tops_feet: 50_000,
            storm_motion_degrees: 240,
            storm_motion_knots: 35,
            is_pds: false,
        };
        let candidate = ClassificationCandidate::Wwp(WwpCandidate {
            header: text_header("WWP1"),
            pil: Some("WWP".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "WWP1.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.family, Some("wwp_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_wwp)
            .is_some());
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_cf6)
            .is_none());
        assert!(enrichment.body.is_none());
    }

    #[test]
    fn assembles_saw_candidate_with_body_shape() {
        let candidate = ClassificationCandidate::Saw(SawCandidate {
            header: text_header("SAW2"),
            pil: Some("SAW".to_string()),
            bbb_kind: None,
            body_request: Some(BodyContributionRequest {
                text: "MAZ000-RIZ000-CWZ000-\nLAT...LON 41087082 39507704 41247704 42827082\n"
                    .to_string(),
                plan: crate::body::body_extraction_plan(&[
                    crate::BodyExtractorId::Ugc,
                    crate::BodyExtractorId::LatLon,
                ]),
                reference_time: Some(Utc::now()),
                input_format: BodyInputFormat::PlainText,
            }),
            bulletin: SawBulletin {
                saw_number: 2,
                watch_number: 542,
                watch_type: SpcWatchType::SevereThunderstorm,
                action: SawAction::Issue,
                is_test: false,
                replaces_watch_number: None,
                valid_from: Some("2025-07-25T17:45:00+00:00".to_string()),
                valid_to: Some("2025-07-26T01:00:00+00:00".to_string()),
                polygon: Some(vec![GeoPoint {
                    lat: 41.08,
                    lon: -70.82,
                }]),
            },
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "SAW2.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.family, Some("saw_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_saw)
            .is_some());
        assert!(enrichment.body.is_some());
    }

    #[test]
    fn assembles_sel_candidate_with_body_shape() {
        let candidate = ClassificationCandidate::Sel(SelCandidate {
            header: text_header("SEL2"),
            pil: Some("SEL".to_string()),
            bbb_kind: None,
            body_request: Some(BodyContributionRequest {
                text: "IAC001-022320-\n".to_string(),
                plan: crate::body::body_extraction_plan(&[crate::BodyExtractorId::Ugc]),
                reference_time: Some(Utc::now()),
                input_format: BodyInputFormat::PlainText,
            }),
            bulletin: SelBulletin {
                watch_number: 542,
                watch_type: SpcWatchType::SevereThunderstorm,
                is_test: false,
            },
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "SEL2.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.family, Some("sel_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_sel)
            .is_some());
        assert!(enrichment.body.is_some());
    }

    #[test]
    fn assembles_cf6_candidate_shape() {
        let issues = vec![ProductParseIssue::new(
            "cf6_parse",
            "trace_value_flattened",
            "trace precipitation or snow value flattened to 0.0",
            Some("1 70 50 ...".to_string()),
        )];
        let bulletin = Cf6Bulletin {
            station: "TEST STATION".to_string(),
            month: 3,
            year: 2026,
            rows: vec![Cf6DayRow {
                day: 1,
                max_temp_f: Some(70),
                min_temp_f: Some(50),
                avg_temp_f: Some(60),
                departure_f: Some(0),
                heating_degree_days: Some(5),
                cooling_degree_days: Some(0),
                precip_inches: Some(0.0),
                snow_inches: Some(0.0),
                snow_depth_inches: Some(0.0),
                avg_wind_mph: Some(8.5),
                max_wind_mph: Some(20),
                avg_wind_dir_degrees: Some(180),
                minutes_sunshine: Some(600),
                possible_sunshine_minutes: Some(720),
                sky_cover: Some("CLR".to_string()),
                weather_codes: Some("RA".to_string()),
                gust_mph: Some(30),
                gust_dir_degrees: Some(190),
            }],
        };
        let candidate = ClassificationCandidate::Cf6(Cf6Candidate {
            header: text_header("CF6GSN"),
            pil: Some("CF6".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues,
        });

        let enrichment = assemble_product_enrichment(candidate, "CF6GSN.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.family, Some("cf6_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_cf6)
            .is_some());
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_dsm)
            .is_none());
        assert!(enrichment.body.is_none());
        assert_eq!(enrichment.issues.len(), 1);
    }

    #[test]
    fn assembles_dsm_candidate_shape() {
        let bulletin = DsmBulletin {
            summaries: vec![DsmSummary {
                station: "KCQC".to_string(),
                date: "2026-03-10".to_string(),
                max_temp_f: Some(63),
                max_temp_time: Some("2026-03-10T15:53:00+00:00".to_string()),
                min_temp_f: Some(40),
                min_temp_time: Some("2026-03-10T06:27:00+00:00".to_string()),
                coop_max_temp_f: Some(63),
                coop_min_temp_f: Some(40),
                min_sea_level_pressure_mb_tenths: Some(9671),
                min_slp_time: Some("2026-03-10T06:08:00+00:00".to_string()),
                precip_day_inches: Some(0.0),
                hourly_precip_inches: vec![Some(0.0); 24],
                avg_wind_mph: Some(28.0),
                max_wind_mph: Some(28.0),
                max_wind_time: Some("2026-03-10T20:59:00+00:00".to_string()),
                max_wind_dir_degrees: Some(280),
                max_gust_mph: Some(43.0),
                max_gust_time: Some("2026-03-10T15:31:00+00:00".to_string()),
                max_gust_dir_degrees: Some(290),
            }],
        };
        let candidate = ClassificationCandidate::Dsm(DsmCandidate {
            source: ProductEnrichmentSource::TextHeader,
            header: Some(text_header("DSMCQC")),
            wmo_header: None,
            pil: Some("DSM".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "DSMCQC.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.family, Some("dsm_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_dsm)
            .is_some());
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_hml)
            .is_none());
        assert!(enrichment.body.is_none());
    }

    #[test]
    fn assembles_hml_candidate_shape() {
        let bulletin = HmlBulletin {
            documents: vec![HmlDocument {
                station_id: "AAMC1".to_string(),
                station_name: Some("ARROYO SECO".to_string()),
                originator: Some("MTR".to_string()),
                generation_time: Some("2026-03-10T00:02:00Z".to_string()),
                observed: Some(HmlSeries {
                    issued: Some("2026-03-10T00:00:00Z".to_string()),
                    primary_name: Some("Stage".to_string()),
                    primary_units: Some("FT".to_string()),
                    secondary_name: None,
                    secondary_units: None,
                    rows: vec![HmlDatum {
                        valid: "2026-03-10T00:00:00Z".to_string(),
                        primary: Some(2.5),
                        secondary: None,
                    }],
                }),
                forecast: None,
            }],
        };
        let candidate = ClassificationCandidate::Hml(HmlCandidate {
            header: text_header("HMLMTR"),
            pil: Some("HML".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "HMLMTR.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.family, Some("hml_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_hml)
            .is_some());
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_mos)
            .is_none());
        assert!(enrichment.body.is_none());
    }

    #[test]
    fn assembles_mos_candidate_shape() {
        let mut values = BTreeMap::new();
        values.insert("TMP".to_string(), "20".to_string());
        values.insert("WSP".to_string(), "05".to_string());
        let bulletin = MosBulletin {
            sections: vec![MosSection {
                station: "KBCK".to_string(),
                model: "NAM".to_string(),
                runtime: "2026-03-10T00:00:00Z".to_string(),
                forecasts: vec![MosForecastRow {
                    valid: "2026-03-10T00:00:00Z".to_string(),
                    values,
                }],
            }],
        };
        let candidate = ClassificationCandidate::Mos(MosCandidate {
            header: text_header("METNC1"),
            pil: Some("MET".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "METNC1.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.family, Some("mos_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_mos)
            .is_some());
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_lsr)
            .is_none());
        assert!(enrichment.body.is_none());
    }

    #[test]
    fn assembles_metar_candidate_shape() {
        let (bulletin, issues) = parse_metar_bulletin(
            "METAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n",
        )
        .expect("metar bulletin should parse");
        let candidate = ClassificationCandidate::Metar(MetarCandidate {
            source: ProductEnrichmentSource::WmoBulletin,
            header: None,
            wmo_header: Some(wmo_header("SAGL31", "BGGH")),
            pil: None,
            bbb_kind: None,
            body_request: None,
            bulletin,
            issues,
        });

        let enrichment = assemble_product_enrichment(candidate, "SAGL31.TXT", b"ignored");

        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_metar)
                .map(MetarBulletin::report_count),
            Some(1)
        );
    }

    #[test]
    fn assembles_taf_candidate_shape() {
        let bulletin = parse_taf_bulletin("TAF AMD\nWBCF 070244Z 0703/0803 18012KT P6SM SCT050\n")
            .expect("taf bulletin should parse");
        let candidate = ClassificationCandidate::Taf(TafCandidate {
            source: ProductEnrichmentSource::WmoBulletin,
            header: None,
            wmo_header: Some(wmo_header("FTXX01", "KWBC")),
            pil: None,
            bbb_kind: None,
            body_request: None,
            bulletin,
        });

        let enrichment = assemble_product_enrichment(candidate, "TAFWBCFJ.TXT", b"ignored");

        assert_eq!(enrichment.family, Some("taf_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_taf)
            .is_some());
    }

    #[test]
    fn assembles_dcp_candidate_shape() {
        let header = wmo_header("SXMS50", "KWAL");
        let bulletin = parse_dcp_bulletin(
            "MISDCPSV.TXT",
            &header,
            "83786162 066025814\n16.23\n003\n137\n071\n088\n12.9\n137\n007\n00000\n 42-0NN  45E\n",
        )
        .expect("dcp bulletin should parse");
        let candidate = ClassificationCandidate::Dcp(DcpCandidate { header, bulletin });

        let enrichment = assemble_product_enrichment(candidate, "MISDCPSV.TXT", b"ignored");

        assert_eq!(enrichment.family, Some("dcp_telemetry_bulletin"));
        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_dcp)
            .is_some());
    }

    #[test]
    fn assembles_unsupported_wmo_candidate_shape() {
        let candidate = ClassificationCandidate::UnsupportedWmo(UnsupportedWmoCandidate {
            header: wmo_header("WAAB31", "LATI"),
            code: "unsupported_airmet_bulletin",
            message: "recognized valid WMO AIRMET bulletin, but textual AIRMET parsing is not implemented",
            line: Some("LAAA AIRMET 1 VALID 090100/090500 LATI-".to_string()),
        });

        let enrichment = assemble_product_enrichment(candidate, "WAAB31.TXT", b"ignored");

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::WmoBulletin
        );
        assert_eq!(enrichment.issues[0].code, "unsupported_airmet_bulletin");
    }

    #[test]
    fn assembles_text_parse_failure_issue_shape() {
        let enrichment = assemble_product_enrichment(
            ClassificationCandidate::TextParseFailure(ParserError::InvalidWmoHeader {
                line: "INVALID HEADER".to_string(),
            }),
            "TAFPDKGA.TXT",
            b"ignored",
        );

        assert_eq!(
            enrichment.source,
            crate::ProductEnrichmentSource::TextHeader
        );
        assert_eq!(enrichment.issues[0].code, "invalid_wmo_header");
    }

    #[test]
    fn assembles_unknown_non_text_shape() {
        let enrichment = assemble_product_enrichment(
            ClassificationCandidate::Unknown,
            "mystery.bin",
            b"ignored",
        );

        assert_eq!(enrichment.source, crate::ProductEnrichmentSource::Unknown);
        assert_eq!(enrichment.container, "raw");
        assert!(enrichment.family.is_none());
    }

    #[test]
    fn text_generic_candidate_assembles_body_from_plan() {
        let candidate = ClassificationCandidate::TextGeneric(TextGenericCandidate {
            header: text_header("TAFPDK"),
            pil: Some("TAF".to_string()),
            title: Some("Terminal Aerodrome Forecast"),
            body_request: Some(BodyContributionRequest {
                text: "/O.NEW.KDMX.TO.W.0001.250301T1200Z-250301T1300Z/".to_string(),
                plan: crate::data::text_product_catalog_entry("SVR")
                    .and_then(crate::data::body_extraction_plan_for_entry)
                    .expect("SVR should have body extraction plan"),
                reference_time: Some(Utc::now()),
                input_format: BodyInputFormat::PlainText,
            }),
            bbb_kind: None,
            reference_time: Some(Utc::now()),
        });

        let enrichment = assemble_product_enrichment(candidate, "TAFPDKGA.TXT", b"ignored");

        assert!(enrichment.body.is_some());
        assert!(enrichment
            .body
            .as_ref()
            .and_then(|body| body.as_vtec_event())
            .is_some());
    }

    #[test]
    fn specialized_candidates_without_body_request_remain_bodyless() {
        let bulletin = parse_pirep_bulletin("DEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n")
            .expect("pirep bulletin should parse");
        let candidate = ClassificationCandidate::Pirep(PirepCandidate {
            source: ProductEnrichmentSource::TextHeader,
            header: Some(text_header("PIRBOU")),
            wmo_header: None,
            pil: Some("PIR".to_string()),
            bbb_kind: None,
            body_request: None,
            bulletin,
        });

        let enrichment = assemble_product_enrichment(candidate, "PIRBOU.TXT", b"ignored");

        assert!(enrichment.body.is_none());
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn body_request_issues_are_appended_to_text_generic_output() {
        let candidate = ClassificationCandidate::TextGeneric(TextGenericCandidate {
            header: text_header("ZZZBOX"),
            pil: None,
            title: None,
            body_request: Some(BodyContributionRequest {
                text: "plain text".to_string(),
                plan: crate::body::body_extraction_plan(&[
                    crate::body::BodyExtractorId::TimeMotLoc,
                ]),
                reference_time: None,
                input_format: BodyInputFormat::PlainText,
            }),
            bbb_kind: None,
            reference_time: None,
        });

        let enrichment = assemble_product_enrichment(candidate, "ZZZBOX.TXT", b"ignored");

        assert_eq!(enrichment.issues.len(), 1);
        assert_eq!(enrichment.issues[0].code, "missing_reference_time");
    }

    #[test]
    fn specialized_candidate_with_body_request_assembles_both_artifact_and_body() {
        let bulletin = parse_sigmet_bulletin(
            "CONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
        )
        .expect("sigmet bulletin should parse");
        let candidate = ClassificationCandidate::Sigmet(SigmetCandidate {
            source: crate::ProductEnrichmentSource::TextHeader,
            header: Some(text_header("SIGABC")),
            wmo_header: None,
            pil: Some("SIG".to_string()),
            bbb_kind: None,
            body_request: Some(BodyContributionRequest {
                text: "IAC001-011300-\n/O.NEW.KDMX.TO.W.0001.250301T1200Z-250301T1300Z/\nLAT...LON 4143 9613 4145 9610 4140 9608 4138 9612\n".to_string(),
                plan: crate::body::body_extraction_plan(&[
                    crate::body::BodyExtractorId::VtecEvents,
                ]),
                reference_time: Some(Utc::now()),
                input_format: BodyInputFormat::PlainText,
            }),
            bulletin,
            issues: Vec::new(),
        });

        let enrichment = assemble_product_enrichment(candidate, "SIGABC.TXT", b"ignored");

        assert!(enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_sigmet)
            .is_some());
        assert!(enrichment.body.is_some());
        assert!(enrichment
            .body
            .as_ref()
            .and_then(|body| body.as_vtec_event())
            .is_some());
    }
}
