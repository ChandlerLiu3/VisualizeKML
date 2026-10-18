use glam::Vec3;
use visualize_kml::camera::OrbitCamera;
use visualize_kml::geometry::project_document;
use visualize_kml::parse_kml;

#[test]
fn east_west_regression_fixture_projects_all_cardinal_markers_correctly() {
    let document = parse_kml(include_str!("fixtures/coordinate-directions-0.2.2.kml"))
        .expect("coordinate direction fixture should parse");
    let projected = project_document(&document).expect("fixture route should project");
    let marker_position = |name: &str| {
        let marker = document
            .markers
            .iter()
            .find(|marker| marker.name == name)
            .expect("direction marker should exist");
        projected.project_point(&marker.point)
    };

    let east = marker_position("East");
    let west = marker_position("West");
    let north = marker_position("North");
    let south = marker_position("South");
    assert!(
        east[0] > west[0],
        "east must project to greater +X than west"
    );
    assert!(
        (east[2] - west[2]).abs() < f32::EPSILON,
        "east-west displacement must not change northing"
    );
    assert!(
        north[2] > south[2],
        "north must project to greater +Z than south"
    );
    assert!(
        (north[0] - south[0]).abs() < f32::EPSILON,
        "north-south displacement must not change easting"
    );
}

#[test]
fn signed_latitude_directions_are_consistent_in_both_hemispheres() {
    let document = parse_kml(include_str!("fixtures/latitude-hemispheres-0.2.3.kml"))
        .expect("hemisphere regression KML should parse");
    let projected = project_document(&document).expect("hemisphere regression KML should project");
    let north_points = &document.tracks[0].segments[0].points;
    let south_points = &document.tracks[1].segments[0].points;
    let north_start = projected.project_point(&north_points[0]);
    let north = projected.project_point(&north_points[1]);
    let south_start = projected.project_point(&south_points[0]);
    let south = projected.project_point(&south_points[1]);
    let camera = OrbitCamera::new(Vec3::ZERO, 3.2);
    let view_projection = camera.view_projection(1.0);
    let screen_y = |point: [f32; 3]| {
        let clip = view_projection * Vec3::from_array(point).extend(1.0);
        clip.y / clip.w
    };

    assert!(
        north[2] > north_start[2],
        "30°N to 31°N must move north (+Z)"
    );
    assert!(
        screen_y(north) > screen_y(north_start),
        "31°N must appear above 30°N"
    );
    assert!(
        south[2] < south_start[2],
        "30°S to 31°S must move south (-Z)"
    );
    assert!(
        screen_y(south) < screen_y(south_start),
        "31°S must appear below 30°S"
    );
}

#[test]
fn altitude_does_not_change_northing_for_southern_latitudes() {
    let document = parse_kml(
        r#"<kml><Placemark><LineString><coordinates>20,-30,100 20,-30,200</coordinates></LineString></Placemark></kml>"#,
    )
    .expect("altitude-axis regression KML should parse");
    let projected = project_document(&document).expect("altitude-axis KML should project");
    let segment = &document.tracks[0].segments[0];
    let low = projected.project_point(&segment.points[0]);
    let high = projected.project_point(&segment.points[1]);

    assert!((high[0] - low[0]).abs() < f32::EPSILON);
    assert!((high[2] - low[2]).abs() < f32::EPSILON);
    assert!(high[1] > low[1], "altitude must remain on +Y");
}
