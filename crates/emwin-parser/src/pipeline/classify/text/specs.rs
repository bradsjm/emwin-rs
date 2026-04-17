use super::{ProductEnrichmentSource, SupportedFamilySpec};

pub(crate) const FD_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const METAR_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const TAF_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const PIREP_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const SIGMET_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const LSR_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const CWA_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const WWP_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const CF6_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const DSM_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const HML_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const MOS_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const ERO_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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

pub(crate) const SPC_OUTLOOK_TEXT_SPEC: SupportedFamilySpec = SupportedFamilySpec {
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
