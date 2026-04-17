use crate::ProductEnrichment;

use super::super::ClassificationCandidate;
use super::{fallback, specialized};

/// Assembles the public enrichment result from a parsed classification candidate.
///
/// The filename and raw bytes remain inputs so the unknown-product path can
/// preserve the existing container detection semantics.
pub(crate) fn assemble_product_enrichment(
    candidate: ClassificationCandidate,
    filename: &str,
    raw_bytes: &[u8],
) -> ProductEnrichment {
    match candidate {
        ClassificationCandidate::TextGeneric(candidate) => {
            specialized::assemble_from_text_generic(candidate, filename)
        }
        ClassificationCandidate::Fd(candidate) => {
            specialized::assemble_from_fd(candidate, filename)
        }
        ClassificationCandidate::Pirep(candidate) => {
            specialized::assemble_from_pirep(candidate, filename)
        }
        ClassificationCandidate::Sigmet(candidate) => {
            specialized::assemble_from_sigmet(candidate, filename)
        }
        ClassificationCandidate::Lsr(candidate) => {
            specialized::assemble_from_lsr(candidate, filename)
        }
        ClassificationCandidate::Cli(candidate) => {
            specialized::assemble_from_cli(candidate, filename)
        }
        ClassificationCandidate::Cwa(candidate) => {
            specialized::assemble_from_cwa(candidate, filename)
        }
        ClassificationCandidate::Wwp(candidate) => {
            specialized::assemble_from_wwp(candidate, filename)
        }
        ClassificationCandidate::Saw(candidate) => {
            specialized::assemble_from_saw(candidate, filename)
        }
        ClassificationCandidate::Sel(candidate) => {
            specialized::assemble_from_sel(candidate, filename)
        }
        ClassificationCandidate::Cf6(candidate) => {
            specialized::assemble_from_cf6(candidate, filename)
        }
        ClassificationCandidate::Dsm(candidate) => {
            specialized::assemble_from_dsm(candidate, filename)
        }
        ClassificationCandidate::Hml(candidate) => {
            specialized::assemble_from_hml(candidate, filename)
        }
        ClassificationCandidate::Mos(candidate) => {
            specialized::assemble_from_mos(candidate, filename)
        }
        ClassificationCandidate::Mcd(candidate) => {
            specialized::assemble_from_mcd(candidate, filename)
        }
        ClassificationCandidate::Ero(candidate) => {
            specialized::assemble_from_ero(candidate, filename)
        }
        ClassificationCandidate::SpcOutlook(candidate) => {
            specialized::assemble_from_spc_outlook(candidate, filename)
        }
        ClassificationCandidate::Metar(candidate) => {
            specialized::assemble_from_metar(candidate, filename)
        }
        ClassificationCandidate::Taf(candidate) => {
            specialized::assemble_from_taf(candidate, filename)
        }
        ClassificationCandidate::Dcp(candidate) => {
            specialized::assemble_from_dcp(candidate, filename)
        }
        ClassificationCandidate::MalformedFamily(candidate) => {
            fallback::assemble_from_malformed_family(candidate, filename)
        }
        ClassificationCandidate::NonText(candidate) => fallback::assemble_from_non_text(candidate),
        ClassificationCandidate::UnsupportedWmo(candidate) => {
            fallback::assemble_from_unsupported_wmo(candidate, filename)
        }
        ClassificationCandidate::TextParseFailure(error) => {
            fallback::assemble_from_text_parse_failure(filename, error)
        }
        ClassificationCandidate::Unknown => fallback::assemble_unknown(filename, raw_bytes),
    }
}
