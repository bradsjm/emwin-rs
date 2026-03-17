//! Strategy-based classification for parsed envelopes.
//!
//! The classifier keeps explicit ordered strategies, but the implementation is
//! split into focused AFOS and WMO modules so routing policy and family-specific
//! guards stay easier to audit.

mod common;
mod context;
mod text;
mod wmo;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};

use crate::ProductEnrichmentSource;
use crate::pipeline::candidate::{ClassificationCandidate, DsmCandidate};
use crate::pipeline::{EnvelopeKind, ParsedEnvelope};
use crate::specialized::dsm::parse_dsm_bulletin;

use self::common::{filename_stem, first_nonempty_line, malformed_supported_family};
use self::context::{TextClassificationContext, WmoClassificationContext};

type TextStrategy = for<'a> fn(&TextClassificationContext<'a>) -> Option<ClassificationCandidate>;
type WmoStrategy = for<'a> fn(&WmoClassificationContext<'a>) -> Option<ClassificationCandidate>;

const TEXT_STRATEGIES: &[TextStrategy] = &[
    text::classify_text_fd,
    text::classify_text_metar,
    text::classify_text_taf,
    text::classify_text_pirep,
    text::classify_text_sigmet,
    text::classify_text_lsr,
    text::classify_text_cli,
    text::classify_text_cwa,
    text::classify_text_wwp,
    text::classify_text_saw,
    text::classify_text_sel,
    text::classify_text_cf6,
    text::classify_text_dsm,
    text::classify_text_hml,
    text::classify_text_mos,
    text::classify_text_mcd,
    text::classify_text_ero,
    text::classify_text_spc_outlook,
];

const WMO_STRATEGIES: &[WmoStrategy] = &[
    wmo::classify_wmo_fd,
    wmo::classify_wmo_pirep,
    wmo::classify_wmo_dsm,
    wmo::classify_wmo_metar,
    wmo::classify_wmo_taf,
    wmo::classify_wmo_dcp,
    wmo::classify_wmo_sigmet,
    wmo::classify_wmo_cwa,
    wmo::classify_wmo_airmet_unsupported,
    wmo::classify_wmo_surface_observation_unsupported,
    wmo::classify_wmo_canadian_text_unsupported,
    wmo::classify_wmo_unknown_valid,
];

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
    let Some(context) = TextClassificationContext::from_envelope(envelope) else {
        return ClassificationCandidate::Unknown;
    };

    for strategy in TEXT_STRATEGIES {
        if let Some(candidate) = strategy(&context) {
            return candidate;
        }
    }

    let body_request = context.body_request();

    ClassificationCandidate::TextGeneric(crate::pipeline::candidate::TextGenericCandidate {
        header: context.header.clone(),
        pil: context.pil,
        title: context.title,
        body_request,
        bbb_kind: context.bbb_kind,
        reference_time: context.reference_time,
    })
}

fn classify_wmo_envelope(envelope: &ParsedEnvelope) -> ClassificationCandidate {
    let Some(context) = WmoClassificationContext::from_envelope(envelope) else {
        return ClassificationCandidate::Unknown;
    };

    for strategy in WMO_STRATEGIES {
        if let Some(candidate) = strategy(&context) {
            return candidate;
        }
    }

    ClassificationCandidate::Unknown
}

fn classify_unknown_text_envelope(envelope: &ParsedEnvelope) -> Option<ClassificationCandidate> {
    let text = envelope.normalized.text_str()?;
    if !text::looks_like_dsm_text_product("", text) {
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
