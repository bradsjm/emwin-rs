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
    filename_stem, first_nonempty_line, looks_like_multipart_taf_bulletin,
    malformed_supported_family, starts_with_icao_sigmet_line, unsupported_wmo_candidate,
    unsupported_wmo_family_candidate,
};
use super::context::WmoClassificationContext;
use super::text::{
    looks_like_airmet_wmo_bulletin, looks_like_canadian_text_bulletin, looks_like_cwa_text_product,
    looks_like_dsm_text_product, looks_like_fd_wmo_bulletin, looks_like_metar_wmo_bulletin,
    looks_like_pirep_text_product, looks_like_sigmet_wmo_bulletin,
    looks_like_surface_observation_bulletin, looks_like_taf_wmo_bulletin,
};

pub(super) fn classify_wmo_fd(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
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

pub(super) fn classify_wmo_metar(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if context.header.ttaaii.starts_with("SACN")
        || first_nonempty_line(context.body_text).is_some_and(|line| line.starts_with("NPL SA "))
    {
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

pub(super) fn classify_wmo_dsm(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !looks_like_dsm_text_product("", context.body_text) {
        return None;
    }
    let reference_time = context
        .header
        .timestamp(Utc::now())
        .unwrap_or_else(Utc::now);
    let Some((bulletin, issues)) = parse_dsm_bulletin(context.body_text, reference_time) else {
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
        issues,
    }))
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

fn looks_like_international_pirep_bulletin(body_text: &str) -> bool {
    first_nonempty_line(body_text).is_some_and(|line| {
        let upper = line.to_ascii_uppercase();
        upper == "PIREP" || (upper.starts_with("PIREP ") && upper.contains(" OBSD AT "))
    })
}

pub(super) fn classify_wmo_cwa(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
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
