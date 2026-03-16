//! Parsed candidate types produced by pipeline classification.
//!
//! Each candidate owns the parsed artifacts needed to build the public
//! `ProductEnrichment` result. Owning the artifacts avoids the old
//! probe-then-reparse flow, keeps classification and assembly in sync, and
//! removes `expect(...)`-based invariants from the dispatch path.

use chrono::{DateTime, Utc};

use crate::body::{BodyExtractionPlan, BodyInputFormat};
use crate::data::NonTextProductMeta;
use crate::{
    BbbKind, Cf6Bulletin, CliBulletin, CwaBulletin, DcpBulletin, DsmBulletin, EroBulletin,
    FdBulletin, HmlBulletin, LsrBulletin, McdBulletin, MetarBulletin, MosBulletin, ParserError,
    PirepBulletin, ProductEnrichmentSource, ProductParseIssue, SawBulletin, SelBulletin,
    SigmetBulletin, SpcOutlookBulletin, TafBulletin, TextProductHeader, WmoHeader, WwpBulletin,
};

/// Internal classification result passed from strategy dispatch into assembly.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ClassificationCandidate {
    /// Generic AFOS text product that should continue into body enrichment.
    TextGeneric(TextGenericCandidate),
    /// Parsed FD bulletin candidate.
    Fd(FdCandidate),
    /// Parsed PIREP bulletin candidate.
    Pirep(PirepCandidate),
    /// Parsed SIGMET bulletin candidate.
    Sigmet(SigmetCandidate),
    /// Parsed LSR bulletin candidate.
    Lsr(LsrCandidate),
    /// Parsed CLI bulletin candidate.
    Cli(CliCandidate),
    /// Parsed CWA bulletin candidate.
    Cwa(CwaCandidate),
    /// Parsed WWP bulletin candidate.
    Wwp(WwpCandidate),
    /// Parsed SAW bulletin candidate.
    Saw(SawCandidate),
    /// Parsed SEL bulletin candidate.
    Sel(SelCandidate),
    /// Parsed CF6 bulletin candidate.
    Cf6(Cf6Candidate),
    /// Parsed DSM bulletin candidate.
    Dsm(DsmCandidate),
    /// Parsed HML bulletin candidate.
    Hml(HmlCandidate),
    /// Parsed MOS bulletin candidate.
    Mos(MosCandidate),
    /// Parsed MCD/MPD bulletin candidate.
    Mcd(McdCandidate),
    /// Parsed ERO bulletin candidate.
    Ero(EroCandidate),
    /// Parsed SPC outlook bulletin candidate.
    SpcOutlook(SpcOutlookCandidate),
    /// Parsed METAR bulletin candidate.
    Metar(MetarCandidate),
    /// Parsed TAF bulletin candidate.
    Taf(TafCandidate),
    /// Parsed DCP bulletin candidate.
    Dcp(DcpCandidate),
    /// Recognized supported family whose structured parser failed without losing family identity.
    MalformedFamily(MalformedFamilyCandidate),
    /// Filename-classified non-text product candidate.
    NonText(NonTextProductMeta),
    /// Recognized WMO bulletin that is intentionally unsupported.
    UnsupportedWmo(UnsupportedWmoCandidate),
    /// Text parse failure that preserves the legacy issue shape.
    TextParseFailure(ParserError),
    /// Payload with no richer classification available.
    Unknown,
}

/// Owned request to run generic body extraction for a candidate.
///
/// Candidates own the request instead of borrowing the original envelope so
/// assembly can remain a simple conversion step without lifetime plumbing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BodyContributionRequest {
    /// Conditioned body text to run through the generic extractor registry.
    pub(crate) text: String,
    /// Ordered extractor and QC configuration derived from header metadata.
    pub(crate) plan: BodyExtractionPlan,
    /// Reference time used by time-aware body parsers.
    pub(crate) reference_time: Option<DateTime<Utc>>,
    /// Input format hint for plan-driven body parsing.
    pub(crate) input_format: BodyInputFormat,
}

/// Generic AFOS text product candidate used for body enrichment.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextGenericCandidate {
    /// Parsed text product header.
    pub(crate) header: TextProductHeader,
    /// Three-character PIL prefix when present.
    pub(crate) pil: Option<String>,
    /// Human-readable catalog title.
    pub(crate) title: Option<&'static str>,
    /// Optional generic body extraction request for this candidate.
    pub(crate) body_request: Option<BodyContributionRequest>,
    /// Classified BBB meaning when present.
    pub(crate) bbb_kind: Option<BbbKind>,
    /// Timestamp resolved from the WMO header for time-aware body parsing.
    pub(crate) reference_time: Option<DateTime<Utc>>,
}

/// Parsed FD bulletin candidate from either AFOS or WMO-only flows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FdCandidate {
    /// Output source associated with this candidate.
    pub(crate) source: ProductEnrichmentSource,
    /// Product family emitted for the candidate.
    pub(crate) family: &'static str,
    /// Human-readable title emitted for the candidate.
    pub(crate) title: &'static str,
    /// Text header when the bulletin came from AFOS parsing.
    pub(crate) header: Option<TextProductHeader>,
    /// WMO-only header when the bulletin came from fallback parsing.
    pub(crate) wmo_header: Option<WmoHeader>,
    /// Three-character PIL prefix when the AFOS path provided one.
    pub(crate) pil: Option<String>,
    /// BBB meaning for AFOS-derived candidates.
    pub(crate) bbb_kind: Option<BbbKind>,
    /// Optional generic body extraction request for future coexistence.
    pub(crate) body_request: Option<BodyContributionRequest>,
    /// Parsed FD bulletin payload.
    pub(crate) bulletin: FdBulletin,
}

/// Parsed PIREP bulletin candidate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PirepCandidate {
    /// Output source associated with the candidate.
    pub(crate) source: ProductEnrichmentSource,
    /// AFOS text header for the bulletin when present.
    pub(crate) header: Option<TextProductHeader>,
    /// WMO-only header for the bulletin when present.
    pub(crate) wmo_header: Option<WmoHeader>,
    /// Three-character PIL prefix when present.
    pub(crate) pil: Option<String>,
    /// BBB meaning for the text header.
    pub(crate) bbb_kind: Option<BbbKind>,
    /// Optional generic body extraction request for future coexistence.
    pub(crate) body_request: Option<BodyContributionRequest>,
    /// Parsed PIREP bulletin payload.
    pub(crate) bulletin: PirepBulletin,
}

/// Parsed SIGMET bulletin candidate from text or WMO-only paths.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SigmetCandidate {
    /// Output source associated with the candidate.
    pub(crate) source: ProductEnrichmentSource,
    /// AFOS text header when present.
    pub(crate) header: Option<TextProductHeader>,
    /// WMO-only header when present.
    pub(crate) wmo_header: Option<WmoHeader>,
    /// Three-character PIL prefix when present.
    pub(crate) pil: Option<String>,
    /// BBB meaning for AFOS-derived candidates.
    pub(crate) bbb_kind: Option<BbbKind>,
    /// Optional generic body extraction request for future coexistence.
    pub(crate) body_request: Option<BodyContributionRequest>,
    /// Parsed SIGMET bulletin payload.
    pub(crate) bulletin: SigmetBulletin,
    /// Non-fatal parse issues.
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LsrCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: LsrBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CliCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: CliBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CwaCandidate {
    pub(crate) header: Option<TextProductHeader>,
    pub(crate) wmo_header: Option<WmoHeader>,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: CwaBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WwpCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: WwpBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SawCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: SawBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SelCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: SelBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cf6Candidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: Cf6Bulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DsmCandidate {
    pub(crate) source: ProductEnrichmentSource,
    pub(crate) header: Option<TextProductHeader>,
    pub(crate) wmo_header: Option<WmoHeader>,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: DsmBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HmlCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: HmlBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MosCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: MosBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McdCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: McdBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EroCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: EroBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpcOutlookCandidate {
    pub(crate) header: TextProductHeader,
    pub(crate) pil: Option<String>,
    pub(crate) bbb_kind: Option<BbbKind>,
    pub(crate) body_request: Option<BodyContributionRequest>,
    pub(crate) bulletin: SpcOutlookBulletin,
    pub(crate) issues: Vec<ProductParseIssue>,
}

/// Parsed METAR bulletin candidate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetarCandidate {
    /// Output source associated with the candidate.
    pub(crate) source: ProductEnrichmentSource,
    /// Text header when the bulletin came through AFOS parsing.
    pub(crate) header: Option<TextProductHeader>,
    /// WMO-only header that identified the bulletin.
    pub(crate) wmo_header: Option<WmoHeader>,
    /// Three-character PIL prefix when present.
    pub(crate) pil: Option<String>,
    /// BBB meaning for AFOS-derived candidates.
    pub(crate) bbb_kind: Option<BbbKind>,
    /// Optional generic body extraction request for future coexistence.
    pub(crate) body_request: Option<BodyContributionRequest>,
    /// Parsed METAR bulletin payload.
    pub(crate) bulletin: MetarBulletin,
    /// Non-fatal parse issues emitted during METAR parsing.
    pub(crate) issues: Vec<ProductParseIssue>,
}

/// Parsed TAF bulletin candidate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TafCandidate {
    /// Output source associated with the candidate.
    pub(crate) source: ProductEnrichmentSource,
    /// Text header when the bulletin came through AFOS parsing.
    pub(crate) header: Option<TextProductHeader>,
    /// WMO-only header that identified the bulletin.
    pub(crate) wmo_header: Option<WmoHeader>,
    /// Three-character PIL prefix when present.
    pub(crate) pil: Option<String>,
    /// BBB meaning for AFOS-derived candidates.
    pub(crate) bbb_kind: Option<BbbKind>,
    /// Optional generic body extraction request for future coexistence.
    pub(crate) body_request: Option<BodyContributionRequest>,
    /// Parsed TAF bulletin payload.
    pub(crate) bulletin: TafBulletin,
}

/// Parsed DCP bulletin candidate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DcpCandidate {
    /// WMO-only header that identified the bulletin.
    pub(crate) header: WmoHeader,
    /// Parsed DCP bulletin payload.
    pub(crate) bulletin: DcpBulletin,
}

/// Recognized supported family that could not produce a structured artifact.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MalformedFamilyCandidate {
    /// Output source associated with this candidate.
    pub(crate) source: ProductEnrichmentSource,
    /// Product family emitted for the candidate.
    pub(crate) family: &'static str,
    /// Human-readable title emitted for the candidate.
    pub(crate) title: &'static str,
    /// Text header when the bulletin came from AFOS parsing.
    pub(crate) header: Option<TextProductHeader>,
    /// WMO-only header when the bulletin came from fallback parsing.
    pub(crate) wmo_header: Option<WmoHeader>,
    /// Three-character PIL prefix when present.
    pub(crate) pil: Option<String>,
    /// BBB meaning for AFOS-derived candidates.
    pub(crate) bbb_kind: Option<BbbKind>,
    /// Optional generic body extraction request for future coexistence.
    pub(crate) body_request: Option<BodyContributionRequest>,
    /// Non-fatal parse issues explaining the degraded result.
    pub(crate) issues: Vec<ProductParseIssue>,
}

/// Unsupported-but-recognized WMO bulletin candidate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnsupportedWmoCandidate {
    /// WMO header preserved for output.
    pub(crate) header: WmoHeader,
    /// Stable machine-readable issue code.
    pub(crate) code: &'static str,
    /// Stable human-readable issue message.
    pub(crate) message: &'static str,
    /// Optional representative line from the source text.
    pub(crate) line: Option<String>,
}
