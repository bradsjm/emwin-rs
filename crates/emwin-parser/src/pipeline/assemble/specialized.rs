use super::super::candidate::{
    Cf6Candidate, CliCandidate, CwaCandidate, DcpCandidate, DsmCandidate, EroCandidate,
    FdCandidate, HmlCandidate, LsrCandidate, McdCandidate, MetarCandidate, MosCandidate,
    PirepCandidate, SawCandidate, SelCandidate, SigmetCandidate, SpcOutlookCandidate, TafCandidate,
    TextGenericCandidate, WwpCandidate,
};
use super::{
    EnrichmentBase, SpecializedAssemblyInput, assemble_optional_body,
    assemble_specialized_enrichment, build_enrichment, container_from_filename, office_for_headers,
    wmo_office_entry,
};
use crate::{ProductArtifact, ProductEnrichment, ProductEnrichmentSource};

/// Assembles a generic AFOS text product and runs body enrichment.
pub(super) fn assemble_from_text_generic(
    candidate: TextGenericCandidate,
    filename: &str,
) -> ProductEnrichment {
    let TextGenericCandidate {
        header,
        pil,
        title,
        body_request,
        bbb_kind,
        reference_time: _reference_time,
    } = candidate;
    let (body, issues) = assemble_optional_body(body_request);

    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::TextHeader,
        family: Some("nws_text_product"),
        title,
        container: container_from_filename(filename),
        pil,
        wmo_prefix: None,
        office: wmo_office_entry(&header.cccc).copied(),
        header: Some(header),
        wmo_header: None,
        bbb_kind,
        body,
        parsed: None,
        issues,
    })
}

/// Assembles an FD bulletin candidate without reparsing it.
pub(super) fn assemble_from_fd(candidate: FdCandidate, filename: &str) -> ProductEnrichment {
    let FdCandidate {
        source,
        family,
        title,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request,
        bulletin,
    } = candidate;
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source,
        family,
        title,
        filename: filename.to_string(),
        pil,
        header,
        wmo_header,
        bbb_kind,
        body_request,
        issues: Vec::new(),
        parsed: ProductArtifact::Fd(bulletin),
    })
}

/// Assembles a PIREP bulletin candidate without reparsing it.
pub(super) fn assemble_from_pirep(candidate: PirepCandidate, filename: &str) -> ProductEnrichment {
    let PirepCandidate {
        source,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request,
        bulletin,
    } = candidate;
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source,
        family: "pirep_bulletin",
        title: "Pilot report bulletin",
        filename: filename.to_string(),
        pil,
        header,
        wmo_header,
        bbb_kind,
        body_request,
        issues: Vec::new(),
        parsed: ProductArtifact::Pirep(bulletin),
    })
}

/// Assembles a SIGMET candidate without reparsing it.
pub(super) fn assemble_from_sigmet(
    candidate: SigmetCandidate,
    filename: &str,
) -> ProductEnrichment {
    let SigmetCandidate {
        source,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request,
        bulletin,
        issues,
    } = candidate;
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source,
        family: "sigmet_bulletin",
        title: "SIGMET bulletin",
        filename: filename.to_string(),
        pil,
        header,
        wmo_header,
        bbb_kind,
        body_request,
        issues,
        parsed: ProductArtifact::Sigmet(bulletin),
    })
}

pub(super) fn assemble_from_lsr(candidate: LsrCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "lsr_bulletin",
        title: "Local Storm Report",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Lsr(candidate.bulletin),
    })
}

pub(super) fn assemble_from_cli(candidate: CliCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "cli_bulletin",
        title: "Daily climate report",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Cli(candidate.bulletin),
    })
}

pub(super) fn assemble_from_cwa(candidate: CwaCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: if candidate.header.is_some() {
            ProductEnrichmentSource::TextHeader
        } else {
            ProductEnrichmentSource::WmoBulletin
        },
        family: "cwa_bulletin",
        title: "Center Weather Advisory",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: candidate.header,
        wmo_header: candidate.wmo_header,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Cwa(candidate.bulletin),
    })
}

pub(super) fn assemble_from_wwp(candidate: WwpCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "wwp_bulletin",
        title: "Severe Thunderstorm Watch Probabilities",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Wwp(candidate.bulletin),
    })
}

pub(super) fn assemble_from_saw(candidate: SawCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "saw_bulletin",
        title: "SPC preliminary notice of watch",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Saw(candidate.bulletin),
    })
}

pub(super) fn assemble_from_sel(candidate: SelCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "sel_bulletin",
        title: "SPC watch bulletin",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Sel(candidate.bulletin),
    })
}

pub(super) fn assemble_from_cf6(candidate: Cf6Candidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "cf6_bulletin",
        title: "Climate F-6 products",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Cf6(candidate.bulletin),
    })
}

pub(super) fn assemble_from_dsm(candidate: DsmCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: candidate.source,
        family: "dsm_bulletin",
        title: "Asos Daily Summary",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: candidate.header,
        wmo_header: candidate.wmo_header,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Dsm(candidate.bulletin),
    })
}

pub(super) fn assemble_from_hml(candidate: HmlCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "hml_bulletin",
        title: "Hyrdo Obs/Forecasts XML",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Hml(candidate.bulletin),
    })
}

pub(super) fn assemble_from_mos(candidate: MosCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "mos_bulletin",
        title: "MOS guidance bulletin",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Mos(candidate.bulletin),
    })
}

pub(super) fn assemble_from_mcd(candidate: McdCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "mcd_bulletin",
        title: "Mesoscale discussion bulletin",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Mcd(candidate.bulletin),
    })
}

pub(super) fn assemble_from_ero(candidate: EroCandidate, filename: &str) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "ero_bulletin",
        title: "Excessive rainfall outlook",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::Ero(candidate.bulletin),
    })
}

pub(super) fn assemble_from_spc_outlook(
    candidate: SpcOutlookCandidate,
    filename: &str,
) -> ProductEnrichment {
    assemble_specialized_enrichment(SpecializedAssemblyInput {
        source: ProductEnrichmentSource::TextHeader,
        family: "spc_outlook_bulletin",
        title: "SPC outlook bulletin",
        filename: filename.to_string(),
        pil: candidate.pil,
        header: Some(candidate.header),
        wmo_header: None,
        bbb_kind: candidate.bbb_kind,
        body_request: candidate.body_request,
        issues: candidate.issues,
        parsed: ProductArtifact::SpcOutlook(candidate.bulletin),
    })
}

/// Assembles a parsed METAR candidate.
pub(super) fn assemble_from_metar(candidate: MetarCandidate, filename: &str) -> ProductEnrichment {
    let MetarCandidate {
        source,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request: _body_request,
        bulletin,
        issues,
    } = candidate;

    build_enrichment(EnrichmentBase {
        source,
        family: Some("metar_collective"),
        title: Some("METAR bulletin"),
        container: container_from_filename(filename),
        pil,
        wmo_prefix: None,
        office: office_for_headers(header.as_ref(), wmo_header.as_ref()),
        header,
        wmo_header,
        bbb_kind,
        body: None,
        parsed: Some(ProductArtifact::Metar(bulletin)),
        issues,
    })
}

/// Assembles a parsed TAF candidate.
pub(super) fn assemble_from_taf(candidate: TafCandidate, filename: &str) -> ProductEnrichment {
    let TafCandidate {
        source,
        header,
        wmo_header,
        pil,
        bbb_kind,
        body_request: _body_request,
        bulletin,
    } = candidate;

    build_enrichment(EnrichmentBase {
        source,
        family: Some("taf_bulletin"),
        title: Some("Terminal Aerodrome Forecast"),
        container: container_from_filename(filename),
        pil,
        wmo_prefix: None,
        office: office_for_headers(header.as_ref(), wmo_header.as_ref()),
        header,
        wmo_header,
        bbb_kind,
        body: None,
        parsed: Some(ProductArtifact::Taf(bulletin)),
        issues: Vec::new(),
    })
}

/// Assembles a parsed DCP candidate.
pub(super) fn assemble_from_dcp(candidate: DcpCandidate, filename: &str) -> ProductEnrichment {
    let DcpCandidate { header, bulletin } = candidate;

    build_enrichment(EnrichmentBase {
        source: ProductEnrichmentSource::WmoBulletin,
        family: Some("dcp_telemetry_bulletin"),
        title: Some("GOES DCP telemetry bulletin"),
        container: container_from_filename(filename),
        pil: None,
        wmo_prefix: None,
        office: wmo_office_entry(&header.cccc).copied(),
        header: None,
        wmo_header: Some(header),
        bbb_kind: None,
        body: None,
        parsed: Some(ProductArtifact::Dcp(bulletin)),
        issues: Vec::new(),
    })
}
