//! Input normalization and validation helpers for archive queries.

use super::{PersistError, PersistResult};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

pub(crate) fn encode_cursor<T: Serialize>(cursor: &T) -> PersistResult<String> {
    let bytes = serde_json::to_vec(cursor)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn decode_optional_cursor<T>(cursor: Option<&str>) -> PersistResult<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    cursor.map(decode_cursor).transpose()
}

fn decode_cursor<T>(cursor: &str) -> PersistResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|err| PersistError::InvalidRequest(format!("invalid cursor: {err}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| PersistError::InvalidRequest(format!("invalid cursor payload: {err}")))
}

pub(crate) fn validate_lat(name: &str, value: f64) -> PersistResult<()> {
    if !value.is_finite() || !(-90.0..=90.0).contains(&value) {
        return Err(PersistError::InvalidRequest(format!(
            "{name} must be a finite value between -90 and 90"
        )));
    }
    Ok(())
}

pub(crate) fn validate_lon(name: &str, value: f64) -> PersistResult<()> {
    if !value.is_finite() || !(-180.0..=180.0).contains(&value) {
        return Err(PersistError::InvalidRequest(format!(
            "{name} must be a finite value between -180 and 180"
        )));
    }
    Ok(())
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
    query: &super::ProductListQuery,
) -> PersistResult<Option<(f64, f64, f64, f64)>> {
    match (query.min_lat, query.max_lat, query.min_lon, query.max_lon) {
        (None, None, None, None) => Ok(None),
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
            Ok(Some((min_lat, max_lat, min_lon, max_lon)))
        }
        _ => Err(PersistError::InvalidRequest(
            "min_lat, max_lat, min_lon, and max_lon must be provided together".to_string(),
        )),
    }
}

pub(crate) fn validate_point_distance(
    query: &super::ProductListQuery,
) -> PersistResult<Option<(f64, f64, f64)>> {
    if query.distance_miles.is_some() && (query.lat.is_none() || query.lon.is_none()) {
        return Err(PersistError::InvalidRequest(
            "distance_miles requires both lat and lon".to_string(),
        ));
    }

    let distance_miles = match query.distance_miles {
        Some(distance_miles) if !distance_miles.is_finite() || distance_miles <= 0.0 => {
            return Err(PersistError::InvalidRequest(
                "distance_miles must be a finite value greater than 0".to_string(),
            ));
        }
        Some(distance_miles) => distance_miles,
        None => 5.0,
    };

    match (query.lat, query.lon) {
        (Some(lat), Some(lon)) => {
            validate_lat("lat", lat)?;
            validate_lon("lon", lon)?;
            Ok(Some((lat, lon, distance_miles * 1_609.344)))
        }
        (None, None) => Ok(None),
        _ => Err(PersistError::InvalidRequest(
            "lat and lon must be provided together".to_string(),
        )),
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
