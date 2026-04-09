//! WMO-only classification strategies and unsupported bulletin recognition.

use chrono::Utc;

use crate::ProductEnrichmentSource;
use crate::pipeline::candidate::{
    ClassificationCandidate, CwaCandidate, DcpCandidate, DsmCandidate, FdCandidate, MetarCandidate,
    PirepCandidate, SigmetCandidate, TafCandidate,
};
use crate::specialized::cwa::parse_cwa_bulletin;
use crate::specialized::dcp::parse_dcp_bulletin;
use crate::specialized::dsm::parse_dsm_bulletin;
use crate::specialized::fd::parse_fd_bulletin;
use crate::specialized::metar::parse_metar_bulletin;
use crate::specialized::pirep::parse_pirep_bulletin;
use crate::specialized::sigmet::parse_sigmet_bulletin;
use crate::specialized::taf::parse_taf_bulletin;

use super::common::{
    SupportedFamilySpec, classify_supported_wmo, filename_stem, first_nonempty_line,
    looks_like_multipart_taf_bulletin, malformed_supported_family, parsed, parsed_with_issues,
    require_reference_time, starts_with_icao_sigmet_line, unsupported_wmo_candidate,
    unsupported_wmo_family_candidate,
};
use super::context::WmoClassificationContext;
use super::text::{
    looks_like_airmet_wmo_bulletin, looks_like_canadian_text_bulletin, looks_like_cwa_text_product,
    looks_like_dsm_text_product, looks_like_fd_wmo_bulletin, looks_like_metar_wmo_bulletin,
    looks_like_pirep_text_product, looks_like_sigmet_wmo_bulletin,
    looks_like_surface_observation_bulletin, looks_like_taf_wmo_bulletin,
};

const FD_WMO_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::WmoBulletin,
    family: "fd_bulletin",
    title: "Winds and temperatures aloft",
    issue_kind: "fd_parse",
    invalid_code: "invalid_fd_bulletin",
    invalid_message: "recognized FD bulletin, but structured parsing failed",
    missing_reference_code: Some("missing_reference_time"),
    missing_reference_message: Some(
        "recognized FD bulletin, but WMO timestamp could not be resolved",
    ),
    malformed_pil: None,
};

const METAR_WMO_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::WmoBulletin,
    family: "metar_collective",
    title: "METAR bulletin",
    issue_kind: "metar_parse",
    invalid_code: "invalid_metar_bulletin",
    invalid_message: "recognized METAR bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const TAF_WMO_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::WmoBulletin,
    family: "taf_bulletin",
    title: "Terminal Aerodrome Forecast",
    issue_kind: "taf_parse",
    invalid_code: "invalid_taf_bulletin",
    invalid_message: "recognized TAF bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const DSM_WMO_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::WmoBulletin,
    family: "dsm_bulletin",
    title: "Daily summary message",
    issue_kind: "dsm_parse",
    invalid_code: "invalid_dsm_bulletin",
    invalid_message: "recognized DSM bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const PIREP_WMO_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::WmoBulletin,
    family: "pirep_bulletin",
    title: "Pilot report bulletin",
    issue_kind: "pirep_parse",
    invalid_code: "invalid_pirep_bulletin",
    invalid_message: "recognized PIREP bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const SIGMET_WMO_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::WmoBulletin,
    family: "sigmet_bulletin",
    title: "SIGMET bulletin",
    issue_kind: "sigmet_parse",
    invalid_code: "invalid_sigmet_bulletin",
    invalid_message: "recognized SIGMET bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const CWA_WMO_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::WmoBulletin,
    family: "cwa_bulletin",
    title: "Center Weather Advisory",
    issue_kind: "cwa_parse",
    invalid_code: "invalid_cwa_bulletin",
    invalid_message: "recognized CWA bulletin, but structured parsing failed",
    missing_reference_code: Some("missing_reference_time"),
    missing_reference_message: Some(
        "recognized CWA bulletin, but WMO timestamp could not be resolved",
    ),
    malformed_pil: Some("CWA"),
};

pub(super) fn classify_wmo_fd(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_wmo(
        context,
        |context| looks_like_fd_wmo_bulletin(context.filename, context.body_text),
        &FD_WMO_SPEC,
        |context| {
            let reference_time = require_reference_time(context.header.timestamp(Utc::now()))?;
            parsed(
                parse_fd_bulletin(
                    context.body_text,
                    Some(filename_stem(context.filename)),
                    reference_time,
                )
                .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Fd(FdCandidate {
                source: ProductEnrichmentSource::WmoBulletin,
                family: FD_WMO_SPEC.family,
                title: FD_WMO_SPEC.title,
                header: None,
                wmo_header: Some(parts.header),
                pil: None,
                bbb_kind: None,
                body_request: None,
                bulletin,
            })
        },
    )
}

pub(super) fn classify_wmo_metar(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if context.header.ttaaii.starts_with("SACN")
        || first_nonempty_line(context.body_text).is_some_and(|line| line.starts_with("NPL SA "))
    {
        return None;
    }
    classify_supported_wmo(
        context,
        |context| looks_like_metar_wmo_bulletin(context.body_text),
        &METAR_WMO_SPEC,
        |context| {
            parsed_with_issues(
                parse_metar_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, issues| {
            ClassificationCandidate::Metar(MetarCandidate {
                source: ProductEnrichmentSource::WmoBulletin,
                header: None,
                wmo_header: Some(parts.header),
                pil: None,
                bbb_kind: None,
                body_request: None,
                bulletin,
                issues,
            })
        },
    )
}

pub(super) fn classify_wmo_taf(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if looks_like_multipart_taf_bulletin(context.body_text) {
        return Some(unsupported_wmo_family_candidate(
            context.header,
            "multipart_taf_bulletin",
            Some("Multipart terminal aerodrome forecast"),
            "unsupported_multipart_taf_bulletin",
            "recognized multipart TAF bulletin, but multipart TAF assembly is not implemented",
            context.body_text,
        ));
    }
    classify_supported_wmo(
        context,
        |context| looks_like_taf_wmo_bulletin(context.body_text),
        &TAF_WMO_SPEC,
        |context| {
            parsed(
                parse_taf_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Taf(TafCandidate {
                source: ProductEnrichmentSource::WmoBulletin,
                header: None,
                wmo_header: Some(parts.header),
                pil: None,
                bbb_kind: None,
                body_request: None,
                bulletin,
            })
        },
    )
}

pub(super) fn classify_wmo_dsm(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_wmo(
        context,
        |context| looks_like_dsm_text_product("", context.body_text),
        &DSM_WMO_SPEC,
        |context| {
            parsed_with_issues(
                parse_dsm_bulletin(
                    context.body_text,
                    context
                        .header
                        .timestamp(Utc::now())
                        .unwrap_or_else(Utc::now),
                )
                .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, issues| {
            ClassificationCandidate::Dsm(DsmCandidate {
                source: ProductEnrichmentSource::WmoBulletin,
                header: None,
                wmo_header: Some(parts.header),
                pil: None,
                bbb_kind: None,
                body_request: None,
                bulletin,
                issues,
            })
        },
    )
}

pub(super) fn classify_wmo_pirep(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if looks_like_international_pirep_bulletin(context.body_text) {
        return Some(unsupported_wmo_family_candidate(
            context.header,
            "international_pirep_bulletin",
            Some("International pilot report bulletin"),
            "unsupported_international_pirep_bulletin",
            "recognized international PIREP bulletin, but this parser only supports domestic slash-tag PIREP structure",
            context.body_text,
        ));
    }
    classify_supported_wmo(
        context,
        |context| looks_like_pirep_text_product("", context.body_text),
        &PIREP_WMO_SPEC,
        |context| {
            parsed(
                parse_pirep_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Pirep(PirepCandidate {
                source: ProductEnrichmentSource::WmoBulletin,
                header: None,
                wmo_header: Some(parts.header),
                pil: None,
                bbb_kind: None,
                body_request: None,
                bulletin,
            })
        },
    )
}

pub(super) fn classify_wmo_dcp(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    let bulletin = parse_dcp_bulletin(context.filename, context.header, context.body_text)?;

    Some(ClassificationCandidate::Dcp(DcpCandidate {
        header: context.header.clone(),
        bulletin,
    }))
}

pub(super) fn classify_wmo_sigmet(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !looks_like_sigmet_wmo_bulletin(context.body_text) {
        return None;
    }
    if first_nonempty_line(context.body_text)
        .is_some_and(|line| starts_with_icao_sigmet_line(line) && !line.starts_with("KZAK SIGMET "))
    {
        return Some(unsupported_wmo_family_candidate(
            context.header,
            "international_sigmet_bulletin",
            Some("International SIGMET bulletin"),
            "unsupported_international_sigmet_bulletin",
            "recognized international SIGMET bulletin, but this parser only supports domestic SIGMET structure",
            context.body_text,
        ));
    }
    classify_supported_wmo(
        context,
        |context| looks_like_sigmet_wmo_bulletin(context.body_text),
        &SIGMET_WMO_SPEC,
        |context| {
            parsed(
                parse_sigmet_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Sigmet(SigmetCandidate {
                source: ProductEnrichmentSource::WmoBulletin,
                header: None,
                wmo_header: Some(parts.header),
                pil: None,
                bbb_kind: None,
                body_request: None,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
}

fn looks_like_international_pirep_bulletin(body_text: &str) -> bool {
    first_nonempty_line(body_text).is_some_and(|line| {
        let upper = line.to_ascii_uppercase();
        upper == "PIREP" || (upper.starts_with("PIREP ") && upper.contains(" OBSD AT "))
    })
}

pub(super) fn classify_wmo_cwa(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_wmo(
        context,
        |context| looks_like_cwa_text_product("", context.body_text),
        &CWA_WMO_SPEC,
        |context| {
            let reference_time = require_reference_time(context.header.timestamp(Utc::now()))?;
            parsed(
                parse_cwa_bulletin(context.body_text, reference_time)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Cwa(CwaCandidate {
                header: None,
                wmo_header: Some(parts.header),
                pil: Some("CWA".to_string()),
                bbb_kind: None,
                body_request: None,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
}

pub(super) fn classify_wmo_airmet_unsupported(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    looks_like_airmet_wmo_bulletin(context.body_text).then(|| {
        unsupported_wmo_family_candidate(
            context.header,
            "airmet_bulletin",
            Some("AIRMET bulletin"),
            "unsupported_airmet_bulletin",
            "recognized valid WMO AIRMET bulletin, but textual AIRMET parsing is not implemented",
            context.body_text,
        )
    })
}

pub(super) fn classify_wmo_surface_observation_unsupported(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    looks_like_surface_observation_bulletin(context.body_text).then(|| {
        unsupported_wmo_family_candidate(
            context.header,
            "surface_observation_bulletin",
            Some("Surface observation bulletin"),
            "unsupported_surface_observation_bulletin",
            "recognized valid WMO surface observation bulletin, but parsing is not implemented",
            context.body_text,
        )
    })
}

pub(super) fn classify_wmo_canadian(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    let family = classify_canadian_wmo_family(context)?;

    match family {
        CanadianWmoFamily::SurfaceObservation => classify_canadian_surface_observation(context),
        CanadianWmoFamily::TornadoWarning => Some(canadian_unsupported_candidate(
            context,
            "canadian_tornado_warning_bulletin",
            Some("Canadian tornado warning bulletin"),
            "unsupported_canadian_tornado_warning_bulletin",
            "recognized valid WMO Canadian tornado warning bulletin, but parsing is not implemented",
        )),
        CanadianWmoFamily::SevereThunderstormWarning => Some(canadian_unsupported_candidate(
            context,
            "canadian_severe_thunderstorm_warning_bulletin",
            Some("Canadian severe thunderstorm warning bulletin"),
            "unsupported_canadian_severe_thunderstorm_warning_bulletin",
            "recognized valid WMO Canadian severe thunderstorm warning bulletin, but parsing is not implemented",
        )),
        CanadianWmoFamily::TropicalCyclonePublicInformation => {
            Some(canadian_unsupported_candidate(
                context,
                "canadian_tropical_cyclone_public_information",
                Some("Canadian tropical cyclone public information"),
                "unsupported_canadian_tropical_cyclone_public_information",
                "recognized valid WMO Canadian tropical cyclone public information bulletin, but parsing is not implemented",
            ))
        }
        CanadianWmoFamily::TropicalCycloneWatchWarning => Some(canadian_unsupported_candidate(
            context,
            "canadian_tropical_cyclone_watch_warning_bulletin",
            Some("Canadian tropical cyclone watch/warning bulletin"),
            "unsupported_canadian_tropical_cyclone_watch_warning_bulletin",
            "recognized valid WMO Canadian tropical cyclone watch/warning bulletin, but parsing is not implemented",
        )),
        CanadianWmoFamily::TropicalCycloneTechnicalDiscussion => {
            Some(canadian_unsupported_candidate(
                context,
                "canadian_tropical_cyclone_technical_discussion",
                Some("Canadian tropical cyclone technical discussion"),
                "unsupported_canadian_tropical_cyclone_technical_discussion",
                "recognized valid WMO Canadian tropical cyclone technical discussion bulletin, but parsing is not implemented",
            ))
        }
        CanadianWmoFamily::StormSummary => Some(canadian_unsupported_candidate(
            context,
            "canadian_storm_summary",
            Some("Canadian storm summary"),
            "unsupported_canadian_storm_summary",
            "recognized valid WMO Canadian storm summary bulletin, but parsing is not implemented",
        )),
        CanadianWmoFamily::SpecialWeatherStatement => Some(canadian_unsupported_candidate(
            context,
            "canadian_special_weather_statement",
            Some("Canadian special weather statement"),
            "unsupported_canadian_special_weather_statement",
            "recognized valid WMO Canadian special weather statement bulletin, but parsing is not implemented",
        )),
        CanadianWmoFamily::VolcanicAshBulletin => Some(canadian_unsupported_candidate(
            context,
            "canadian_volcanic_ash_bulletin",
            Some("Canadian volcanic ash bulletin"),
            "unsupported_canadian_volcanic_ash_bulletin",
            "recognized valid WMO Canadian volcanic ash bulletin, but parsing is not implemented",
        )),
        CanadianWmoFamily::Residual => Some(canadian_unsupported_candidate(
            context,
            "canadian_text_bulletin",
            Some("Canadian text bulletin"),
            "unsupported_canadian_text_bulletin",
            "recognized valid WMO Canadian text bulletin, but parsing is not implemented",
        )),
    }
}

pub(super) fn classify_wmo_unknown_valid(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    Some(unsupported_wmo_candidate(
        context.header,
        "unsupported_wmo_bulletin",
        "recognized valid WMO bulletin without AFOS line, but no parser is available",
        context.body_text,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanadianWmoFamily {
    SurfaceObservation,
    TornadoWarning,
    SevereThunderstormWarning,
    TropicalCyclonePublicInformation,
    TropicalCycloneWatchWarning,
    TropicalCycloneTechnicalDiscussion,
    StormSummary,
    SpecialWeatherStatement,
    VolcanicAshBulletin,
    Residual,
}

fn classify_canadian_wmo_family(
    context: &WmoClassificationContext<'_>,
) -> Option<CanadianWmoFamily> {
    let ttaaii = context.header.ttaaii.as_str();

    if ttaaii.starts_with("SACN") {
        return Some(CanadianWmoFamily::SurfaceObservation);
    }
    if ttaaii.starts_with("WFCN") {
        return Some(CanadianWmoFamily::TornadoWarning);
    }
    if ttaaii.starts_with("WUCN") {
        return Some(CanadianWmoFamily::SevereThunderstormWarning);
    }
    if ttaaii.starts_with("WTCN") && is_tropical_canadian_number(ttaaii) {
        return Some(CanadianWmoFamily::TropicalCycloneWatchWarning);
    }
    if ttaaii.starts_with("FXCN") && is_tropical_canadian_number(ttaaii) {
        return Some(CanadianWmoFamily::TropicalCycloneTechnicalDiscussion);
    }
    if ttaaii.starts_with("WWCN") {
        return Some(if is_tropical_canadian_number(ttaaii) {
            CanadianWmoFamily::StormSummary
        } else {
            CanadianWmoFamily::SpecialWeatherStatement
        });
    }
    if ttaaii.starts_with("WOCN") {
        return Some(if is_tropical_canadian_number(ttaaii) {
            CanadianWmoFamily::TropicalCyclonePublicInformation
        } else {
            CanadianWmoFamily::SpecialWeatherStatement
        });
    }
    if ttaaii.starts_with("FVCN") && matches!(canadian_sequence(ttaaii), Some(1..=4)) {
        return Some(CanadianWmoFamily::VolcanicAshBulletin);
    }
    looks_like_canadian_text_bulletin(context.header, context.body_text)
        .then_some(CanadianWmoFamily::Residual)
}

fn classify_canadian_surface_observation(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    let Some((bulletin, issues)) = parse_metar_bulletin(context.body_text) else {
        return Some(malformed_supported_family(
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
            "recognized Canadian surface observation bulletin, but structured parsing failed",
            first_nonempty_line(context.body_text),
        ));
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

fn canadian_unsupported_candidate(
    context: &WmoClassificationContext<'_>,
    family: &'static str,
    title: Option<&'static str>,
    code: &'static str,
    message: &'static str,
) -> ClassificationCandidate {
    unsupported_wmo_family_candidate(
        context.header,
        family,
        title,
        code,
        message,
        context.body_text,
    )
}

fn canadian_sequence(ttaaii: &str) -> Option<u8> {
    let digits = ttaaii.get(4..6)?;
    digits.parse().ok()
}

fn is_tropical_canadian_number(ttaaii: &str) -> bool {
    matches!(canadian_sequence(ttaaii), Some(31..=33 | 41..=43))
}
