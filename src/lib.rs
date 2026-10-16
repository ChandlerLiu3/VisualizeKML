#[derive(Debug, Clone, PartialEq)]
pub struct TrackPoint {
    pub lon: f64,
    pub lat: f64,
    pub alt: f64,
}

pub fn parse_track_points_from_kml(xml: &str) -> Vec<TrackPoint> {
    unimplemented!("KML track parser is not implemented yet; this is the expected red step in TDD.")
}

pub fn parse_track_metadata(xml: &str) -> (String, String, f64) {
    unimplemented!("KML metadata parser is not implemented yet; this is the expected red step in TDD.")
}
