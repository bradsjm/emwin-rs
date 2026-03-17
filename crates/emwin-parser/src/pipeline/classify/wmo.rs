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
    filename_stem, first_nonempty_line, malformed_supported_family, starts_with_icao_sigmet_line,
    unsupported_wmo_candidate,
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

pub(super) fn classify_wmo_taf(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
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

pub(super) fn classify_wmo_pirep(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if looks_like_international_pirep_bulletin(context.body_text) {
        return Some(unsupported_wmo_candidate(
            context.header,
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
        return Some(unsupported_wmo_candidate(
            context.header,
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
        unsupported_wmo_candidate(
            context.header,
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
        unsupported_wmo_candidate(
            context.header,
            "unsupported_surface_observation_bulletin",
            "recognized valid WMO surface observation bulletin, but parsing is not implemented",
            context.body_text,
        )
    })
}

pub(super) fn classify_wmo_canadian_text_unsupported(
    context: &WmoClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    looks_like_canadian_text_bulletin(context.header, context.body_text).then(|| {
        unsupported_wmo_candidate(
            context.header,
            "unsupported_canadian_text_bulletin",
            "recognized valid WMO Canadian text bulletin, but parsing is not implemented",
            context.body_text,
        )
    })
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
