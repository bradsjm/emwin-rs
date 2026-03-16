//! Strategy-based classification for parsed envelopes.
//!
//! Phase 2 moves specialized parser selection out of assembly and into ordered
//! registries. Each strategy either returns a fully parsed candidate or yields
//! `None`, allowing later strategies to run without panicking or reparsing.

use chrono::{DateTime, Utc};

use crate::body::BodyInputFormat;
use crate::data::{
    ResolvedTextProductPolicy, TextProductBodyBehavior, TextProductRouting,
    resolved_text_product_policy,
};
use crate::specialized::cf6::parse_cf6_bulletin;
use crate::specialized::cli::parse_cli_bulletin;
use crate::specialized::cwa::parse_cwa_bulletin;
use crate::specialized::dcp::parse_dcp_bulletin;
use crate::specialized::dsm::parse_dsm_bulletin;
use crate::specialized::ero::parse_ero_bulletin;
use crate::specialized::fd::parse_fd_bulletin;
use crate::specialized::hml::parse_hml_bulletin;
use crate::specialized::lsr::parse_lsr_bulletin;
use crate::specialized::mcd::parse_mcd_bulletin;
use crate::specialized::metar::parse_metar_bulletin;
use crate::specialized::mos::parse_mos_bulletin;
use crate::specialized::pirep::parse_pirep_bulletin;
use crate::specialized::saw::parse_saw_bulletin;
use crate::specialized::sel::parse_sel_bulletin;
use crate::specialized::sigmet::parse_sigmet_bulletin;
use crate::specialized::spc_outlook::parse_spc_outlook_bulletin;
use crate::specialized::taf::parse_taf_bulletin;
use crate::specialized::wwp::parse_wwp_bulletin;
use crate::{BbbKind, ProductEnrichmentSource, TextProductHeader, WmoHeader, enrich_header};

use super::candidate::{
    BodyContributionRequest, Cf6Candidate, ClassificationCandidate, CliCandidate, CwaCandidate,
    DcpCandidate, DsmCandidate, EroCandidate, FdCandidate, HmlCandidate, LsrCandidate,
    MalformedFamilyCandidate, McdCandidate, MetarCandidate, MosCandidate, PirepCandidate,
    SawCandidate, SelCandidate, SigmetCandidate, SpcOutlookCandidate, TafCandidate,
    TextGenericCandidate, UnsupportedWmoCandidate, WwpCandidate,
};
use super::{EnvelopeKind, ParsedEnvelope};

type TextStrategy = for<'a> fn(&TextClassificationContext<'a>) -> Option<ClassificationCandidate>;
type WmoStrategy = for<'a> fn(&WmoClassificationContext<'a>) -> Option<ClassificationCandidate>;

const TEXT_STRATEGIES: &[TextStrategy] = &[
    classify_text_fd,
    classify_text_metar,
    classify_text_taf,
    classify_text_pirep,
    classify_text_sigmet,
    classify_text_lsr,
    classify_text_cli,
    classify_text_cwa,
    classify_text_wwp,
    classify_text_saw,
    classify_text_sel,
    classify_text_cf6,
    classify_text_dsm,
    classify_text_hml,
    classify_text_mos,
    classify_text_mcd,
    classify_text_ero,
    classify_text_spc_outlook,
];

const WMO_STRATEGIES: &[WmoStrategy] = &[
    classify_wmo_fd,
    classify_wmo_pirep,
    classify_wmo_dsm,
    classify_wmo_metar,
    classify_wmo_taf,
    classify_wmo_dcp,
    classify_wmo_sigmet,
    classify_wmo_cwa,
    classify_wmo_airmet_unsupported,
    classify_wmo_surface_observation_unsupported,
    classify_wmo_canadian_text_unsupported,
    classify_wmo_unknown_valid,
];

/// Borrowed context shared across AFOS text-product strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TextClassificationContext<'a> {
    /// Original filename used for filename-sensitive parsing rules.
    filename: &'a str,
    /// Parsed AFOS text product header.
    header: &'a TextProductHeader,
    /// Conditioned body text after header removal.
    body_text: &'a str,
    /// Resolved text-product policy after applying exact-AFOS overrides.
    policy: Option<ResolvedTextProductPolicy>,
    /// Three-character PIL prefix when present.
    pil: Option<String>,
    /// Human-readable title from the PIL catalog.
    title: Option<&'static str>,
    /// Generic body extraction plan derived from the PIL catalog.
    body_plan: Option<crate::body::BodyExtractionPlan>,
    /// BBB meaning for amendment/correction markers.
    bbb_kind: Option<BbbKind>,
    /// Timestamp resolved from the WMO header.
    reference_time: Option<DateTime<Utc>>,
}

/// Borrowed context shared across WMO-only fallback strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WmoClassificationContext<'a> {
    /// Original filename used by routing-sensitive parsers.
    filename: &'a str,
    /// Parsed WMO header without AFOS state.
    header: &'a WmoHeader,
    /// Conditioned body text after header removal.
    body_text: &'a str,
}

/// Classifies an envelope into a fully parsed internal candidate.
///
/// Specialized strategies run in explicit priority order. When no specialized
/// text strategy matches, AFOS payloads fall back to a generic text candidate.
/// WMO-only payloads always end in an unsupported-WMO candidate rather than an
/// untyped kind enum.
pub(crate) fn classify(envelope: &ParsedEnvelope) -> ClassificationCandidate {
    match envelope.kind {
        EnvelopeKind::TextAfos => classify_text_envelope(envelope),
        EnvelopeKind::TextWmoOnly => classify_wmo_envelope(envelope),
        EnvelopeKind::NonText => envelope
            .non_text_meta
            .clone()
            .map(ClassificationCandidate::NonText)
            .unwrap_or(ClassificationCandidate::Unknown),
        EnvelopeKind::Unknown => classify_unknown_text_envelope(envelope).unwrap_or_else(|| {
            envelope
                .parse_error
                .clone()
                .map(ClassificationCandidate::TextParseFailure)
                .unwrap_or(ClassificationCandidate::Unknown)
        }),
    }
}

fn classify_text_envelope(envelope: &ParsedEnvelope) -> ClassificationCandidate {
    if envelope.text_bytes().is_none() {
        return ClassificationCandidate::Unknown;
    }
    let Some(header) = envelope.header.as_ref() else {
        return ClassificationCandidate::Unknown;
    };
    let Some(body_text) = envelope.body_text() else {
        return ClassificationCandidate::Unknown;
    };
    let header_enrichment = enrich_header(header);
    let policy = resolved_text_product_policy(&header.afos);
    let context = TextClassificationContext {
        filename: envelope.filename(),
        pil: header_enrichment.pil_nnn.map(str::to_string),
        title: policy.map(|policy| policy.title),
        body_plan: body_extraction_plan_for_policy(policy),
        policy,
        bbb_kind: header_enrichment.bbb_kind,
        reference_time: header.timestamp(Utc::now()),
        header,
        body_text,
    };

    for strategy in TEXT_STRATEGIES {
        if let Some(candidate) = strategy(&context) {
            return candidate;
        }
    }

    ClassificationCandidate::TextGeneric(TextGenericCandidate {
        header: context.header.clone(),
        pil: context.pil,
        title: context.title,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bbb_kind: context.bbb_kind,
        reference_time: context.reference_time,
    })
}

fn body_extraction_plan_for_policy(
    policy: Option<ResolvedTextProductPolicy>,
) -> Option<crate::body::BodyExtractionPlan> {
    let policy = policy?;
    match policy.body_behavior {
        TextProductBodyBehavior::Never => None,
        TextProductBodyBehavior::Catalog => {
            Some(crate::body::body_extraction_plan(policy.extractors))
        }
    }
}

fn classify_wmo_envelope(envelope: &ParsedEnvelope) -> ClassificationCandidate {
    if envelope.text_bytes().is_none() {
        return ClassificationCandidate::Unknown;
    }
    let Some(header) = envelope.wmo_header.as_ref() else {
        return ClassificationCandidate::Unknown;
    };
    let Some(body_text) = envelope.body_text() else {
        return ClassificationCandidate::Unknown;
    };
    let context = WmoClassificationContext {
        filename: envelope.filename(),
        header,
        body_text,
    };

    for strategy in WMO_STRATEGIES {
        if let Some(candidate) = strategy(&context) {
            return candidate;
        }
    }

    ClassificationCandidate::Unknown
}

/// Returns the filename stem without path or extension.
fn filename_stem(filename: &str) -> &str {
    filename
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .unwrap_or(filename)
        .split_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(filename)
}

/// Detects whether AFOS text resembles an FD bulletin.
fn looks_like_fd_text_product(afos: &str, body_text: &str) -> bool {
    matches!(
        afos.get(..3),
        Some("FD0" | "FD1" | "FD2" | "FD3" | "FD8" | "FD9" | "FDI")
    ) || body_text.contains("DATA BASED ON ")
        && body_text.contains("VALID ")
        && body_text
            .lines()
            .any(|line| line.trim_start().starts_with("FT "))
}

/// Detects whether a WMO-only bulletin resembles an FD bulletin.
fn looks_like_fd_wmo_bulletin(filename: &str, body_text: &str) -> bool {
    filename_stem(filename).starts_with("FD")
        && body_text.contains("DATA BASED ON ")
        && body_text.contains("VALID ")
        && body_text
            .lines()
            .any(|line| line.trim_start().starts_with("FT "))
}

/// Detects whether AFOS text resembles a PIREP bulletin.
fn looks_like_pirep_text_product(afos: &str, body_text: &str) -> bool {
    let trimmed = body_text.trim_start();
    let has_kind = trimmed.starts_with("UA ")
        || trimmed.starts_with("UUA ")
        || body_text.contains("\nUA ")
        || body_text.contains("\nUUA ")
        || body_text.contains(" UA ")
        || body_text.contains(" UUA ");
    afos.starts_with("PIR")
        || afos.eq_ignore_ascii_case("PRCUS")
        || afos.eq_ignore_ascii_case("PIREP")
        || body_text.trim_start().starts_with("ARP ")
        || ((body_text.contains("/OV ") || body_text.contains("/OV"))
            && body_text.contains("/TM")
            && has_kind)
}

/// Detects whether AFOS text resembles a CLI bulletin.
fn looks_like_cli_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CLI")
        || (body_text.to_ascii_uppercase().contains("CLIMATE SUMMARY")
            && body_text.to_ascii_uppercase().contains("WEATHER ITEM"))
}

/// Detects whether AFOS text resembles a SIGMET bulletin.
fn looks_like_sigmet_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SIG")
        || afos.starts_with("WS")
        || body_text.trim_start().starts_with("CONVECTIVE SIGMET ")
        || body_text.trim_start().starts_with("KZAK SIGMET ")
        || body_text.trim_start().starts_with("SIGMET ")
}

fn looks_like_lsr_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("LSR") && {
        let uppercase = body_text.to_ascii_uppercase();
        (uppercase.contains("..TIME..") && uppercase.contains("..DATE.."))
            || uppercase.contains("PRELIMINARY LOCAL STORM REPORT")
            || uppercase.contains("LOCAL STORM REPORT")
    }
}

fn looks_like_cwa_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CWA")
        || (body_text.contains(" CWA ") && body_text.contains("VALID UNTIL"))
        || body_text
            .lines()
            .next()
            .is_some_and(|line| line.contains(" CWA "))
}

fn looks_like_wwp_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("WWP")
        && body_text.contains("PROBABILITY TABLE:")
        && (body_text.contains("ATTRIBUTE TABLE:")
            || body_text.contains("WATCH PROBABILITIES FOR WT"))
}

fn looks_like_saw_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SAW") && body_text.contains("WW ") && body_text.contains("SPC AWW")
}

fn looks_like_sel_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SEL") && body_text.contains("URGENT - IMMEDIATE BROADCAST REQUESTED") && {
        let uppercase = body_text.to_ascii_uppercase();
        uppercase.contains("WATCH NUMBER") || uppercase.contains("WATCH - NUMBER")
    }
}

fn looks_like_cf6_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CF6")
        || (body_text.contains("PRELIMINARY LOCAL CLIMATOLOGICAL DATA")
            && body_text.contains("MONTH:")
            && body_text.contains("YEAR:"))
}

fn looks_like_dsm_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("DSM")
        || body_text.contains(" DS ")
            && body_text.lines().any(|line| {
                let trimmed = line.trim();
                trimmed.len() >= 7
                    && trimmed.as_bytes().get(4..7) == Some(b" DS")
                    && trimmed.contains('/')
            })
}

fn looks_like_hml_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("HML") && body_text.contains("<?xml")
}

fn looks_like_mos_text_product(afos: &str, body_text: &str) -> bool {
    matches!(
        afos.get(..3),
        Some("MET" | "MAV" | "MEX" | "FRH" | "FTP" | "ECS" | "LAV" | "LEV" | "NBE" | "NBS" | "NBX")
    ) && ((body_text.contains("GUIDANCE") && body_text.contains("MOS GUIDANCE"))
        || (body_text.contains("GUIDANCE")
            && body_text.lines().any(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("HR")
                    || trimmed.starts_with("FHR")
                    || trimmed.starts_with("UTC")
            }))
        || body_text
            .lines()
            .any(|line| line.trim_start().starts_with(".B ")))
}

fn looks_like_mcd_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "SWOMCD" | "FFGMPD")
        || body_text.contains("MESOSCALE DISCUSSION")
        || body_text.contains("MPD")
        || body_text.contains("AREAS AFFECTED")
}

fn looks_like_ero_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "RBG94E" | "RBG98E" | "RBG99E")
        && body_text.contains("Excessive Rainfall")
        && body_text.contains("TO THE RIGHT OF A LINE FROM")
}

fn looks_like_spc_outlook_text_product(afos: &str, body_text: &str) -> bool {
    matches!(
        afos,
        "PTSDY1" | "PTSDY2" | "PTSDY3" | "PTSD48" | "PFWFD1" | "PFWFD2" | "PFWF38"
    ) && body_text.contains("VALID")
}

/// Detects whether WMO-only text resembles a SIGMET bulletin.
fn looks_like_sigmet_wmo_bulletin(body_text: &str) -> bool {
    let Some(first_line) = first_nonempty_line(body_text) else {
        return false;
    };
    first_line.starts_with("SIGMET ")
        || starts_with_icao_sigmet_line(first_line)
        || (first_line.contains(" SIGMET ") && first_line.contains(" VALID "))
}

fn looks_like_metar_wmo_bulletin(body_text: &str) -> bool {
    body_text.lines().map(str::trim).any(|line| {
        line == "METAR"
            || line == "SPECI"
            || line.starts_with("METAR ")
            || line.starts_with("SPECI ")
    })
}

fn looks_like_metar_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "METAR" | "SPECI") || looks_like_metar_wmo_bulletin(body_text)
}

fn looks_like_taf_wmo_bulletin(body_text: &str) -> bool {
    body_text.lines().map(str::trim).any(|line| {
        line == "TAF"
            || line.starts_with("TAF ")
            || (line.len() > 3
                && line.starts_with("TAF")
                && line
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()))
    })
}

fn looks_like_taf_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("TAF")
        && (looks_like_taf_wmo_bulletin(body_text) || looks_like_station_led_taf_body(body_text))
}

fn looks_like_station_led_taf_body(body_text: &str) -> bool {
    let mut parts = body_text.split_whitespace();
    let Some(station) = parts.next() else {
        return false;
    };
    let Some(issue_time) = parts.next() else {
        return false;
    };
    (3..=4).contains(&station.len())
        && station.chars().all(|ch| ch.is_ascii_alphanumeric())
        && issue_time.len() == 7
        && issue_time.ends_with('Z')
        && issue_time[..6].chars().all(|ch| ch.is_ascii_digit())
}

/// Detects whether WMO-only text resembles an AIRMET bulletin.
fn looks_like_airmet_wmo_bulletin(body_text: &str) -> bool {
    first_nonempty_line(body_text)
        .is_some_and(|line| line.contains(" AIRMET ") && line.contains(" VALID "))
}

/// Detects Canadian Environment Canada text bulletins.
fn looks_like_canadian_text_bulletin(header: &WmoHeader, body_text: &str) -> bool {
    header.cccc.starts_with("CW") || body_text.contains("ENVIRONMENT CANADA")
}

/// Detects unsupported surface observation bulletins.
fn looks_like_surface_observation_bulletin(body_text: &str) -> bool {
    first_nonempty_line(body_text).is_some_and(|line| line.starts_with("NPL SA "))
}

/// Returns the first non-empty line from conditioned body text.
fn first_nonempty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn classify_unknown_text_envelope(envelope: &ParsedEnvelope) -> Option<ClassificationCandidate> {
    let text = envelope.normalized.text_str()?;
    if !looks_like_dsm_text_product("", text) {
        return None;
    }
    let reference_time = filename_reference_time(envelope.filename()).unwrap_or_else(Utc::now);
    let Some(bulletin) = parse_dsm_bulletin(text, reference_time) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::Unknown,
            "dsm_bulletin",
            "Daily summary message",
            None,
            None,
            None,
            None,
            None,
            "dsm_parse",
            "invalid_dsm_bulletin",
            "recognized DSM bulletin without bulletin headers, but structured parsing failed",
            first_nonempty_line(text),
        ));
    };

    Some(ClassificationCandidate::Dsm(DsmCandidate {
        source: ProductEnrichmentSource::Unknown,
        header: None,
        wmo_header: None,
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    }))
}

fn filename_reference_time(filename: &str) -> Option<DateTime<Utc>> {
    let stem = filename_stem(filename);
    let prefix = stem.get(..12)?;
    chrono::NaiveDateTime::parse_from_str(prefix, "%Y%m%d%H%M")
        .ok()
        .map(|naive| naive.and_utc())
}

#[allow(clippy::too_many_arguments)]
fn malformed_supported_family(
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

/// Checks whether the line begins with `<CCCC> SIGMET`.
fn starts_with_icao_sigmet_line(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    let Some(origin) = parts.next() else {
        return false;
    };
    let Some(sigmet) = parts.next() else {
        return false;
    };
    origin.len() == 4 && origin.chars().all(|ch| ch.is_ascii_uppercase()) && sigmet == "SIGMET"
}

fn classify_text_fd(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Fd) {
        return None;
    }
    if !looks_like_fd_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(reference_time) = context.reference_time else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "fd_bulletin",
            "Winds and temperatures aloft",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "fd_parse",
            "missing_reference_time",
            "recognized FD bulletin, but header timestamp could not be resolved",
            first_nonempty_line(context.body_text),
        ));
    };
    let Some(bulletin) = parse_fd_bulletin(
        context.body_text,
        Some(context.header.afos.as_str()),
        reference_time,
    ) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "fd_bulletin",
            "Winds and temperatures aloft",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "fd_parse",
            "invalid_fd_bulletin",
            "recognized FD bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Fd(FdCandidate {
        source: ProductEnrichmentSource::TextHeader,
        family: "fd_bulletin",
        title: "Winds and temperatures aloft",
        header: Some(context.header.clone()),
        wmo_header: None,
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
    }))
}

fn classify_text_metar(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_metar_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let bulletin_text = if matches!(context.header.afos.as_str(), "METAR" | "SPECI")
        && !looks_like_metar_wmo_bulletin(context.body_text)
    {
        format!("{} {}", context.header.afos, context.body_text.trim_start())
    } else {
        context.body_text.to_string()
    };
    let Some((bulletin, issues)) = parse_metar_bulletin(&bulletin_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "metar_collective",
            "METAR bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            None,
            "metar_parse",
            "invalid_metar_bulletin",
            "recognized METAR bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Metar(MetarCandidate {
        source: ProductEnrichmentSource::TextHeader,
        header: Some(context.header.clone()),
        wmo_header: None,
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: None,
        bulletin,
        issues,
    }))
}

fn classify_text_taf(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_taf_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_taf_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "taf_bulletin",
            "Terminal Aerodrome Forecast",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            None,
            "taf_parse",
            "invalid_taf_bulletin",
            "recognized TAF bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Taf(TafCandidate {
        source: ProductEnrichmentSource::TextHeader,
        header: Some(context.header.clone()),
        wmo_header: None,
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: None,
        bulletin,
    }))
}

fn classify_text_pirep(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_pirep_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_pirep_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "pirep_bulletin",
            "Pilot report bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "pirep_parse",
            "invalid_pirep_bulletin",
            "recognized PIREP bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Pirep(PirepCandidate {
        source: ProductEnrichmentSource::TextHeader,
        header: Some(context.header.clone()),
        wmo_header: None,
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
    }))
}

fn classify_text_sigmet(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Sigmet) {
        return None;
    }
    if !looks_like_sigmet_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_sigmet_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "sigmet_bulletin",
            "SIGMET bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "sigmet_parse",
            "invalid_sigmet_bulletin",
            "recognized SIGMET bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Sigmet(SigmetCandidate {
        source: ProductEnrichmentSource::TextHeader,
        header: Some(context.header.clone()),
        wmo_header: None,
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_lsr(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_lsr_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(reference_time) = context.reference_time else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "lsr_bulletin",
            "Local storm report bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "lsr_parse",
            "missing_reference_time",
            "recognized LSR bulletin, but header timestamp could not be resolved",
            first_nonempty_line(context.body_text),
        ));
    };
    let Some((bulletin, issues)) = parse_lsr_bulletin(context.body_text, reference_time) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "lsr_bulletin",
            "Local storm report bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "lsr_parse",
            "invalid_lsr_bulletin",
            "recognized LSR bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::Lsr(LsrCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues,
    }))
}

fn classify_text_cli(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Cli) {
        return None;
    }
    if !looks_like_cli_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let bulletin = parse_cli_bulletin(context.body_text)?;
    Some(ClassificationCandidate::Cli(CliCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_cwa(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Cwa) {
        return None;
    }
    if !looks_like_cwa_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(reference_time) = context.reference_time else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "cwa_bulletin",
            "Center Weather Advisory",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "cwa_parse",
            "missing_reference_time",
            "recognized CWA bulletin, but header timestamp could not be resolved",
            first_nonempty_line(context.body_text),
        ));
    };
    let Some(bulletin) = parse_cwa_bulletin(context.body_text, reference_time) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "cwa_bulletin",
            "Center Weather Advisory",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "cwa_parse",
            "invalid_cwa_bulletin",
            "recognized CWA bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::Cwa(CwaCandidate {
        header: Some(context.header.clone()),
        wmo_header: None,
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_wwp(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Wwp) {
        return None;
    }
    if !looks_like_wwp_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_wwp_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "wwp_bulletin",
            "Watch probability table",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "wwp_parse",
            "invalid_wwp_bulletin",
            "recognized WWP bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::Wwp(WwpCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_cf6(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_cf6_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some((bulletin, issues)) = parse_cf6_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "cf6_bulletin",
            "Climate summary bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "cf6_parse",
            "invalid_cf6_bulletin",
            "recognized CF6 bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::Cf6(Cf6Candidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues,
    }))
}

fn classify_text_saw(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Saw) {
        return None;
    }
    if !looks_like_saw_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let bulletin = parse_saw_bulletin(
        context.body_text,
        Some(context.header.afos.as_str()),
        context.reference_time?,
    )?;
    Some(ClassificationCandidate::Saw(SawCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_sel(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Sel) {
        return None;
    }
    if !looks_like_sel_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let bulletin = parse_sel_bulletin(context.body_text)?;
    Some(ClassificationCandidate::Sel(SelCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_dsm(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_dsm_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_dsm_bulletin(
        context.body_text,
        context.reference_time.unwrap_or_else(Utc::now),
    ) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "dsm_bulletin",
            "Daily summary message",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "dsm_parse",
            "invalid_dsm_bulletin",
            "recognized DSM bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::Dsm(DsmCandidate {
        source: ProductEnrichmentSource::TextHeader,
        header: Some(context.header.clone()),
        wmo_header: None,
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_hml(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Hml) {
        return None;
    }
    if !looks_like_hml_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_hml_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "hml_bulletin",
            "Hydrological Markup Language bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "hml_parse",
            "invalid_hml_bulletin",
            "recognized HML bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::Hml(HmlCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_mos(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_mos_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(reference_time) = context.reference_time else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "mos_bulletin",
            "Model output statistics guidance",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "mos_parse",
            "missing_reference_time",
            "recognized MOS bulletin, but header timestamp could not be resolved",
            first_nonempty_line(context.body_text),
        ));
    };
    let Some(bulletin) = parse_mos_bulletin(context.body_text, reference_time) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "mos_bulletin",
            "Model output statistics guidance",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "mos_parse",
            "invalid_mos_bulletin",
            "recognized MOS bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::Mos(MosCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_mcd(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Mcd) {
        return None;
    }
    if !looks_like_mcd_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let bulletin = parse_mcd_bulletin(
        context.body_text,
        Some(context.header.afos.as_str()),
        context.reference_time?,
    )?;
    Some(ClassificationCandidate::Mcd(McdCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_ero(context: &TextClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::Ero) {
        return None;
    }
    if !looks_like_ero_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_ero_bulletin(context.body_text, Some(context.header.afos.as_str()))
    else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "ero_bulletin",
            "Excessive rainfall outlook",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "ero_parse",
            "invalid_ero_bulletin",
            "recognized ERO bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::Ero(EroCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_text_spc_outlook(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if context.policy.map(|entry| entry.routing) != Some(TextProductRouting::SpcOutlook) {
        return None;
    }
    if !looks_like_spc_outlook_text_product(&context.header.afos, context.body_text) {
        return None;
    }
    let Some(bulletin) =
        parse_spc_outlook_bulletin(context.body_text, Some(context.header.afos.as_str()))
    else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "spc_outlook_bulletin",
            "SPC outlook bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            build_body_request(context.body_plan, context.body_text, context.reference_time),
            "spc_outlook_parse",
            "invalid_spc_outlook_bulletin",
            "recognized SPC outlook bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };
    Some(ClassificationCandidate::SpcOutlook(SpcOutlookCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: build_body_request(
            context.body_plan,
            context.body_text,
            context.reference_time,
        ),
        bulletin,
        issues: Vec::new(),
    }))
}

fn build_body_request(
    body_plan: Option<crate::body::BodyExtractionPlan>,
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

fn detect_body_input_format(body_text: &str) -> BodyInputFormat {
    let trimmed = body_text.trim_start_matches(|character: char| character.is_ascii_control());
    if trimmed.starts_with("<?xml") || trimmed.starts_with("<alert") {
        BodyInputFormat::CapXml
    } else {
        BodyInputFormat::PlainText
    }
}

fn classify_wmo_fd(context: &WmoClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_fd_wmo_bulletin(context.filename, context.body_text) {
        return None;
    }
    let Some(reference_time) = context.header.timestamp(Utc::now()) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::WmoBulletin,
            "fd_bulletin",
            "Winds and temperatures aloft",
            None,
            Some(context.header.clone()),
            None,
            None,
            None,
            "fd_parse",
            "missing_reference_time",
            "recognized FD bulletin, but WMO timestamp could not be resolved",
            first_nonempty_line(context.body_text),
        ));
    };
    let Some(bulletin) = parse_fd_bulletin(
        context.body_text,
        Some(filename_stem(context.filename)),
        reference_time,
    ) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::WmoBulletin,
            "fd_bulletin",
            "Winds and temperatures aloft",
            None,
            Some(context.header.clone()),
            None,
            None,
            None,
            "fd_parse",
            "invalid_fd_bulletin",
            "recognized FD bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Fd(FdCandidate {
        source: ProductEnrichmentSource::WmoBulletin,
        family: "fd_bulletin",
        title: "Winds and temperatures aloft",
        header: None,
        wmo_header: Some(context.header.clone()),
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
    }))
}

fn classify_wmo_metar(context: &WmoClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if first_nonempty_line(context.body_text).is_some_and(|line| line.starts_with("NPL SA ")) {
        return None;
    }
    let Some((bulletin, issues)) = parse_metar_bulletin(context.body_text) else {
        return looks_like_metar_wmo_bulletin(context.body_text).then(|| {
            malformed_supported_family(
                ProductEnrichmentSource::WmoBulletin,
                "metar_collective",
                "METAR bulletin",
                None,
                Some(context.header.clone()),
                None,
                None,
                None,
                "metar_parse",
                "invalid_metar_bulletin",
                "recognized METAR bulletin, but structured parsing failed",
                first_nonempty_line(context.body_text),
            )
        });
    };

    Some(ClassificationCandidate::Metar(MetarCandidate {
        source: ProductEnrichmentSource::WmoBulletin,
        header: None,
        wmo_header: Some(context.header.clone()),
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues,
    }))
}

fn classify_wmo_taf(context: &WmoClassificationContext<'_>) -> Option<ClassificationCandidate> {
    let Some(bulletin) = parse_taf_bulletin(context.body_text) else {
        return looks_like_taf_wmo_bulletin(context.body_text).then(|| {
            malformed_supported_family(
                ProductEnrichmentSource::WmoBulletin,
                "taf_bulletin",
                "Terminal Aerodrome Forecast",
                None,
                Some(context.header.clone()),
                None,
                None,
                None,
                "taf_parse",
                "invalid_taf_bulletin",
                "recognized TAF bulletin, but structured parsing failed",
                first_nonempty_line(context.body_text),
            )
        });
    };

    Some(ClassificationCandidate::Taf(TafCandidate {
        source: ProductEnrichmentSource::WmoBulletin,
        header: None,
        wmo_header: Some(context.header.clone()),
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
    }))
}

fn classify_wmo_dsm(context: &WmoClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_dsm_text_product("", context.body_text) {
        return None;
    }
    let reference_time = context
        .header
        .timestamp(Utc::now())
        .unwrap_or_else(Utc::now);
    let Some(bulletin) = parse_dsm_bulletin(context.body_text, reference_time) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::WmoBulletin,
            "dsm_bulletin",
            "Daily summary message",
            None,
            Some(context.header.clone()),
            None,
            None,
            None,
            "dsm_parse",
            "invalid_dsm_bulletin",
            "recognized DSM bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Dsm(DsmCandidate {
        source: ProductEnrichmentSource::WmoBulletin,
        header: None,
        wmo_header: Some(context.header.clone()),
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_wmo_pirep(context: &WmoClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_pirep_text_product("", context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_pirep_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::WmoBulletin,
            "pirep_bulletin",
            "Pilot report bulletin",
            None,
            Some(context.header.clone()),
            None,
            None,
            None,
            "pirep_parse",
            "invalid_pirep_bulletin",
            "recognized PIREP bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Pirep(PirepCandidate {
        source: ProductEnrichmentSource::WmoBulletin,
        header: None,
        wmo_header: Some(context.header.clone()),
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
    }))
}

fn classify_wmo_dcp(context: &WmoClassificationContext<'_>) -> Option<ClassificationCandidate> {
    let bulletin = parse_dcp_bulletin(context.filename, context.header, context.body_text)?;

    Some(ClassificationCandidate::Dcp(DcpCandidate {
        header: context.header.clone(),
        bulletin,
    }))
}

fn classify_wmo_sigmet(context: &WmoClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_sigmet_wmo_bulletin(context.body_text) {
        return None;
    }
    let Some(bulletin) = parse_sigmet_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::WmoBulletin,
            "sigmet_bulletin",
            "SIGMET bulletin",
            None,
            Some(context.header.clone()),
            None,
            None,
            None,
            "sigmet_parse",
            "invalid_sigmet_bulletin",
            "recognized SIGMET bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Sigmet(SigmetCandidate {
        source: ProductEnrichmentSource::WmoBulletin,
        header: None,
        wmo_header: Some(context.header.clone()),
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_wmo_cwa(context: &WmoClassificationContext<'_>) -> Option<ClassificationCandidate> {
    if !looks_like_cwa_text_product("", context.body_text) {
        return None;
    }
    let Some(reference_time) = context.header.timestamp(Utc::now()) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::WmoBulletin,
            "cwa_bulletin",
            "Center Weather Advisory",
            None,
            Some(context.header.clone()),
            Some("CWA".to_string()),
            None,
            None,
            "cwa_parse",
            "missing_reference_time",
            "recognized CWA bulletin, but WMO timestamp could not be resolved",
            first_nonempty_line(context.body_text),
        ));
    };
    let Some(bulletin) = parse_cwa_bulletin(context.body_text, reference_time) else {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::WmoBulletin,
            "cwa_bulletin",
            "Center Weather Advisory",
            None,
            Some(context.header.clone()),
            Some("CWA".to_string()),
            None,
            None,
            "cwa_parse",
            "invalid_cwa_bulletin",
            "recognized CWA bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
    };

    Some(ClassificationCandidate::Cwa(CwaCandidate {
        header: None,
        wmo_header: Some(context.header.clone()),
        pil: Some("CWA".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    }))
}

fn classify_wmo_airmet_unsupported(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    looks_like_airmet_wmo_bulletin(context.body_text).then(|| {
        ClassificationCandidate::UnsupportedWmo(UnsupportedWmoCandidate {
            header: context.header.clone(),
            code: "unsupported_airmet_bulletin",
            message:
                "recognized valid WMO AIRMET bulletin, but textual AIRMET parsing is not implemented",
            line: first_nonempty_line(context.body_text).map(str::to_string),
        })
    })
}

fn classify_wmo_surface_observation_unsupported(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    looks_like_surface_observation_bulletin(context.body_text).then(|| {
        ClassificationCandidate::UnsupportedWmo(UnsupportedWmoCandidate {
            header: context.header.clone(),
            code: "unsupported_surface_observation_bulletin",
            message:
                "recognized valid WMO surface observation bulletin, but parsing is not implemented",
            line: first_nonempty_line(context.body_text).map(str::to_string),
        })
    })
}

fn classify_wmo_canadian_text_unsupported(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    looks_like_canadian_text_bulletin(context.header, context.body_text).then(|| {
        ClassificationCandidate::UnsupportedWmo(UnsupportedWmoCandidate {
            header: context.header.clone(),
            code: "unsupported_canadian_text_bulletin",
            message: "recognized valid WMO Canadian text bulletin, but parsing is not implemented",
            line: first_nonempty_line(context.body_text).map(str::to_string),
        })
    })
}

fn classify_wmo_unknown_valid(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    Some(ClassificationCandidate::UnsupportedWmo(
        UnsupportedWmoCandidate {
            header: context.header.clone(),
            code: "unsupported_wmo_bulletin",
            message: "recognized valid WMO bulletin without AFOS line, but no parser is available",
            line: first_nonempty_line(context.body_text).map(str::to_string),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        TextClassificationContext, build_body_request, classify, classify_text_cf6,
        classify_text_cwa, classify_text_dsm, classify_text_fd, classify_text_hml,
        classify_text_lsr, classify_text_mos, classify_text_saw, classify_text_sel,
        classify_text_wwp,
    };
    use crate::body::{BodyExtractorId, body_extraction_plan};
    use crate::data::resolved_text_product_policy;
    use crate::header::BbbKind;
    use crate::pipeline::candidate::{ClassificationCandidate, FdCandidate};
    use crate::pipeline::{NormalizedInput, ParsedEnvelope};
    use crate::{ProductEnrichmentSource, TextProductHeader};
    use chrono::{TimeZone, Utc};

    fn with_specialized_context<T>(
        pil: &'static str,
        afos: &'static str,
        body_text: &'static str,
        body_plan: Option<crate::body::BodyExtractionPlan>,
        f: impl FnOnce(&TextClassificationContext<'_>) -> T,
    ) -> T {
        let header = TextProductHeader {
            ttaaii: "FTUS80".to_string(),
            cccc: "KWBC".to_string(),
            ddhhmm: "100000".to_string(),
            bbb: None,
            afos: afos.to_string(),
        };
        let policy = resolved_text_product_policy(afos).expect("expected catalog policy");
        let context = TextClassificationContext {
            filename: "sample.TXT",
            header: &header,
            body_text,
            policy: Some(policy),
            pil: Some(pil.to_string()),
            title: Some(policy.title),
            body_plan,
            bbb_kind: None,
            reference_time: Some(Utc::now()),
        };

        f(&context)
    }

    #[test]
    fn afos_fd_strategy_returns_fd_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "FD1US1.TXT",
            b"000 \nFTUS80 KWBC 070000\nFD1US1\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::Fd(_)
        ));
    }

    #[test]
    fn afos_pirep_strategy_returns_pirep_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "PIRXXX.TXT",
            b"000 \nUAUS01 KBOU 070000\nPIRBOU\nDEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::Pirep(_)
        ));
    }

    #[test]
    fn afos_sigmet_strategy_returns_sigmet_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "SIGABC.TXT",
            b"000 \nWSUS31 KKCI 070000\nSIGABC\nCONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::Sigmet(_)
        ));
    }

    #[test]
    fn local_lsr_sample_returns_lsr_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "LSRBMX.TXT",
            b"000 \nNWUS54 KBMX 100015\nLSRBMX\n..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
        ));

        let ClassificationCandidate::Lsr(candidate) = classify(&envelope) else {
            panic!("expected lsr candidate");
        };

        assert!(candidate.header.afos.starts_with("LSR"));
        assert!(candidate.body_request.is_none());
        assert_eq!(candidate.bulletin.reports.len(), 1);
    }

    #[test]
    fn local_cwa_active_sample_returns_wmo_only_cwa_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "CWAZLC.TXT",
            b"000 \nFAUS22 KZLC 100229\nZLC CWA 202 100229\nZLC CWA 202 VALID UNTIL 100430\nFROM SLC-SHR-DDY AREA TS.\n",
        ));

        let ClassificationCandidate::Cwa(candidate) = classify(&envelope) else {
            panic!("expected cwa candidate");
        };

        assert!(candidate.header.is_none());
        assert!(candidate.wmo_header.is_some());
        assert!(candidate.body_request.is_none());
        assert!(!candidate.bulletin.is_cancelled);
    }

    #[test]
    fn local_cwa_cancel_sample_returns_wmo_only_cwa_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "CWAZFW.TXT",
            b"000 \nFAUS24 KZFW 100038\nZFW CWA 101 100038\nZFW CWA 101 VALID UNTIL 100200\nCANCEL CWA 101. ERROR CORRECTED.\n",
        ));

        let ClassificationCandidate::Cwa(candidate) = classify(&envelope) else {
            panic!("expected cwa candidate");
        };

        assert!(candidate.header.is_none());
        assert!(candidate.wmo_header.is_some());
        assert!(candidate.bulletin.is_cancelled);
    }

    #[test]
    fn local_wwp_sample_returns_wwp_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "WWP1.TXT",
            b"000 \nWWUS40 KWNS 102008\nWWP1\nTORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\nPROB OF 2 OR MORE TORNADOES : 20%\nPROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES : 10%\nPROB OF 10 OR MORE SEVERE WIND EVENTS : 70%\nPROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS : 40%\nPROB OF 10 OR MORE SEVERE HAIL EVENTS : 60%\nPROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES : 30%\nPROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS : 95%\nATTRIBUTE TABLE:\nMAX HAIL /INCHES/ : 2.0\nMAX WIND GUSTS SURFACE /KNOTS/ : 70\nMAX TOPS /X 100 FEET/ : 500\nMEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 24035\nPARTICULARLY DANGEROUS SITUATION : NO\n",
        ));

        let ClassificationCandidate::Wwp(candidate) = classify(&envelope) else {
            panic!("expected wwp candidate");
        };

        assert!(candidate.header.afos.starts_with("WWP"));
        assert!(candidate.body_request.is_none());
        assert_eq!(candidate.bulletin.watch_number, 31);
    }

    #[test]
    fn local_saw_sample_returns_saw_candidate_with_body_request() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "SAW2.TXT",
            b"000 \nWWUS30 KWNS 251740\nSAW2\n\x1eSPC AWW 251740\nWW 542 SEVERE TSTM CT DE MA NJ NY PA RI CW 251745Z - 260100Z\nLAT...LON 41087082 39507704 41247704 42827082\n",
        ));

        let ClassificationCandidate::Saw(candidate) = classify(&envelope) else {
            panic!("expected saw candidate");
        };

        assert!(candidate.header.afos.starts_with("SAW"));
        assert!(candidate.body_request.is_some());
        assert_eq!(candidate.bulletin.watch_number, 542);
    }

    #[test]
    fn local_sel_sample_returns_sel_candidate_with_body_request() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "SEL2.TXT",
            b"844 \nWWUS20 KWNS 251740\nSEL2  \n\x1eSPC WW 251740\nCTZ000-DEZ000-MAZ000-NJZ000-NYZ000-PAZ000-RIZ000-CWZ000-260100-\n\nURGENT - IMMEDIATE BROADCAST REQUESTED\nSevere Thunderstorm Watch Number 542\nNWS Storm Prediction Center Norman OK\n145 PM EDT Fri Jul 25 2025\n",
        ));

        let ClassificationCandidate::Sel(candidate) = classify(&envelope) else {
            panic!("expected sel candidate");
        };

        assert!(candidate.header.afos.starts_with("SEL"));
        assert!(candidate.body_request.is_some());
        assert_eq!(candidate.bulletin.watch_number, 542);
    }

    #[test]
    fn local_cf6_sample_returns_cf6_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "CF6GSN.TXT",
            b"000 \nCXGM50 PGUM 100030\nCF6GSN\nPRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD PCP SNW SND AWD MWD DIR MIN PSBL SKY WX GST GDR\n 1 70 50 60 0 5 0 0.10 0.0 0 8.5 20 180 600 720 CLR RA 30 190\n",
        ));

        let ClassificationCandidate::Cf6(candidate) = classify(&envelope) else {
            panic!("expected cf6 candidate");
        };

        assert!(candidate.header.afos.starts_with("CF6"));
        assert!(candidate.body_request.is_none());
        assert_eq!(candidate.bulletin.rows.len(), 1);
    }

    #[test]
    fn local_dsm_sample_returns_dsm_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "DSMCQC.TXT",
            b"000 \nCXUS45 KABQ 110415\nDSMCQC\nKCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
        ));

        let ClassificationCandidate::Dsm(candidate) = classify(&envelope) else {
            panic!("expected dsm candidate");
        };

        assert!(
            candidate
                .header
                .as_ref()
                .is_some_and(|header| header.afos.starts_with("DSM"))
        );
        assert!(candidate.body_request.is_none());
        assert_eq!(candidate.bulletin.summaries[0].station, "KCQC");
    }

    #[test]
    fn local_hml_sample_returns_hml_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "HMLMTR.TXT",
            br#"000 
SRUS56 KMTR 100002
HMLMTR
<?xml version="1.0"?>
<site id="AAMC1" name="ARROYO SECO" originator="MTR" generationtime="2026-03-10T00:02:00Z">
  <observed issued="2026-03-10T00:00:00Z" primaryName="Stage" primaryUnits="FT">
    <datum><valid>2026-03-10T00:00:00Z</valid><primary>2.5</primary></datum>
  </observed>
</site>
"#,
        ));

        let ClassificationCandidate::Hml(candidate) = classify(&envelope) else {
            panic!("expected hml candidate");
        };

        assert!(candidate.header.afos.starts_with("HML"));
        assert!(candidate.body_request.is_none());
        assert!(!candidate.bulletin.documents.is_empty());
    }

    #[test]
    fn local_met_sample_returns_mos_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "METBCK.TXT",
            b"000 \nFOUS46 KWNO 100000\nMETBCK\nKBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\nWND 05 06 07\n",
        ));

        let ClassificationCandidate::Mos(candidate) = classify(&envelope) else {
            panic!("expected mos candidate");
        };

        assert!(candidate.header.afos.starts_with("MET"));
        assert!(candidate.body_request.is_none());
        assert_eq!(candidate.bulletin.sections[0].station, "KBCK");
    }

    #[test]
    fn local_ftp_sample_returns_mos_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "FTPACR.TXT",
            b"000 \nFOAK12 KWNO 100000\nFTPACR\n.B FTP 0310 DH06/DC03100600\nAHP 12/08/13/09\n",
        ));

        let ClassificationCandidate::Mos(candidate) = classify(&envelope) else {
            panic!("expected mos candidate");
        };

        assert!(candidate.header.afos.starts_with("FTP"));
        assert!(candidate.body_request.is_none());
        assert_eq!(candidate.bulletin.sections[0].station, "AHP");
    }

    #[test]
    fn afos_generic_fallback_returns_text_generic_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "TAFPDKGA.TXT",
            b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::TextGeneric(_)
        ));
    }

    #[test]
    fn text_generic_candidate_uses_catalog_body_behavior() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "SVRDMX.TXT",
            b"000 \nWUUS53 KDMX 022320\nSVRDMX\n/O.NEW.KDMX.SV.W.0001.250301T1200Z-250301T1300Z/\n",
        ));

        let ClassificationCandidate::TextGeneric(candidate) = classify(&envelope) else {
            panic!("expected generic text candidate");
        };

        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn text_generic_candidate_omits_body_request_when_catalog_body_behavior_is_never() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "ZZZXXX.TXT",
            b"000 \nFXUS61 KBOX 022101\nZZZBOX\nBody\n",
        ));

        let ClassificationCandidate::TextGeneric(candidate) = classify(&envelope) else {
            panic!("expected generic text candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn afos_strategy_precedence_prefers_pirep_when_catalog_routing_matches() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "PIRXXX.TXT",
            b"000 \nUAUS01 KBOU 070000\nPIRBOU\nDEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\nCONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::Pirep(_)
        ));
    }

    #[test]
    fn fd_candidate_has_no_body_request_by_default() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "FD1US1.TXT",
            b"000 \nFTUS80 KWBC 070000\nFD1US1\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
        ));

        let ClassificationCandidate::Fd(candidate) = classify(&envelope) else {
            panic!("expected fd candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn fd_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "FD1US1.TXT",
            b"000 \nFTUS80 KWBC 070000\nFD1US1\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
        ));

        let ClassificationCandidate::Fd(candidate) = classify(&envelope) else {
            panic!("expected fd candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn pirep_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "PIRXXX.TXT",
            b"000 \nUAUS01 KBOU 070000\nPIRBOU\nDEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n",
        ));

        let ClassificationCandidate::Pirep(candidate) = classify(&envelope) else {
            panic!("expected pirep candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn sigmet_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "SIGABC.TXT",
            b"000 \nWSUS31 KKCI 070000\nSIGABC\nCONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
        ));

        let ClassificationCandidate::Sigmet(candidate) = classify(&envelope) else {
            panic!("expected sigmet candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn lsr_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "LSRBMX.TXT",
            b"000 \nNWUS54 KBMX 100015\nLSRBMX\n..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
        ));

        let ClassificationCandidate::Lsr(candidate) = classify(&envelope) else {
            panic!("expected lsr candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn cwa_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "CWAZLC.TXT",
            b"000 \nFAUS22 KZLC 100229\nCWAZLC\nZLC2 CWA 100230\nZLC CWA 202 VALID UNTIL 100630\nFROM 75W BIL-15NNE SHR-55SW DDY-45S OCS-35SSE SLC-75W BIL\nAREA MOD/ISO SEV MTN WAVE FL350-ABV FL450. ALTITUDE CHANGE OF +/-25KTS. RPRTD BY ACFT. VISIBLE ON SATELLITE. CWSU 100230Z. CO ID MT UT WY\n=\n",
        ));

        let ClassificationCandidate::Cwa(candidate) = classify(&envelope) else {
            panic!("expected cwa candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn wwp_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "WWP1.TXT",
            b"000 \nWWUS40 KWNS 102008\nWWP1\nTORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\nPROB OF 2 OR MORE TORNADOES : 20%\nPROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES : 10%\nPROB OF 10 OR MORE SEVERE WIND EVENTS : 70%\nPROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS : 40%\nPROB OF 10 OR MORE SEVERE HAIL EVENTS : 60%\nPROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES : 30%\nPROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS : 95%\nATTRIBUTE TABLE:\nMAX HAIL /INCHES/ : 2.0\nMAX WIND GUSTS SURFACE /KNOTS/ : 70\nMAX TOPS /X 100 FEET/ : 500\nMEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 24035\nPARTICULARLY DANGEROUS SITUATION : NO\n",
        ));

        let ClassificationCandidate::Wwp(candidate) = classify(&envelope) else {
            panic!("expected wwp candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn cf6_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "CF6GSN.TXT",
            b"000 \nCXGM50 PGUM 100030\nCF6GSN\nPRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD PCP SNW SND AWD MWD DIR MIN PSBL SKY WX GST GDR\n 1 70 50 60 0 5 0 0.10 0.0 0 8.5 20 180 600 720 CLR RA 30 190\n",
        ));

        let ClassificationCandidate::Cf6(candidate) = classify(&envelope) else {
            panic!("expected cf6 candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn dsm_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "DSMCQC.TXT",
            b"000 \nCXUS45 KABQ 110415\nDSMCQC\nKCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
        ));

        let ClassificationCandidate::Dsm(candidate) = classify(&envelope) else {
            panic!("expected dsm candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn hml_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "HMLMTR.TXT",
            br#"000 
SRUS56 KMTR 100002
HMLMTR
<?xml version="1.0"?>
<site id="AAMC1" name="ARROYO SECO" originator="MTR" generationtime="2026-03-10T00:02:00Z">
  <observed issued="2026-03-10T00:00:00Z" primaryName="Stage" primaryUnits="FT">
    <datum><valid>2026-03-10T00:00:00Z</valid><primary>2.5</primary></datum>
  </observed>
</site>
"#,
        ));

        let ClassificationCandidate::Hml(candidate) = classify(&envelope) else {
            panic!("expected hml candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn mos_candidate_body_request_follows_catalog_policy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "METBCK.TXT",
            b"000 \nFOUS46 KWNO 100000\nMETBCK\nKBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\nWND 05 06 07\n",
        ));

        let ClassificationCandidate::Mos(candidate) = classify(&envelope) else {
            panic!("expected mos candidate");
        };

        assert!(candidate.body_request.is_none());
    }

    #[test]
    fn specialized_strategy_requires_matching_catalog_routing() {
        let header = TextProductHeader {
            ttaaii: "FTUS80".to_string(),
            cccc: "KWBC".to_string(),
            ddhhmm: "070000".to_string(),
            bbb: None,
            afos: "FD1US1".to_string(),
        };
        let context = TextClassificationContext {
            filename: "FD1US1.TXT",
            header: &header,
            body_text: "DATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
            policy: resolved_text_product_policy("PIRBOU"),
            pil: Some("FD1".to_string()),
            title: Some("Winds and Temperatures Aloft"),
            body_plan: None,
            bbb_kind: None,
            reference_time: Some(Utc::now()),
        };

        assert!(classify_text_fd(&context).is_none());
    }

    #[test]
    fn saw_strategy_requires_matching_catalog_routing() {
        with_specialized_context(
            "SEL",
            "SEL2",
            "URGENT - IMMEDIATE BROADCAST REQUESTED\nSevere Thunderstorm Watch Number 542\n",
            Some(body_extraction_plan(&[BodyExtractorId::Ugc])),
            |context| assert!(classify_text_saw(context).is_none()),
        );
    }

    #[test]
    fn sel_strategy_requires_matching_catalog_routing() {
        with_specialized_context(
            "SAW",
            "SAW2",
            "SPC AWW 251740\nWW 542 SEVERE TSTM CT DE MA NJ NY PA RI CW 251745Z - 260100Z\nLAT...LON 41087082 39507704 41247704 42827082\n",
            Some(body_extraction_plan(&[
                BodyExtractorId::Ugc,
                BodyExtractorId::LatLon,
            ])),
            |context| assert!(classify_text_sel(context).is_none()),
        );
    }

    #[test]
    fn malformed_lsr_stays_in_family_with_issue() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "LSRBMX.TXT",
            b"000 \nNWUS54 KBMX 100015\nLSRBMX\nPreliminary Local Storm Report\nNo standard report block\n",
        ));

        let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
            panic!("expected malformed-family candidate");
        };
        assert_eq!(candidate.family, "lsr_bulletin");
        assert_eq!(candidate.issues[0].code, "invalid_lsr_bulletin");
    }

    #[test]
    fn malformed_wwp_stays_in_family_with_issue() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "WWP1.TXT",
            b"000 \nWWUS40 KWNS 102012\nWWP1\nTORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\n",
        ));

        let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
            panic!("expected malformed-family candidate");
        };
        assert_eq!(candidate.family, "wwp_bulletin");
        assert_eq!(candidate.issues[0].code, "invalid_wwp_bulletin");
    }

    #[test]
    fn malformed_cf6_stays_in_family_with_issue() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "CF6GSN.TXT",
            b"000 \nCXGM50 PGUM 100030\nCF6GSN\nPRELIMINARY LOCAL CLIMATOLOGICAL DATA\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD WTR\n",
        ));

        let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
            panic!("expected malformed-family candidate");
        };
        assert_eq!(candidate.family, "cf6_bulletin");
        assert_eq!(candidate.issues[0].code, "invalid_cf6_bulletin");
    }

    #[test]
    fn malformed_hml_stays_in_family_with_issue() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "HMLMTR.TXT",
            b"000 \nSRUS56 KMTR 100002\nHMLMTR\n<?xml version=\"1.0\"?><site><observed><datum></site>\n",
        ));

        let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
            panic!("expected malformed-family candidate");
        };
        assert_eq!(candidate.family, "hml_bulletin");
        assert_eq!(candidate.issues[0].code, "invalid_hml_bulletin");
    }

    #[test]
    fn malformed_standard_mos_stays_in_family_with_issue() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "METBCK.TXT",
            b"000 \nFOUS46 KWNO 100000\nMETNC1\nKBCK   NAM MOS GUIDANCE    3/10/2026  0000 UTC\nTMP 41 38 36\n",
        ));

        let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
            panic!("expected malformed-family candidate");
        };
        assert_eq!(candidate.family, "mos_bulletin");
        assert_eq!(candidate.issues[0].code, "invalid_mos_bulletin");
    }

    #[test]
    fn malformed_ftp_stays_in_family_with_issue() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "FTPACR.TXT",
            b"000 \nFOAK12 KWNO 100000\nFTPACR\n.B NMC 0311\n.B1 bad header\n.END\n",
        ));

        let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
            panic!("expected malformed-family candidate");
        };
        assert_eq!(candidate.family, "mos_bulletin");
        assert_eq!(candidate.issues[0].code, "invalid_mos_bulletin");
    }

    #[test]
    fn invalid_wmo_only_non_cwa_does_not_hit_cwa_strategy() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "FXUS61.TXT",
            b"FXUS61 KBOX 022101\nAREA FORECAST DISCUSSION\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::UnsupportedWmo(_)
        ));
    }

    #[test]
    fn specialized_candidate_can_carry_body_request_when_metadata_enables_catalog_body_behavior() {
        let reference_time = Utc
            .with_ymd_and_hms(2025, 3, 7, 0, 0, 0)
            .single()
            .expect("valid reference time");
        let request = build_body_request(
            Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
            "/O.NEW.KDMX.TO.W.0001.250301T1200Z-250301T1300Z/",
            Some(reference_time),
        );

        let candidate = ClassificationCandidate::Fd(FdCandidate {
            source: ProductEnrichmentSource::TextHeader,
            family: "fd_bulletin",
            title: "Winds and temperatures aloft",
            header: Some(TextProductHeader {
                ttaaii: "FTUS80".to_string(),
                cccc: "KWBC".to_string(),
                ddhhmm: "070000".to_string(),
                bbb: None,
                afos: "FD1US1".to_string(),
            }),
            wmo_header: None,
            pil: Some("FD1".to_string()),
            bbb_kind: Some(BbbKind::Amendment),
            body_request: request,
            bulletin: crate::specialized::fd::parse_fd_bulletin(
                "DATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
                Some("FD1US1"),
                reference_time,
            )
            .expect("fd bulletin should parse"),
        });

        let ClassificationCandidate::Fd(candidate) = candidate else {
            panic!("expected fd candidate");
        };
        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn lsr_classifier_carries_body_request_when_plan_is_enabled() {
        let candidate = with_specialized_context(
            "LSR",
            "LSRBMX",
            "..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
            Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
            classify_text_lsr,
        );

        let ClassificationCandidate::Lsr(candidate) = candidate.expect("expected lsr candidate")
        else {
            panic!("expected lsr candidate");
        };
        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn cwa_classifier_carries_body_request_when_plan_is_enabled() {
        let candidate = with_specialized_context(
            "CWA",
            "CWAZLC",
            "ZLC2 CWA 100230\nZLC CWA 202 VALID UNTIL 100630\nFROM 75W BIL-15NNE SHR-55SW DDY-45S OCS-35SSE SLC-75W BIL\nAREA MOD/ISO SEV MTN WAVE FL350-ABV FL450. ALTITUDE CHANGE OF +/-25KTS. RPRTD BY ACFT. VISIBLE ON SATELLITE. CWSU 100230Z. CO ID MT UT WY\n=\n",
            Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
            classify_text_cwa,
        );

        let ClassificationCandidate::Cwa(candidate) = candidate.expect("expected cwa candidate")
        else {
            panic!("expected cwa candidate");
        };
        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn wwp_classifier_carries_body_request_when_plan_is_enabled() {
        let candidate = with_specialized_context(
            "WWP",
            "WWP1",
            "TORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\nPROB OF 2 OR MORE TORNADOES : 20%\nPROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES : 10%\nPROB OF 10 OR MORE SEVERE WIND EVENTS : 70%\nPROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS : 40%\nPROB OF 10 OR MORE SEVERE HAIL EVENTS : 60%\nPROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES : 30%\nPROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS : 95%\nATTRIBUTE TABLE:\nMAX HAIL /INCHES/ : 2.0\nMAX WIND GUSTS SURFACE /KNOTS/ : 70\nMAX TOPS /X 100 FEET/ : 500\nMEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 24035\nPARTICULARLY DANGEROUS SITUATION : NO\n",
            Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
            classify_text_wwp,
        );

        let ClassificationCandidate::Wwp(candidate) = candidate.expect("expected wwp candidate")
        else {
            panic!("expected wwp candidate");
        };
        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn cf6_classifier_carries_body_request_when_plan_is_enabled() {
        let candidate = with_specialized_context(
            "CF6",
            "CF6GSN",
            "PRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD PCP SNW SND AWD MWD DIR MIN PSBL SKY WX GST GDR\n 1 70 50 60 0 5 0 0.10 0.0 0 8.5 20 180 600 720 CLR RA 30 190\n",
            Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
            classify_text_cf6,
        );

        let ClassificationCandidate::Cf6(candidate) = candidate.expect("expected cf6 candidate")
        else {
            panic!("expected cf6 candidate");
        };
        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn dsm_classifier_carries_body_request_when_plan_is_enabled() {
        let candidate = with_specialized_context(
            "DSM",
            "DSMCQC",
            "KCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
            Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
            classify_text_dsm,
        );

        let ClassificationCandidate::Dsm(candidate) = candidate.expect("expected dsm candidate")
        else {
            panic!("expected dsm candidate");
        };
        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn hml_classifier_carries_body_request_when_plan_is_enabled() {
        let candidate = with_specialized_context(
            "HML",
            "HMLMTR",
            "<?xml version=\"1.0\"?>\n<site id=\"AAMC1\" name=\"ARROYO SECO\" originator=\"MTR\" generationtime=\"2026-03-10T00:02:00Z\">\n  <observed issued=\"2026-03-10T00:00:00Z\" primaryName=\"Stage\" primaryUnits=\"FT\">\n    <datum><valid>2026-03-10T00:00:00Z</valid><primary>2.5</primary></datum>\n  </observed>\n</site>\n",
            Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
            classify_text_hml,
        );

        let ClassificationCandidate::Hml(candidate) = candidate.expect("expected hml candidate")
        else {
            panic!("expected hml candidate");
        };
        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn mos_classifier_carries_body_request_when_plan_is_enabled() {
        let candidate = with_specialized_context(
            "MET",
            "METBCK",
            "KBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\nWND 05 06 07\n",
            Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
            classify_text_mos,
        );

        let ClassificationCandidate::Mos(candidate) = candidate.expect("expected mos candidate")
        else {
            panic!("expected mos candidate");
        };
        assert!(candidate.body_request.is_some());
    }

    #[test]
    fn wmo_metar_strategy_returns_metar_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "SAGL31.TXT",
            b"000 \nSAGL31 BGGH 070200\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::Metar(_)
        ));
    }

    #[test]
    fn wmo_taf_strategy_returns_taf_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "TAFWBCFJ.TXT",
            b"000 \nFTXX01 KWBC 070200\nTAF AMD\nWBCF 070244Z 0703/0803 18012KT P6SM SCT050\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::Taf(_)
        ));
    }

    #[test]
    fn wmo_dcp_strategy_returns_dcp_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "MISDCPSV.TXT",
            b"SXMS50 KWAL 070258\n83786162 066025814\n16.23\n003\n137\n071\n088\n12.9\n137\n007\n00000\n 42-0NN  45E\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::Dcp(_)
        ));
    }

    #[test]
    fn wmo_sigmet_strategy_returns_sigmet_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "WVID21.TXT",
            b"WVID21 WAAA 090100\nWAAF SIGMET 05 VALID 090100/090700 WAAA-\nWAAF UJUNG PANDANG FIR VA ERUPTION MT IBU=\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::Sigmet(_)
        ));
    }

    #[test]
    fn wmo_airmet_returns_unsupported_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "WAAB31.TXT",
            b"WAAB31 LATI 090038\nLAAA AIRMET 1 VALID 090100/090500 LATI-\nLAAA TIRANA FIR MOD ICE=\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::UnsupportedWmo(_)
        ));
    }

    #[test]
    fn wmo_canadian_text_returns_unsupported_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "FPCN11.TXT",
            b"FPCN11 CWWG 090059 AAD\nUPDATED FORECASTS FOR SOUTHERN MANITOBA ISSUED BY ENVIRONMENT CANADA\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::UnsupportedWmo(_)
        ));
    }

    #[test]
    fn wmo_surface_observation_returns_unsupported_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "SAHOURLY.TXT",
            b"SACN74 CWAO 090000 RRC\n\nNPL SA 0000 AUTO8 M M M=\n",
        ));

        assert!(matches!(
            classify(&envelope),
            ClassificationCandidate::UnsupportedWmo(_)
        ));
    }

    #[test]
    fn unknown_valid_wmo_returns_generic_unsupported_candidate() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "UNKNOWN.TXT",
            b"ABCD12 EFGH 090000\nSOME UNKNOWN BODY\n",
        ));

        let ClassificationCandidate::UnsupportedWmo(candidate) = classify(&envelope) else {
            panic!("expected unsupported WMO candidate");
        };

        assert_eq!(candidate.code, "unsupported_wmo_bulletin");
    }

    #[test]
    fn failed_fd_parse_stays_with_fd_family() {
        let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
            "FDFAIL.TXT",
            b"000 \nSAGL31 BGGH 070200\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n",
        ));

        let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
            panic!("expected malformed-family candidate");
        };
        assert_eq!(candidate.family, "fd_bulletin");
        assert_eq!(candidate.issues[0].code, "invalid_fd_bulletin");
    }
}
