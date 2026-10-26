use visualize_kml::{parse_kml, AltitudeReference, KmlError};

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
fn attaches_the_placemark_name_to_its_track() {
    let xml = r#"
        <kml><Document>
          <Placemark>
            <name>Ridge traverse</name>
            <LineString><coordinates>10,20,0 10.01,20,1</coordinates></LineString>
          </Placemark>
          <Placemark>
            <LineString><coordinates>11,21,0 11.01,21,1</coordinates></LineString>
          </Placemark>
        </Document></kml>
    "#;

    let document = parse_kml(xml).expect("named tracks should parse");
    assert_eq!(document.tracks.len(), 2);
    assert_eq!(
        document.tracks[0].name.as_deref(),
        Some("Ridge traverse"),
        "the track should keep the placemark name"
    );
    assert_eq!(
        document.tracks[1].name, None,
        "unnamed placemarks must not invent a track name"
    );
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
fn invalid_distance_metadata_is_a_warning_instead_of_a_parse_error() {
    let xml = r#"<kml><Document><ExtendedData><Data name="Distance"><value>unknown</value></Data></ExtendedData><Placemark><LineString><coordinates>0,0 1,1</coordinates></LineString></Placemark></Document></kml>"#;
    let document = parse_kml(xml).expect("a bad Distance value must not hide the track");

    assert_eq!(document.distance, 0.0);
    assert!(document
        .warnings
        .iter()
        .any(|warning| warning.contains("Distance")));
}

#[test]
fn preserves_altitude_reference_for_tracks_and_point_markers() {
    let xml = r#"
        <kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
          <Placemark>
            <LineString>
              <altitudeMode>absolute</altitudeMode>
              <coordinates>0,0,10 0.001,0,20</coordinates>
            </LineString>
          </Placemark>
          <Placemark>
            <Point>
              <altitudeMode>relativeToGround</altitudeMode>
              <coordinates>0,0,15</coordinates>
            </Point>
          </Placemark>
          <Placemark>
            <Point>
              <altitudeMode>clampToGround</altitudeMode>
              <coordinates>0,0,0</coordinates>
            </Point>
          </Placemark>
          <Placemark>
            <gx:Track>
              <gx:altitudeMode>absolute</gx:altitudeMode>
              <gx:coord>0 0 1</gx:coord>
              <gx:coord>0.001 0 2</gx:coord>
            </gx:Track>
          </Placemark>
        </kml>
    "#;
    let document = parse_kml(xml).expect("all geometries should parse");

    assert!(document.tracks[0].segments[0]
        .points
        .iter()
        .all(|point| point.altitude_reference == AltitudeReference::Absolute));
    assert_eq!(
        document.markers[0].point.altitude_reference,
        AltitudeReference::RelativeToGround
    );
    assert_eq!(
        document.markers[1].point.altitude_reference,
        AltitudeReference::ClampToGround
    );
    assert!(document.tracks[1].segments[0]
        .points
        .iter()
        .all(|point| point.altitude_reference == AltitudeReference::Absolute));
}

#[test]
fn leaves_unspecified_altitude_reference_unknown() {
    let xml = r#"<kml><Placemark><LineString><coordinates>0,0,10 0.001,0,20</coordinates></LineString></Placemark></kml>"#;
    let document = parse_kml(xml).expect("valid track should parse");

    assert!(document.tracks[0].segments[0]
        .points
        .iter()
        .all(|point| point.altitude_reference == AltitudeReference::Unknown));
}
