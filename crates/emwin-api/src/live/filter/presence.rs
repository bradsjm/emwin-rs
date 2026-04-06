use super::body::{
    body_hvtec_codes_len, body_latlon_len, body_time_mot_loc_len, body_ugc_sections_len,
    body_vtec_codes_len, body_wind_hail_len,
};
use super::shared::matches_optional_presence;
use emwin_parser::ProductBody;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BodyPresenceFilter {
    pub(crate) has_vtec: Option<bool>,
    pub(crate) has_ugc: Option<bool>,
    pub(crate) has_hvtec: Option<bool>,
    pub(crate) has_latlon: Option<bool>,
    pub(crate) has_time_mot_loc: Option<bool>,
    pub(crate) has_wind_hail: Option<bool>,
}

impl BodyPresenceFilter {
    pub(crate) fn has_constraints(&self) -> bool {
        self.has_vtec.is_some()
            || self.has_ugc.is_some()
            || self.has_hvtec.is_some()
            || self.has_latlon.is_some()
            || self.has_time_mot_loc.is_some()
            || self.has_wind_hail.is_some()
    }

    pub(crate) fn matches(&self, body: Option<&ProductBody>) -> bool {
        matches_optional_presence(self.has_vtec, body.map_or(0, body_vtec_codes_len))
            && matches_optional_presence(self.has_ugc, body.map_or(0, body_ugc_sections_len))
            && matches_optional_presence(self.has_hvtec, body.map_or(0, body_hvtec_codes_len))
            && matches_optional_presence(self.has_latlon, body.map_or(0, body_latlon_len))
            && matches_optional_presence(
                self.has_time_mot_loc,
                body.map_or(0, body_time_mot_loc_len),
            )
            && matches_optional_presence(self.has_wind_hail, body.map_or(0, body_wind_hail_len))
    }
}
