use super::shared::FileFilterInput;
use emwin_parser::{
    GeoPoint, ProductBody, bounds_contains, distance_miles as geo_distance_miles, point_in_polygon,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LocationFilter {
    pub(crate) center: Option<GeoPoint>,
    pub(crate) distance_miles: Option<f64>,
    pub(crate) bbox: Option<LocationBoundingBox>,
}

impl Eq for LocationFilter {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocationBoundingBox {
    pub(crate) min_lat: f64,
    pub(crate) max_lat: f64,
    pub(crate) min_lon: f64,
    pub(crate) max_lon: f64,
}

impl Eq for LocationBoundingBox {}

impl LocationFilter {
    const DEFAULT_DISTANCE_MILES: f64 = 5.0;

    pub(crate) fn try_from_input(
        input: &FileFilterInput,
    ) -> Result<Self, super::FileFilterInputError> {
        let lat = input.lat;
        let lon = input.lon;

        if lat.is_some() != lon.is_some() {
            return Err(super::FileFilterInputError::new(
                "lat and lon must be provided together",
            ));
        }

        let center = match (lat, lon) {
            (Some(lat), Some(lon)) => {
                if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
                    return Err(super::FileFilterInputError::new(
                        "lat must be a finite value between -90 and 90",
                    ));
                }
                if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
                    return Err(super::FileFilterInputError::new(
                        "lon must be a finite value between -180 and 180",
                    ));
                }
                Some(GeoPoint { lat, lon })
            }
            _ => None,
        };

        let distance_miles = match input.distance_miles {
            Some(distance_miles) => {
                if center.is_none() {
                    return Err(super::FileFilterInputError::new(
                        "distance_miles requires both lat and lon",
                    ));
                }
                if !distance_miles.is_finite() || distance_miles <= 0.0 {
                    return Err(super::FileFilterInputError::new(
                        "distance_miles must be a finite value greater than 0",
                    ));
                }
                Some(distance_miles)
            }
            None if center.is_some() => Some(Self::DEFAULT_DISTANCE_MILES),
            None => None,
        };

        let bbox = match (input.min_lat, input.max_lat, input.min_lon, input.max_lon) {
            (None, None, None, None) => None,
            (Some(min_lat), Some(max_lat), Some(min_lon), Some(max_lon)) => {
                validate_lat("min_lat", min_lat)?;
                validate_lat("max_lat", max_lat)?;
                validate_lon("min_lon", min_lon)?;
                validate_lon("max_lon", max_lon)?;
                if min_lat > max_lat {
                    return Err(super::FileFilterInputError::new(
                        "min_lat must be less than or equal to max_lat",
                    ));
                }
                if min_lon > max_lon {
                    return Err(super::FileFilterInputError::new(
                        "min_lon must be less than or equal to max_lon",
                    ));
                }
                Some(LocationBoundingBox {
                    min_lat,
                    max_lat,
                    min_lon,
                    max_lon,
                })
            }
            _ => {
                return Err(super::FileFilterInputError::new(
                    "min_lat, max_lat, min_lon, and max_lon must be provided together",
                ));
            }
        };

        Ok(Self {
            center,
            distance_miles,
            bbox,
        })
    }

    pub(crate) fn has_constraints(&self) -> bool {
        self.center.is_some() || self.bbox.is_some()
    }

    pub(crate) fn matches(&self, body: Option<&ProductBody>) -> bool {
        if !self.has_constraints() {
            return true;
        }

        let Some(body) = body else {
            return false;
        };

        if let Some(bbox) = self.bbox
            && !matches_bbox(body, bbox)
        {
            return false;
        }

        let Some(center) = self.center else {
            return true;
        };

        if body.iter_polygons().any(|polygon| {
            polygon
                .bounds
                .is_some_and(|bounds| bounds_contains(bounds, center))
                && point_in_polygon(center, polygon.points)
        }) {
            return true;
        }

        let Some(radius_miles) = self.distance_miles else {
            return false;
        };

        body.iter_location_points()
            .any(|point| geo_distance_miles(center, point) <= radius_miles)
    }
}

fn validate_lat(name: &str, value: f64) -> Result<(), super::FileFilterInputError> {
    if !value.is_finite() || !(-90.0..=90.0).contains(&value) {
        return Err(super::FileFilterInputError::new(format!(
            "{name} must be a finite value between -90 and 90"
        )));
    }
    Ok(())
}

fn validate_lon(name: &str, value: f64) -> Result<(), super::FileFilterInputError> {
    if !value.is_finite() || !(-180.0..=180.0).contains(&value) {
        return Err(super::FileFilterInputError::new(format!(
            "{name} must be a finite value between -180 and 180"
        )));
    }
    Ok(())
}

fn matches_bbox(body: &ProductBody, bbox: LocationBoundingBox) -> bool {
    body.iter_polygons().any(|polygon| {
        polygon
            .points
            .iter()
            .any(|&(lat, lon)| point_in_bbox(GeoPoint { lat, lon }, bbox))
            || bbox_corners(bbox)
                .into_iter()
                .any(|corner| point_in_polygon(corner, polygon.points))
            || polygon
                .points
                .windows(2)
                .any(|segment| segment_intersects_bbox(segment[0], segment[1], bbox))
    }) || time_mot_loc_paths(body).into_iter().any(|path| {
        path.windows(2)
            .any(|segment| segment_intersects_bbox(segment[0], segment[1], bbox))
    }) || body
        .iter_location_points()
        .any(|point| point_in_bbox(point, bbox))
}

fn bbox_corners(bbox: LocationBoundingBox) -> [GeoPoint; 4] {
    [
        GeoPoint {
            lat: bbox.min_lat,
            lon: bbox.min_lon,
        },
        GeoPoint {
            lat: bbox.min_lat,
            lon: bbox.max_lon,
        },
        GeoPoint {
            lat: bbox.max_lat,
            lon: bbox.max_lon,
        },
        GeoPoint {
            lat: bbox.max_lat,
            lon: bbox.min_lon,
        },
    ]
}

fn point_in_bbox(point: GeoPoint, bbox: LocationBoundingBox) -> bool {
    point.lat >= bbox.min_lat
        && point.lat <= bbox.max_lat
        && point.lon >= bbox.min_lon
        && point.lon <= bbox.max_lon
}

fn segment_intersects_bbox(start: (f64, f64), end: (f64, f64), bbox: LocationBoundingBox) -> bool {
    let start = GeoPoint {
        lat: start.0,
        lon: start.1,
    };
    let end = GeoPoint {
        lat: end.0,
        lon: end.1,
    };
    if point_in_bbox(start, bbox) || point_in_bbox(end, bbox) {
        return true;
    }

    let edges = [
        ((bbox.min_lat, bbox.min_lon), (bbox.min_lat, bbox.max_lon)),
        ((bbox.min_lat, bbox.max_lon), (bbox.max_lat, bbox.max_lon)),
        ((bbox.max_lat, bbox.max_lon), (bbox.max_lat, bbox.min_lon)),
        ((bbox.max_lat, bbox.min_lon), (bbox.min_lat, bbox.min_lon)),
    ];

    edges
        .into_iter()
        .any(|(edge_start, edge_end)| segments_intersect(start, end, edge_start, edge_end))
}

fn segments_intersect(
    a_start: GeoPoint,
    a_end: GeoPoint,
    b_start: (f64, f64),
    b_end: (f64, f64),
) -> bool {
    let b_start = GeoPoint {
        lat: b_start.0,
        lon: b_start.1,
    };
    let b_end = GeoPoint {
        lat: b_end.0,
        lon: b_end.1,
    };

    let o1 = orientation(a_start, a_end, b_start);
    let o2 = orientation(a_start, a_end, b_end);
    let o3 = orientation(b_start, b_end, a_start);
    let o4 = orientation(b_start, b_end, a_end);

    if o1 != o2 && o3 != o4 {
        return true;
    }

    (o1 == 0 && on_segment(a_start, b_start, a_end))
        || (o2 == 0 && on_segment(a_start, b_end, a_end))
        || (o3 == 0 && on_segment(b_start, a_start, b_end))
        || (o4 == 0 && on_segment(b_start, a_end, b_end))
}

fn orientation(a: GeoPoint, b: GeoPoint, c: GeoPoint) -> i8 {
    let value = (b.lon - a.lon) * (c.lat - b.lat) - (b.lat - a.lat) * (c.lon - b.lon);
    if value.abs() < 1e-9 {
        0
    } else if value > 0.0 {
        1
    } else {
        -1
    }
}

fn on_segment(start: GeoPoint, middle: GeoPoint, end: GeoPoint) -> bool {
    middle.lon >= start.lon.min(end.lon)
        && middle.lon <= start.lon.max(end.lon)
        && middle.lat >= start.lat.min(end.lat)
        && middle.lat <= start.lat.max(end.lat)
}

fn time_mot_loc_paths(body: &ProductBody) -> Vec<&[(f64, f64)]> {
    match body {
        ProductBody::VtecEvent(vtec) => vtec
            .segments
            .iter()
            .flat_map(|segment| {
                segment
                    .time_mot_loc
                    .iter()
                    .map(|entry| entry.points.as_slice())
            })
            .collect(),
        ProductBody::Generic(generic) => generic
            .time_mot_loc
            .iter()
            .flat_map(|entries| entries.iter().map(|entry| entry.points.as_slice()))
            .collect(),
    }
}
