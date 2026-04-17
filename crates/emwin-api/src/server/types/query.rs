use emwin_service::{ArchiveFilterInput, FileFilterInput};
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct IncidentsQuery {
    pub(crate) office: Option<String>,
    pub(crate) phenomena: Option<String>,
    pub(crate) significance: Option<String>,
    pub(crate) etn: Option<i64>,
    pub(crate) status: Option<String>,
    pub(crate) updated_after: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) updated_before: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) active_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema, Clone, Default)]
#[into_params(parameter_in = Query)]
pub(crate) struct ArchiveFilterParams {
    pub(crate) filename: Option<String>,
    pub(crate) source_receiver: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) pil: Option<String>,
    pub(crate) family: Option<String>,
    pub(crate) artifact_kind: Option<String>,
    pub(crate) container: Option<String>,
    pub(crate) wmo_prefix: Option<String>,
    pub(crate) office: Option<String>,
    pub(crate) office_city: Option<String>,
    pub(crate) office_state: Option<String>,
    pub(crate) bbb_kind: Option<String>,
    pub(crate) cccc: Option<String>,
    pub(crate) ttaaii: Option<String>,
    pub(crate) afos: Option<String>,
    pub(crate) bbb: Option<String>,
    pub(crate) has_issues: Option<String>,
    pub(crate) issue_kind: Option<String>,
    pub(crate) issue_code: Option<String>,
    pub(crate) has_vtec: Option<String>,
    pub(crate) has_ugc: Option<String>,
    pub(crate) has_hvtec: Option<String>,
    pub(crate) has_latlon: Option<String>,
    pub(crate) has_time_mot_loc: Option<String>,
    pub(crate) has_wind_hail: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) county: Option<String>,
    pub(crate) zone: Option<String>,
    pub(crate) fire_zone: Option<String>,
    pub(crate) marine_zone: Option<String>,
    pub(crate) vtec_phenomena: Option<String>,
    pub(crate) vtec_significance: Option<String>,
    pub(crate) vtec_action: Option<String>,
    pub(crate) vtec_office: Option<String>,
    #[schema(value_type = Option<i64>)]
    pub(crate) etn: Option<String>,
    pub(crate) hvtec_nwslid: Option<String>,
    pub(crate) hvtec_severity: Option<String>,
    pub(crate) hvtec_cause: Option<String>,
    pub(crate) hvtec_record: Option<String>,
    pub(crate) wind_hail_kind: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) lat: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) lon: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) distance_miles: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) min_lat: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) max_lat: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) min_lon: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) max_lon: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) min_wind_mph: Option<String>,
    #[schema(value_type = Option<f64>)]
    pub(crate) min_hail_inches: Option<String>,
    #[schema(value_type = Option<usize>)]
    pub(crate) min_size: Option<String>,
    #[schema(value_type = Option<usize>)]
    pub(crate) max_size: Option<String>,
    #[schema(value_type = Option<i64>)]
    pub(crate) source_timestamp_after: Option<String>,
    #[schema(value_type = Option<i64>)]
    pub(crate) source_timestamp_before: Option<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub(crate) ingested_after: Option<String>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub(crate) ingested_before: Option<String>,
}

impl ArchiveFilterParams {
    pub(crate) fn into_archive_filter_input(self) -> Result<ArchiveFilterInput, String> {
        Ok(ArchiveFilterInput {
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
            has_issues: self.has_issues,
            issue_kind: self.issue_kind,
            issue_code: self.issue_code,
            has_vtec: self.has_vtec,
            has_ugc: self.has_ugc,
            has_hvtec: self.has_hvtec,
            has_latlon: self.has_latlon,
            has_time_mot_loc: self.has_time_mot_loc,
            has_wind_hail: self.has_wind_hail,
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
            lat: parse_query_value("lat", self.lat)?,
            lon: parse_query_value("lon", self.lon)?,
            distance_miles: parse_query_value("distance_miles", self.distance_miles)?,
            min_lat: parse_query_value("min_lat", self.min_lat)?,
            max_lat: parse_query_value("max_lat", self.max_lat)?,
            min_lon: parse_query_value("min_lon", self.min_lon)?,
            max_lon: parse_query_value("max_lon", self.max_lon)?,
            min_wind_mph: parse_query_value("min_wind_mph", self.min_wind_mph)?,
            min_hail_inches: parse_query_value("min_hail_inches", self.min_hail_inches)?,
            min_size: parse_query_value("min_size", self.min_size)?,
            max_size: parse_query_value("max_size", self.max_size)?,
            source_timestamp_after: parse_query_value(
                "source_timestamp_after",
                self.source_timestamp_after,
            )?,
            source_timestamp_before: parse_query_value(
                "source_timestamp_before",
                self.source_timestamp_before,
            )?,
            ingested_after: parse_datetime_value("ingested_after", self.ingested_after)?,
            ingested_before: parse_datetime_value("ingested_before", self.ingested_before)?,
        })
    }

    pub(crate) fn into_product_list_query(
        self,
        default_limit: usize,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<emwin_service::ProductListQuery, String> {
        self.into_archive_filter_input()?
            .into_product_list_query(default_limit, limit, cursor)
            .map_err(|err| err.to_string())
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ProductsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct FeaturesQuery {
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct FeaturesGeoJsonQuery {
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct FacetAggregateHttpQuery {
    pub(crate) dimension: String,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct TimeseriesAggregateHttpQuery {
    pub(crate) measure: String,
    pub(crate) start: chrono::DateTime<chrono::Utc>,
    pub(crate) end: chrono::DateTime<chrono::Utc>,
    pub(crate) bucket: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct CellAggregateHttpQuery {
    pub(crate) measure: String,
    pub(crate) precision: u8,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct IncidentProductsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct IncidentEventsQuery {
    pub(crate) action: Option<String>,
    pub(crate) office: Option<String>,
    pub(crate) phenomena: Option<String>,
    pub(crate) significance: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) etn: Option<String>,
}

macro_rules! define_events_query {
    (@fields [$($fields:tt)*]) => {
        #[derive(Debug, Default, Deserialize, IntoParams)]
        #[into_params(parameter_in = Query)]
        pub(crate) struct EventsQuery {
            pub(crate) event: Option<String>,
            $($fields)*
        }
    };
    (@fields [$($fields:tt)*] $field:ident, string; $( $rest:tt )*) => {
        define_events_query!(
            @fields
            [$($fields)* pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, f64; $( $rest:tt )*) => {
        define_events_query!(
            @fields
            [$($fields)* pub(crate) $field: Option<f64>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, usize; $( $rest:tt )*) => {
        define_events_query!(
            @fields
            [$($fields)* pub(crate) $field: Option<usize>,]
            $( $rest )*
        );
    };
    ($( $rows:tt )*) => {
        define_events_query!(@fields [] $( $rows )*);
    };
}

macro_rules! build_file_filter_input_from_events_query {
    ($query:expr, $( $field:ident, $kind:ident; )*) => {
        FileFilterInput {
            $($field: $query.$field,)*
        }
    };
}

emwin_service::emwin_file_filter_fields!(define_events_query);

impl From<EventsQuery> for FileFilterInput {
    fn from(query: EventsQuery) -> Self {
        emwin_service::emwin_file_filter_fields!(build_file_filter_input_from_events_query, query)
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ArchiveIssuesQuery {
    pub(crate) product_id: Option<i64>,
    pub(crate) kind: Option<String>,
    pub(crate) code: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

fn parse_query_value<T>(name: &'static str, raw: Option<String>) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value
            .parse::<T>()
            .map(Some)
            .map_err(|err| format!("invalid `{name}` query parameter `{value}`: {err}")),
        None => Ok(None),
    }
}

fn parse_datetime_value(
    name: &'static str,
    raw: Option<String>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    match raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => chrono::DateTime::parse_from_rfc3339(value)
            .map(|timestamp| Some(timestamp.with_timezone(&chrono::Utc)))
            .map_err(|err| format!("invalid `{name}` query parameter `{value}`: {err}")),
        None => Ok(None),
    }
}
