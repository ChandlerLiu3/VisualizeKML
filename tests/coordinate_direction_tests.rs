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
