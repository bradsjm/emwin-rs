//! Geographic helpers used by product enrichment and filtering.
//!
//! The functions here stay allocation-free and operate on plain coordinate values so parsers and
//! filters can reuse them without bringing in heavier geometry dependencies.

#![allow(missing_docs)]

use serde::Serialize;

/// Latitude and longitude in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

/// Axis-aligned bounding box in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GeoBounds {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

const EARTH_RADIUS_MILES: f64 = 3_958.761_3;
const EPSILON: f64 = 1e-9;

/// Computes great-circle distance in miles with the haversine formula.
pub fn distance_miles(a: GeoPoint, b: GeoPoint) -> f64 {
    let lat1 = a.lat.to_radians();
    let lat2 = b.lat.to_radians();
    let delta_lat = (b.lat - a.lat).to_radians();
    let delta_lon = (b.lon - a.lon).to_radians();

    let sin_lat = (delta_lat / 2.0).sin();
    let sin_lon = (delta_lon / 2.0).sin();
    let haversine = sin_lat * sin_lat + lat1.cos() * lat2.cos() * sin_lon * sin_lon;
    let arc = 2.0 * haversine.sqrt().atan2((1.0 - haversine).sqrt());

    EARTH_RADIUS_MILES * arc
}

/// Returns the bounding box for a polygon expressed as `(lat, lon)` pairs.
pub fn polygon_bounds(points: &[(f64, f64)]) -> Option<GeoBounds> {
    let &(first_lat, first_lon) = points.first()?;
    let mut bounds = GeoBounds {
        min_lat: first_lat,
        max_lat: first_lat,
        min_lon: first_lon,
        max_lon: first_lon,
    };

    for &(lat, lon) in &points[1..] {
        bounds.min_lat = bounds.min_lat.min(lat);
        bounds.max_lat = bounds.max_lat.max(lat);
        bounds.min_lon = bounds.min_lon.min(lon);
        bounds.max_lon = bounds.max_lon.max(lon);
    }

    Some(bounds)
}

pub fn bounds_contains(bounds: GeoBounds, point: GeoPoint) -> bool {
    point.lat >= bounds.min_lat
        && point.lat <= bounds.max_lat
        && point.lon >= bounds.min_lon
        && point.lon <= bounds.max_lon
}

pub fn point_in_polygon(point: GeoPoint, polygon: &[(f64, f64)]) -> bool {
    if polygon.len() < 3 {
        return false;
    }

    let mut inside = false;
    let mut previous = *polygon.last().expect("polygon length checked");

    for &current in polygon {
        if point_on_segment(point, previous, current) {
            return true;
        }

        let (current_lat, current_lon) = current;
        let (previous_lat, previous_lon) = previous;
        let intersects = ((current_lat > point.lat) != (previous_lat > point.lat))
            && (point.lon
                <= (previous_lon - current_lon) * (point.lat - current_lat)
                    / (previous_lat - current_lat)
                    + current_lon);

        if intersects {
            inside = !inside;
        }

        previous = current;
    }

    inside
}

fn point_on_segment(point: GeoPoint, start: (f64, f64), end: (f64, f64)) -> bool {
    let (start_lat, start_lon) = start;
    let (end_lat, end_lon) = end;
    let cross = (point.lat - start_lat) * (end_lon - start_lon)
        - (point.lon - start_lon) * (end_lat - start_lat);
    if cross.abs() > EPSILON {
        return false;
    }

    let min_lat = start_lat.min(end_lat) - EPSILON;
    let max_lat = start_lat.max(end_lat) + EPSILON;
    let min_lon = start_lon.min(end_lon) - EPSILON;
    let max_lon = start_lon.max(end_lon) + EPSILON;

    point.lat >= min_lat && point.lat <= max_lat && point.lon >= min_lon && point.lon <= max_lon
}

#[cfg(test)]
mod tests {
    use super::{GeoPoint, bounds_contains, distance_miles, point_in_polygon, polygon_bounds};

    #[test]
    fn haversine_distance_is_zero_for_same_point() {
        let point = GeoPoint {
            lat: 41.2565,
            lon: -95.9345,
        };

        assert_eq!(distance_miles(point, point), 0.0);
    }

    #[test]
    fn haversine_distance_matches_known_city_pair() {
        let omaha = GeoPoint {
            lat: 41.2565,
            lon: -95.9345,
        };
        let lincoln = GeoPoint {
            lat: 40.8136,
            lon: -96.7026,
        };

        let distance = distance_miles(omaha, lincoln);
        assert!(distance > 45.0);
        assert!(distance < 55.0);
    }

    #[test]
    fn polygon_bounds_cover_vertices() {
        let bounds = polygon_bounds(&[(41.0, -97.0), (42.0, -95.0), (40.0, -96.0)])
            .expect("bounds should exist");

        assert!(bounds_contains(
            bounds,
            GeoPoint {
                lat: 41.5,
                lon: -96.2,
            }
        ));
        assert!(!bounds_contains(
            bounds,
            GeoPoint {
                lat: 43.0,
                lon: -96.2,
            }
        ));
    }

    #[test]
    fn point_in_polygon_handles_inside_outside_and_edges() {
        let polygon = &[
            (41.0, -97.0),
            (42.0, -97.0),
            (42.0, -95.0),
            (41.0, -95.0),
            (41.0, -97.0),
        ];

        assert!(point_in_polygon(
            GeoPoint {
                lat: 41.5,
                lon: -96.0,
            },
            polygon,
        ));
        assert!(!point_in_polygon(
            GeoPoint {
                lat: 40.5,
                lon: -96.0,
            },
            polygon,
        ));
        assert!(point_in_polygon(
            GeoPoint {
                lat: 41.0,
                lon: -96.0,
            },
            polygon,
        ));
        assert!(point_in_polygon(
            GeoPoint {
                lat: 41.0,
                lon: -97.0,
            },
            polygon,
        ));
    }
}
