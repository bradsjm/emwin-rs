use crate::emwin_archive_filter_fields;
use crate::error::{ServiceError, ServiceResult};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use strum::{Display, IntoStaticStr};

macro_rules! archive_filter_input_type {
    (string) => {
        Option<String>
    };
    (bool_string) => {
        Option<String>
    };
    (f64) => {
        Option<f64>
    };
    (usize) => {
        Option<usize>
    };
    (i64) => {
        Option<i64>
    };
    (datetime_utc) => {
        Option<DateTime<Utc>>
    };
}

macro_rules! define_archive_filter_input {
    ($( $field:ident, $kind:ident; )*) => {
        /// Flat archive filter input accepted by adapters before type-specific query building.
        #[derive(Debug, Clone, Default)]
        pub struct ArchiveFilterInput {
            $(pub $field: archive_filter_input_type!($kind),)*
        }
    };
}

macro_rules! archive_filter_query_value {
    ($value:ident, $field:ident, bool_string) => {
        parse_archive_bool(stringify!($field), $value.$field.as_deref())?
    };
    ($value:ident, $field:ident, $kind:ident) => {
        $value.$field
    };
}

macro_rules! build_product_list_query_from_filter {
    ($value:ident, $default_limit:ident, $limit:ident, $cursor:ident;
        $( $field:ident, $kind:ident; )*
    ) => {
        ProductListQuery {
            $($field: archive_filter_query_value!($value, $field, $kind),)*
            limit: $limit.unwrap_or($default_limit),
            cursor: $cursor,
        }
    };
}

emwin_archive_filter_fields!(define_archive_filter_input);

impl ArchiveFilterInput {
    pub fn into_product_list_query(
        self,
        default_limit: usize,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> ServiceResult<ProductListQuery> {
        let query = emwin_archive_filter_fields!(
            build_product_list_query_from_filter,
            self,
            default_limit,
            limit,
            cursor
        );
        query.validate()?;
        Ok(query)
    }
}

/// Stable incident identity used across archive endpoints.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IncidentKey {
    pub office: String,
    pub phenomena: String,
    pub significance: String,
    pub etn: i64,
}

/// Cursor for incident list pagination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentCursor {
    pub latest_product_timestamp_utc: DateTime<Utc>,
    pub office: String,
    pub phenomena: String,
    pub significance: String,
    pub etn: i64,
}

/// Cursor for one incident's product list pagination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentProductsCursor {
    pub source_timestamp_utc: i64,
    pub product_id: i64,
}

/// Cursor for archive product list pagination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductCursor {
    pub source_timestamp_utc: i64,
    pub product_id: i64,
}

/// Feature kind returned from the archive feature projection.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Display,
    IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FeatureKind {
    Polygon,
    TimeMotLocPath,
    UgcPoint,
    HvtecPoint,
    SearchPoint,
}

impl FeatureKind {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn ordinal(self) -> i16 {
        match self {
            Self::Polygon => 1,
            Self::TimeMotLocPath => 2,
            Self::UgcPoint => 3,
            Self::HvtecPoint => 4,
            Self::SearchPoint => 5,
        }
    }
}

impl FromStr for FeatureKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "polygon" => Ok(Self::Polygon),
            "time_mot_loc_path" => Ok(Self::TimeMotLocPath),
            "ugc_point" => Ok(Self::UgcPoint),
            "hvtec_point" => Ok(Self::HvtecPoint),
            "search_point" => Ok(Self::SearchPoint),
            _ => Err(format!("invalid feature kind `{value}`")),
        }
    }
}

/// Cursor for archive feature pagination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureCursor {
    pub source_timestamp_utc: i64,
    pub product_id: i64,
    pub feature_kind: FeatureKind,
    pub feature_row_id: i64,
}

/// Cursor for archive issue pagination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedIssueCursor {
    pub source_timestamp_utc: i64,
    pub product_id: i64,
    pub issue_id: i64,
}

/// Incident list query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentListQuery {
    pub office: Option<String>,
    pub phenomena: Option<String>,
    pub significance: Option<String>,
    pub etn: Option<i64>,
    pub status: Option<String>,
    pub updated_after: Option<DateTime<Utc>>,
    pub updated_before: Option<DateTime<Utc>>,
    pub active_at: Option<DateTime<Utc>>,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for IncidentListQuery {
    fn default() -> Self {
        Self {
            office: None,
            phenomena: None,
            significance: None,
            etn: None,
            status: None,
            updated_after: None,
            updated_before: None,
            active_at: None,
            limit: 100,
            cursor: None,
        }
    }
}

/// Pagination query for one incident's related products.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentProductsQuery {
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for IncidentProductsQuery {
    fn default() -> Self {
        Self {
            limit: 100,
            cursor: None,
        }
    }
}

/// Product archive query.
#[derive(Debug, Clone, PartialEq)]
pub struct ProductListQuery {
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
    pub has_issues: Option<bool>,
    pub issue_kind: Option<String>,
    pub issue_code: Option<String>,
    pub has_vtec: Option<bool>,
    pub has_ugc: Option<bool>,
    pub has_hvtec: Option<bool>,
    pub has_latlon: Option<bool>,
    pub has_time_mot_loc: Option<bool>,
    pub has_wind_hail: Option<bool>,
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
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for ProductListQuery {
    fn default() -> Self {
        Self {
            filename: None,
            source_receiver: None,
            source: None,
            pil: None,
            family: None,
            artifact_kind: None,
            container: None,
            wmo_prefix: None,
            office: None,
            office_city: None,
            office_state: None,
            bbb_kind: None,
            cccc: None,
            ttaaii: None,
            afos: None,
            bbb: None,
            has_issues: None,
            issue_kind: None,
            issue_code: None,
            has_vtec: None,
            has_ugc: None,
            has_hvtec: None,
            has_latlon: None,
            has_time_mot_loc: None,
            has_wind_hail: None,
            state: None,
            county: None,
            zone: None,
            fire_zone: None,
            marine_zone: None,
            vtec_phenomena: None,
            vtec_significance: None,
            vtec_action: None,
            vtec_office: None,
            etn: None,
            hvtec_nwslid: None,
            hvtec_severity: None,
            hvtec_cause: None,
            hvtec_record: None,
            wind_hail_kind: None,
            lat: None,
            lon: None,
            distance_miles: None,
            min_lat: None,
            max_lat: None,
            min_lon: None,
            max_lon: None,
            min_wind_mph: None,
            min_hail_inches: None,
            min_size: None,
            max_size: None,
            source_timestamp_after: None,
            source_timestamp_before: None,
            ingested_after: None,
            ingested_before: None,
            limit: 100,
            cursor: None,
        }
    }
}

impl ProductListQuery {
    pub fn validate(&self) -> ServiceResult<()> {
        validate_archive_size_inputs(self.min_size, self.max_size)?;
        validate_archive_spatial_inputs(
            self.lat,
            self.lon,
            self.distance_miles,
            self.min_lat,
            self.max_lat,
            self.min_lon,
            self.max_lon,
        )
    }
}

/// Feature list query.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FeatureListQuery {
    pub filters: ProductListQuery,
    pub kind: Option<FeatureKind>,
}

impl FeatureListQuery {
    pub fn validate(&self) -> ServiceResult<()> {
        self.filters.validate()
    }
}

/// Facet dimension accepted by aggregate endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FacetDimension {
    Office,
    Family,
    ArtifactKind,
    Phenomena,
    Significance,
    Status,
    IssueKind,
    IssueCode,
}

impl FacetDimension {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Office => "office",
            Self::Family => "family",
            Self::ArtifactKind => "artifact_kind",
            Self::Phenomena => "phenomena",
            Self::Significance => "significance",
            Self::Status => "status",
            Self::IssueKind => "issue_kind",
            Self::IssueCode => "issue_code",
        }
    }
}

impl FromStr for FacetDimension {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "office" => Ok(Self::Office),
            "family" => Ok(Self::Family),
            "artifact_kind" => Ok(Self::ArtifactKind),
            "phenomena" => Ok(Self::Phenomena),
            "significance" => Ok(Self::Significance),
            "status" => Ok(Self::Status),
            "issue_kind" => Ok(Self::IssueKind),
            "issue_code" => Ok(Self::IssueCode),
            _ => Err(format!("invalid facet dimension `{value}`")),
        }
    }
}

/// Facet aggregate query.
#[derive(Debug, Clone, PartialEq)]
pub struct FacetAggregateQuery {
    pub filters: ProductListQuery,
    pub dimension: FacetDimension,
    pub limit: usize,
}

/// Timeseries measure accepted by aggregate endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TimeseriesMeasure {
    ProductCount,
    IssueCount,
    IncidentCount,
}

impl TimeseriesMeasure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProductCount => "product_count",
            Self::IssueCount => "issue_count",
            Self::IncidentCount => "incident_count",
        }
    }
}

impl FromStr for TimeseriesMeasure {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "product_count" => Ok(Self::ProductCount),
            "issue_count" => Ok(Self::IssueCount),
            "incident_count" => Ok(Self::IncidentCount),
            _ => Err(format!("invalid timeseries measure `{value}`")),
        }
    }
}

/// Timeseries bucket size accepted by aggregate endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TimeseriesBucket {
    Hour,
    Day,
    Week,
}

impl TimeseriesBucket {
    pub fn duration(self) -> chrono::Duration {
        match self {
            Self::Hour => chrono::Duration::hours(1),
            Self::Day => chrono::Duration::days(1),
            Self::Week => chrono::Duration::weeks(1),
        }
    }

    pub fn postgres_interval(self) -> &'static str {
        match self {
            Self::Hour => "1 hour",
            Self::Day => "1 day",
            Self::Week => "7 days",
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
        }
    }
}

impl FromStr for TimeseriesBucket {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "hour" => Ok(Self::Hour),
            "day" => Ok(Self::Day),
            "week" => Ok(Self::Week),
            _ => Err(format!("invalid timeseries bucket `{value}`")),
        }
    }
}

/// Timeseries aggregate query.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeseriesAggregateQuery {
    pub filters: ProductListQuery,
    pub measure: TimeseriesMeasure,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub bucket: TimeseriesBucket,
}

/// Cell aggregate measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CellMeasure {
    ProductCount,
}

impl CellMeasure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProductCount => "product_count",
        }
    }
}

impl FromStr for CellMeasure {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "product_count" => Ok(Self::ProductCount),
            _ => Err(format!("invalid cell measure `{value}`")),
        }
    }
}

/// Cell aggregate query.
#[derive(Debug, Clone, PartialEq)]
pub struct CellAggregateQuery {
    pub filters: ProductListQuery,
    pub measure: CellMeasure,
    pub precision: u8,
    pub limit: usize,
}

/// Archived issue list query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedIssueListQuery {
    pub product_id: Option<i64>,
    pub kind: Option<String>,
    pub code: Option<String>,
    pub limit: usize,
    pub cursor: Option<String>,
}

impl Default for ArchivedIssueListQuery {
    fn default() -> Self {
        Self {
            product_id: None,
            kind: None,
            code: None,
            limit: 100,
            cursor: None,
        }
    }
}

pub fn build_feature_list_query(
    filters: ArchiveFilterInput,
    kind: Option<String>,
    default_limit: usize,
    limit: Option<usize>,
    cursor: Option<String>,
) -> ServiceResult<FeatureListQuery> {
    let query = FeatureListQuery {
        filters: filters.into_product_list_query(default_limit, limit, cursor)?,
        kind: parse_optional_enum_arg("feature kind", kind.as_deref())?,
    };
    query.validate()?;
    Ok(query)
}

pub fn build_facet_aggregate_query(
    filters: ArchiveFilterInput,
    dimension: &str,
    limit: Option<usize>,
) -> ServiceResult<FacetAggregateQuery> {
    let query = FacetAggregateQuery {
        filters: filters.into_product_list_query(100, Some(100), None)?,
        dimension: parse_required_enum_arg("facet dimension", dimension)?,
        limit: limit.unwrap_or(20),
    };
    query.filters.validate()?;
    Ok(query)
}

pub fn build_timeseries_aggregate_query(
    filters: ArchiveFilterInput,
    measure: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    bucket: &str,
) -> ServiceResult<TimeseriesAggregateQuery> {
    let query = TimeseriesAggregateQuery {
        filters: filters.into_product_list_query(100, Some(100), None)?,
        measure: parse_required_enum_arg("timeseries measure", measure)?,
        start,
        end,
        bucket: parse_required_enum_arg("timeseries bucket", bucket)?,
    };
    query.filters.validate()?;
    Ok(query)
}

pub fn build_cell_aggregate_query(
    filters: ArchiveFilterInput,
    measure: &str,
    precision: u8,
    limit: Option<usize>,
) -> ServiceResult<CellAggregateQuery> {
    let query = CellAggregateQuery {
        filters: filters.into_product_list_query(100, Some(100), None)?,
        measure: parse_required_enum_arg("cell measure", measure)?,
        precision,
        limit: limit.unwrap_or(100),
    };
    query.filters.validate()?;
    Ok(query)
}

pub fn parse_archive_bool(name: &str, raw: Option<&str>) -> ServiceResult<Option<bool>> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("true") || value == "1" => Ok(Some(true)),
        Some(value) if value.eq_ignore_ascii_case("false") || value == "0" => Ok(Some(false)),
        Some(value) => Err(ServiceError::InvalidRequest(format!(
            "{name} must be one of: true, false, 1, 0; got `{value}`"
        ))),
        None => Ok(None),
    }
}

pub fn encode_cursor<T>(value: &T) -> ServiceResult<String>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec(value)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn decode_optional_cursor<T>(value: Option<&str>) -> ServiceResult<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    value
        .map(|raw| {
            let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|err| {
                ServiceError::InvalidRequest(format!("invalid cursor encoding: {err}"))
            })?;
            serde_json::from_slice(&bytes).map_err(|err| {
                ServiceError::InvalidRequest(format!("invalid cursor payload: {err}"))
            })
        })
        .transpose()
}

fn parse_optional_enum_arg<T>(name: &str, raw: Option<&str>) -> ServiceResult<Option<T>>
where
    T: FromStr<Err = String>,
{
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<T>().map_err(|message| {
                ServiceError::InvalidRequest(format!("invalid {name}: {message}"))
            })
        })
        .transpose()
}

fn parse_required_enum_arg<T>(name: &str, raw: &str) -> ServiceResult<T>
where
    T: FromStr<Err = String>,
{
    raw.trim()
        .parse::<T>()
        .map_err(|message| ServiceError::InvalidRequest(format!("invalid {name}: {message}")))
}

fn validate_archive_spatial_inputs(
    lat: Option<f64>,
    lon: Option<f64>,
    distance_miles: Option<f64>,
    min_lat: Option<f64>,
    max_lat: Option<f64>,
    min_lon: Option<f64>,
    max_lon: Option<f64>,
) -> ServiceResult<()> {
    match (min_lat, max_lat, min_lon, max_lon) {
        (None, None, None, None) => {}
        (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) => {
            validate_lat("min_lat", min_lat)?;
            validate_lat("max_lat", max_lat)?;
            validate_lon("min_lon", min_lon)?;
            validate_lon("max_lon", max_lon)?;
            if min_lat > max_lat {
                return Err(ServiceError::InvalidRequest(
                    "min_lat must be less than or equal to max_lat".to_string(),
                ));
            }
            if min_lon > max_lon {
                return Err(ServiceError::InvalidRequest(
                    "min_lon must be less than or equal to max_lon".to_string(),
                ));
            }
        }
        _ => {
            return Err(ServiceError::InvalidRequest(
                "min_lat, max_lat, min_lon, and max_lon must be provided together".to_string(),
            ));
        }
    }

    match (lat, lon) {
        (None, None) => {}
        (Some(lat), Some(lon)) => {
            validate_lat("lat", lat)?;
            validate_lon("lon", lon)?;
        }
        _ => {
            return Err(ServiceError::InvalidRequest(
                "lat and lon must be provided together".to_string(),
            ));
        }
    }

    if let Some(distance_miles) = distance_miles {
        if lat.is_none() || lon.is_none() {
            return Err(ServiceError::InvalidRequest(
                "distance_miles requires both lat and lon".to_string(),
            ));
        }
        if !distance_miles.is_finite() || distance_miles < 0.0 {
            return Err(ServiceError::InvalidRequest(
                "distance_miles must be a non-negative finite number".to_string(),
            ));
        }
    }

    Ok(())
}

fn validate_archive_size_inputs(
    min_size: Option<usize>,
    max_size: Option<usize>,
) -> ServiceResult<()> {
    if min_size.zip(max_size).is_some_and(|(min, max)| min > max) {
        return Err(ServiceError::InvalidRequest(
            "min_size must be less than or equal to max_size".to_string(),
        ));
    }
    Ok(())
}

fn validate_lat(name: &str, value: f64) -> ServiceResult<()> {
    if !value.is_finite() || !(-90.0..=90.0).contains(&value) {
        return Err(ServiceError::InvalidRequest(format!(
            "{name} must be a finite latitude between -90 and 90"
        )));
    }
    Ok(())
}

fn validate_lon(name: &str, value: f64) -> ServiceResult<()> {
    if !value.is_finite() || !(-180.0..=180.0).contains(&value) {
        return Err(ServiceError::InvalidRequest(format!(
            "{name} must be a finite longitude between -180 and 180"
        )));
    }
    Ok(())
}
