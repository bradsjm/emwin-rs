use super::shared::{
    bbb_kind_name, matches_option_set, matches_serialized_option, normalize_lower, normalize_upper,
    product_source_name,
};
use emwin_parser::ProductEnrichment;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProductFilter {
    pub(crate) source: Option<BTreeSet<String>>,
    pub(crate) pil: Option<BTreeSet<String>>,
    pub(crate) family: Option<BTreeSet<String>>,
    pub(crate) container: Option<BTreeSet<String>>,
    pub(crate) wmo_prefix: Option<BTreeSet<String>>,
    pub(crate) office: Option<BTreeSet<String>>,
    pub(crate) office_city: Option<BTreeSet<String>>,
    pub(crate) office_state: Option<BTreeSet<String>>,
    pub(crate) bbb_kind: Option<BTreeSet<String>>,
}

impl ProductFilter {
    pub(crate) fn has_constraints(&self) -> bool {
        self.source.is_some()
            || self.pil.is_some()
            || self.family.is_some()
            || self.container.is_some()
            || self.wmo_prefix.is_some()
            || self.office.is_some()
            || self.office_city.is_some()
            || self.office_state.is_some()
            || self.bbb_kind.is_some()
    }

    pub(crate) fn matches(&self, product: &ProductEnrichment) -> bool {
        matches_serialized_option(&self.source, Some(product.source), product_source_name)
            && matches_option_set(&self.pil, product.pil.as_deref(), normalize_upper)
            && matches_option_set(&self.family, product.family, normalize_lower)
            && matches_option_set(&self.container, Some(product.container), normalize_lower)
            && matches_option_set(&self.wmo_prefix, product.wmo_prefix, normalize_upper)
            && matches_option_set(
                &self.office,
                product.office.as_ref().map(|office| office.code),
                normalize_upper,
            )
            && matches_option_set(
                &self.office_city,
                product.office.as_ref().map(|office| office.city),
                normalize_lower,
            )
            && matches_option_set(
                &self.office_state,
                product.office.as_ref().map(|office| office.state),
                normalize_upper,
            )
            && matches_serialized_option(&self.bbb_kind, product.bbb_kind, bbb_kind_name)
    }
}
