use super::shared::FileFilterInput;
use emwin_parser::{
    GeoPoint, ProductBody, bounds_contains, distance_miles as geo_distance_miles, point_in_polygon,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LocationFilter {
    pub(crate) center: Option<GeoPoint>,
    pub(crate) distance_miles: Option<f64>,
}

impl Eq for LocationFilter {}

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

        Ok(Self {
            center,
            distance_miles,
        })
    }

    pub(crate) fn has_constraints(&self) -> bool {
        self.center.is_some()
    }

    pub(crate) fn matches(&self, body: Option<&ProductBody>) -> bool {
        let Some(center) = self.center else {
            return true;
        };
        let Some(body) = body else {
            return false;
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
