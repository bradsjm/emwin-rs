//! Input normalization and validation helpers for archive queries.

use super::{PersistError, PersistResult};
use emwin_service::{ProductListQuery, ServiceError};

pub(crate) fn encode_cursor<T: serde::Serialize>(cursor: &T) -> PersistResult<String> {
    emwin_service::archive::encode_cursor(cursor).map_err(map_service_error)
}

pub(crate) fn decode_optional_cursor<T>(cursor: Option<&str>) -> PersistResult<Option<T>>
where
    T: for<'de> serde::Deserialize<'de>,
{
    emwin_service::archive::decode_optional_cursor(cursor).map_err(map_service_error)
}

pub(crate) fn split_csv_values(
    raw_values: Option<&str>,
    normalize: fn(&str) -> String,
) -> Option<Vec<String>> {
    let values = raw_values
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize)
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

pub(crate) fn split_csv_i64(raw_values: Option<&str>) -> PersistResult<Option<Vec<i64>>> {
    let mut values = Vec::new();
    for raw_value in raw_values.into_iter().flat_map(|value| value.split(',')) {
        let raw_value = raw_value.trim();
        if raw_value.is_empty() {
            continue;
        }
        let value = raw_value.parse::<i64>().map_err(|err| {
            PersistError::InvalidRequest(format!("invalid etn value `{raw_value}`: {err}"))
        })?;
        values.push(value);
    }
    Ok((!values.is_empty()).then_some(values))
}

pub(crate) fn normalize_upper(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

pub(crate) fn normalize_lower(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(crate) fn validate_bbox(
    query: &ProductListQuery,
) -> PersistResult<Option<(f64, f64, f64, f64)>> {
    query.validate().map_err(map_service_error)?;
    match (query.min_lat, query.max_lat, query.min_lon, query.max_lon) {
        (None, None, None, None) => Ok(None),
        (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) => {
            Ok(Some((min_lat, max_lat, min_lon, max_lon)))
        }
        _ => unreachable!("validated product queries must provide complete bbox inputs"),
    }
}

pub(crate) fn validate_point_distance(
    query: &ProductListQuery,
) -> PersistResult<Option<(f64, f64, f64)>> {
    query.validate().map_err(map_service_error)?;

    let distance_miles = query.distance_miles.unwrap_or(5.0);

    match (query.lat, query.lon) {
        (Some(lat), Some(lon)) => Ok(Some((lat, lon, distance_miles * 1_609.344))),
        (None, None) => Ok(None),
        _ => unreachable!("validated product queries must provide lat/lon together"),
    }
}

pub(crate) fn map_service_error(error: ServiceError) -> PersistError {
    match error {
        ServiceError::InvalidRequest(message)
        | ServiceError::Runtime(message)
        | ServiceError::NotConfigured(message) => PersistError::InvalidRequest(message),
        ServiceError::InvalidConfig(message) => PersistError::InvalidConfig(message),
        ServiceError::Io(error) => PersistError::Io(error),
        ServiceError::Json(error) => PersistError::Json(error),
    }
}

#[cfg(test)]
mod tests {
    use crate::error::PersistError;
    use emwin_service::{FeatureCursor, FeatureKind, ProductCursor, ProductListQuery};

    use super::{
        decode_optional_cursor, encode_cursor, normalize_upper, split_csv_i64, split_csv_values,
        validate_bbox, validate_point_distance,
    };

    #[test]
    fn cursor_round_trip_preserves_payload() {
        let cursor = ProductCursor {
            source_timestamp_utc: 123,
            product_id: 456,
        };
        let encoded = encode_cursor(&cursor).expect("cursor should encode");
        let decoded =
            decode_optional_cursor::<ProductCursor>(Some(&encoded)).expect("cursor should decode");
        assert_eq!(decoded, Some(cursor));
    }

    #[test]
    fn invalid_cursor_is_rejected() {
        let error = decode_optional_cursor::<FeatureCursor>(Some("not-base64"))
            .expect_err("cursor should be rejected");
        match error {
            PersistError::InvalidRequest(message) => {
                assert!(message.contains("invalid cursor"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn split_csv_values_trims_and_ignores_empty_segments() {
        let values =
            split_csv_values(Some("  a, ,B ,, c "), normalize_upper).expect("values should parse");
        assert_eq!(values, vec!["A", "B", "C"]);
    }

    #[test]
    fn split_csv_i64_rejects_invalid_values() {
        let error = split_csv_i64(Some("1, nope, 3")).expect_err("invalid value should fail");
        match error {
            PersistError::InvalidRequest(message) => {
                assert!(message.contains("invalid etn value `nope`"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn validate_bbox_rejects_partial_input() {
        let query = ProductListQuery {
            min_lat: Some(10.0),
            ..ProductListQuery::default()
        };
        let error = validate_bbox(&query).expect_err("partial bbox should fail");
        match error {
            PersistError::InvalidRequest(message) => {
                assert_eq!(
                    message,
                    "min_lat, max_lat, min_lon, and max_lon must be provided together"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn validate_point_distance_rejects_distance_without_coordinates() {
        let query = ProductListQuery {
            distance_miles: Some(10.0),
            ..ProductListQuery::default()
        };
        let error =
            validate_point_distance(&query).expect_err("distance without coordinates should fail");
        match error {
            PersistError::InvalidRequest(message) => {
                assert_eq!(message, "distance_miles requires both lat and lon");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn feature_cursor_round_trip_preserves_feature_kind() {
        let cursor = FeatureCursor {
            source_timestamp_utc: 123,
            product_id: 456,
            feature_kind: FeatureKind::HvtecPoint,
            feature_row_id: 789,
        };
        let encoded = encode_cursor(&cursor).expect("cursor should encode");
        let decoded =
            decode_optional_cursor::<FeatureCursor>(Some(&encoded)).expect("cursor should decode");
        assert_eq!(decoded, Some(cursor));
    }
}
