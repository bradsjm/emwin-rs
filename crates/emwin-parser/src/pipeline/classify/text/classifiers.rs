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

use super::super::common::{
    SupportedFamilyFailure, classify_supported_text, classify_supported_text_guarded,
    first_nonempty_line, looks_like_multipart_taf_bulletin, malformed_supported_family, parsed,
    parsed_with_issues, require_reference_time,
};
use super::super::context::TextClassificationContext;
use super::guards::*;
use super::specs::{
    CF6_TEXT_SPEC, CWA_TEXT_SPEC, DSM_TEXT_SPEC, ERO_TEXT_SPEC, FD_TEXT_SPEC, HML_TEXT_SPEC,
    LSR_TEXT_SPEC, METAR_TEXT_SPEC, MOS_TEXT_SPEC, PIREP_TEXT_SPEC, SIGMET_TEXT_SPEC,
    SPC_OUTLOOK_TEXT_SPEC, TAF_TEXT_SPEC, WWP_TEXT_SPEC,
};

pub(crate) fn classify_text_fd(
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
                .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_metar(
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
                parse_metar_bulletin(&bulletin_text).ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_taf(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_pirep(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_sigmet(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_lsr(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_cli(
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

pub(crate) fn classify_text_cwa(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_wwp(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_cf6(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_saw(
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

pub(crate) fn classify_text_sel(
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

pub(crate) fn classify_text_dsm(
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
                .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_hml(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_mos(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_mcd(
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

pub(crate) fn classify_text_ero(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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

pub(crate) fn classify_text_spc_outlook(
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
                    .ok_or(SupportedFamilyFailure::ParseFailure)?,
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
