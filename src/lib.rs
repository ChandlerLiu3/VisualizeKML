pub mod analysis;
pub mod camera;
pub mod geometry;

use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum AltitudeReference {
    Absolute,
    RelativeToGround,
    ClampToGround,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackPoint {
    pub lon: f64,
    pub lat: f64,
    pub alt: f64,
    pub has_altitude: bool,
    pub altitude_reference: AltitudeReference,
}

impl TrackPoint {
    /// Altitude that can be used for display, or `None` when the source does not
    /// define one.
    ///
    /// `clampToGround` points are pinned to terrain, so any altitude value stored
    /// in the file is not meaningful for rendering.
    pub fn usable_altitude(&self) -> Option<f64> {
        if self.altitude_reference == AltitudeReference::ClampToGround {
            return None;
        }
        (self.has_altitude && self.alt.is_finite()).then_some(self.alt)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackSegment {
    pub points: Vec<TrackPoint>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    pub name: Option<String>,
    pub segments: Vec<TrackSegment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackMarker {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub style_url: Option<String>,
    pub extended_data: BTreeMap<String, String>,
    pub point: TrackPoint,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct KmlDocument {
    pub name: String,
    pub sport: String,
    pub distance: f64,
    pub tracks: Vec<Track>,
    pub markers: Vec<TrackMarker>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KmlError {
    Xml(String),
    NoTrackData,
}

impl fmt::Display for KmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(message) => write!(f, "invalid KML XML: {message}"),
            Self::NoTrackData => write!(f, "KML contains no usable LineString or gx:Track data"),
        }
    }
}

impl Error for KmlError {}

#[derive(Debug)]
enum Capture {
    DocumentName,
    PlacemarkName,
    Description,
    StyleUrl,
    DataValue(String),
    Coordinates,
    PointCoordinates,
    GxCoordinate,
    AltitudeMode,
}

#[derive(Debug, Default)]
struct PlacemarkBuilder {
    id: Option<String>,
    name: String,
    description: String,
    style_url: Option<String>,
    extended_data: BTreeMap<String, String>,
    point: Option<TrackPoint>,
    point_altitude_reference: AltitudeReference,
    has_point: bool,
}

pub fn parse_kml(xml: &str) -> Result<KmlDocument, KmlError> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut stack = Vec::<String>::new();
    let mut capture: Option<Capture> = None;
    let mut captured_text = String::new();
    let mut data_name: Option<String> = None;
    let mut document = KmlDocument::default();
    let mut tracks = Vec::<Track>::new();
    let mut current_track = Track::default();
    let mut line_segment = TrackSegment::default();
    let mut gx_segment = TrackSegment::default();
    let mut current_placemark: Option<PlacemarkBuilder> = None;
    let mut line_altitude_reference = AltitudeReference::Unknown;
    let mut gx_altitude_reference = AltitudeReference::Unknown;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = local_name(element.name().as_ref());
                if name == "Placemark" {
                    current_placemark = Some(PlacemarkBuilder {
                        id: element_attribute(&element, b"id")?,
                        ..PlacemarkBuilder::default()
                    });
                } else if name == "LineString" {
                    line_altitude_reference = AltitudeReference::Unknown;
                } else if name == "Track" {
                    gx_altitude_reference = AltitudeReference::Unknown;
                } else if name == "Point" {
                    if let Some(placemark) = &mut current_placemark {
                        placemark.has_point = true;
                        placemark.point_altitude_reference = AltitudeReference::Unknown;
                    }
                }
                if name == "Data" || name == "SimpleData" {
                    data_name = name_attribute(&element)?;
                    if name == "SimpleData" {
                        if let Some(key) = data_name.clone() {
                            capture = Some(Capture::DataValue(key));
                            captured_text.clear();
                        }
                    }
                }
                if name == "name" && stack.last().is_some_and(|parent| parent == "Document") {
                    capture = Some(Capture::DocumentName);
                    captured_text.clear();
                } else if name == "name" && current_placemark.is_some() {
                    capture = Some(Capture::PlacemarkName);
                    captured_text.clear();
                } else if name == "description" && current_placemark.is_some() {
                    capture = Some(Capture::Description);
                    captured_text.clear();
                } else if name == "styleUrl" && current_placemark.is_some() {
                    capture = Some(Capture::StyleUrl);
                    captured_text.clear();
                } else if name == "value" {
                    if let Some(key) = data_name.clone() {
                        capture = Some(Capture::DataValue(key));
                        captured_text.clear();
                    }
                } else if name == "coordinates" && stack.iter().any(|item| item == "LineString") {
                    capture = Some(Capture::Coordinates);
                    captured_text.clear();
                } else if name == "coordinates"
                    && stack.iter().any(|item| item == "Point")
                    && current_placemark.is_some()
                {
                    if let Some(placemark) = &mut current_placemark {
                        placemark.has_point = true;
                    }
                    capture = Some(Capture::PointCoordinates);
                    captured_text.clear();
                } else if name == "coord" && stack.iter().any(|item| item == "Track") {
                    capture = Some(Capture::GxCoordinate);
                    captured_text.clear();
                } else if name == "altitudeMode" {
                    capture = Some(Capture::AltitudeMode);
                    captured_text.clear();
                }
                stack.push(name);
            }
            Ok(Event::Empty(element)) => {
                let name = local_name(element.name().as_ref());
                if name == "Data" || name == "SimpleData" {
                    data_name = name_attribute(&element)?;
                } else if name == "Point" {
                    if let Some(placemark) = &mut current_placemark {
                        placemark.has_point = true;
                    }
                }
            }
            Ok(Event::Text(text)) => {
                if capture.is_some() {
                    captured_text.push_str(
                        &text
                            .unescape()
                            .map_err(|error| KmlError::Xml(error.to_string()))?,
                    );
                }
            }
            Ok(Event::CData(text)) => {
                if capture.is_some() {
                    captured_text.push_str(&String::from_utf8_lossy(text.into_inner().as_ref()));
                }
            }
            Ok(Event::End(element)) => {
                let name = local_name(element.name().as_ref());
                let Some(open_name) = stack.pop() else {
                    return Err(KmlError::Xml(format!("unexpected closing element {name}")));
                };
                if open_name != name {
                    return Err(KmlError::Xml(format!(
                        "closing element {name} does not match {open_name}"
                    )));
                }

                match capture.take() {
                    Some(Capture::DocumentName) if name == "name" => {
                        document.name = captured_text.trim().to_owned();
                    }
                    Some(Capture::PlacemarkName) if name == "name" => {
                        if let Some(placemark) = &mut current_placemark {
                            placemark.name = captured_text.trim().to_owned();
                        }
                    }
                    Some(Capture::Description) if name == "description" => {
                        if let Some(placemark) = &mut current_placemark {
                            placemark.description = captured_text.trim().to_owned();
                        }
                    }
                    Some(Capture::StyleUrl) if name == "styleUrl" => {
                        if let Some(placemark) = &mut current_placemark {
                            placemark.style_url = Some(captured_text.trim().to_owned());
                        }
                    }
                    Some(Capture::DataValue(key)) if name == "value" || name == "SimpleData" => {
                        let value = captured_text.trim();
                        if let Some(placemark) = &mut current_placemark {
                            placemark
                                .extended_data
                                .insert(key.clone(), value.to_owned());
                        }
                        match key.as_str() {
                            "SportTypeName" => document.sport = value.to_owned(),
                            "Distance" if !value.is_empty() => match value.parse::<f64>() {
                                Ok(distance) if distance.is_finite() => {
                                    document.distance = distance;
                                }
                                _ => document.warnings.push(format!(
                                    "Distance 元数据 \"{value}\" 不是有效数字，已忽略"
                                )),
                            },
                            _ => {}
                        }
                    }
                    Some(Capture::Coordinates) if name == "coordinates" => {
                        append_coordinates(&mut line_segment, &captured_text);
                    }
                    Some(Capture::PointCoordinates) if name == "coordinates" => {
                        if let Some(placemark) = &mut current_placemark {
                            let points = parse_coordinates(&captured_text);
                            if let Some(point) = points.into_iter().next() {
                                placemark.point = Some(point);
                            }
                        }
                    }
                    Some(Capture::GxCoordinate) if name == "coord" => {
                        if let Some(point) = parse_gx_coordinate(&captured_text) {
                            append_unique(&mut gx_segment.points, point);
                        }
                    }
                    Some(Capture::AltitudeMode) if name == "altitudeMode" => {
                        let mode = parse_altitude_reference(&captured_text);
                        if stack.iter().any(|item| item == "Point") {
                            if let Some(placemark) = &mut current_placemark {
                                placemark.point_altitude_reference = mode;
                            }
                        } else if stack.iter().any(|item| item == "Track") {
                            gx_altitude_reference = mode;
                        } else if stack.iter().any(|item| item == "LineString") {
                            line_altitude_reference = mode;
                        }
                    }
                    other => {
                        capture = other;
                    }
                }

                if name == "Data" || name == "SimpleData" {
                    data_name = None;
                } else if name == "LineString" {
                    let segment = std::mem::take(&mut line_segment);
                    let mut segment = segment;
                    set_segment_altitude_reference(&mut segment, &line_altitude_reference);
                    if segment.points.len() >= 2 {
                        current_track.segments.push(segment);
                    }
                } else if name == "Track" {
                    let segment = std::mem::take(&mut gx_segment);
                    let mut segment = segment;
                    set_segment_altitude_reference(&mut segment, &gx_altitude_reference);
                    if segment.points.len() >= 2 {
                        current_track.segments.push(segment);
                    }
                } else if name == "Placemark" && !current_track.segments.is_empty() {
                    if current_track.name.is_none() {
                        current_track.name = current_placemark
                            .as_ref()
                            .map(|placemark| placemark.name.trim())
                            .filter(|name| !name.is_empty())
                            .map(str::to_owned);
                    }
                    tracks.push(std::mem::take(&mut current_track));
                }
                if name == "Placemark" {
                    if let Some(placemark) = current_placemark.take() {
                        if let Some(mut point) = placemark.point {
                            point.altitude_reference = placemark.point_altitude_reference;
                            document.markers.push(TrackMarker {
                                id: placemark.id,
                                name: placemark.name,
                                description: placemark.description,
                                style_url: placemark.style_url,
                                extended_data: placemark.extended_data,
                                point,
                            });
                        } else if placemark.has_point {
                            document.warnings.push(format!(
                                "Placemark {:?} has no valid Point coordinate and was skipped",
                                placemark.name
                            ));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(KmlError::Xml(error.to_string())),
        }
    }

    if !stack.is_empty() {
        return Err(KmlError::Xml(format!(
            "unclosed element {}",
            stack.last().expect("stack was checked as non-empty")
        )));
    }
    if line_segment.points.len() >= 2 {
        current_track.segments.push(line_segment);
    }
    if gx_segment.points.len() >= 2 {
        current_track.segments.push(gx_segment);
    }
    if !current_track.segments.is_empty() {
        tracks.push(current_track);
    }
    if tracks.is_empty() {
        return Err(KmlError::NoTrackData);
    }

    document.tracks = tracks;
    Ok(document)
}

pub fn parse_track_points_from_kml(xml: &str) -> Result<Vec<TrackPoint>, KmlError> {
    Ok(parse_kml(xml)?
        .tracks
        .into_iter()
        .flat_map(|track| track.segments)
        .flat_map(|segment| segment.points)
        .collect())
}

pub fn parse_track_metadata(xml: &str) -> Result<(String, String, f64), KmlError> {
    let document = parse_kml(xml)?;
    Ok((document.name, document.sport, document.distance))
}

fn local_name(name: &[u8]) -> String {
    let local = name.rsplit(|byte| *byte == b':').next().unwrap_or(name);
    String::from_utf8_lossy(local).into_owned()
}

fn name_attribute(element: &quick_xml::events::BytesStart<'_>) -> Result<Option<String>, KmlError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| KmlError::Xml(error.to_string()))?;
        if attribute.key.local_name().as_ref() == b"name" {
            let value = attribute
                .unescape_value()
                .map_err(|error| KmlError::Xml(error.to_string()))?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

fn element_attribute(
    element: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, KmlError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| KmlError::Xml(error.to_string()))?;
        if attribute.key.local_name().as_ref() == name {
            let value = attribute
                .unescape_value()
                .map_err(|error| KmlError::Xml(error.to_string()))?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

fn append_coordinates(segment: &mut TrackSegment, text: &str) {
    for point in parse_coordinates(text) {
        append_unique(&mut segment.points, point);
    }
}

fn parse_coordinates(text: &str) -> Vec<TrackPoint> {
    text.split_whitespace()
        .filter_map(|coordinate| parse_coordinate_tuple(coordinate, ','))
        .collect()
}

fn parse_coordinate_tuple(text: &str, separator: char) -> Option<TrackPoint> {
    let mut values = text.trim().split(separator);
    let lon = values.next()?.trim().parse::<f64>().ok()?;
    let lat = values.next()?.trim().parse::<f64>().ok()?;
    let (alt, has_altitude) = match values.next() {
        Some(value) if !value.trim().is_empty() => (value.trim().parse::<f64>().ok()?, true),
        None | Some(_) => (0.0, false),
    };
    if values.next().is_some()
        || !lon.is_finite()
        || !lat.is_finite()
        || !alt.is_finite()
        || !(-180.0..=180.0).contains(&lon)
        || !(-90.0..=90.0).contains(&lat)
    {
        return None;
    }
    Some(TrackPoint {
        lon,
        lat,
        alt,
        has_altitude,
        altitude_reference: AltitudeReference::Unknown,
    })
}

fn parse_gx_coordinate(text: &str) -> Option<TrackPoint> {
    let mut values = text.split_whitespace();
    let lon = values.next()?.parse::<f64>().ok()?;
    let lat = values.next()?.parse::<f64>().ok()?;
    let (alt, has_altitude) = match values.next() {
        Some(value) if !value.trim().is_empty() => (value.parse::<f64>().ok()?, true),
        None | Some(_) => (0.0, false),
    };
    if values.next().is_some()
        || !lon.is_finite()
        || !lat.is_finite()
        || !alt.is_finite()
        || !(-180.0..=180.0).contains(&lon)
        || !(-90.0..=90.0).contains(&lat)
    {
        return None;
    }
    Some(TrackPoint {
        lon,
        lat,
        alt,
        has_altitude,
        altitude_reference: AltitudeReference::Unknown,
    })
}

fn parse_altitude_reference(text: &str) -> AltitudeReference {
    match text.trim() {
        "absolute" => AltitudeReference::Absolute,
        "relativeToGround" => AltitudeReference::RelativeToGround,
        "clampToGround" => AltitudeReference::ClampToGround,
        _ => AltitudeReference::Unknown,
    }
}

fn set_segment_altitude_reference(segment: &mut TrackSegment, reference: &AltitudeReference) {
    for point in &mut segment.points {
        point.altitude_reference = *reference;
    }
}

fn append_unique(points: &mut Vec<TrackPoint>, point: TrackPoint) {
    if points.last() != Some(&point) {
        points.push(point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(alt: f64, has_altitude: bool, reference: AltitudeReference) -> TrackPoint {
        TrackPoint {
            lon: 0.0,
            lat: 0.0,
            alt,
            has_altitude,
            altitude_reference: reference,
        }
    }

    #[test]
    fn usable_altitude_ignores_clamp_to_ground_and_missing_values() {
        assert_eq!(
            point(120.0, true, AltitudeReference::Absolute).usable_altitude(),
            Some(120.0)
        );
        assert_eq!(
            point(120.0, true, AltitudeReference::RelativeToGround).usable_altitude(),
            Some(120.0)
        );
        assert_eq!(
            point(120.0, true, AltitudeReference::ClampToGround).usable_altitude(),
            None
        );
        assert_eq!(
            point(120.0, false, AltitudeReference::Unknown).usable_altitude(),
            None
        );
        assert_eq!(
            point(f64::NAN, true, AltitudeReference::Unknown).usable_altitude(),
            None
        );
    }
}
