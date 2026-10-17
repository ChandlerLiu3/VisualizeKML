use visualize_kml::{parse_track_metadata, parse_track_points_from_kml};

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
