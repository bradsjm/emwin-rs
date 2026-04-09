//! Shared helpers used by AFOS and WMO classification strategies.

use chrono::{DateTime, Utc};

use crate::body::{BodyExtractionPlan, BodyInputFormat};
use crate::data::TextProductRouting;
use crate::pipeline::candidate::{
    BodyContributionRequest, ClassificationCandidate, MalformedFamilyCandidate,
    UnsupportedWmoCandidate,
};
use crate::{BbbKind, ProductEnrichmentSource, TextProductHeader, WmoHeader};

use super::context::{TextClassificationContext, WmoClassificationContext};

pub(super) struct SupportedFamilySpec {
    pub(super) source: ProductEnrichmentSource,
    pub(super) family: &'static str,
    pub(super) title: &'static str,
    pub(super) issue_kind: &'static str,
    pub(super) invalid_code: &'static str,
    pub(super) invalid_message: &'static str,
    pub(super) missing_reference_code: Option<&'static str>,
    pub(super) missing_reference_message: Option<&'static str>,
    pub(super) malformed_pil: Option<&'static str>,
}

pub(super) struct TextCandidateParts {
    pub(super) header: TextProductHeader,
    pub(super) pil: Option<String>,
    pub(super) bbb_kind: Option<BbbKind>,
    pub(super) body_request: Option<BodyContributionRequest>,
}

pub(super) struct WmoCandidateParts {
    pub(super) header: WmoHeader,
}

pub(super) enum SupportedFamilyFailure {
    MissingReferenceTime,
    ParseFailure,
}

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

pub(super) fn classify_supported_text<T, P, B>(
    context: &TextClassificationContext<'_>,
    routing: TextProductRouting,
    guard: fn(&str, &str) -> bool,
    spec: &SupportedFamilySpec,
    parse: P,
    build: B,
) -> Option<ClassificationCandidate>
where
    P: FnOnce(
        &TextClassificationContext<'_>,
    ) -> Result<(T, Vec<crate::ProductParseIssue>), SupportedFamilyFailure>,
    B: FnOnce(TextCandidateParts, T, Vec<crate::ProductParseIssue>) -> ClassificationCandidate,
{
    if !context.has_routing(routing) || !guard(&context.header.afos, context.body_text) {
        return None;
    }

    match parse(context) {
        Ok((bulletin, issues)) => Some(build(text_candidate_parts(context), bulletin, issues)),
        Err(SupportedFamilyFailure::MissingReferenceTime) => {
            Some(text_missing_reference_time(context, spec))
        }
        Err(SupportedFamilyFailure::ParseFailure) => Some(text_invalid_family(context, spec)),
    }
}

pub(super) fn classify_supported_text_guarded<T, P, B>(
    context: &TextClassificationContext<'_>,
    guard: fn(&str, &str) -> bool,
    spec: &SupportedFamilySpec,
    parse: P,
    build: B,
) -> Option<ClassificationCandidate>
where
    P: FnOnce(
        &TextClassificationContext<'_>,
    ) -> Result<(T, Vec<crate::ProductParseIssue>), SupportedFamilyFailure>,
    B: FnOnce(TextCandidateParts, T, Vec<crate::ProductParseIssue>) -> ClassificationCandidate,
{
    if !guard(&context.header.afos, context.body_text) {
        return None;
    }

    match parse(context) {
        Ok((bulletin, issues)) => Some(build(text_candidate_parts(context), bulletin, issues)),
        Err(SupportedFamilyFailure::MissingReferenceTime) => {
            Some(text_missing_reference_time(context, spec))
        }
        Err(SupportedFamilyFailure::ParseFailure) => Some(text_invalid_family(context, spec)),
    }
}

pub(super) fn classify_supported_wmo<T, P, B>(
    context: &WmoClassificationContext<'_>,
    guard: impl FnOnce(&WmoClassificationContext<'_>) -> bool,
    spec: &SupportedFamilySpec,
    parse: P,
    build: B,
) -> Option<ClassificationCandidate>
where
    P: FnOnce(
        &WmoClassificationContext<'_>,
    ) -> Result<(T, Vec<crate::ProductParseIssue>), SupportedFamilyFailure>,
    B: FnOnce(WmoCandidateParts, T, Vec<crate::ProductParseIssue>) -> ClassificationCandidate,
{
    if !guard(context) {
        return None;
    }

    match parse(context) {
        Ok((bulletin, issues)) => Some(build(wmo_candidate_parts(context), bulletin, issues)),
        Err(SupportedFamilyFailure::MissingReferenceTime) => {
            Some(wmo_missing_reference_time(context, spec))
        }
        Err(SupportedFamilyFailure::ParseFailure) => Some(wmo_invalid_family(context, spec)),
    }
}

pub(super) fn parsed<T>(
    bulletin: T,
) -> Result<(T, Vec<crate::ProductParseIssue>), SupportedFamilyFailure> {
    Ok((bulletin, Vec::new()))
}

pub(super) fn parsed_with_issues<T>(
    value: (T, Vec<crate::ProductParseIssue>),
) -> Result<(T, Vec<crate::ProductParseIssue>), SupportedFamilyFailure> {
    Ok(value)
}

pub(super) fn require_reference_time(
    reference_time: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>, SupportedFamilyFailure> {
    reference_time.ok_or(SupportedFamilyFailure::MissingReferenceTime)
}

pub(super) fn text_candidate_parts(context: &TextClassificationContext<'_>) -> TextCandidateParts {
    TextCandidateParts {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: context.body_request(),
    }
}

pub(super) fn wmo_candidate_parts(context: &WmoClassificationContext<'_>) -> WmoCandidateParts {
    WmoCandidateParts {
        header: context.header.clone(),
    }
}

pub(super) fn text_invalid_family(
    context: &TextClassificationContext<'_>,
    spec: &SupportedFamilySpec,
) -> ClassificationCandidate {
    malformed_supported_family(
        spec.source,
        spec.family,
        spec.title,
        Some(context.header.clone()),
        None,
        context.pil.clone(),
        context.bbb_kind,
        context.body_request(),
        spec.issue_kind,
        spec.invalid_code,
        spec.invalid_message,
        first_nonempty_line(context.body_text),
    )
}

pub(super) fn text_missing_reference_time(
    context: &TextClassificationContext<'_>,
    spec: &SupportedFamilySpec,
) -> ClassificationCandidate {
    malformed_supported_family(
        spec.source,
        spec.family,
        spec.title,
        Some(context.header.clone()),
        None,
        context.pil.clone(),
        context.bbb_kind,
        context.body_request(),
        spec.issue_kind,
        spec.missing_reference_code.unwrap_or(spec.invalid_code),
        spec.missing_reference_message
            .unwrap_or(spec.invalid_message),
        first_nonempty_line(context.body_text),
    )
}

pub(super) fn wmo_invalid_family(
    context: &WmoClassificationContext<'_>,
    spec: &SupportedFamilySpec,
) -> ClassificationCandidate {
    malformed_supported_family(
        spec.source,
        spec.family,
        spec.title,
        None,
        Some(context.header.clone()),
        spec.malformed_pil.map(str::to_string),
        None,
        None,
        spec.issue_kind,
        spec.invalid_code,
        spec.invalid_message,
        first_nonempty_line(context.body_text),
    )
}

pub(super) fn wmo_missing_reference_time(
    context: &WmoClassificationContext<'_>,
    spec: &SupportedFamilySpec,
) -> ClassificationCandidate {
    malformed_supported_family(
        spec.source,
        spec.family,
        spec.title,
        None,
        Some(context.header.clone()),
        spec.malformed_pil.map(str::to_string),
        None,
        None,
        spec.issue_kind,
        spec.missing_reference_code.unwrap_or(spec.invalid_code),
        spec.missing_reference_message
            .unwrap_or(spec.invalid_message),
        first_nonempty_line(context.body_text),
    )
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
