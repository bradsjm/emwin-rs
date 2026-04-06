use super::shared::{matches_option_set, normalize_upper};
use emwin_parser::ProductEnrichment;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HeaderFilter {
    pub(crate) cccc: Option<BTreeSet<String>>,
    pub(crate) ttaaii: Option<BTreeSet<String>>,
    pub(crate) afos: Option<BTreeSet<String>>,
    pub(crate) bbb: Option<BTreeSet<String>>,
}

impl HeaderFilter {
    pub(crate) fn has_constraints(&self) -> bool {
        self.cccc.is_some() || self.ttaaii.is_some() || self.afos.is_some() || self.bbb.is_some()
    }

    pub(crate) fn matches(&self, product: &ProductEnrichment) -> bool {
        if !self.has_constraints() {
            return true;
        }

        let header_matches = product.header.as_ref().is_some_and(|header| {
            matches_option_set(&self.cccc, Some(header.cccc.as_str()), normalize_upper)
                && matches_option_set(&self.ttaaii, Some(header.ttaaii.as_str()), normalize_upper)
                && matches_option_set(&self.afos, Some(header.afos.as_str()), normalize_upper)
                && matches_option_set(&self.bbb, header.bbb.as_deref(), normalize_upper)
        });
        let wmo_header_matches = product.wmo_header.as_ref().is_some_and(|header| {
            matches_option_set(&self.cccc, Some(header.cccc.as_str()), normalize_upper)
                && matches_option_set(&self.ttaaii, Some(header.ttaaii.as_str()), normalize_upper)
                && self.afos.is_none()
                && matches_option_set(&self.bbb, header.bbb.as_deref(), normalize_upper)
        });

        header_matches || wmo_header_matches
    }
}
