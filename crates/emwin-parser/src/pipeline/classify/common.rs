//! Shared helpers used by AFOS and WMO classification strategies.

use chrono::{DateTime, Utc};

use crate::body::{BodyExtractionPlan, BodyInputFormat};
use crate::pipeline::candidate::{
    BodyContributionRequest, ClassificationCandidate, MalformedFamilyCandidate,
    UnsupportedWmoCandidate,
};
use crate::{BbbKind, ProductEnrichmentSource, TextProductHeader, WmoHeader};

/// Returns the filename stem without path or extension.
pub(super) fn filename_stem(filename: &str) -> &str {
    filename
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .unwrap_or(filename)
        .split_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(filename)
}

/// Returns the first non-empty line from conditioned body text.
pub(super) fn first_nonempty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

/// Checks whether the line begins with `<CCCC> SIGMET`.
pub(super) fn starts_with_icao_sigmet_line(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    let Some(origin) = parts.next() else {
        return false;
    };
    let Some(sigmet) = parts.next() else {
        return false;
    };
    origin.len() == 4 && origin.chars().all(|ch| ch.is_ascii_uppercase()) && sigmet == "SIGMET"
}

pub(super) fn looks_like_multipart_taf_bulletin(body_text: &str) -> bool {
    body_text.lines().map(str::trim).any(|line| {
        let upper = line.to_ascii_uppercase();
        upper.contains("PART ")
            && upper.contains(" OF ")
            && upper.contains(" TAF ")
            && upper.split_whitespace().nth(1).is_some_and(is_ascii_digits)
            && upper.split_whitespace().nth(3).is_some_and(is_ascii_digits)
    })
}

/// Builds a generic-body contribution request from catalog policy and conditioned text.
pub(super) fn build_body_request(
    body_plan: Option<BodyExtractionPlan>,
    body_text: &str,
    reference_time: Option<DateTime<Utc>>,
) -> Option<BodyContributionRequest> {
    body_plan.map(|plan| BodyContributionRequest {
        text: body_text.to_string(),
        plan,
        reference_time,
        input_format: detect_body_input_format(body_text),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn malformed_supported_family(
    source: ProductEnrichmentSource,
    family: &'static str,
    title: &'static str,
    header: Option<TextProductHeader>,
    wmo_header: Option<WmoHeader>,
    pil: Option<String>,
    bbb_kind: Option<BbbKind>,
    body_request: Option<BodyContributionRequest>,
    kind: &'static str,
    code: &'static str,
    message: &'static str,
    line: Option<&str>,
) -> ClassificationCandidate {
    ClassificationCandidate::MalformedFamily(MalformedFamilyCandidate {
        source,
        family,
        title,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request,
        issues: vec![crate::ProductParseIssue::new(
            kind,
            code,
            message,
            line.map(str::to_string),
        )],
    })
}

/// Produces a stable unsupported-WMO candidate anchored to the first content line.
pub(super) fn unsupported_wmo_candidate(
    header: &WmoHeader,
    code: &'static str,
    message: &'static str,
    body_text: &str,
) -> ClassificationCandidate {
    unsupported_wmo_family_candidate(
        header,
        "unsupported_wmo_bulletin",
        None,
        code,
        message,
        body_text,
    )
}

/// Produces an unsupported-WMO candidate with an explicit public family identity.
pub(super) fn unsupported_wmo_family_candidate(
    header: &WmoHeader,
    family: &'static str,
    title: Option<&'static str>,
    code: &'static str,
    message: &'static str,
    body_text: &str,
) -> ClassificationCandidate {
    ClassificationCandidate::UnsupportedWmo(UnsupportedWmoCandidate {
        family,
        title,
        header: header.clone(),
        code,
        message,
        line: first_nonempty_line(body_text).map(str::to_string),
    })
}

fn detect_body_input_format(body_text: &str) -> BodyInputFormat {
    let trimmed = body_text.trim_start_matches(|character: char| character.is_ascii_control());
    if trimmed.starts_with("<?xml") || trimmed.starts_with("<alert") {
        BodyInputFormat::CapXml
    } else {
        BodyInputFormat::PlainText
    }
}

fn is_ascii_digits(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
}
