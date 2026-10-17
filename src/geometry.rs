use crate::{KmlDocument, TrackPoint, TrackSegment};
use std::error::Error;
use std::fmt;

const EARTH_RADIUS_METERS: f64 = 6_378_137.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedTracks {
    pub segments: Vec<Vec<[f32; 3]>>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
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
        .any(|point| !point.lon.is_finite() || !point.lat.is_finite() || !point.alt.is_finite())
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
    let alt_min = points
        .iter()
        .map(|point| point.alt)
        .fold(f64::INFINITY, f64::min);
    let alt_max = points
        .iter()
        .map(|point| point.alt)
        .fold(f64::NEG_INFINITY, f64::max);

    let origin_lon = (lon_min + lon_max) * 0.5;
    let origin_lat = (lat_min + lat_max) * 0.5;
    let origin_alt = (alt_min + alt_max) * 0.5;
    let origin_lat_radians = origin_lat.to_radians();

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
                    [east, point.alt - origin_alt, north]
                })
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
                },
                TrackPoint {
                    lon: 0.001,
                    lat: 0.001,
                    alt: 110.0,
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
                },
                TrackPoint {
                    lon: 0.001,
                    lat: 0.0,
                    alt: 0.0,
                },
                TrackPoint {
                    lon: 0.0,
                    lat: 0.001,
                    alt: 0.0,
                },
            ],
        };
        let projected = project_segments(&[&segment]).expect("valid WGS84 points should project");
        let points = &projected.segments[0];

        assert!(points[1][0] > points[0][0]);
        assert!((points[1][2] - points[0][2]).abs() < 1e-6);
        assert!(points[2][2] > points[0][2]);
        assert!((points[2][0] - points[0][0]).abs() < 1e-6);
    }
}
