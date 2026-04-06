use crate::error::{PersistError, PersistResult};
use crate::{
    CellAggregateQuery, FacetAggregateQuery, FeatureListQuery, ProductListQuery,
    TimeseriesAggregateQuery,
};
use chrono::{DateTime, Utc};
use std::str::FromStr;

#[derive(Debug, Clone, Default)]
pub struct ArchiveFilterInput {
    pub filename: Option<String>,
    pub source_receiver: Option<String>,
    pub source: Option<String>,
    pub pil: Option<String>,
    pub family: Option<String>,
    pub artifact_kind: Option<String>,
    pub container: Option<String>,
    pub wmo_prefix: Option<String>,
    pub office: Option<String>,
    pub office_city: Option<String>,
    pub office_state: Option<String>,
    pub bbb_kind: Option<String>,
    pub cccc: Option<String>,
    pub ttaaii: Option<String>,
    pub afos: Option<String>,
    pub bbb: Option<String>,
    pub has_issues: Option<String>,
    pub issue_kind: Option<String>,
    pub issue_code: Option<String>,
    pub has_vtec: Option<String>,
    pub has_ugc: Option<String>,
    pub has_hvtec: Option<String>,
    pub has_latlon: Option<String>,
    pub has_time_mot_loc: Option<String>,
    pub has_wind_hail: Option<String>,
    pub state: Option<String>,
    pub county: Option<String>,
    pub zone: Option<String>,
    pub fire_zone: Option<String>,
    pub marine_zone: Option<String>,
    pub vtec_phenomena: Option<String>,
    pub vtec_significance: Option<String>,
    pub vtec_action: Option<String>,
    pub vtec_office: Option<String>,
    pub etn: Option<String>,
    pub hvtec_nwslid: Option<String>,
    pub hvtec_severity: Option<String>,
    pub hvtec_cause: Option<String>,
    pub hvtec_record: Option<String>,
    pub wind_hail_kind: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub distance_miles: Option<f64>,
    pub min_lat: Option<f64>,
    pub max_lat: Option<f64>,
    pub min_lon: Option<f64>,
    pub max_lon: Option<f64>,
    pub min_wind_mph: Option<f64>,
    pub min_hail_inches: Option<f64>,
    pub min_size: Option<usize>,
    pub max_size: Option<usize>,
    pub source_timestamp_after: Option<i64>,
    pub source_timestamp_before: Option<i64>,
    pub ingested_after: Option<DateTime<Utc>>,
    pub ingested_before: Option<DateTime<Utc>>,
}

impl ArchiveFilterInput {
    pub fn into_product_list_query(
        self,
        default_limit: usize,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> PersistResult<ProductListQuery> {
        validate_archive_size_inputs(self.min_size, self.max_size)?;
        validate_archive_spatial_inputs(
            self.lat,
            self.lon,
            self.distance_miles,
            self.min_lat,
            self.max_lat,
            self.min_lon,
            self.max_lon,
        )?;
        Ok(ProductListQuery {
            filename: self.filename,
            source_receiver: self.source_receiver,
            source: self.source,
            pil: self.pil,
            family: self.family,
            artifact_kind: self.artifact_kind,
            container: self.container,
            wmo_prefix: self.wmo_prefix,
            office: self.office,
            office_city: self.office_city,
            office_state: self.office_state,
            bbb_kind: self.bbb_kind,
            cccc: self.cccc,
            ttaaii: self.ttaaii,
            afos: self.afos,
            bbb: self.bbb,
            has_issues: parse_archive_bool("has_issues", self.has_issues.as_deref())?,
            issue_kind: self.issue_kind,
            issue_code: self.issue_code,
            has_vtec: parse_archive_bool("has_vtec", self.has_vtec.as_deref())?,
            has_ugc: parse_archive_bool("has_ugc", self.has_ugc.as_deref())?,
            has_hvtec: parse_archive_bool("has_hvtec", self.has_hvtec.as_deref())?,
            has_latlon: parse_archive_bool("has_latlon", self.has_latlon.as_deref())?,
            has_time_mot_loc: parse_archive_bool(
                "has_time_mot_loc",
                self.has_time_mot_loc.as_deref(),
            )?,
            has_wind_hail: parse_archive_bool("has_wind_hail", self.has_wind_hail.as_deref())?,
            state: self.state,
            county: self.county,
            zone: self.zone,
            fire_zone: self.fire_zone,
            marine_zone: self.marine_zone,
            vtec_phenomena: self.vtec_phenomena,
            vtec_significance: self.vtec_significance,
            vtec_action: self.vtec_action,
            vtec_office: self.vtec_office,
            etn: self.etn,
            hvtec_nwslid: self.hvtec_nwslid,
            hvtec_severity: self.hvtec_severity,
            hvtec_cause: self.hvtec_cause,
            hvtec_record: self.hvtec_record,
            wind_hail_kind: self.wind_hail_kind,
            lat: self.lat,
            lon: self.lon,
            distance_miles: self.distance_miles,
            min_lat: self.min_lat,
            max_lat: self.max_lat,
            min_lon: self.min_lon,
            max_lon: self.max_lon,
            min_wind_mph: self.min_wind_mph,
            min_hail_inches: self.min_hail_inches,
            min_size: self.min_size,
            max_size: self.max_size,
            source_timestamp_after: self.source_timestamp_after,
            source_timestamp_before: self.source_timestamp_before,
            ingested_after: self.ingested_after,
            ingested_before: self.ingested_before,
            limit: limit.unwrap_or(default_limit),
            cursor,
        })
    }
}

pub fn build_feature_list_query(
    filters: ArchiveFilterInput,
    kind: Option<String>,
    default_limit: usize,
    limit: Option<usize>,
    cursor: Option<String>,
) -> PersistResult<FeatureListQuery> {
    Ok(FeatureListQuery {
        filters: filters.into_product_list_query(default_limit, limit, cursor)?,
        kind: parse_optional_enum_arg("feature kind", kind.as_deref())?,
    })
}

pub fn build_facet_aggregate_query(
    filters: ArchiveFilterInput,
    dimension: &str,
    limit: Option<usize>,
) -> PersistResult<FacetAggregateQuery> {
    Ok(FacetAggregateQuery {
        filters: filters.into_product_list_query(100, Some(100), None)?,
        dimension: parse_required_enum_arg("facet dimension", dimension)?,
        limit: limit.unwrap_or(20),
    })
}

pub fn build_timeseries_aggregate_query(
    filters: ArchiveFilterInput,
    measure: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    bucket: &str,
) -> PersistResult<TimeseriesAggregateQuery> {
    Ok(TimeseriesAggregateQuery {
        filters: filters.into_product_list_query(100, Some(100), None)?,
        measure: parse_required_enum_arg("timeseries measure", measure)?,
        start,
        end,
        bucket: parse_required_enum_arg("timeseries bucket", bucket)?,
    })
}

pub fn build_cell_aggregate_query(
    filters: ArchiveFilterInput,
    measure: &str,
    precision: u8,
    limit: Option<usize>,
) -> PersistResult<CellAggregateQuery> {
    Ok(CellAggregateQuery {
        filters: filters.into_product_list_query(100, Some(100), None)?,
        measure: parse_required_enum_arg("cell measure", measure)?,
        precision,
        limit: limit.unwrap_or(100),
    })
}

pub fn parse_archive_bool(name: &str, raw: Option<&str>) -> PersistResult<Option<bool>> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("true") || value == "1" => Ok(Some(true)),
        Some(value) if value.eq_ignore_ascii_case("false") || value == "0" => Ok(Some(false)),
        Some(value) => Err(PersistError::InvalidRequest(format!(
            "{name} must be one of: true, false, 1, 0; got `{value}`"
        ))),
        None => Ok(None),
    }
}

fn parse_optional_enum_arg<T>(name: &str, raw: Option<&str>) -> PersistResult<Option<T>>
where
    T: FromStr<Err = String>,
{
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<T>().map_err(|message| {
                PersistError::InvalidRequest(format!("invalid {name}: {message}"))
            })
        })
        .transpose()
}

fn parse_required_enum_arg<T>(name: &str, raw: &str) -> PersistResult<T>
where
    T: FromStr<Err = String>,
{
    raw.trim()
        .parse::<T>()
        .map_err(|message| PersistError::InvalidRequest(format!("invalid {name}: {message}")))
}

fn validate_archive_spatial_inputs(
    lat: Option<f64>,
    lon: Option<f64>,
    distance_miles: Option<f64>,
    min_lat: Option<f64>,
    max_lat: Option<f64>,
    min_lon: Option<f64>,
    max_lon: Option<f64>,
) -> PersistResult<()> {
    match (min_lat, max_lat, min_lon, max_lon) {
        (None, None, None, None) => {}
        (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) => {
            validate_lat("min_lat", min_lat)?;
            validate_lat("max_lat", max_lat)?;
            validate_lon("min_lon", min_lon)?;
            validate_lon("max_lon", max_lon)?;
            if min_lat > max_lat {
                return Err(PersistError::InvalidRequest(
                    "min_lat must be less than or equal to max_lat".to_string(),
                ));
            }
            if min_lon > max_lon {
                return Err(PersistError::InvalidRequest(
                    "min_lon must be less than or equal to max_lon".to_string(),
                ));
            }
        }
        _ => {
            return Err(PersistError::InvalidRequest(
                "min_lat, max_lat, min_lon, and max_lon must be provided together".to_string(),
            ));
        }
    }

    match (lat, lon) {
        (Some(lat), Some(lon)) => {
            validate_lat("lat", lat)?;
            validate_lon("lon", lon)?;
        }
        (None, None) => {}
        _ => {
            return Err(PersistError::InvalidRequest(
                "lat and lon must be provided together".to_string(),
            ));
        }
    }

    if distance_miles.is_some() && (lat.is_none() || lon.is_none()) {
        return Err(PersistError::InvalidRequest(
            "distance_miles requires both lat and lon".to_string(),
        ));
    }
    if let Some(distance_miles) = distance_miles
        && (!distance_miles.is_finite() || distance_miles <= 0.0)
    {
        return Err(PersistError::InvalidRequest(
            "distance_miles must be a finite value greater than 0".to_string(),
        ));
    }

    Ok(())
}

fn validate_archive_size_inputs(
    min_size: Option<usize>,
    max_size: Option<usize>,
) -> PersistResult<()> {
    if min_size.zip(max_size).is_some_and(|(min, max)| min > max) {
        return Err(PersistError::InvalidRequest(
            "min_size must be less than or equal to max_size".to_string(),
        ));
    }
    Ok(())
}

fn validate_lat(name: &str, value: f64) -> PersistResult<()> {
    if !value.is_finite() || !(-90.0..=90.0).contains(&value) {
        return Err(PersistError::InvalidRequest(format!(
            "{name} must be a finite value between -90 and 90"
        )));
    }
    Ok(())
}

fn validate_lon(name: &str, value: f64) -> PersistResult<()> {
    if !value.is_finite() || !(-180.0..=180.0).contains(&value) {
        return Err(PersistError::InvalidRequest(format!(
            "{name} must be a finite value between -180 and 180"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ArchiveFilterInput, build_feature_list_query};

    #[test]
    fn product_list_query_rejects_invalid_size_range() {
        let error = ArchiveFilterInput {
            min_size: Some(10),
            max_size: Some(1),
            ..ArchiveFilterInput::default()
        }
        .into_product_list_query(100, None, None)
        .expect_err("invalid size range should fail");

        assert!(
            error
                .to_string()
                .contains("min_size must be less than or equal to max_size")
        );
    }

    #[test]
    fn feature_list_query_preserves_artifact_kind_filter() {
        let query = build_feature_list_query(
            ArchiveFilterInput {
                artifact_kind: Some("nws_text_product,cap_message".to_string()),
                ..ArchiveFilterInput::default()
            },
            None,
            100,
            None,
            None,
        )
        .expect("feature list query should build");

        assert_eq!(
            query.filters.artifact_kind.as_deref(),
            Some("nws_text_product,cap_message")
        );
    }
}
