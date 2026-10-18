use visualize_kml::geometry::project_document;
use visualize_kml::{parse_kml, parse_track_metadata, parse_track_points_from_kml};

#[test]
fn sample_kml_can_be_parsed_into_points() {
    let xml = include_str!("../sample.kml");
    let points = parse_track_points_from_kml(xml);

    assert!(
        !points.is_empty(),
        "sample KML should yield at least one track point"
    );
    assert_eq!(
        points[0].lon, 115.613736,
        "first point longitude should match sample data"
    );
    assert_eq!(
        points[0].lat, 39.823807,
        "first point latitude should match sample data"
    );
    assert_eq!(
        points[0].alt, 1196.194546,
        "first point altitude should match sample data"
    );
    assert!(
        points.len() >= 2,
        "sample KML should include more than a single point"
    );
}

#[test]
fn sample_kml_metadata_is_extracted() {
    let xml = include_str!("../sample.kml");
    let (name, sport, distance) = parse_track_metadata(xml);

    assert_eq!(
        name, "2025-10-14 11:09:28",
        "document name should be preserved"
    );
    assert_eq!(
        sport, "徒步",
        "sport type should be extracted from ExtendedData"
    );
    assert!(
        (distance - 9110.99625616947).abs() < 1e-6,
        "distance should match sample metadata"
    );
}

#[test]
fn sample_route_preserves_east_west_direction_after_projection() {
    let document = parse_kml(include_str!("../sample.kml")).expect("sample KML should parse");
    let projected = project_document(&document).expect("sample route should project");
    let points: Vec<_> = document
        .tracks
        .iter()
        .flat_map(|track| &track.segments)
        .flat_map(|segment| &segment.points)
        .collect();
    let westernmost = points
        .iter()
        .copied()
        .min_by(|left, right| left.lon.total_cmp(&right.lon))
        .expect("sample should have route points");
    let easternmost = points
        .iter()
        .copied()
        .max_by(|left, right| left.lon.total_cmp(&right.lon))
        .expect("sample should have route points");

    assert!(
        projected.project_point(easternmost)[0] > projected.project_point(westernmost)[0],
        "the sample's easternmost route point must have a greater +X coordinate"
    );
}

#[test]
fn real_sample_route_preserves_signed_latitude_direction() {
    let document = parse_kml(include_str!("../sample.kml")).expect("sample KML should parse");
    let projected = project_document(&document).expect("sample route should project");
    let mut northward_steps = 0;
    let mut southward_steps = 0;

    for segment in document.tracks.iter().flat_map(|track| &track.segments) {
        for pair in segment.points.windows(2) {
            let latitude_delta = pair[1].lat - pair[0].lat;
            if latitude_delta == 0.0 {
                continue;
            }
            let northing_delta =
                projected.project_point(&pair[1])[2] - projected.project_point(&pair[0])[2];
            assert_eq!(
                northing_delta.is_sign_positive(),
                latitude_delta.is_sign_positive(),
                "sample latitude delta must preserve its northing sign"
            );
            if latitude_delta > 0.0 {
                northward_steps += 1;
            } else {
                southward_steps += 1;
            }
        }
    }

    assert!(
        northward_steps > 0,
        "sample should contain northbound steps"
    );
    assert!(
        southward_steps > 0,
        "sample should contain southbound steps"
    );
}
