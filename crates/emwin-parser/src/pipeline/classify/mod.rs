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

struct TextFamilySpec {
    family: &'static str,
    title: &'static str,
    classify: TextStrategy,
}

struct WmoFamilySpec {
    family: &'static str,
    title: &'static str,
    classify: WmoStrategy,
}

const TEXT_SPECS: &[TextFamilySpec] = &[
    TextFamilySpec {
        family: "fd_bulletin",
        title: "Winds and temperatures aloft",
        classify: text::classify_text_fd,
    },
    TextFamilySpec {
        family: "metar_collective",
        title: "METAR bulletin",
        classify: text::classify_text_metar,
    },
    TextFamilySpec {
        family: "taf_bulletin",
        title: "Terminal Aerodrome Forecast",
        classify: text::classify_text_taf,
    },
    TextFamilySpec {
        family: "pirep_bulletin",
        title: "Pilot report bulletin",
        classify: text::classify_text_pirep,
    },
    TextFamilySpec {
        family: "sigmet_bulletin",
        title: "SIGMET bulletin",
        classify: text::classify_text_sigmet,
    },
    TextFamilySpec {
        family: "lsr_bulletin",
        title: "Local storm report bulletin",
        classify: text::classify_text_lsr,
    },
    TextFamilySpec {
        family: "cli_bulletin",
        title: "Daily climate report",
        classify: text::classify_text_cli,
    },
    TextFamilySpec {
        family: "cwa_bulletin",
        title: "Center Weather Advisory",
        classify: text::classify_text_cwa,
    },
    TextFamilySpec {
        family: "wwp_bulletin",
        title: "Watch probability table",
        classify: text::classify_text_wwp,
    },
    TextFamilySpec {
        family: "saw_bulletin",
        title: "SPC preliminary notice of watch",
        classify: text::classify_text_saw,
    },
    TextFamilySpec {
        family: "sel_bulletin",
        title: "SPC watch bulletin",
        classify: text::classify_text_sel,
    },
    TextFamilySpec {
        family: "cf6_bulletin",
        title: "Climate summary bulletin",
        classify: text::classify_text_cf6,
    },
    TextFamilySpec {
        family: "dsm_bulletin",
        title: "Daily summary message",
        classify: text::classify_text_dsm,
    },
    TextFamilySpec {
        family: "hml_bulletin",
        title: "Hydrological Markup Language bulletin",
        classify: text::classify_text_hml,
    },
    TextFamilySpec {
        family: "mos_bulletin",
        title: "Model output statistics guidance",
        classify: text::classify_text_mos,
    },
    TextFamilySpec {
        family: "mcd_bulletin",
        title: "Mesoscale discussion bulletin",
        classify: text::classify_text_mcd,
    },
    TextFamilySpec {
        family: "ero_bulletin",
        title: "Excessive rainfall outlook",
        classify: text::classify_text_ero,
    },
    TextFamilySpec {
        family: "spc_outlook_bulletin",
        title: "SPC outlook bulletin",
        classify: text::classify_text_spc_outlook,
    },
];

const WMO_SPECS: &[WmoFamilySpec] = &[
    WmoFamilySpec {
        family: "fd_bulletin",
        title: "Winds and temperatures aloft",
        classify: wmo::classify_wmo_fd,
    },
    WmoFamilySpec {
        family: "pirep_bulletin",
        title: "Pilot report bulletin",
        classify: wmo::classify_wmo_pirep,
    },
    WmoFamilySpec {
        family: "dsm_bulletin",
        title: "Daily summary message",
        classify: wmo::classify_wmo_dsm,
    },
    WmoFamilySpec {
        family: "metar_collective",
        title: "METAR bulletin",
        classify: wmo::classify_wmo_metar,
    },
    WmoFamilySpec {
        family: "taf_bulletin",
        title: "Terminal Aerodrome Forecast",
        classify: wmo::classify_wmo_taf,
    },
    WmoFamilySpec {
        family: "dcp_telemetry_bulletin",
        title: "GOES DCP telemetry bulletin",
        classify: wmo::classify_wmo_dcp,
    },
    WmoFamilySpec {
        family: "sigmet_bulletin",
        title: "SIGMET bulletin",
        classify: wmo::classify_wmo_sigmet,
    },
    WmoFamilySpec {
        family: "cwa_bulletin",
        title: "Center Weather Advisory",
        classify: wmo::classify_wmo_cwa,
    },
    WmoFamilySpec {
        family: "airmet_bulletin",
        title: "AIRMET bulletin",
        classify: wmo::classify_wmo_airmet_unsupported,
    },
    WmoFamilySpec {
        family: "canadian_text_bulletin",
        title: "Canadian text bulletin",
        classify: wmo::classify_wmo_canadian,
    },
    WmoFamilySpec {
        family: "surface_observation_bulletin",
        title: "Surface observation bulletin",
        classify: wmo::classify_wmo_surface_observation_unsupported,
    },
    WmoFamilySpec {
        family: "unsupported_wmo_bulletin",
        title: "Unsupported WMO bulletin",
        classify: wmo::classify_wmo_unknown_valid,
    },
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

    for spec in TEXT_SPECS {
        debug_assert!(!spec.family.is_empty() && !spec.title.is_empty());
        if let Some(candidate) = (spec.classify)(&context) {
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

    for spec in WMO_SPECS {
        debug_assert!(!spec.family.is_empty() && !spec.title.is_empty());
        if let Some(candidate) = (spec.classify)(&context) {
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
    let Some((bulletin, issues)) = parse_dsm_bulletin(text, reference_time) else {
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
        issues,
    }))
}

fn filename_reference_time(filename: &str) -> Option<DateTime<Utc>> {
    let stem = filename_stem(filename);
    let prefix = stem.get(..12)?;
    chrono::NaiveDateTime::parse_from_str(prefix, "%Y%m%d%H%M")
        .ok()
        .map(|naive| naive.and_utc())
}
