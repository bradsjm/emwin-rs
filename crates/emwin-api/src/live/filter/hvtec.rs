use super::body::body_hvtec_codes;
use super::shared::{
    hvtec_cause_name, hvtec_record_name, hvtec_severity_name, matches_option_set,
    matches_serialized_option, normalize_upper,
};
use emwin_parser::{HvtecCode, ProductBody};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HvtecFilter {
    pub(crate) present: Option<bool>,
    pub(crate) nwslid: Option<BTreeSet<String>>,
    pub(crate) severity: Option<BTreeSet<String>>,
    pub(crate) cause: Option<BTreeSet<String>>,
    pub(crate) record: Option<BTreeSet<String>>,
}

impl HvtecFilter {
    pub(crate) fn has_constraints(&self) -> bool {
        self.present.is_some()
            || self.nwslid.is_some()
            || self.severity.is_some()
            || self.cause.is_some()
            || self.record.is_some()
    }

    pub(crate) fn matches(&self, body: Option<&ProductBody>) -> bool {
        if !self.has_constraints() {
            return true;
        }

        let codes = body.map(body_hvtec_codes).unwrap_or_default();
        if let Some(present) = self.present
            && present == codes.is_empty()
        {
            return false;
        }

        if self.nwslid.is_none()
            && self.severity.is_none()
            && self.cause.is_none()
            && self.record.is_none()
        {
            return true;
        }

        if codes.is_empty() {
            return false;
        }

        codes.iter().any(|code| self.matches_code(code))
    }

    fn matches_code(&self, code: &HvtecCode) -> bool {
        matches_option_set(&self.nwslid, Some(code.nwslid.as_str()), normalize_upper)
            && matches_serialized_option(&self.severity, Some(code.severity), hvtec_severity_name)
            && matches_serialized_option(&self.cause, Some(code.cause), hvtec_cause_name)
            && matches_serialized_option(&self.record, Some(code.record), hvtec_record_name)
    }
}
