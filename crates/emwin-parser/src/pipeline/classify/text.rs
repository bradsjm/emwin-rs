//! AFOS-backed classification strategies and recognition guards.

use chrono::Utc;

use crate::ProductEnrichmentSource;
use crate::data::TextProductRouting;
use crate::pipeline::candidate::{
    Cf6Candidate, ClassificationCandidate, CliCandidate, CwaCandidate, DsmCandidate, EroCandidate,
    FdCandidate, HmlCandidate, LsrCandidate, McdCandidate, MetarCandidate, MosCandidate,
    PirepCandidate, SawCandidate, SelCandidate, SigmetCandidate, SpcOutlookCandidate, TafCandidate,
    WwpCandidate,
};
use crate::specialized::cf6::parse_cf6_bulletin;
use crate::specialized::cli::parse_cli_bulletin;
use crate::specialized::cwa::parse_cwa_bulletin;
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

use super::common::{
    filename_stem, first_nonempty_line, malformed_supported_family, starts_with_icao_sigmet_line,
};
use super::context::TextClassificationContext;

pub(super) fn classify_text_fd(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(context, TextProductRouting::Fd, looks_like_fd_text_product) {
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
            context.body_request(),
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
    }))
}

pub(super) fn classify_text_metar(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
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

pub(super) fn classify_text_taf(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
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

pub(super) fn classify_text_pirep(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Pirep,
        looks_like_pirep_text_product,
    ) {
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
    }))
}

pub(super) fn classify_text_sigmet(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Sigmet,
        looks_like_sigmet_text_product,
    ) {
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_lsr(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Lsr,
        looks_like_lsr_text_product,
    ) {
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
            context.body_request(),
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues,
    }))
}

pub(super) fn classify_text_cli(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Cli,
        looks_like_cli_text_product,
    ) {
        return None;
    }
    let bulletin = parse_cli_bulletin(context.body_text)?;
    Some(ClassificationCandidate::Cli(CliCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_cwa(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Cwa,
        looks_like_cwa_text_product,
    ) {
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
            context.body_request(),
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_wwp(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Wwp,
        looks_like_wwp_text_product,
    ) {
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_cf6(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Cf6,
        looks_like_cf6_text_product,
    ) {
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues,
    }))
}

pub(super) fn classify_text_saw(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Saw,
        looks_like_saw_text_product,
    ) {
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_sel(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Sel,
        looks_like_sel_text_product,
    ) {
        return None;
    }
    let bulletin = parse_sel_bulletin(context.body_text)?;
    Some(ClassificationCandidate::Sel(SelCandidate {
        header: context.header.clone(),
        pil: context.pil.clone(),
        bbb_kind: context.bbb_kind,
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_dsm(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Dsm,
        looks_like_dsm_text_product,
    ) {
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_hml(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Hml,
        looks_like_hml_text_product,
    ) {
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_mos(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Mos,
        looks_like_mos_text_product,
    ) {
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
            context.body_request(),
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_mcd(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Mcd,
        looks_like_mcd_text_product,
    ) {
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_ero(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::Ero,
        looks_like_ero_text_product,
    ) {
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

pub(super) fn classify_text_spc_outlook(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if !matches_routed_text_family(
        context,
        TextProductRouting::SpcOutlook,
        looks_like_spc_outlook_text_product,
    ) {
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
            context.body_request(),
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
        body_request: context.body_request(),
        bulletin,
        issues: Vec::new(),
    }))
}

fn matches_routed_text_family(
    context: &TextClassificationContext<'_>,
    routing: TextProductRouting,
    guard: fn(&str, &str) -> bool,
) -> bool {
    context.has_routing(routing) && guard(&context.header.afos, context.body_text)
}

/// Detects whether AFOS text resembles an FD bulletin.
pub(super) fn looks_like_fd_text_product(afos: &str, body_text: &str) -> bool {
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
pub(super) fn looks_like_fd_wmo_bulletin(filename: &str, body_text: &str) -> bool {
    filename_stem(filename).starts_with("FD")
        && body_text.contains("DATA BASED ON ")
        && body_text.contains("VALID ")
        && body_text
            .lines()
            .any(|line| line.trim_start().starts_with("FT "))
}

/// Detects whether AFOS text resembles a PIREP bulletin.
pub(super) fn looks_like_pirep_text_product(afos: &str, body_text: &str) -> bool {
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
pub(super) fn looks_like_cli_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CLI")
        || (body_text.to_ascii_uppercase().contains("CLIMATE SUMMARY")
            && body_text.to_ascii_uppercase().contains("WEATHER ITEM"))
}

/// Detects whether AFOS text resembles a SIGMET bulletin.
pub(super) fn looks_like_sigmet_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SIG")
        || afos.starts_with("WS")
        || body_text.trim_start().starts_with("CONVECTIVE SIGMET ")
        || body_text.trim_start().starts_with("KZAK SIGMET ")
        || body_text.trim_start().starts_with("SIGMET ")
}

pub(super) fn looks_like_lsr_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("LSR") && {
        let uppercase = body_text.to_ascii_uppercase();
        (uppercase.contains("..TIME..") && uppercase.contains("..DATE.."))
            || uppercase.contains("PRELIMINARY LOCAL STORM REPORT")
            || uppercase.contains("LOCAL STORM REPORT")
    }
}

pub(super) fn looks_like_cwa_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CWA")
        || (body_text.contains(" CWA ") && body_text.contains("VALID UNTIL"))
        || body_text
            .lines()
            .next()
            .is_some_and(|line| line.contains(" CWA "))
}

pub(super) fn looks_like_wwp_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("WWP")
        && body_text.contains("PROBABILITY TABLE:")
        && (body_text.contains("ATTRIBUTE TABLE:")
            || body_text.contains("WATCH PROBABILITIES FOR WT"))
}

pub(super) fn looks_like_saw_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SAW") && body_text.contains("WW ") && body_text.contains("SPC AWW")
}

pub(super) fn looks_like_sel_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SEL") && body_text.contains("URGENT - IMMEDIATE BROADCAST REQUESTED") && {
        let uppercase = body_text.to_ascii_uppercase();
        uppercase.contains("WATCH NUMBER") || uppercase.contains("WATCH - NUMBER")
    }
}

pub(super) fn looks_like_cf6_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CF6")
        || (body_text.contains("PRELIMINARY LOCAL CLIMATOLOGICAL DATA")
            && body_text.contains("MONTH:")
            && body_text.contains("YEAR:"))
}

pub(super) fn looks_like_dsm_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("DSM")
        || body_text.contains(" DS ")
            && body_text.lines().any(|line| {
                let trimmed = line.trim();
                trimmed.len() >= 7
                    && trimmed.as_bytes().get(4..7) == Some(b" DS")
                    && trimmed.contains('/')
            })
}

pub(super) fn looks_like_hml_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("HML") && body_text.contains("<?xml")
}

pub(super) fn looks_like_mos_text_product(afos: &str, body_text: &str) -> bool {
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

pub(super) fn looks_like_mcd_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "SWOMCD" | "FFGMPD")
        || body_text.contains("MESOSCALE DISCUSSION")
        || body_text.contains("MPD")
        || body_text.contains("AREAS AFFECTED")
}

pub(super) fn looks_like_ero_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "RBG94E" | "RBG98E" | "RBG99E")
        && body_text.contains("Excessive Rainfall")
        && body_text.contains("TO THE RIGHT OF A LINE FROM")
}

pub(super) fn looks_like_spc_outlook_text_product(afos: &str, body_text: &str) -> bool {
    matches!(
        afos,
        "PTSDY1" | "PTSDY2" | "PTSDY3" | "PTSD48" | "PFWFD1" | "PFWFD2" | "PFWF38"
    ) && body_text.contains("VALID")
}

/// Detects whether WMO-only text resembles a SIGMET bulletin.
pub(super) fn looks_like_sigmet_wmo_bulletin(body_text: &str) -> bool {
    let Some(first_line) = first_nonempty_line(body_text) else {
        return false;
    };
    first_line.starts_with("SIGMET ")
        || starts_with_icao_sigmet_line(first_line)
        || (first_line.contains(" SIGMET ") && first_line.contains(" VALID "))
}

pub(super) fn looks_like_metar_wmo_bulletin(body_text: &str) -> bool {
    body_text.lines().map(str::trim).any(|line| {
        line == "METAR"
            || line == "SPECI"
            || line.starts_with("METAR ")
            || line.starts_with("SPECI ")
    })
}

pub(super) fn looks_like_metar_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "METAR" | "SPECI") || looks_like_metar_wmo_bulletin(body_text)
}

pub(super) fn looks_like_taf_wmo_bulletin(body_text: &str) -> bool {
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

pub(super) fn looks_like_taf_text_product(afos: &str, body_text: &str) -> bool {
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
pub(super) fn looks_like_airmet_wmo_bulletin(body_text: &str) -> bool {
    first_nonempty_line(body_text)
        .is_some_and(|line| line.contains(" AIRMET ") && line.contains(" VALID "))
}

/// Detects Canadian Environment Canada text bulletins.
pub(super) fn looks_like_canadian_text_bulletin(
    header: &crate::WmoHeader,
    body_text: &str,
) -> bool {
    header.cccc.starts_with("CW") || body_text.contains("ENVIRONMENT CANADA")
}

/// Detects unsupported surface observation bulletins.
pub(super) fn looks_like_surface_observation_bulletin(body_text: &str) -> bool {
    first_nonempty_line(body_text).is_some_and(|line| line.starts_with("NPL SA "))
}
