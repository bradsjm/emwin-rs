use emwin_live::FileFilterInput;
use emwin_service::ArchiveFilterInput;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Deserialize, IntoParams)]
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

macro_rules! define_archive_filter_params {
    (@fields [$($fields:tt)*]) => {
        #[derive(Debug, Deserialize, IntoParams, ToSchema, Clone, Default)]
        pub(crate) struct ArchiveFilterParams {
            $($fields)*
        }
    };
    (@fields [$($fields:tt)*] $field:ident, string; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, bool_string; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, f64; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* #[schema(value_type = Option<f64>)] pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, usize; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* #[schema(value_type = Option<usize>)] pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, i64; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* #[schema(value_type = Option<i64>)] pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    (@fields [$($fields:tt)*] $field:ident, datetime_utc; $( $rest:tt )*) => {
        define_archive_filter_params!(
            @fields
            [$($fields)* #[schema(value_type = Option<String>, format = DateTime)] pub(crate) $field: Option<String>,]
            $( $rest )*
        );
    };
    ($( $rows:tt )*) => {
        define_archive_filter_params!(@fields [] $( $rows )*);
    };
}

macro_rules! archive_filter_input_value_from_param {
    ($value:ident, $field:ident, string) => {
        $value.$field
    };
    ($value:ident, $field:ident, bool_string) => {
        $value.$field
    };
    ($value:ident, $field:ident, f64) => {
        parse_query_value(stringify!($field), $value.$field)?
    };
    ($value:ident, $field:ident, usize) => {
        parse_query_value(stringify!($field), $value.$field)?
    };
    ($value:ident, $field:ident, i64) => {
        parse_query_value(stringify!($field), $value.$field)?
    };
    ($value:ident, $field:ident, datetime_utc) => {
        parse_datetime_value(stringify!($field), $value.$field)?
    };
}

macro_rules! build_archive_filter_input_from_params {
    ($value:ident; $( $field:ident, $kind:ident; )*) => {
        ArchiveFilterInput {
            $($field: archive_filter_input_value_from_param!($value, $field, $kind),)*
        }
    };
}

emwin_service::emwin_archive_filter_fields!(define_archive_filter_params);

impl ArchiveFilterParams {
    pub(crate) fn into_archive_filter_input(self) -> Result<ArchiveFilterInput, String> {
        Ok(emwin_service::emwin_archive_filter_fields!(
            build_archive_filter_input_from_params,
            self
        ))
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
pub(crate) struct ProductsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FeaturesQuery {
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FeaturesGeoJsonQuery {
    pub(crate) kind: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct FacetAggregateHttpQuery {
    pub(crate) dimension: String,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct TimeseriesAggregateHttpQuery {
    pub(crate) measure: String,
    pub(crate) start: chrono::DateTime<chrono::Utc>,
    pub(crate) end: chrono::DateTime<chrono::Utc>,
    pub(crate) bucket: String,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct CellAggregateHttpQuery {
    pub(crate) measure: String,
    pub(crate) precision: u8,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct IncidentProductsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
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

emwin_live::emwin_file_filter_fields!(define_events_query);

impl From<EventsQuery> for FileFilterInput {
    fn from(query: EventsQuery) -> Self {
        emwin_live::emwin_file_filter_fields!(build_file_filter_input_from_events_query, query)
    }
}

#[derive(Debug, Deserialize, IntoParams)]
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
