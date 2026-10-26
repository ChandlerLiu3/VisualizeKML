use crate::{KmlDocument, TrackMarker, TrackPoint};

/// Minimum elevation prominence before an inferred terrain marker is proposed.
///
/// Terrain inference is experimental and only compiled with the
/// `terrain-candidates` cargo feature.
pub const MIN_TERRAIN_PROMINENCE_METERS: f64 = 30.0;
/// Minimum spacing between two inferred terrain markers.
pub const MIN_TERRAIN_MARKER_DISTANCE_METERS: f64 = 500.0;
const DUPLICATE_MARKER_DISTANCE_METERS: f64 = 5.0;
#[cfg(feature = "terrain-candidates")]
const EXPLICIT_MATCH_DISTANCE_METERS: f64 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    Start,
    End,
    Highest,
    Lowest,
    Peak,
    Pass,
    Valley,
    PeakCandidate,
    PassCandidate,
    ValleyCandidate,
    Other,
}

impl MarkerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "起点",
            Self::End => "终点",
            Self::Highest => "最高点",
            Self::Lowest => "最低点",
            Self::Peak => "峰顶",
            Self::Pass => "垭口",
            Self::Valley => "谷底",
            Self::PeakCandidate => "峰顶候选",
            Self::PassCandidate => "垭口候选",
            Self::ValleyCandidate => "谷底候选",
            Self::Other => "KML 标记",
        }
    }

    #[cfg(feature = "terrain-candidates")]
    fn explicit_kind(self) -> Option<Self> {
        match self {
            Self::Peak => Some(Self::PeakCandidate),
            Self::Pass => Some(Self::PassCandidate),
            Self::Valley => Some(Self::ValleyCandidate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerSource {
    Kml,
    TrackEndpoint,
    ElevationProfile,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzedMarker {
    pub kind: MarkerKind,
    pub source: MarkerSource,
    pub name: String,
    pub point: TrackPoint,
    pub kml_metadata: Option<TrackMarker>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalysisResult {
    pub markers: Vec<AnalyzedMarker>,
    pub warnings: Vec<String>,
}

pub fn analyze_document(document: &KmlDocument) -> AnalysisResult {
    let mut result = AnalysisResult {
        markers: Vec::new(),
        warnings: document.warnings.clone(),
    };

    for marker in &document.markers {
        let kind = classify_marker(marker);
        if matches!(kind, MarkerKind::Start | MarkerKind::End)
            && result.markers.iter().any(|existing| existing.kind == kind)
        {
            result.warnings.push(format!(
                "Multiple explicit {} markers were found; only the first is used",
                kind.label()
            ));
            continue;
        }
        push_unique(
            &mut result.markers,
            AnalyzedMarker {
                kind,
                source: MarkerSource::Kml,
                name: marker.name.trim().to_owned(),
                point: marker.point.clone(),
                kml_metadata: Some(marker.clone()),
            },
            DUPLICATE_MARKER_DISTANCE_METERS,
        );
    }

    let segments: Vec<&[TrackPoint]> = document
        .tracks
        .iter()
        .flat_map(|track| track.segments.iter())
        .map(|segment| segment.points.as_slice())
        .collect();
    let ordered_points: Vec<&TrackPoint> =
        segments.iter().flat_map(|segment| segment.iter()).collect();

    if let Some(first) = ordered_points.first() {
        add_endpoint_if_missing(&mut result.markers, MarkerKind::Start, first);
    }
    if let Some(last) = ordered_points.last() {
        add_endpoint_if_missing(&mut result.markers, MarkerKind::End, last);
    }

    let altitude_points: Vec<&TrackPoint> = ordered_points
        .iter()
        .copied()
        .filter(|point| point.usable_altitude().is_some())
        .collect();

    if let Some(highest) =
        altitude_points
            .iter()
            .copied()
            .reduce(|best, point| if point.alt > best.alt { point } else { best })
    {
        push_unique(
            &mut result.markers,
            AnalyzedMarker {
                kind: MarkerKind::Highest,
                source: MarkerSource::ElevationProfile,
                name: String::new(),
                point: highest.clone(),
                kml_metadata: None,
            },
            DUPLICATE_MARKER_DISTANCE_METERS,
        );

        let lowest = altitude_points
            .iter()
            .copied()
            .reduce(|best, point| if point.alt < best.alt { point } else { best })
            .expect("altitude points are non-empty when the highest point exists");
        push_unique(
            &mut result.markers,
            AnalyzedMarker {
                kind: MarkerKind::Lowest,
                source: MarkerSource::ElevationProfile,
                name: String::new(),
                point: lowest.clone(),
                kml_metadata: None,
            },
            DUPLICATE_MARKER_DISTANCE_METERS,
        );
    } else {
        result.warnings.push(
            "No valid altitude data is available; elevation markers were not generated".to_owned(),
        );
    }

    // Terrain inference stays opt-in until it has been validated against real
    // tracks; the default build only derives markers that are backed by data.
    #[cfg(feature = "terrain-candidates")]
    add_terrain_candidates(&mut result, &segments);

    result
}

#[cfg(feature = "terrain-candidates")]
fn add_terrain_candidates(result: &mut AnalysisResult, segments: &[&[TrackPoint]]) {
    for &segment in segments {
        for run in altitude_runs(segment) {
            add_peak_candidates(result, run);
        }
    }
}

fn add_endpoint_if_missing(
    markers: &mut Vec<AnalyzedMarker>,
    kind: MarkerKind,
    point: &TrackPoint,
) {
    if !markers.iter().any(|marker| marker.kind == kind) {
        push_unique(
            markers,
            AnalyzedMarker {
                kind,
                source: MarkerSource::TrackEndpoint,
                name: String::new(),
                point: point.clone(),
                kml_metadata: None,
            },
            DUPLICATE_MARKER_DISTANCE_METERS,
        );
    }
}

fn classify_marker(marker: &TrackMarker) -> MarkerKind {
    let mut semantic_text = format!(
        "{} {} {}",
        marker.name,
        marker.id.as_deref().unwrap_or_default(),
        marker.style_url.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    for (key, value) in &marker.extended_data {
        semantic_text.push(' ');
        semantic_text.push_str(&key.to_lowercase());
        semantic_text.push(' ');
        semantic_text.push_str(&value.to_lowercase());
    }

    if contains_any(&semantic_text, &["起点", "出发点", "start"]) {
        MarkerKind::Start
    } else if contains_any(&semantic_text, &["终点", "结束点", "finish", "end"]) {
        MarkerKind::End
    } else if contains_any(&semantic_text, &["垭口", "山口", "saddle", "pass"]) {
        MarkerKind::Pass
    } else if contains_any(&semantic_text, &["山峰", "峰顶", "summit", "peak"]) {
        MarkerKind::Peak
    } else if contains_any(&semantic_text, &["谷底", "谷地", "valley"]) {
        MarkerKind::Valley
    } else {
        MarkerKind::Other
    }
}

fn contains_any(text: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| text.contains(term))
}

#[cfg(feature = "terrain-candidates")]
fn altitude_runs(points: &[TrackPoint]) -> Vec<&[TrackPoint]> {
    let mut runs = Vec::new();
    let mut start = None;
    for (index, point) in points.iter().enumerate() {
        if point.usable_altitude().is_some() {
            start.get_or_insert(index);
        } else if let Some(run_start) = start.take() {
            runs.push(&points[run_start..index]);
        }
    }
    if let Some(run_start) = start {
        runs.push(&points[run_start..]);
    }
    runs
}

#[cfg(feature = "terrain-candidates")]
fn add_peak_candidates(result: &mut AnalysisResult, points: &[TrackPoint]) {
    if points.len() < 3 {
        return;
    }

    let elevations: Vec<f64> = points.iter().map(|point| point.alt).collect();
    let extrema = local_extrema(&elevations);
    let peak_indices = prominent_extrema(points, &extrema.peaks, &elevations);
    for index in peak_indices {
        let point = &points[index];
        let kind = MarkerKind::PeakCandidate;
        if has_explicit_near(result, kind, point) {
            continue;
        }
        push_unique(
            &mut result.markers,
            AnalyzedMarker {
                kind,
                source: MarkerSource::ElevationProfile,
                name: String::new(),
                point: point.clone(),
                kml_metadata: None,
            },
            DUPLICATE_MARKER_DISTANCE_METERS,
        );
    }
}

#[cfg(feature = "terrain-candidates")]
#[derive(Default)]
struct Extrema {
    peaks: Vec<usize>,
}

#[cfg(feature = "terrain-candidates")]
fn local_extrema(elevations: &[f64]) -> Extrema {
    let mut extrema = Extrema::default();
    let mut start = 0;

    while start < elevations.len() {
        let mut end = start;
        while end + 1 < elevations.len() && elevations[end + 1] == elevations[start] {
            end += 1;
        }
        if start > 0 && end + 1 < elevations.len() {
            let before = elevations[start - 1];
            let value = elevations[start];
            let after = elevations[end + 1];
            let representative = start + (end - start) / 2;
            if value > before && value > after {
                extrema.peaks.push(representative);
            }
        }
        start = end + 1;
    }

    extrema
}

#[cfg(feature = "terrain-candidates")]
fn prominent_extrema(
    points: &[TrackPoint],
    candidates: &[usize],
    elevations: &[f64],
) -> Vec<usize> {
    let mut ranked: Vec<(usize, f64)> = candidates
        .iter()
        .copied()
        .filter_map(|index| {
            let prominence = prominence_at(index, elevations);
            (prominence >= MIN_TERRAIN_PROMINENCE_METERS).then_some((index, prominence))
        })
        .collect();
    ranked.sort_by(|left, right| right.1.total_cmp(&left.1));

    let mut selected: Vec<usize> = Vec::new();
    for (candidate, _) in ranked {
        if selected.iter().all(|&existing| {
            distance_meters(&points[candidate], &points[existing])
                >= MIN_TERRAIN_MARKER_DISTANCE_METERS
        }) {
            selected.push(candidate);
        }
    }
    selected.sort_unstable();
    selected
}

#[cfg(feature = "terrain-candidates")]
fn prominence_at(index: usize, elevations: &[f64]) -> f64 {
    let left = &elevations[..index];
    let right = &elevations[index + 1..];
    (elevations[index] - left.iter().copied().fold(f64::INFINITY, f64::min))
        .min(elevations[index] - right.iter().copied().fold(f64::INFINITY, f64::min))
}

#[cfg(feature = "terrain-candidates")]
fn has_explicit_near(
    result: &AnalysisResult,
    candidate_kind: MarkerKind,
    point: &TrackPoint,
) -> bool {
    result.markers.iter().any(|marker| {
        marker.source == MarkerSource::Kml
            && marker.kind.explicit_kind() == Some(candidate_kind)
            && distance_meters(&marker.point, point) <= EXPLICIT_MATCH_DISTANCE_METERS
    })
}

fn push_unique(markers: &mut Vec<AnalyzedMarker>, marker: AnalyzedMarker, threshold: f64) {
    if markers.iter().any(|existing| {
        existing.kind == marker.kind && distance_meters(&existing.point, &marker.point) < threshold
    }) {
        return;
    }
    markers.push(marker);
}

pub fn distance_meters(a: &TrackPoint, b: &TrackPoint) -> f64 {
    let lat_a = a.lat.to_radians();
    let lat_b = b.lat.to_radians();
    let delta_lat = (b.lat - a.lat).to_radians();
    let delta_lon = (b.lon - a.lon).to_radians();
    let h = (delta_lat * 0.5).sin().powi(2)
        + lat_a.cos() * lat_b.cos() * (delta_lon * 0.5).sin().powi(2);
    2.0 * 6_378_137.0 * h.sqrt().asin()
}
