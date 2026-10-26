use visualize_kml::analysis::{analyze_document, MarkerKind, MarkerSource};
use visualize_kml::{parse_kml, KmlError};

#[test]
fn parses_point_placemark_metadata_without_fetching_description_resources() {
    let xml = r#"
        <kml xmlns="http://www.opengis.net/kml/2.2">
          <Document>
            <Placemark id="start-marker">
              <name>起点</name>
              <description><![CDATA[起步点 <img src="https://example.invalid/pic.png"/>]]></description>
              <styleUrl>#startStyle</styleUrl>
              <ExtendedData><Data name="Role"><value>start</value></Data></ExtendedData>
              <Point><coordinates>10,20,30</coordinates></Point>
            </Placemark>
            <Placemark>
              <name>Invalid point</name>
              <Point><coordinates>181,20,30</coordinates></Point>
            </Placemark>
            <Placemark>
              <LineString><coordinates>10,20,30 10.01,20.01,40</coordinates></LineString>
            </Placemark>
          </Document>
        </kml>
    "#;

    let document = parse_kml(xml).expect("document with a valid route should parse");
    assert_eq!(document.markers.len(), 1);
    let marker = &document.markers[0];
    assert_eq!(marker.name, "起点");
    assert_eq!(
        marker.description,
        r#"起步点 <img src="https://example.invalid/pic.png"/>"#
    );
    assert_eq!(marker.style_url.as_deref(), Some("#startStyle"));
    assert_eq!(marker.id.as_deref(), Some("start-marker"));
    assert_eq!(
        marker.extended_data.get("Role").map(String::as_str),
        Some("start")
    );
    let analyzed_start = analyze_document(&document)
        .markers
        .into_iter()
        .find(|marker| marker.kind == MarkerKind::Start)
        .expect("explicit start should be recognized");
    assert_eq!(
        analyzed_start
            .kml_metadata
            .as_ref()
            .and_then(|metadata| metadata.style_url.as_deref()),
        Some("#startStyle")
    );
    assert!(
        !document.warnings.is_empty(),
        "invalid marker should be reported"
    );
}

#[test]
fn prefers_explicit_start_and_end_and_infers_highest_from_track() {
    let xml = r#"
        <kml><Document>
          <Placemark><name>Start</name><Point><coordinates>10,20,100</coordinates></Point></Placemark>
          <Placemark><name>Finish</name><Point><coordinates>10.03,20,50</coordinates></Point></Placemark>
          <Placemark><LineString><coordinates>10,20,1 10.01,20,90 10.02,20,90 10.03,20,3</coordinates></LineString></Placemark>
        </Document></kml>
    "#;
    let document = parse_kml(xml).expect("valid tracks and points should parse");
    let markers = analyze_document(&document);

    let start = markers
        .markers
        .iter()
        .find(|marker| marker.kind == MarkerKind::Start)
        .unwrap();
    let end = markers
        .markers
        .iter()
        .find(|marker| marker.kind == MarkerKind::End)
        .unwrap();
    let highest = markers
        .markers
        .iter()
        .find(|marker| marker.kind == MarkerKind::Highest)
        .unwrap();

    assert_eq!(start.source, MarkerSource::Kml);
    assert_eq!(start.point.lon, 10.0);
    assert_eq!(end.source, MarkerSource::Kml);
    assert_eq!(end.point.lon, 10.03);
    assert_eq!(highest.source, MarkerSource::ElevationProfile);
    assert_eq!(
        highest.point.lon, 10.01,
        "ties choose the first track point"
    );
}

#[cfg(not(feature = "terrain-candidates"))]
#[test]
fn infers_start_end_without_unvalidated_terrain_candidates() {
    let xml = r#"
        <kml><Placemark><LineString><coordinates>
          10,20,0 10.01,20,100 10.02,20,0 10.03,20,80 10.04,20,-50 10.05,20,0
        </coordinates></LineString></Placemark></kml>
    "#;
    let document = parse_kml(xml).expect("valid track should parse");
    let markers = analyze_document(&document);

    assert_eq!(
        markers
            .markers
            .iter()
            .find(|marker| marker.kind == MarkerKind::Start)
            .unwrap()
            .source,
        MarkerSource::TrackEndpoint
    );
    assert_eq!(
        markers
            .markers
            .iter()
            .find(|marker| marker.kind == MarkerKind::End)
            .unwrap()
            .source,
        MarkerSource::TrackEndpoint
    );
    assert!(
        !markers.markers.iter().any(|marker| matches!(
            marker.kind,
            MarkerKind::PeakCandidate | MarkerKind::PassCandidate | MarkerKind::ValleyCandidate
        )),
        "local terrain candidates must remain disabled without validation data"
    );

    let noisy_xml = r#"
        <kml><Placemark><LineString><coordinates>
          10,20,0 10.0001,20,4 10.0002,20,1 10.0003,20,5 10.0004,20,0
        </coordinates></LineString></Placemark></kml>
    "#;
    let noisy = analyze_document(&parse_kml(noisy_xml).unwrap());
    assert!(!noisy.markers.iter().any(|marker| matches!(
        marker.kind,
        MarkerKind::PeakCandidate | MarkerKind::PassCandidate | MarkerKind::ValleyCandidate
    )));

    let close_extrema_xml = r#"
        <kml><Placemark><LineString><coordinates>
          10,20,0 10.001,20,100 10.002,20,0 10.003,20,90 10.004,20,-100 10.005,20,0
        </coordinates></LineString></Placemark></kml>
    "#;
    let close_extrema = analyze_document(&parse_kml(close_extrema_xml).unwrap());
    let terrain_candidates: Vec<_> = close_extrema
        .markers
        .iter()
        .filter(|marker| {
            matches!(
                marker.kind,
                MarkerKind::PeakCandidate | MarkerKind::PassCandidate | MarkerKind::ValleyCandidate
            )
        })
        .collect();
    assert!(
        terrain_candidates.is_empty(),
        "terrain candidates must not be emitted before real-track validation"
    );
}

#[test]
fn unnamed_explicit_markers_do_not_receive_generated_names() {
    let xml = r#"
        <kml><Document>
          <Placemark id="start-marker"><Point><coordinates>10,20,10</coordinates></Point></Placemark>
          <Placemark><LineString><coordinates>10,20,10 10.01,20,80</coordinates></LineString></Placemark>
        </Document></kml>
    "#;
    let markers = analyze_document(&parse_kml(xml).expect("valid track should parse"));

    assert!(markers
        .markers
        .iter()
        .filter(|marker| matches!(marker.kind, MarkerKind::Start | MarkerKind::End))
        .all(|marker| marker.name.is_empty()));
    assert!(markers
        .markers
        .iter()
        .filter(|marker| matches!(marker.kind, MarkerKind::Highest | MarkerKind::Lowest))
        .all(|marker| marker.name.is_empty()));
}

#[test]
fn emits_global_highest_and_lowest_without_synthesized_names_or_local_candidates() {
    let xml = r#"
        <kml><Placemark><LineString><coordinates>
          10,20,10 10.001,20,120 10.002,20,30 10.003,20,-40 10.004,20,20
        </coordinates></LineString></Placemark></kml>
    "#;
    let markers = analyze_document(&parse_kml(xml).expect("valid track should parse"));

    let highest = markers
        .markers
        .iter()
        .find(|marker| marker.kind == MarkerKind::Highest)
        .expect("global highest point should be marked");
    let lowest = markers
        .markers
        .iter()
        .find(|marker| marker.kind == MarkerKind::Lowest)
        .expect("global lowest point should be marked");
    assert_eq!(highest.point.alt, 120.0);
    assert_eq!(lowest.point.alt, -40.0);
    assert!(highest.name.is_empty());
    assert!(lowest.name.is_empty());
    #[cfg(not(feature = "terrain-candidates"))]
    assert!(markers.markers.iter().all(|marker| !matches!(
        marker.kind,
        MarkerKind::PeakCandidate | MarkerKind::PassCandidate | MarkerKind::ValleyCandidate
    )));
    assert!(markers.markers.iter().all(|marker| {
        !matches!(marker.kind, MarkerKind::Start | MarkerKind::End) || marker.name.is_empty()
    }));
}

#[test]
fn missing_altitude_suppresses_elevation_markers_and_duplicate_points_are_deduplicated() {
    let xml = r#"
        <kml><Document>
          <Placemark><name>起点</name><Point><coordinates>10,20</coordinates></Point></Placemark>
          <Placemark><name>起点</name><Point><coordinates>10,20</coordinates></Point></Placemark>
          <Placemark><LineString><coordinates>10,20 10.01,20 10.02,20</coordinates></LineString></Placemark>
        </Document></kml>
    "#;
    let document = parse_kml(xml).expect("valid route without altitude should parse");
    let markers = analyze_document(&document);

    assert_eq!(
        markers
            .markers
            .iter()
            .filter(|marker| marker.kind == MarkerKind::Start)
            .count(),
        1
    );
    assert!(!markers.markers.iter().any(|marker| matches!(
        marker.kind,
        MarkerKind::Highest
            | MarkerKind::Lowest
            | MarkerKind::PeakCandidate
            | MarkerKind::PassCandidate
            | MarkerKind::ValleyCandidate
    )));
    assert!(
        markers
            .warnings
            .iter()
            .any(|warning| warning.contains("No valid altitude")),
        "missing elevation data should be explained to the user"
    );
    assert!(document
        .tracks
        .iter()
        .flat_map(|track| &track.segments)
        .flat_map(|segment| &segment.points)
        .all(|point| !point.has_altitude));
}

#[test]
fn point_only_kml_reports_missing_track_data() {
    let xml = r#"<kml><Placemark><name>Start</name><Point><coordinates>10,20,30</coordinates></Point></Placemark></kml>"#;
    assert!(matches!(parse_kml(xml), Err(KmlError::NoTrackData)));
}

#[test]
fn identifies_semantic_markers_from_extended_data_and_style_reference() {
    let xml = r#"
        <kml><Document>
          <Placemark>
            <name>Unnamed point</name>
            <ExtendedData><SchemaData><SimpleData name="landmark_type">summit</SimpleData></SchemaData></ExtendedData>
            <Point><coordinates>10,20,100</coordinates></Point>
          </Placemark>
          <Placemark>
            <name>Unnamed pass</name>
            <styleUrl>#mountain_pass</styleUrl>
            <Point><coordinates>10.01,20,80</coordinates></Point>
          </Placemark>
          <Placemark><Point/></Placemark>
          <Placemark><LineString><coordinates>10,20,0 10.02,20,50</coordinates></LineString></Placemark>
        </Document></kml>
    "#;
    let document = parse_kml(xml).expect("valid track should parse despite an empty marker");
    let analysis = analyze_document(&document);

    assert!(analysis
        .markers
        .iter()
        .any(|marker| marker.kind == MarkerKind::Peak && marker.source == MarkerSource::Kml));
    assert!(analysis
        .markers
        .iter()
        .any(|marker| marker.kind == MarkerKind::Pass && marker.source == MarkerSource::Kml));
    assert!(document
        .warnings
        .iter()
        .any(|warning| warning.contains("Point coordinate")));
}

#[test]
fn sample_kml_provides_explicit_endpoints_without_unvalidated_local_candidates() {
    let document = parse_kml(include_str!("../sample.kml")).expect("sample KML should parse");
    let analysis = analyze_document(&document);
    let start = analysis
        .markers
        .iter()
        .find(|marker| marker.kind == MarkerKind::Start)
        .expect("sample has a start marker");
    let end = analysis
        .markers
        .iter()
        .find(|marker| marker.kind == MarkerKind::End)
        .expect("sample has an end marker");
    assert_eq!(start.source, MarkerSource::Kml);
    assert_eq!(end.source, MarkerSource::Kml);

    let highest_track_altitude = document
        .tracks
        .iter()
        .flat_map(|track| &track.segments)
        .flat_map(|segment| &segment.points)
        .filter_map(|point| point.usable_altitude())
        .fold(f64::NEG_INFINITY, f64::max);
    assert_eq!(
        analysis
            .markers
            .iter()
            .find(|marker| marker.kind == MarkerKind::Highest)
            .expect("sample has an elevation maximum")
            .point
            .alt,
        highest_track_altitude
    );
    #[cfg(not(feature = "terrain-candidates"))]
    assert!(!analysis.markers.iter().any(|marker| matches!(
        marker.kind,
        MarkerKind::PeakCandidate | MarkerKind::PassCandidate | MarkerKind::ValleyCandidate
    )));
}

#[cfg(feature = "terrain-candidates")]
#[test]
fn derives_prominent_peak_candidates_when_the_feature_is_enabled() {
    let xml = r#"
        <kml><Placemark><LineString><coordinates>
          10,20,0 10.01,20,200 10.02,20,0
        </coordinates></LineString></Placemark></kml>
    "#;
    let analysis = analyze_document(&parse_kml(xml).expect("valid track should parse"));
    let peak = analysis
        .markers
        .iter()
        .find(|marker| marker.kind == MarkerKind::PeakCandidate)
        .expect("a prominent local peak should be inferred");

    assert_eq!(peak.source, MarkerSource::ElevationProfile);
    assert_eq!(peak.point.alt, 200.0);
}
