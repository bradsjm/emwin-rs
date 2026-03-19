use super::super::candidate::{MalformedFamilyCandidate, UnsupportedWmoCandidate};
use super::*;
use crate::data::NonTextProductMeta;

/// Assembles a recognized supported family that could not produce a structured artifact.
pub(super) fn assemble_from_malformed_family(
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
pub(super) fn assemble_from_non_text(candidate: NonTextProductMeta) -> ProductEnrichment {
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
pub(super) fn assemble_from_unsupported_wmo(
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
pub(super) fn assemble_from_text_parse_failure(
    filename: &str,
    error: ParserError,
) -> ProductEnrichment {
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
pub(super) fn assemble_unknown(filename: &str, raw_bytes: &[u8]) -> ProductEnrichment {
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
