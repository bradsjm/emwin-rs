//! Shared borrowed contexts for AFOS and WMO strategy evaluation.

use chrono::{DateTime, Utc};

use crate::body::BodyExtractionPlan;
use crate::data::{
    ResolvedTextProductPolicy, TextProductBodyBehavior, TextProductRouting,
    resolved_text_product_policy,
};
use crate::pipeline::ParsedEnvelope;
use crate::pipeline::candidate::BodyContributionRequest;
use crate::{BbbKind, TextProductHeader, WmoHeader, enrich_header};

use super::common::build_body_request;

/// Borrowed context shared across AFOS text-product strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TextClassificationContext<'a> {
    /// Original filename used for filename-sensitive parsing rules.
    pub(super) filename: &'a str,
    /// Parsed AFOS text product header.
    pub(super) header: &'a TextProductHeader,
    /// Conditioned body text after header removal.
    pub(super) body_text: &'a str,
    /// Resolved text-product policy after applying exact-AFOS overrides.
    pub(super) policy: Option<ResolvedTextProductPolicy>,
    /// Three-character PIL prefix when present.
    pub(super) pil: Option<String>,
    /// Human-readable title from the PIL catalog.
    pub(super) title: Option<&'static str>,
    /// Generic body extraction plan derived from the PIL catalog.
    pub(super) body_plan: Option<BodyExtractionPlan>,
    /// BBB meaning for amendment/correction markers.
    pub(super) bbb_kind: Option<BbbKind>,
    /// Timestamp resolved from the WMO header.
    pub(super) reference_time: Option<DateTime<Utc>>,
}

impl<'a> TextClassificationContext<'a> {
    pub(super) fn from_envelope(envelope: &'a ParsedEnvelope) -> Option<Self> {
        envelope.text_bytes()?;
        let header = envelope.header.as_ref()?;
        let body_text = envelope.body_text()?;

        let header_enrichment = enrich_header(header);
        let policy = resolved_text_product_policy(&header.afos);

        Some(Self {
            filename: envelope.filename(),
            pil: header_enrichment.pil_nnn.map(str::to_string),
            title: policy.map(|policy| policy.title),
            body_plan: body_extraction_plan_for_policy(policy),
            policy,
            bbb_kind: header_enrichment.bbb_kind,
            reference_time: header.timestamp(Utc::now()),
            header,
            body_text,
        })
    }

    pub(super) fn body_request(&self) -> Option<BodyContributionRequest> {
        build_body_request(self.body_plan, self.body_text, self.reference_time)
    }

    pub(super) fn has_routing(&self, routing: TextProductRouting) -> bool {
        self.policy.map(|policy| policy.routing) == Some(routing)
    }
}

/// Borrowed context shared across WMO-only fallback strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WmoClassificationContext<'a> {
    /// Original filename used by routing-sensitive parsers.
    pub(super) filename: &'a str,
    /// Parsed WMO header without AFOS state.
    pub(super) header: &'a WmoHeader,
    /// Conditioned body text after header removal.
    pub(super) body_text: &'a str,
}

impl<'a> WmoClassificationContext<'a> {
    pub(super) fn from_envelope(envelope: &'a ParsedEnvelope) -> Option<Self> {
        envelope.text_bytes()?;
        let header = envelope.wmo_header.as_ref()?;
        let body_text = envelope.body_text()?;

        Some(Self {
            filename: envelope.filename(),
            header,
            body_text,
        })
    }
}

fn body_extraction_plan_for_policy(
    policy: Option<ResolvedTextProductPolicy>,
) -> Option<BodyExtractionPlan> {
    let policy = policy?;
    match policy.body_behavior {
        TextProductBodyBehavior::Never => None,
        TextProductBodyBehavior::Catalog => {
            Some(crate::body::body_extraction_plan(policy.extractors))
        }
    }
}
