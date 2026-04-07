pub(super) use chrono::{TimeZone, Utc};

pub(super) use super::classify;
pub(super) use super::common::build_body_request;
pub(super) use super::context::TextClassificationContext;
pub(super) use super::text::{
    classify_text_cf6, classify_text_cwa, classify_text_dsm, classify_text_fd, classify_text_hml,
    classify_text_lsr, classify_text_mos, classify_text_pirep, classify_text_saw,
    classify_text_sel, classify_text_wwp,
};
pub(super) use crate::body::{BodyExtractorId, body_extraction_plan};
pub(super) use crate::data::resolved_text_product_policy;
pub(super) use crate::header::BbbKind;
pub(super) use crate::pipeline::candidate::{ClassificationCandidate, FdCandidate};
pub(super) use crate::pipeline::{NormalizedInput, ParsedEnvelope};
pub(super) use crate::{ProductEnrichmentSource, TextProductHeader};

fn with_specialized_context<T>(
    pil: &'static str,
    afos: &'static str,
    body_text: &'static str,
    body_plan: Option<crate::body::BodyExtractionPlan>,
    f: impl FnOnce(&TextClassificationContext<'_>) -> T,
) -> T {
    let header = TextProductHeader {
        ttaaii: "FTUS80".to_string(),
        cccc: "KWBC".to_string(),
        ddhhmm: "100000".to_string(),
        bbb: None,
        afos: afos.to_string(),
    };
    let policy = resolved_text_product_policy(afos).expect("expected catalog policy");
    let context = TextClassificationContext {
        filename: "sample.TXT",
        header: &header,
        body_text,
        policy: Some(policy),
        pil: Some(pil.to_string()),
        title: Some(policy.title),
        body_plan,
        bbb_kind: None,
        reference_time: Some(Utc::now()),
    };

    f(&context)
}

#[cfg(test)]
mod afos_strategies;
#[cfg(test)]
mod body_request;
#[cfg(test)]
mod local_samples;
#[cfg(test)]
mod malformed;
#[cfg(test)]
mod routing_guards;
#[cfg(test)]
mod wmo;
