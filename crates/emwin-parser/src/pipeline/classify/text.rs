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
    SupportedFamilySpec, classify_supported_text, classify_supported_text_guarded, filename_stem,
    first_nonempty_line, looks_like_multipart_taf_bulletin, malformed_supported_family, parsed,
    parsed_with_issues, require_reference_time, starts_with_icao_sigmet_line,
};
use super::context::TextClassificationContext;

const FD_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "fd_bulletin",
    title: "Winds and temperatures aloft",
    issue_kind: "fd_parse",
    invalid_code: "invalid_fd_bulletin",
    invalid_message: "recognized FD bulletin, but structured parsing failed",
    missing_reference_code: Some("missing_reference_time"),
    missing_reference_message: Some(
        "recognized FD bulletin, but header timestamp could not be resolved",
    ),
    malformed_pil: None,
};

const METAR_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "metar_collective",
    title: "METAR bulletin",
    issue_kind: "metar_parse",
    invalid_code: "invalid_metar_bulletin",
    invalid_message: "recognized METAR bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const TAF_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "taf_bulletin",
    title: "Terminal Aerodrome Forecast",
    issue_kind: "taf_parse",
    invalid_code: "invalid_taf_bulletin",
    invalid_message: "recognized TAF bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const PIREP_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "pirep_bulletin",
    title: "Pilot report bulletin",
    issue_kind: "pirep_parse",
    invalid_code: "invalid_pirep_bulletin",
    invalid_message: "recognized PIREP bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const SIGMET_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "sigmet_bulletin",
    title: "SIGMET bulletin",
    issue_kind: "sigmet_parse",
    invalid_code: "invalid_sigmet_bulletin",
    invalid_message: "recognized SIGMET bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const LSR_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "lsr_bulletin",
    title: "Local storm report bulletin",
    issue_kind: "lsr_parse",
    invalid_code: "invalid_lsr_bulletin",
    invalid_message: "recognized LSR bulletin, but structured parsing failed",
    missing_reference_code: Some("missing_reference_time"),
    missing_reference_message: Some(
        "recognized LSR bulletin, but header timestamp could not be resolved",
    ),
    malformed_pil: None,
};

const CWA_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "cwa_bulletin",
    title: "Center Weather Advisory",
    issue_kind: "cwa_parse",
    invalid_code: "invalid_cwa_bulletin",
    invalid_message: "recognized CWA bulletin, but structured parsing failed",
    missing_reference_code: Some("missing_reference_time"),
    missing_reference_message: Some(
        "recognized CWA bulletin, but header timestamp could not be resolved",
    ),
    malformed_pil: None,
};

const WWP_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "wwp_bulletin",
    title: "Watch probability table",
    issue_kind: "wwp_parse",
    invalid_code: "invalid_wwp_bulletin",
    invalid_message: "recognized WWP bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const CF6_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "cf6_bulletin",
    title: "Climate summary bulletin",
    issue_kind: "cf6_parse",
    invalid_code: "invalid_cf6_bulletin",
    invalid_message: "recognized CF6 bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const DSM_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "dsm_bulletin",
    title: "Daily summary message",
    issue_kind: "dsm_parse",
    invalid_code: "invalid_dsm_bulletin",
    invalid_message: "recognized DSM bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const HML_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "hml_bulletin",
    title: "Hydrological Markup Language bulletin",
    issue_kind: "hml_parse",
    invalid_code: "invalid_hml_bulletin",
    invalid_message: "recognized HML bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const MOS_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "mos_bulletin",
    title: "Model output statistics guidance",
    issue_kind: "mos_parse",
    invalid_code: "invalid_mos_bulletin",
    invalid_message: "recognized MOS bulletin, but structured parsing failed",
    missing_reference_code: Some("missing_reference_time"),
    missing_reference_message: Some(
        "recognized MOS bulletin, but header timestamp could not be resolved",
    ),
    malformed_pil: None,
};

const ERO_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "ero_bulletin",
    title: "Excessive rainfall outlook",
    issue_kind: "ero_parse",
    invalid_code: "invalid_ero_bulletin",
    invalid_message: "recognized ERO bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

const SPC_OUTLOOK_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
    source: ProductEnrichmentSource::TextHeader,
    family: "spc_outlook_bulletin",
    title: "SPC outlook bulletin",
    issue_kind: "spc_outlook_parse",
    invalid_code: "invalid_spc_outlook_bulletin",
    invalid_message: "recognized SPC outlook bulletin, but structured parsing failed",
    missing_reference_code: None,
    missing_reference_message: None,
    malformed_pil: None,
};

pub(super) fn classify_text_fd(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text(
        context,
        TextProductRouting::Fd,
        looks_like_fd_text_product,
        &FD_TEXT_SPEC,
        |context| {
            let reference_time = require_reference_time(context.reference_time)?;
            parsed(
                parse_fd_bulletin(
                    context.body_text,
                    Some(context.header.afos.as_str()),
                    reference_time,
                )
                .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Fd(FdCandidate {
                source: ProductEnrichmentSource::TextHeader,
                family: FD_TEXT_SPEC.family,
                title: FD_TEXT_SPEC.title,
                header: Some(parts.header),
                wmo_header: None,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
            })
        },
    )
}

pub(super) fn classify_text_metar(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text_guarded(
        context,
        looks_like_metar_text_product,
        &METAR_TEXT_SPEC,
        |context| {
            let bulletin_text = if matches!(context.header.afos.as_str(), "METAR" | "SPECI")
                && !looks_like_metar_wmo_bulletin(context.body_text)
            {
                format!("{} {}", context.header.afos, context.body_text.trim_start())
            } else {
                context.body_text.to_string()
            };
            parsed_with_issues(
                parse_metar_bulletin(&bulletin_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, issues| {
            ClassificationCandidate::Metar(MetarCandidate {
                source: ProductEnrichmentSource::TextHeader,
                header: Some(parts.header),
                wmo_header: None,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: None,
                bulletin,
                issues,
            })
        },
    )
}

pub(super) fn classify_text_taf(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if context.header.afos.starts_with("TAF")
        && looks_like_multipart_taf_bulletin(context.body_text)
    {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "multipart_taf_bulletin",
            "Multipart terminal aerodrome forecast",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            None,
            "taf_parse",
            "unsupported_multipart_taf_bulletin",
            "recognized multipart TAF bulletin, but multipart TAF assembly is not implemented",
            first_nonempty_line(context.body_text),
        ));
    }
    classify_supported_text_guarded(
        context,
        looks_like_taf_text_product,
        &TAF_TEXT_SPEC,
        |context| {
            parsed(
                parse_taf_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Taf(TafCandidate {
                source: ProductEnrichmentSource::TextHeader,
                header: Some(parts.header),
                wmo_header: None,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: None,
                bulletin,
            })
        },
    )
}

pub(super) fn classify_text_pirep(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    if context.header.afos.starts_with("PIR")
        && looks_like_international_pirep_text_product(&context.header.afos, context.body_text)
    {
        return Some(malformed_supported_family(
            ProductEnrichmentSource::TextHeader,
            "international_pirep_bulletin",
            "International pilot report bulletin",
            Some(context.header.clone()),
            None,
            context.pil.clone(),
            context.bbb_kind,
            context.body_request(),
            "pirep_parse",
            "unsupported_international_pirep_bulletin",
            "recognized international PIREP bulletin, but this parser only supports domestic slash-tag PIREP structure",
            first_nonempty_line(context.body_text),
        ));
    }
    classify_supported_text(
        context,
        TextProductRouting::Pirep,
        looks_like_pirep_text_product,
        &PIREP_TEXT_SPEC,
        |context| {
            parsed(
                parse_pirep_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Pirep(PirepCandidate {
                source: ProductEnrichmentSource::TextHeader,
                header: Some(parts.header),
                wmo_header: None,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
            })
        },
    )
}

pub(super) fn classify_text_sigmet(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text(
        context,
        TextProductRouting::Sigmet,
        looks_like_sigmet_text_product,
        &SIGMET_TEXT_SPEC,
        |context| {
            parsed(
                parse_sigmet_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Sigmet(SigmetCandidate {
                source: ProductEnrichmentSource::TextHeader,
                header: Some(parts.header),
                wmo_header: None,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
}

pub(super) fn classify_text_lsr(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text(
        context,
        TextProductRouting::Lsr,
        looks_like_lsr_text_product,
        &LSR_TEXT_SPEC,
        |context| {
            let reference_time = require_reference_time(context.reference_time)?;
            parsed_with_issues(
                parse_lsr_bulletin(context.body_text, reference_time)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, issues| {
            ClassificationCandidate::Lsr(LsrCandidate {
                header: parts.header,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues,
            })
        },
    )
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
    classify_supported_text(
        context,
        TextProductRouting::Cwa,
        looks_like_cwa_text_product,
        &CWA_TEXT_SPEC,
        |context| {
            let reference_time = require_reference_time(context.reference_time)?;
            parsed(
                parse_cwa_bulletin(context.body_text, reference_time)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Cwa(CwaCandidate {
                header: Some(parts.header),
                wmo_header: None,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
}

pub(super) fn classify_text_wwp(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text(
        context,
        TextProductRouting::Wwp,
        looks_like_wwp_text_product,
        &WWP_TEXT_SPEC,
        |context| {
            parsed(
                parse_wwp_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Wwp(WwpCandidate {
                header: parts.header,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
}

pub(super) fn classify_text_cf6(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text(
        context,
        TextProductRouting::Cf6,
        looks_like_cf6_text_product,
        &CF6_TEXT_SPEC,
        |context| {
            parsed(
                parse_cf6_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Cf6(Cf6Candidate {
                header: parts.header,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
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
    classify_supported_text(
        context,
        TextProductRouting::Dsm,
        looks_like_dsm_text_product,
        &DSM_TEXT_SPEC,
        |context| {
            parsed_with_issues(
                parse_dsm_bulletin(
                    context.body_text,
                    context.reference_time.unwrap_or_else(Utc::now),
                )
                .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, issues| {
            ClassificationCandidate::Dsm(DsmCandidate {
                source: ProductEnrichmentSource::TextHeader,
                header: Some(parts.header),
                wmo_header: None,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues,
            })
        },
    )
}

pub(super) fn classify_text_hml(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text(
        context,
        TextProductRouting::Hml,
        looks_like_hml_text_product,
        &HML_TEXT_SPEC,
        |context| {
            parsed(
                parse_hml_bulletin(context.body_text)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Hml(HmlCandidate {
                header: parts.header,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
}

pub(super) fn classify_text_mos(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text(
        context,
        TextProductRouting::Mos,
        looks_like_mos_text_product,
        &MOS_TEXT_SPEC,
        |context| {
            let reference_time = require_reference_time(context.reference_time)?;
            parsed(
                parse_mos_bulletin(context.body_text, reference_time)
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Mos(MosCandidate {
                header: parts.header,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
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
    classify_supported_text(
        context,
        TextProductRouting::Ero,
        looks_like_ero_text_product,
        &ERO_TEXT_SPEC,
        |context| {
            parsed(
                parse_ero_bulletin(context.body_text, Some(context.header.afos.as_str()))
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::Ero(EroCandidate {
                header: parts.header,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
}

pub(super) fn classify_text_spc_outlook(
    context: &TextClassificationContext<'_>,
) -> Option<ClassificationCandidate> {
    classify_supported_text(
        context,
        TextProductRouting::SpcOutlook,
        looks_like_spc_outlook_text_product,
        &SPC_OUTLOOK_TEXT_SPEC,
        |context| {
            parsed(
                parse_spc_outlook_bulletin(context.body_text, Some(context.header.afos.as_str()))
                    .ok_or(super::common::SupportedFamilyFailure::ParseFailure)?,
            )
        },
        |parts, bulletin, _issues| {
            ClassificationCandidate::SpcOutlook(SpcOutlookCandidate {
                header: parts.header,
                pil: parts.pil,
                bbb_kind: parts.bbb_kind,
                body_request: parts.body_request,
                bulletin,
                issues: Vec::new(),
            })
        },
    )
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
            || body_text.contains("WATCH PROBABILITIES FOR WT")
            || body_text.contains("WATCH PROBABILITIES FOR WS"))
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

fn looks_like_international_pirep_text_product(afos: &str, body_text: &str) -> bool {
    afos == "PIREP"
        && body_text.to_ascii_uppercase().contains(" OBSD AT ")
        && !body_text.contains('/')
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
        && (looks_like_taf_wmo_bulletin(body_text)
            || looks_like_station_led_taf_body(body_text)
            || looks_like_multipart_taf_bulletin(body_text))
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
