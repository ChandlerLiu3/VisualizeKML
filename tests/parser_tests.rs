use visualize_kml::{parse_kml, KmlError};

#[test]
fn parses_sample_gx_track_and_metadata() {
    let document = parse_kml(include_str!("../sample.kml")).expect("sample KML should parse");

    assert_eq!(document.name, "2025-10-14 11:09:28");
    assert_eq!(document.sport, "徒步");
    assert!((document.distance - 9110.99625616947).abs() < 1e-6);
    assert_eq!(document.tracks.len(), 1);

    let points = &document.tracks[0].segments[0].points;
    assert_eq!(points[0].lon, 115.613736);
    assert_eq!(points[0].lat, 39.823807);
    assert_eq!(points[0].alt, 1196.194546);
    assert!(points.len() > 100);
}

#[test]
fn parses_linestring_and_multigeometry_into_separate_segments() {
    let xml = r#"
        <kml xmlns="http://www.opengis.net/kml/2.2">
          <Document><Placemark><MultiGeometry>
            <LineString><coordinates>10,20 11,21,5</coordinates></LineString>
            <LineString><coordinates>12,22,6 13,23,7</coordinates></LineString>
          </MultiGeometry></Placemark></Document>
        </kml>
    "#;

    let document = parse_kml(xml).expect("valid MultiGeometry should parse");
    let segments = &document.tracks[0].segments;

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].points[0].alt, 0.0);
    assert_eq!(segments[0].points[1].alt, 5.0);
    assert_eq!(segments[1].points.len(), 2);
}

#[test]
fn ignores_invalid_coordinates_but_reports_documents_without_tracks() {
    let xml = r#"<kml><Placemark><LineString><coordinates>181,0,1 10,91,2 10,20,NaN 11,21,4 12,22,5</coordinates></LineString></Placemark></kml>"#;
    let document = parse_kml(xml).expect("at least one valid coordinate should be retained");
    assert_eq!(document.tracks[0].segments[0].points.len(), 2);
    assert_eq!(document.tracks[0].segments[0].points[0].alt, 4.0);

    let error = parse_kml("<kml><Document/></kml>").expect_err("missing track must be reported");
    assert!(matches!(error, KmlError::NoTrackData));
    let error = parse_kml(
        "<kml><Placemark><LineString><coordinates>11,21,4</coordinates></LineString></Placemark></kml>",
    )
    .expect_err("a single point cannot form a visible trajectory");
    assert!(matches!(error, KmlError::NoTrackData));
}

#[test]
fn reports_malformed_xml() {
    assert!(matches!(
        parse_kml("<kml><Placemark>"),
        Err(KmlError::Xml(_))
    ));
}

#[test]
fn reports_invalid_numeric_metadata() {
    let xml = r#"<kml><Document><ExtendedData><Data name="Distance"><value>unknown</value></Data></ExtendedData><Placemark><LineString><coordinates>0,0 1,1</coordinates></LineString></Placemark></Document></kml>"#;
    assert!(matches!(parse_kml(xml), Err(KmlError::Xml(_))));
}
