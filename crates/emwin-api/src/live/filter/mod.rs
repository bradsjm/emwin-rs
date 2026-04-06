//! Filter completed-file events for server consumers.
//!
//! These filters operate on already-enriched parser output, which keeps query evaluation out of
//! the hot path that assembles files from incoming segments.

mod body;
mod geo;
mod header;
mod hvtec;
mod issues;
mod location;
mod presence;
mod product;
mod shared;
mod size;
mod vtec;
mod wind_hail;

pub(crate) use self::shared::{FileFilterInput, FileFilterInputError};

use self::geo::GeoFilter;
use self::header::HeaderFilter;
use self::hvtec::HvtecFilter;
use self::issues::IssueFilter;
use self::location::LocationFilter;
use self::presence::BodyPresenceFilter;
use self::product::ProductFilter;
use self::shared::{
    csv_numbers, csv_values, normalize_lower, normalize_upper, parse_optional_bool,
};
use self::size::SizeRange;
use self::vtec::VtecFilter;
use self::wind_hail::WindHailFilter;
use crate::live::server_support::wildcard_match;
use emwin_db::CompletedFileMetadata;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FileEventFilter {
    pub(crate) filename_pattern: Option<String>,
    pub(crate) size: SizeRange,
    pub(crate) product: ProductFilter,
    pub(crate) header: HeaderFilter,
    pub(crate) issues: IssueFilter,
    pub(crate) geo: GeoFilter,
    pub(crate) vtec: VtecFilter,
    pub(crate) hvtec: HvtecFilter,
    pub(crate) wind_hail: WindHailFilter,
    pub(crate) location: LocationFilter,
    pub(crate) presence: BodyPresenceFilter,
}

impl Eq for FileEventFilter {}

impl FileEventFilter {
    pub(crate) fn try_from_input(input: &FileFilterInput) -> Result<Self, FileFilterInputError> {
        let location = LocationFilter::try_from_input(input)?;

        if input
            .min_size
            .zip(input.max_size)
            .is_some_and(|(min, max)| min > max)
        {
            return Err(FileFilterInputError::new(
                "min_size must be less than or equal to max_size",
            ));
        }

        Ok(Self {
            filename_pattern: input.filename.clone(),
            size: SizeRange {
                min: input.min_size,
                max: input.max_size,
            },
            product: ProductFilter {
                source: csv_values(input.source.as_deref(), normalize_lower),
                pil: csv_values(input.pil.as_deref(), normalize_upper),
                family: csv_values(input.family.as_deref(), normalize_lower),
                container: csv_values(input.container.as_deref(), normalize_lower),
                wmo_prefix: csv_values(input.wmo_prefix.as_deref(), normalize_upper),
                office: csv_values(input.office.as_deref(), normalize_upper),
                office_city: csv_values(input.office_city.as_deref(), normalize_lower),
                office_state: csv_values(input.office_state.as_deref(), normalize_upper),
                bbb_kind: csv_values(input.bbb_kind.as_deref(), normalize_lower),
            },
            header: HeaderFilter {
                cccc: csv_values(input.cccc.as_deref(), normalize_upper),
                ttaaii: csv_values(input.ttaaii.as_deref(), normalize_upper),
                afos: csv_values(input.afos.as_deref(), normalize_upper),
                bbb: csv_values(input.bbb.as_deref(), normalize_upper),
            },
            issues: IssueFilter {
                has_issues: parse_optional_bool(input.has_issues.as_deref()),
                kinds: csv_values(input.issue_kind.as_deref(), normalize_lower),
                codes: csv_values(input.issue_code.as_deref(), normalize_lower),
            },
            geo: GeoFilter {
                states: csv_values(input.state.as_deref(), normalize_upper),
                counties: csv_values(input.county.as_deref(), normalize_upper),
                zones: csv_values(input.zone.as_deref(), normalize_upper),
                fire_zones: csv_values(input.fire_zone.as_deref(), normalize_upper),
                marine_zones: csv_values(input.marine_zone.as_deref(), normalize_upper),
            },
            vtec: VtecFilter {
                phenomena: csv_values(input.vtec_phenomena.as_deref(), normalize_upper),
                significance: csv_values(input.vtec_significance.as_deref(), normalize_upper),
                action: csv_values(input.vtec_action.as_deref(), normalize_upper),
                office: csv_values(input.vtec_office.as_deref(), normalize_upper),
                etn: csv_numbers(input.etn.as_deref()),
            },
            hvtec: HvtecFilter {
                present: parse_optional_bool(input.has_hvtec.as_deref()),
                nwslid: csv_values(input.hvtec_nwslid.as_deref(), normalize_upper),
                severity: csv_values(input.hvtec_severity.as_deref(), normalize_lower),
                cause: csv_values(input.hvtec_cause.as_deref(), normalize_lower),
                record: csv_values(input.hvtec_record.as_deref(), normalize_lower),
            },
            wind_hail: WindHailFilter {
                present: parse_optional_bool(input.has_wind_hail.as_deref()),
                kinds: csv_values(input.wind_hail_kind.as_deref(), normalize_lower),
                min_wind_mph: input.min_wind_mph,
                min_hail_inches: input.min_hail_inches,
            },
            location,
            presence: BodyPresenceFilter {
                has_vtec: parse_optional_bool(input.has_vtec.as_deref()),
                has_ugc: parse_optional_bool(input.has_ugc.as_deref()),
                has_hvtec: parse_optional_bool(input.has_hvtec.as_deref()),
                has_latlon: parse_optional_bool(input.has_latlon.as_deref()),
                has_time_mot_loc: parse_optional_bool(input.has_time_mot_loc.as_deref()),
                has_wind_hail: parse_optional_bool(input.has_wind_hail.as_deref()),
            },
        })
    }

    pub(crate) fn has_constraints(&self) -> bool {
        self.filename_pattern.is_some()
            || self.size.has_constraints()
            || self.product.has_constraints()
            || self.header.has_constraints()
            || self.issues.has_constraints()
            || self.geo.has_constraints()
            || self.vtec.has_constraints()
            || self.hvtec.has_constraints()
            || self.wind_hail.has_constraints()
            || self.location.has_constraints()
            || self.presence.has_constraints()
    }

    pub(crate) fn matches_metadata(&self, metadata: &CompletedFileMetadata) -> bool {
        if let Some(pattern) = self.filename_pattern.as_deref()
            && !wildcard_match(pattern, &metadata.filename)
        {
            return false;
        }

        if !self.size.matches(metadata.size) {
            return false;
        }
        if !self.product.matches(&metadata.product) {
            return false;
        }
        if !self.header.matches(&metadata.product) {
            return false;
        }
        if !self.issues.matches(&metadata.product.issues) {
            return false;
        }
        if !self.location.matches(metadata.product.body.as_ref()) {
            return false;
        }
        if !self.presence.matches(metadata.product.body.as_ref()) {
            return false;
        }
        if !self.geo.matches(metadata.product.body.as_ref()) {
            return false;
        }
        if !self.vtec.matches(metadata.product.body.as_ref()) {
            return false;
        }
        if !self.hvtec.matches(metadata.product.body.as_ref()) {
            return false;
        }

        self.wind_hail.matches(metadata.product.body.as_ref())
    }
}
