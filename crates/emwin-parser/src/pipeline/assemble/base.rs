use crate::data::{container_from_filename, wmo_office_entry};
use crate::{
    ProductArtifact, ProductBody, ProductEnrichment, ProductEnrichmentSource, ProductParseIssue,
    TextProductHeader, WmoHeader, body::BodyInputFormat, body::enrich_body_from_plan,
    body::enrich_body_from_plan_with_format, wmo_prefix_for_pil,
};

use super::super::candidate::BodyContributionRequest;

pub(super) struct EnrichmentBase {
    pub(super) source: ProductEnrichmentSource,
    pub(super) family: Option<&'static str>,
    pub(super) title: Option<&'static str>,
    pub(super) container: &'static str,
    pub(super) pil: Option<String>,
    pub(super) wmo_prefix: Option<&'static str>,
    pub(super) office: Option<crate::WmoOfficeEntry>,
    pub(super) header: Option<TextProductHeader>,
    pub(super) wmo_header: Option<WmoHeader>,
    pub(super) bbb_kind: Option<crate::BbbKind>,
    pub(super) body: Option<ProductBody>,
    pub(super) parsed: Option<ProductArtifact>,
    pub(super) issues: Vec<ProductParseIssue>,
}

pub(super) fn build_enrichment(base: EnrichmentBase) -> ProductEnrichment {
    ProductEnrichment {
        source: base.source,
        family: base.family,
        title: base.title,
        container: base.container,
        wmo_prefix: base
            .wmo_prefix
            .or_else(|| base.pil.as_deref().and_then(wmo_prefix_for_pil)),
        pil: base.pil,
        office: base.office,
        header: base.header,
        wmo_header: base.wmo_header,
        bbb_kind: base.bbb_kind,
        body: base.body,
        parsed: base.parsed,
        issues: base.issues,
    }
}

pub(super) fn office_for_headers(
    header: Option<&TextProductHeader>,
    wmo_header: Option<&WmoHeader>,
) -> Option<crate::WmoOfficeEntry> {
    header
        .and_then(|header| wmo_office_entry(&header.cccc).copied())
        .or_else(|| wmo_header.and_then(|header| wmo_office_entry(&header.cccc).copied()))
}

pub(super) struct SpecializedAssemblyInput {
    pub(super) source: ProductEnrichmentSource,
    pub(super) family: &'static str,
    pub(super) title: &'static str,
    pub(super) filename: String,
    pub(super) pil: Option<String>,
    pub(super) header: Option<TextProductHeader>,
    pub(super) wmo_header: Option<WmoHeader>,
    pub(super) bbb_kind: Option<crate::BbbKind>,
    pub(super) body_request: Option<BodyContributionRequest>,
    pub(super) issues: Vec<ProductParseIssue>,
    pub(super) parsed: ProductArtifact,
}

pub(super) fn assemble_specialized_enrichment(
    input: SpecializedAssemblyInput,
) -> ProductEnrichment {
    let (body, mut body_issues) = assemble_optional_body(input.body_request);
    body_issues.extend(input.issues);

    build_enrichment(EnrichmentBase {
        source: input.source,
        family: Some(input.family),
        title: Some(input.title),
        container: container_from_filename(&input.filename),
        pil: input.pil,
        wmo_prefix: None,
        office: office_for_headers(input.header.as_ref(), input.wmo_header.as_ref()),
        header: input.header,
        wmo_header: input.wmo_header,
        bbb_kind: input.bbb_kind,
        body,
        parsed: Some(input.parsed),
        issues: body_issues,
    })
}

pub(super) fn assemble_optional_body(
    request: Option<BodyContributionRequest>,
) -> (Option<ProductBody>, Vec<ProductParseIssue>) {
    match request {
        Some(request) => {
            let outcome = match request.input_format {
                BodyInputFormat::PlainText => {
                    enrich_body_from_plan(&request.text, &request.plan, request.reference_time)
                }
                BodyInputFormat::CapXml => enrich_body_from_plan_with_format(
                    &request.text,
                    &request.plan,
                    request.reference_time,
                    request.input_format,
                ),
            };
            (outcome.body, outcome.issues)
        }
        None => (None, Vec::new()),
    }
}
