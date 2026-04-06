use super::body::body_vtec_codes;
use super::shared::{matches_number_set, matches_option_set, normalize_upper};
use emwin_parser::{ProductBody, VtecCode};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct VtecFilter {
    pub(crate) phenomena: Option<BTreeSet<String>>,
    pub(crate) significance: Option<BTreeSet<String>>,
    pub(crate) action: Option<BTreeSet<String>>,
    pub(crate) office: Option<BTreeSet<String>>,
    pub(crate) etn: Option<BTreeSet<u32>>,
}

impl VtecFilter {
    pub(crate) fn has_constraints(&self) -> bool {
        self.phenomena.is_some()
            || self.significance.is_some()
            || self.action.is_some()
            || self.office.is_some()
            || self.etn.is_some()
    }

    pub(crate) fn matches(&self, body: Option<&ProductBody>) -> bool {
        if !self.has_constraints() {
            return true;
        }

        let Some(body) = body else {
            return false;
        };
        let vtec_codes = body_vtec_codes(body);
        if vtec_codes.is_empty() {
            return false;
        }

        vtec_codes.iter().any(|code| self.matches_code(code))
    }

    fn matches_code(&self, code: &VtecCode) -> bool {
        matches_option_set(
            &self.phenomena,
            Some(code.phenomena.as_str()),
            normalize_upper,
        ) && matches_char_set(&self.significance, code.significance)
            && matches_option_set(&self.action, Some(code.action.as_str()), normalize_upper)
            && matches_option_set(&self.office, Some(code.office.as_str()), normalize_upper)
            && matches_number_set(&self.etn, code.etn)
    }
}

fn matches_char_set(allowed: &Option<BTreeSet<String>>, value: char) -> bool {
    match allowed {
        Some(allowed) => allowed.contains(&value.to_ascii_uppercase().to_string()),
        None => true,
    }
}
