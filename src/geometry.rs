use crate::{KmlDocument, TrackPoint, TrackSegment};
use std::error::Error;
use std::fmt;

const EARTH_RADIUS_METERS: f64 = 6_378_137.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedTracks {
    pub segments: Vec<Vec<[f32; 3]>>,
    pub altitudes: Vec<Vec<Option<f64>>>,
    pub altitude_range: Option<(f64, f64)>,
    pub distances: Vec<Vec<f64>>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    projection: LocalProjection,
}

#[derive(Debug, Clone, PartialEq)]
struct LocalProjection {
    origin_lon: f64,
    origin_lat: f64,
    origin_alt: f64,
    origin_lat_radians: f64,
    center: [f64; 3],
    scale: f64,
}

impl ProjectedTracks {
    pub fn project_point(&self, point: &TrackPoint) -> [f32; 3] {
        let delta_lon = (point.lon - self.projection.origin_lon + 180.0).rem_euclid(360.0) - 180.0;
        let local = [
            EARTH_RADIUS_METERS * delta_lon.to_radians() * self.projection.origin_lat_radians.cos(),
            point
                .usable_altitude()
                .map_or(0.0, |altitude| altitude - self.projection.origin_alt),
            EARTH_RADIUS_METERS * (point.lat - self.projection.origin_lat).to_radians(),
        ];
        [
            ((local[0] - self.projection.center[0]) * self.projection.scale) as f32,
            ((local[1] - self.projection.center[1]) * self.projection.scale) as f32,
            ((local[2] - self.projection.center[2]) * self.projection.scale) as f32,
        ]
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GeometryError {
    NoPoints,
    NonFiniteCoordinate,
    CoordinateOutOfRange,
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPoints => write!(f, "trajectory contains no points"),
            Self::NonFiniteCoordinate => write!(f, "trajectory contains a non-finite coordinate"),
            Self::CoordinateOutOfRange => {
                write!(
                    f,
                    "trajectory contains a longitude or latitude outside WGS84 bounds"
                )
            }
        }
    }
}

impl Error for GeometryError {}

pub fn project_document(document: &KmlDocument) -> Result<ProjectedTracks, GeometryError> {
    let segments: Vec<&TrackSegment> = document
        .tracks
        .iter()
        .flat_map(|track| track.segments.iter())
        .collect();
    project_segments(&segments)
}

pub fn project_segments(segments: &[&TrackSegment]) -> Result<ProjectedTracks, GeometryError> {
    let points: Vec<&TrackPoint> = segments
        .iter()
        .flat_map(|segment| segment.points.iter())
        .collect();
    if points.is_empty() {
        return Err(GeometryError::NoPoints);
    }
    if points
        .iter()
        .any(|point| !point.lon.is_finite() || !point.lat.is_finite())
    {
        return Err(GeometryError::NonFiniteCoordinate);
    }
    if points
        .iter()
        .any(|point| !(-180.0..=180.0).contains(&point.lon) || !(-90.0..=90.0).contains(&point.lat))
    {
        return Err(GeometryError::CoordinateOutOfRange);
    }

    let lon_min = points
        .iter()
        .map(|point| point.lon)
        .fold(f64::INFINITY, f64::min);
    let lon_max = points
        .iter()
        .map(|point| point.lon)
        .fold(f64::NEG_INFINITY, f64::max);
    let lat_min = points
        .iter()
        .map(|point| point.lat)
        .fold(f64::INFINITY, f64::min);
    let lat_max = points
        .iter()
        .map(|point| point.lat)
        .fold(f64::NEG_INFINITY, f64::max);
    let altitude_values: Vec<f64> = points
        .iter()
        .filter_map(|point| point.usable_altitude())
        .collect();
    let alt_min = altitude_values
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let alt_max = altitude_values
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    let origin_lon = (lon_min + lon_max) * 0.5;
    let origin_lat = (lat_min + lat_max) * 0.5;
    let origin_alt = if altitude_values.is_empty() {
        0.0
    } else {
        (alt_min + alt_max) * 0.5
    };
    let origin_lat_radians = origin_lat.to_radians();
    let altitude_range = if altitude_values.is_empty() {
        None
    } else {
        Some((alt_min, alt_max))
    };

    let enu_segments: Vec<Vec<[f64; 3]>> = segments
        .iter()
        .map(|segment| {
            segment
                .points
                .iter()
                .map(|point| {
                    let delta_lon = (point.lon - origin_lon + 180.0).rem_euclid(360.0) - 180.0;
                    let east =
                        EARTH_RADIUS_METERS * delta_lon.to_radians() * origin_lat_radians.cos();
                    let north = EARTH_RADIUS_METERS * (point.lat - origin_lat).to_radians();
                    let up = point
                        .usable_altitude()
                        .map_or(0.0, |altitude| altitude - origin_alt);
                    [east, up, north]
                })
                .collect()
        })
        .collect();
    let altitudes: Vec<Vec<Option<f64>>> = segments
        .iter()
        .map(|segment| {
            segment
                .points
                .iter()
                .map(TrackPoint::usable_altitude)
                .collect()
        })
        .collect();

    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for point in enu_segments.iter().flatten() {
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }

    let distances: Vec<Vec<f64>> = enu_segments
        .iter()
        .map(|segment| {
            let mut total = 0.0;
            let mut distances = Vec::with_capacity(segment.len());
            for (index, point) in segment.iter().enumerate() {
                if index > 0 {
                    let previous = segment[index - 1];
                    let delta = [
                        point[0] - previous[0],
                        point[1] - previous[1],
                        point[2] - previous[2],
                    ];
                    total += delta.iter().map(|value| value * value).sum::<f64>().sqrt();
                }
                distances.push(total);
            }
            distances
        })
        .collect();

    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let max_extent = (0..3)
        .map(|axis| max[axis] - min[axis])
        .fold(0.0_f64, f64::max);
    let scale = if max_extent > 0.0 {
        2.0 / max_extent
    } else {
        1.0
    };

    let normalized: Vec<Vec<[f32; 3]>> = enu_segments
        .into_iter()
        .map(|segment| {
            segment
                .into_iter()
                .map(|point| {
                    [
                        ((point[0] - center[0]) * scale) as f32,
                        ((point[1] - center[1]) * scale) as f32,
                        ((point[2] - center[2]) * scale) as f32,
                    ]
                })
                .collect()
        })
        .collect();

    Ok(ProjectedTracks {
        segments: normalized,
        altitudes,
        altitude_range,
        distances,
        bounds_min: [
            ((min[0] - center[0]) * scale) as f32,
            ((min[1] - center[1]) * scale) as f32,
            ((min[2] - center[2]) * scale) as f32,
        ],
        bounds_max: [
            ((max[0] - center[0]) * scale) as f32,
            ((max[1] - center[1]) * scale) as f32,
            ((max[2] - center[2]) * scale) as f32,
        ],
        projection: LocalProjection {
            origin_lon,
            origin_lat,
            origin_alt,
            origin_lat_radians,
            center,
            scale,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enu_projection_centers_and_scales_route() {
        let segments = [TrackSegment {
            points: vec![
                TrackPoint {
                    lon: 0.0,
                    lat: 0.0,
                    alt: 100.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: 0.001,
                    lat: 0.001,
                    alt: 110.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
            ],
        }];
        let projected = project_segments(&[&segments[0]]).expect("points should project");
        let [first, second] = projected.segments[0].as_slice() else {
            panic!("expected two projected points");
        };

        assert!((first[0] + second[0]).abs() < 1e-6);
        assert!((first[1] + second[1]).abs() < 1e-6);
        assert!((first[2] + second[2]).abs() < 1e-6);
        assert!((projected.bounds_max[0] - projected.bounds_min[0]).abs() <= 2.0);
    }

    #[test]
    fn longitude_projects_east_and_latitude_projects_north() {
        let segment = TrackSegment {
            points: vec![
                TrackPoint {
                    lon: 0.0,
                    lat: 0.0,
                    alt: 0.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: 0.001,
                    lat: 0.0,
                    alt: 0.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: -0.001,
                    lat: 0.0,
                    alt: 0.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: 0.0,
                    lat: 0.001,
                    alt: 0.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
            ],
        };
        let projected = project_segments(&[&segment]).expect("valid WGS84 points should project");
        let points = &projected.segments[0];

        assert!(points[1][0] > points[0][0]);
        assert!((points[1][2] - points[0][2]).abs() < 1e-6);
        assert!(points[2][0] < points[0][0]);
        assert!((points[2][2] - points[0][2]).abs() < 1e-6);
        assert!(points[3][2] > points[0][2]);
        assert!((points[3][0] - points[0][0]).abs() < 1e-6);
    }

    #[test]
    fn latitude_direction_and_altitude_axis_are_independent() {
        let segment = TrackSegment {
            points: vec![
                TrackPoint {
                    lon: 0.0,
                    lat: 0.0,
                    alt: 100.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: 0.0,
                    lat: 0.001,
                    alt: 100.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: 0.0,
                    lat: -0.001,
                    alt: 100.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: 0.0,
                    lat: 0.0,
                    alt: 200.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
            ],
        };
        let projected = project_segments(&[&segment]).expect("valid route should project");
        let [origin, north, south, higher] = projected.segments[0].as_slice() else {
            panic!("expected four projected points");
        };

        assert!(
            north[2] > origin[2],
            "increasing latitude must move north (+Z)"
        );
        assert!(
            south[2] < origin[2],
            "decreasing latitude must move south (-Z)"
        );
        assert!((higher[0] - origin[0]).abs() < 1e-6);
        assert!((higher[2] - origin[2]).abs() < 1e-6);
        assert!(
            higher[1] > origin[1],
            "altitude must use the vertical (+Y) axis"
        );
    }

    #[test]
    fn marker_coordinates_use_the_same_local_projection_as_track_vertices() {
        let segment = TrackSegment {
            points: vec![
                TrackPoint {
                    lon: 115.0,
                    lat: 39.0,
                    alt: 100.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: 115.01,
                    lat: 39.01,
                    alt: 150.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
            ],
        };
        let projected = project_segments(&[&segment]).expect("route should project");
        let marker_position = projected.project_point(&segment.points[0]);

        assert_eq!(marker_position, projected.segments[0][0]);
    }

    #[test]
    fn elevation_values_keep_missing_points_and_share_one_global_range() {
        let segments = [
            TrackSegment {
                points: vec![
                    TrackPoint {
                        lon: 0.0,
                        lat: 0.0,
                        alt: 10.0,
                        has_altitude: true,
                        altitude_reference: crate::AltitudeReference::Unknown,
                    },
                    TrackPoint {
                        lon: 0.001,
                        lat: 0.0,
                        alt: 0.0,
                        has_altitude: false,
                        altitude_reference: crate::AltitudeReference::Unknown,
                    },
                ],
            },
            TrackSegment {
                points: vec![TrackPoint {
                    lon: 0.002,
                    lat: 0.0,
                    alt: 30.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Unknown,
                }],
            },
        ];
        let projected =
            project_segments(&[&segments[0], &segments[1]]).expect("tracks should project");

        assert_eq!(projected.altitude_range, Some((10.0, 30.0)));
        assert_eq!(projected.altitudes[0], vec![Some(10.0), None]);
        assert_eq!(projected.altitudes[1], vec![Some(30.0)]);
    }

    #[test]
    fn tracks_without_valid_elevation_have_no_elevation_range() {
        let segment = TrackSegment {
            points: vec![TrackPoint {
                lon: 0.0,
                lat: 0.0,
                alt: 0.0,
                has_altitude: false,
                altitude_reference: crate::AltitudeReference::Unknown,
            }],
        };
        let projected = project_segments(&[&segment]).expect("track should project");

        assert_eq!(projected.altitude_range, None);
        assert_eq!(projected.altitudes, vec![vec![None]]);
    }

    #[test]
    fn non_finite_elevation_is_treated_as_missing_elevation() {
        let segment = TrackSegment {
            points: vec![TrackPoint {
                lon: 0.0,
                lat: 0.0,
                alt: f64::NAN,
                has_altitude: true,
                altitude_reference: crate::AltitudeReference::Unknown,
            }],
        };
        let projected = project_segments(&[&segment]).expect("valid coordinates should project");

        assert_eq!(projected.altitude_range, None);
        assert_eq!(projected.altitudes, vec![vec![None]]);
        assert!(projected.segments[0][0]
            .iter()
            .all(|value| value.is_finite()));
    }

    #[test]
    fn cumulative_route_distances_are_measured_in_meters_per_segment() {
        let segments = [
            TrackSegment {
                points: vec![
                    TrackPoint {
                        lon: 0.0,
                        lat: 0.0,
                        alt: 0.0,
                        has_altitude: false,
                        altitude_reference: crate::AltitudeReference::Unknown,
                    },
                    TrackPoint {
                        lon: 0.001,
                        lat: 0.0,
                        alt: 0.0,
                        has_altitude: false,
                        altitude_reference: crate::AltitudeReference::Unknown,
                    },
                    TrackPoint {
                        lon: 0.002,
                        lat: 0.0,
                        alt: 0.0,
                        has_altitude: false,
                        altitude_reference: crate::AltitudeReference::Unknown,
                    },
                ],
            },
            TrackSegment {
                points: vec![
                    TrackPoint {
                        lon: 1.0,
                        lat: 0.0,
                        alt: 0.0,
                        has_altitude: false,
                        altitude_reference: crate::AltitudeReference::Unknown,
                    },
                    TrackPoint {
                        lon: 1.001,
                        lat: 0.0,
                        alt: 0.0,
                        has_altitude: false,
                        altitude_reference: crate::AltitudeReference::Unknown,
                    },
                ],
            },
        ];
        let projected = project_segments(&[&segments[0], &segments[1]])
            .expect("valid route segments should project");

        assert_eq!(projected.distances.len(), 2);
        assert_eq!(projected.distances[0][0], 0.0);
        assert!(projected.distances[0][1] > 111.0 && projected.distances[0][1] < 112.0);
        assert!(projected.distances[0][2] > 222.0 && projected.distances[0][2] < 223.0);
        assert_eq!(projected.distances[1][0], 0.0);
        assert!(projected.distances[1][1] > 111.0 && projected.distances[1][1] < 112.0);
    }

    #[test]
    fn clamp_to_ground_altitudes_do_not_create_an_elevation_range() {
        let segment = TrackSegment {
            points: vec![
                TrackPoint {
                    lon: 10.0,
                    lat: 20.0,
                    alt: 120.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::ClampToGround,
                },
                TrackPoint {
                    lon: 10.01,
                    lat: 20.0,
                    alt: 620.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::ClampToGround,
                },
            ],
        };
        let projected = project_segments(&[&segment]).expect("clamped route should project");

        assert_eq!(projected.altitude_range, None);
        assert_eq!(projected.altitudes, vec![vec![None, None]]);
        assert!(
            (projected.segments[0][0][1] - projected.segments[0][1][1]).abs() < 1e-6,
            "clamped points must share one ground plane"
        );
    }

    #[test]
    fn markers_without_altitude_share_the_track_vertical_origin() {
        let segment = TrackSegment {
            points: vec![
                TrackPoint {
                    lon: 115.0,
                    lat: 39.0,
                    alt: 0.0,
                    has_altitude: false,
                    altitude_reference: crate::AltitudeReference::Unknown,
                },
                TrackPoint {
                    lon: 115.01,
                    lat: 39.01,
                    alt: 100.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Absolute,
                },
                TrackPoint {
                    lon: 115.02,
                    lat: 39.02,
                    alt: 300.0,
                    has_altitude: true,
                    altitude_reference: crate::AltitudeReference::Absolute,
                },
            ],
        };
        let projected = project_segments(&[&segment]).expect("route should project");
        let marker_position = projected.project_point(&segment.points[0]);

        assert_eq!(marker_position, projected.segments[0][0]);
    }
}
