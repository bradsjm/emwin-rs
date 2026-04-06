use super::body::body_wind_hail_entries;
use super::shared::{is_hail_entry, is_wind_entry, wind_hail_kind_name, wind_speed_mph};
use emwin_parser::ProductBody;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WindHailFilter {
    pub(crate) present: Option<bool>,
    pub(crate) kinds: Option<BTreeSet<String>>,
    pub(crate) min_wind_mph: Option<f64>,
    pub(crate) min_hail_inches: Option<f64>,
}

impl Eq for WindHailFilter {}

impl WindHailFilter {
    pub(crate) fn has_constraints(&self) -> bool {
        self.present.is_some()
            || self.kinds.is_some()
            || self.min_wind_mph.is_some()
            || self.min_hail_inches.is_some()
    }

    pub(crate) fn matches(&self, body: Option<&ProductBody>) -> bool {
        if !self.has_constraints() {
            return true;
        }

        let entries = body.map(body_wind_hail_entries).unwrap_or_default();
        if let Some(present) = self.present
            && present == entries.is_empty()
        {
            return false;
        }

        if entries.is_empty() {
            return self.kinds.is_none()
                && self.min_wind_mph.is_none()
                && self.min_hail_inches.is_none();
        }

        if let Some(kinds) = &self.kinds
            && !entries
                .iter()
                .any(|entry| kinds.contains(wind_hail_kind_name(entry.kind)))
        {
            return false;
        }
        if let Some(min_wind_mph) = self.min_wind_mph
            && !entries.iter().any(|entry| {
                is_wind_entry(entry)
                    && entry
                        .numeric_value
                        .zip(entry.units.as_deref())
                        .is_some_and(|(value, units)| wind_speed_mph(value, units) >= min_wind_mph)
            })
        {
            return false;
        }
        if let Some(min_hail_inches) = self.min_hail_inches
            && !entries.iter().any(|entry| {
                is_hail_entry(entry)
                    && entry
                        .numeric_value
                        .is_some_and(|value| value >= min_hail_inches)
            })
        {
            return false;
        }

        true
    }
}
