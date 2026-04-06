use super::body::{body_ugc_sections, matches_enriched_ugc_codes, matches_geo_states};
use emwin_parser::ProductBody;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GeoFilter {
    pub(crate) states: Option<BTreeSet<String>>,
    pub(crate) counties: Option<BTreeSet<String>>,
    pub(crate) zones: Option<BTreeSet<String>>,
    pub(crate) fire_zones: Option<BTreeSet<String>>,
    pub(crate) marine_zones: Option<BTreeSet<String>>,
}

impl GeoFilter {
    pub(crate) fn has_constraints(&self) -> bool {
        self.states.is_some()
            || self.counties.is_some()
            || self.zones.is_some()
            || self.fire_zones.is_some()
            || self.marine_zones.is_some()
    }

    pub(crate) fn matches(&self, body: Option<&ProductBody>) -> bool {
        if !self.has_constraints() {
            return true;
        }

        let Some(body) = body else {
            return false;
        };
        let sections = body_ugc_sections(body);
        if sections.is_empty() {
            return false;
        }

        matches_geo_states(&self.states, &sections)
            && matches_enriched_ugc_codes(
                &self.counties,
                &sections,
                |section| &section.counties,
                'C',
            )
            && matches_enriched_ugc_codes(&self.zones, &sections, |section| &section.zones, 'Z')
            && matches_enriched_ugc_codes(
                &self.fire_zones,
                &sections,
                |section| &section.fire_zones,
                'F',
            )
            && matches_enriched_ugc_codes(
                &self.marine_zones,
                &sections,
                |section| &section.marine_zones,
                'Z',
            )
    }
}
