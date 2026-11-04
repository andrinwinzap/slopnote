// Imports
use crate::penpath::PenPath;
use crate::shapes::{Ellipse, Line, Polygon, Rectangle, Shape};
use p2d::glamx::DAffine2;
use p2d::math::Vector2;
use p2d::shape::Cuboid;

/// The number of points the stroke is resampled to before analyzing it.
const N_RESAMPLE: usize = 96;
/// The minimum number of flattened input points needed to attempt a recognition.
const MIN_INPUT_POINTS: usize = 8;
/// The radius (in surface coordinates) the pen may wobble around while being held still.
///
/// The brush uses it to detect the pen being held still at the end of a stroke, which triggers the recognition,
/// and the recognition uses it to discard the jitter of the resting pen.
pub const HOLD_RADIUS_SURFACE: f64 = 6.0;
/// The minimum extent (in surface coordinates) of the stroke bounds on its larger axis needed to attempt a recognition.
const MIN_EXTENT_SURFACE: f64 = 10.0;
/// The maximum gap between the endpoints relative to the perimeter for the stroke to be considered closed.
const CLOSED_GAP_PERIMETER_RATIO: f64 = 0.15;
/// The length (in surface coordinates) up to which a stroke is judged as strictly as a short line:
/// strokes this short are as often parts of letters.
const LINE_SHORT_SURFACE: f64 = 60.0;
/// The length (in surface coordinates) from which a stroke is judged as forgivingly as a long line:
/// the longer a line, the harder it is to draw straight, and the less it can be anything else.
const LINE_LONG_SURFACE: f64 = 200.0;
/// The maximum deviation of the points from the line relative to its length, for a short line.
const LINE_SHORT_MAX_DEV_RATIO: f64 = 0.055;
/// The maximum deviation of the points from the line relative to its length, for a long line.
const LINE_LONG_MAX_DEV_RATIO: f64 = 0.08;
/// How much longer than the distance between its ends a short line can be, relative to that distance.
const LINE_SHORT_MAX_LEN_EXCESS_RATIO: f64 = 0.10;
/// How much longer than the distance between its ends a long line can be, relative to that distance.
const LINE_LONG_MAX_LEN_EXCESS_RATIO: f64 = 0.18;
/// How much of a long line is left out at each end, relative to its length: pens land and lift with
/// a little hook, which says nothing about whether the stroke is straight.
const LINE_LONG_HOOK_RATIO: f64 = 0.06;
/// The distance (in surface coordinates) between the points the length of a long line is measured
/// along, so that the tremor of the hand on a slowly drawn line does not add up to any length.
const LINE_LENGTH_SPACING_SURFACE: f64 = 6.0;
/// The deviation from the line (in surface coordinates) that is tolerated regardless of the length.
///
/// The tolerances relative to the length alone are too strict for short lines like fraction bars,
/// where the jitter of the hand is large compared to their length.
const LINE_MIN_DEV_TOLERANCE_SURFACE: f64 = 2.5;
/// The excess of the stroke length over the distance between its ends (in surface coordinates)
/// that is tolerated for a line regardless of its length.
const LINE_MIN_LEN_EXCESS_TOLERANCE_SURFACE: f64 = 4.0;
/// How many times as long a stroke no larger than [HOLD_SHORT_SURFACE] has to be held still
/// for it to be recognized, since it is as likely a part of a letter written slowly.
const HOLD_SHORT_FACTOR: f64 = 3.0;
/// The size (in surface coordinates) up to which a stroke has to be held the longest.
const HOLD_SHORT_SURFACE: f64 = 40.0;
/// The size (in surface coordinates) from which a stroke only has to be held for the set duration.
const HOLD_LONG_SURFACE: f64 = 120.0;
/// The window (in resampled points) used when measuring the turn angle at a point.
const TURN_WINDOW: usize = 3;
/// The minimum fraction of the total turning that must be concentrated in high-turn points
/// for the stroke to be treated as a polygon rather than a smooth curve.
const CORNERNESS_POLYGON_THRESHOLD: f64 = 0.5;
/// The minimum turn angle (radians) for a point to be a corner candidate.
const CORNER_MIN_TURN: f64 = 25.0 * std::f64::consts::PI / 180.0;
/// The minimum separation between two corners, in resampled points.
const CORNER_MIN_SEPARATION: usize = N_RESAMPLE / 12;
/// The maximum mean distance of the stroke points to the polygon outline,
/// relative to the bounds diagonal, for a polygon to be accepted.
const POLYGON_MAX_MEAN_DEV_RATIO: f64 = 0.025;
/// The maximum distance of a single stroke point to the polygon outline,
/// relative to the bounds diagonal, for a polygon to be accepted.
const POLYGON_MAX_DEV_RATIO: f64 = 0.08;
/// The maximum mean of the normalized ellipse fit residuals for an ellipse to be accepted.
const ELLIPSE_MAX_MEAN_RESIDUAL: f64 = 0.10;
/// The maximum single normalized ellipse fit residual for an ellipse to be accepted.
const ELLIPSE_MAX_RESIDUAL: f64 = 0.25;
/// The minimum ratio between the minor and major ellipse radii for the ellipse to be snapped to a circle.
const CIRCLE_RADII_RATIO: f64 = 0.75;
/// The maximum deviation (radians) of a corner angle from a right angle for a quadrilateral to become a rectangle.
const RECT_MAX_ANGLE_DEV: f64 = 30.0 * std::f64::consts::PI / 180.0;
/// The maximum rotation angle (radians) below which a recognized rectangle is snapped to be axis-aligned.
const RECT_AXIS_SNAP_ANGLE: f64 = 5.0 * std::f64::consts::PI / 180.0;
/// The maximum number of polygon corners that are recognized.
const POLYGON_MAX_CORNERS: usize = 6;

/// Attempt to recognize a geometric shape from a drawn pen path.
///
/// Returns the recognized shape when the path resembles one closely enough, `None` otherwise.
///
/// Recognized are: lines, ellipses and circles, rectangles (possibly rotated)
/// and closed polygons with up to 6 corners (triangles, quadrilaterals, ..).
///
/// `zoom` is the zoom of the view the path was drawn in. It converts the tolerances for the jitter of the hand,
/// which are defined in surface coordinates, to document coordinates.
pub fn recognize_shape(path: &PenPath, zoom: f64) -> Option<Shape> {
    let input_points = flattened_points(path);
    if input_points.len() < MIN_INPUT_POINTS {
        return None;
    }
    let input_points = collapse_resting_tail(input_points, HOLD_RADIUS_SURFACE / zoom);

    let (min, max) = points_bounds(&input_points);
    let extents = max - min;
    if extents.x.max(extents.y) < MIN_EXTENT_SURFACE / zoom {
        return None;
    }
    let bounds_diag = extents.length();

    let perimeter = polyline_len(&input_points);
    if perimeter < f64::EPSILON {
        return None;
    }

    let gap = (*input_points.last().unwrap() - *input_points.first().unwrap()).length();
    let closed = gap < CLOSED_GAP_PERIMETER_RATIO * perimeter;

    if closed {
        recognize_closed(&input_points, bounds_diag)
    } else {
        recognize_line(&input_points, perimeter, zoom)
    }
}

/// How many times the set duration the pen has to be held still on the given stroke for it to be
/// recognized.
///
/// A slowly written letter pauses for about as long as a line is held, and its strokes look like
/// short lines. Long strokes are hardly ever letters, so only the short ones have to wait longer.
pub fn hold_factor(path: &PenPath, zoom: f64) -> f64 {
    let points = flattened_points(path);
    if points.is_empty() {
        return HOLD_SHORT_FACTOR;
    }
    let (min, max) = points_bounds(&points);
    let extents = max - min;
    let size = extents.x.max(extents.y) * zoom;
    let long =
        ((size - HOLD_SHORT_SURFACE) / (HOLD_LONG_SURFACE - HOLD_SHORT_SURFACE)).clamp(0.0, 1.0);

    HOLD_SHORT_FACTOR + (1.0 - HOLD_SHORT_FACTOR) * long
}

/// The mean width of the stroke as it was drawn, for the width of the shape that replaces it.
///
/// `width_for_pressure` is the width a stroke has at a given pen pressure, as its style draws it.
/// The width is averaged, and not the pressure, since the two only agree for a curve that is a
/// straight line: for any other, a stroke drawn with the mean pressure is not as wide as the
/// stroke was on average.
///
/// Weighted by length, so that what counts is how wide the stroke was along the way and not how
/// slowly it moved: a slow stretch reports many more events than a quick one covering the same
/// distance. The resting tail at the end of the stroke, where the pen is held still to trigger the
/// recognition, is left out for the same reason - it reports for as long as the hold lasts.
///
/// `None` for a path without elements.
pub fn mean_width(
    path: &PenPath,
    zoom: f64,
    width_for_pressure: impl Fn(f64) -> f64,
) -> Option<f64> {
    let mut elements = vec![path.start];
    elements.extend(path.segments.iter().map(|segment| segment.end()));
    let last_pos = elements.last()?.pos;
    let radius = HOLD_RADIUS_SURFACE / zoom;

    // Keep the element that moves into the hold, and nothing that came to rest after it. A stroke
    // that never leaves the hold radius has no tail to speak of, so all of it is used.
    let kept = elements
        .iter()
        .rposition(|element| (element.pos - last_pos).length() > radius)
        .map_or(elements.len(), |i| (i + 2).min(elements.len()));
    let elements = &elements[..kept];

    // A stroke is drawn with the width changing steadily between two elements, so the mean along
    // it is that of the widths at both ends.
    let (mut weighted, mut length) = (0.0, 0.0);
    for pair in elements.windows(2) {
        let segment = (pair[1].pos - pair[0].pos).length();
        let width =
            (width_for_pressure(pair[0].pressure) + width_for_pressure(pair[1].pressure)) * 0.5;
        weighted += segment * width;
        length += segment;
    }
    if length > f64::EPSILON {
        return Some(weighted / length);
    }

    // Nothing moved, so there is nothing to weigh by.
    Some(
        elements
            .iter()
            .map(|element| width_for_pressure(element.pressure))
            .sum::<f64>()
            / elements.len() as f64,
    )
}

/// Extract the flattened points of the pen path.
fn flattened_points(path: &PenPath) -> Vec<Vector2> {
    let mut points = Vec::new();

    for el in path.to_kurbo_flattened(0.25).elements() {
        match el {
            kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => {
                points.push(Vector2::new(p.x, p.y));
            }
            _ => {}
        }
    }

    points.dedup_by(|a, b| (*a - *b).length() < f64::EPSILON);
    points
}

/// Collapse the points at the end of the stroke where the pen rested, all within `radius` of the last point,
/// into the last point.
///
/// The recognition is triggered by holding the pen still at the end of the stroke. The jitter of the resting pen
/// inflates the stroke length, and on short strokes makes up a large enough part of it to fail the recognition.
fn collapse_resting_tail(mut points: Vec<Vector2>, radius: f64) -> Vec<Vector2> {
    let Some(&last) = points.last() else {
        return points;
    };
    let rest_start = points
        .iter()
        .rposition(|p| (*p - last).length() > radius)
        .map_or(0, |i| i + 1);

    points.truncate(rest_start);
    points.push(last);
    points
}

/// The min/max corners of the bounds of the given points.
fn points_bounds(points: &[Vector2]) -> (Vector2, Vector2) {
    let mut min = points[0];
    let mut max = points[0];

    for p in points {
        min = min.min(*p);
        max = max.max(*p);
    }

    (min, max)
}

/// The total length of the polyline through the given points.
fn polyline_len(points: &[Vector2]) -> f64 {
    points.windows(2).map(|w| (w[1] - w[0]).length()).sum()
}

/// Resample the polyline to `n` points with equal arc-length spacing.
///
/// When `wrap` is true the polyline is treated as a closed loop
/// (an implicit closing segment from the last to the first point is added).
fn resample(points: &[Vector2], n: usize, wrap: bool) -> Vec<Vector2> {
    let mut polyline = points.to_vec();
    if wrap {
        polyline.push(points[0]);
    }

    let total_len = polyline_len(&polyline);
    let interval = total_len / n as f64;

    let mut resampled = Vec::with_capacity(n);
    resampled.push(polyline[0]);

    let mut dist_accum = 0.0;
    let mut prev = polyline[0];
    let mut i = 1;

    while i < polyline.len() && resampled.len() < n {
        let seg_len = (polyline[i] - prev).length();

        if dist_accum + seg_len >= interval && seg_len > 0.0 {
            let t = (interval - dist_accum) / seg_len;
            let new_point = prev + (polyline[i] - prev) * t;
            resampled.push(new_point);
            prev = new_point;
            dist_accum = 0.0;
        } else {
            dist_accum += seg_len;
            prev = polyline[i];
            i += 1;
        }
    }

    while resampled.len() < n {
        resampled.push(*polyline.last().unwrap());
    }

    resampled
}

/// Recognize an open stroke as a line.
///
/// Short strokes have to be nearly straight, since they could as well be parts of letters. The
/// longer a stroke, the more forgiving this gets: the hooks at its ends are left out, so that they
/// do not tilt the line it is measured from, and it may wander further from that line.
fn recognize_line(points: &[Vector2], perimeter: f64, zoom: f64) -> Option<Shape> {
    let first = *points.first().unwrap();
    let last = *points.last().unwrap();
    let long = (((last - first).length() * zoom - LINE_SHORT_SURFACE)
        / (LINE_LONG_SURFACE - LINE_SHORT_SURFACE))
        .clamp(0.0, 1.0);
    let lerp = |short: f64, long_value: f64| short + (long_value - short) * long;

    let resampled = resample(points, N_RESAMPLE, false);
    let hook = (resampled.len() as f64 * LINE_LONG_HOOK_RATIO * long).round() as usize;
    let body = &resampled[hook..resampled.len() - hook];
    let (body_first, body_last) = (*body.first().unwrap(), *body.last().unwrap());
    let span = (body_last - body_first).length();
    if span < f64::EPSILON {
        return None;
    }

    // On long strokes measured over points some distance apart, so that the tremor of the hand on a
    // slowly drawn line does not add up to any length. A stroke that turns back still does.
    let spacing = lerp(1.0, LINE_LENGTH_SPACING_SURFACE) / zoom;
    let spaced = resample(
        body,
        ((perimeter / spacing).round() as usize).clamp(2, N_RESAMPLE),
        false,
    );
    let len_excess_tolerance = (lerp(
        LINE_SHORT_MAX_LEN_EXCESS_RATIO,
        LINE_LONG_MAX_LEN_EXCESS_RATIO,
    ) * span)
        .max(LINE_MIN_LEN_EXCESS_TOLERANCE_SURFACE / zoom);
    if polyline_len(&spaced) - span > len_excess_tolerance {
        return None;
    }

    let direction = (body_last - body_first) / span;
    let max_dev = body
        .iter()
        .map(|p| direction.perp_dot(*p - body_first).abs())
        .fold(0.0, f64::max);
    let dev_tolerance = (lerp(LINE_SHORT_MAX_DEV_RATIO, LINE_LONG_MAX_DEV_RATIO) * span)
        .max(LINE_MIN_DEV_TOLERANCE_SURFACE / zoom);
    if max_dev > dev_tolerance {
        return None;
    }

    // The line reaches as far as the stroke did
    let on_line = |p: Vector2| body_first + direction * direction.dot(p - body_first);
    Some(Shape::Line(Line {
        start: on_line(first),
        end: on_line(last),
    }))
}

/// Smooth a closed resampled loop with a cyclic three-point moving average.
///
/// Suppresses input noise before measuring turn angles while mostly preserving corners.
fn smooth_loop(points: &[Vector2], iterations: usize) -> Vec<Vector2> {
    let n = points.len();
    let mut smoothed = points.to_vec();

    for _ in 0..iterations {
        let prev = smoothed.clone();
        for i in 0..n {
            smoothed[i] = (prev[(i + n - 1) % n] + prev[i] * 2.0 + prev[(i + 1) % n]) * 0.25;
        }
    }

    smoothed
}

/// Recognize a closed stroke as an ellipse, rectangle or polygon.
fn recognize_closed(points: &[Vector2], bounds_diag: f64) -> Option<Shape> {
    let resampled = resample(points, N_RESAMPLE, true);
    let smoothed = smooth_loop(&resampled, 2);
    let turns = loop_turn_angles(&smoothed);

    let total_turning: f64 = turns.iter().sum();
    if total_turning < f64::EPSILON {
        return None;
    }
    let mean_turn = total_turning / turns.len() as f64;

    // The fraction of the total turning concentrated in high-turn samples.
    // Near 0 for smooth curves (ellipses), near 1 for polygons.
    let cornerness: f64 =
        turns.iter().filter(|&&t| t > 1.5 * mean_turn).sum::<f64>() / total_turning;

    if cornerness < CORNERNESS_POLYGON_THRESHOLD {
        recognize_ellipse(&resampled)
    } else {
        let corners = detect_corners(&resampled, &turns, mean_turn);

        (3..=POLYGON_MAX_CORNERS)
            .contains(&corners.len())
            .then(|| recognize_polygon(&resampled, &corners, bounds_diag))
            .flatten()
            // Flat ellipses concentrate their turning at the high-curvature ends of the
            // major axis and can end up here, so fall back to the ellipse fit.
            .or_else(|| recognize_ellipse(&resampled))
    }
}

/// The turn angles at each point of a closed resampled loop.
///
/// The turn angle at a point is the angle between the chords leading to and away from it,
/// measured over a window of [`TURN_WINDOW`] points.
fn loop_turn_angles(points: &[Vector2]) -> Vec<f64> {
    let n = points.len();

    (0..n)
        .map(|i| {
            let prev = points[(i + n - TURN_WINDOW) % n];
            let next = points[(i + TURN_WINDOW) % n];
            let v_in = points[i] - prev;
            let v_out = next - points[i];

            if v_in.length() < f64::EPSILON || v_out.length() < f64::EPSILON {
                0.0
            } else {
                v_in.angle_to(v_out).abs()
            }
        })
        .collect()
}

/// Detect the corner indices of a closed resampled loop from its turn angles.
///
/// Corners are local turning maxima above a threshold, with clusters of
/// adjacent high-turn points merged into a single corner.
fn detect_corners(points: &[Vector2], turns: &[f64], mean_turn: f64) -> Vec<Vector2> {
    let n = turns.len();
    let threshold = (1.8 * mean_turn).max(CORNER_MIN_TURN);

    let mut candidates = (0..n).filter(|&i| turns[i] > threshold).collect::<Vec<_>>();
    // Sort by turn angle, strongest first, then greedily suppress neighbors.
    candidates.sort_by(|&a, &b| turns[b].total_cmp(&turns[a]));

    let mut corners: Vec<usize> = Vec::new();
    for i in candidates {
        let cyclic_dist = |a: usize, b: usize| {
            let d = a.abs_diff(b);
            d.min(n - d)
        };
        if corners
            .iter()
            .all(|&c| cyclic_dist(c, i) >= CORNER_MIN_SEPARATION)
        {
            corners.push(i);
        }
    }

    corners.sort_unstable();
    corners.into_iter().map(|i| points[i]).collect()
}

/// Recognize a closed smooth stroke as an ellipse (or circle) by fitting via its second moments.
fn recognize_ellipse(points: &[Vector2]) -> Option<Shape> {
    let n = points.len() as f64;
    let center = points.iter().copied().sum::<Vector2>() / n;

    // Covariance matrix of the centered points.
    let (mut cxx, mut cxy, mut cyy) = (0.0, 0.0, 0.0);
    for p in points {
        let d = *p - center;
        cxx += d.x * d.x;
        cxy += d.x * d.y;
        cyy += d.y * d.y;
    }
    cxx /= n;
    cxy /= n;
    cyy /= n;

    // Eigenvalues/-vectors of the symmetric covariance matrix.
    let trace_half = (cxx + cyy) * 0.5;
    let det = cxx * cyy - cxy * cxy;
    let discr = (trace_half * trace_half - det).max(0.0).sqrt();
    let (lambda1, lambda2) = (trace_half + discr, trace_half - discr);
    if lambda2 <= f64::EPSILON {
        return None;
    }

    let major_axis = if cxy.abs() > f64::EPSILON {
        Vector2::new(lambda1 - cyy, cxy).normalize()
    } else if cxx >= cyy {
        Vector2::X
    } else {
        Vector2::Y
    };
    let minor_axis = Vector2::new(-major_axis.y, major_axis.x);

    // For points distributed on an ellipse outline, the variance along an axis is ~half the
    // squared semi-axis.
    let mut a = (2.0 * lambda1).sqrt();
    let mut b = (2.0 * lambda2).sqrt();

    let normalized_radii = points
        .iter()
        .map(|p| {
            let d = *p - center;
            let u = d.dot(major_axis) / a;
            let v = d.dot(minor_axis) / b;
            (u * u + v * v).sqrt()
        })
        .collect::<Vec<f64>>();

    // Correct the radii so that the mean normalized radius is 1.
    let scale = normalized_radii.iter().sum::<f64>() / n;
    if scale < f64::EPSILON {
        return None;
    }
    a *= scale;
    b *= scale;

    let mean_residual = normalized_radii
        .iter()
        .map(|r| (r / scale - 1.0).abs())
        .sum::<f64>()
        / n;
    let max_residual = normalized_radii
        .iter()
        .map(|r| (r / scale - 1.0).abs())
        .fold(0.0, f64::max);

    if mean_residual > ELLIPSE_MAX_MEAN_RESIDUAL || max_residual > ELLIPSE_MAX_RESIDUAL {
        return None;
    }

    let (radii, angle) = if b / a > CIRCLE_RADII_RATIO {
        // Close to a circle, snap to one.
        let r = (a + b) * 0.5;
        (Vector2::new(r, r), 0.0)
    } else {
        (Vector2::new(a, b), Vector2::X.angle_to(major_axis))
    };

    Some(Shape::Ellipse(Ellipse {
        radii,
        affine: DAffine2::from_angle_translation(angle, center),
    }))
}

/// Recognize a closed stroke with detected corners as a rectangle or polygon.
fn recognize_polygon(
    resampled: &[Vector2],
    corners: &[Vector2],
    bounds_diag: f64,
) -> Option<Shape> {
    // Verify that the stroke stays close to the polygon outline through the corners.
    let mut dev_sum = 0.0;
    let mut dev_max: f64 = 0.0;
    for p in resampled {
        let dist = (0..corners.len())
            .map(|i| dist_to_segment(*p, corners[i], corners[(i + 1) % corners.len()]))
            .fold(f64::MAX, f64::min);
        dev_sum += dist;
        dev_max = dev_max.max(dist);
    }
    let dev_mean = dev_sum / resampled.len() as f64;

    if dev_mean > POLYGON_MAX_MEAN_DEV_RATIO * bounds_diag
        || dev_max > POLYGON_MAX_DEV_RATIO * bounds_diag
    {
        return None;
    }

    if corners.len() == 4
        && let Some(rect) = try_rectangle(corners)
    {
        return Some(Shape::Rectangle(rect));
    }

    Some(Shape::Polygon(Polygon {
        start: corners[0],
        path: corners[1..].to_vec(),
    }))
}

/// The distance of a point to a line segment.
fn dist_to_segment(p: Vector2, a: Vector2, b: Vector2) -> f64 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < f64::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Try to build a rectangle out of four corners.
///
/// Succeeds when all corner angles are close to right angles.
/// The rectangle orientation is the circular mean of the edge directions,
/// snapped to be axis-aligned when it is close to it.
fn try_rectangle(corners: &[Vector2]) -> Option<Rectangle> {
    debug_assert_eq!(corners.len(), 4);

    // All corner angles must be close to right angles.
    for i in 0..4 {
        let prev = corners[(i + 3) % 4];
        let next = corners[(i + 1) % 4];
        let v_in = corners[i] - prev;
        let v_out = next - corners[i];

        if v_in.length() < f64::EPSILON || v_out.length() < f64::EPSILON {
            return None;
        }

        let angle = v_in.angle_to(v_out).abs();
        if (angle - std::f64::consts::FRAC_PI_2).abs() > RECT_MAX_ANGLE_DEV {
            return None;
        }
    }

    // Circular mean of the edge directions with period pi/2.
    let (mut sin_sum, mut cos_sum) = (0.0, 0.0);
    for i in 0..4 {
        let edge = corners[(i + 1) % 4] - corners[i];
        let angle = edge.y.atan2(edge.x) * 4.0;
        sin_sum += angle.sin() * edge.length();
        cos_sum += angle.cos() * edge.length();
    }
    let mut orientation = sin_sum.atan2(cos_sum) / 4.0;
    if orientation.abs() < RECT_AXIS_SNAP_ANGLE
        || (orientation.abs() - std::f64::consts::FRAC_PI_4).abs() < RECT_AXIS_SNAP_ANGLE
    {
        orientation =
            (orientation / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
    }

    // Bounds of the corners in the rotated frame determine center and extents.
    let rot_inv = DAffine2::from_angle(-orientation);
    let local = corners
        .iter()
        .map(|c| rot_inv.transform_point2(*c))
        .collect::<Vec<Vector2>>();
    let (local_min, local_max) = points_bounds(&local);

    let half_extents = (local_max - local_min) * 0.5;
    let local_center = (local_min + local_max) * 0.5;
    let center = DAffine2::from_angle(orientation).transform_point2(local_center);

    Some(Rectangle {
        cuboid: Cuboid::new(half_extents),
        affine: DAffine2::from_angle_translation(orientation, center),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::penpath::Element;

    fn pen_path_from_points(points: impl IntoIterator<Item = Vector2>) -> PenPath {
        PenPath::try_from_elements(points.into_iter().map(|p| Element::new(p, 0.5))).unwrap()
    }

    // Deterministic pseudo-random jitter to simulate hand wobble.
    fn jitter(i: usize, magnitude: f64) -> Vector2 {
        let a = ((i as f64) * 12.9898).sin() * 43758.5453;
        let b = ((i as f64) * 78.233).sin() * 24634.6345;
        Vector2::new(a.fract(), b.fract()) * magnitude
    }

    /// A path through the given positions, each with its own pressure.
    fn pressured_path(points: impl IntoIterator<Item = (Vector2, f64)>) -> PenPath {
        PenPath::try_from_elements(
            points
                .into_iter()
                .map(|(p, pressure)| Element::new(p, pressure)),
        )
        .unwrap()
    }

    #[test]
    fn mean_width_of_a_steady_stroke_is_its_width() {
        let path =
            pressured_path((0..=20).map(|i| (Vector2::new(100.0 + 5.0 * i as f64, 100.0), 0.8)));

        let pressure = mean_width(&path, 1.0, |pressure| pressure).unwrap();

        assert!((pressure - 0.8).abs() < 1e-9, "{pressure}");
    }

    #[test]
    fn mean_width_is_weighted_by_length_and_not_by_event_count() {
        // A long quick stretch at 0.8, then a short slow stretch at 0.2 that reports ten times as
        // often. Counting events would call this stroke light, but nearly all of the ink is heavy.
        let quick = (0..=4).map(|i| (Vector2::new(100.0 + 50.0 * i as f64, 100.0), 0.8));
        let slow = (1..=40).map(|i| (Vector2::new(300.0 + 0.25 * i as f64, 100.0), 0.2));
        let path = pressured_path(quick.chain(slow));

        let pressure = mean_width(&path, 1.0, |pressure| pressure).unwrap();

        assert!(
            pressure > 0.7,
            "expected the long heavy stretch to win, got {pressure}"
        );
    }

    #[test]
    fn mean_width_is_the_mean_of_the_widths_and_not_the_width_of_the_mean_pressure() {
        // Half of the stroke at the lightest pressure and half at the hardest, with a width that
        // goes with the square of the pressure. Its mean pressure is 0.5, which would be a width
        // of 0.25, but the stroke was on average a good deal wider than that.
        let light = (0..=10).map(|i| (Vector2::new(100.0 + 5.0 * i as f64, 100.0), 0.0));
        let hard = (1..=10).map(|i| (Vector2::new(150.0 + 5.0 * i as f64, 100.0), 1.0));
        let path = pressured_path(light.chain(hard));

        let width = mean_width(&path, 1.0, |pressure| pressure * pressure).unwrap();

        assert!(width > 0.4, "expected about half, got {width}");
    }

    #[test]
    fn mean_width_leaves_out_the_resting_tail() {
        // Drawn at 0.8, then held still for the recognition: a hundred events at 0.1, all within
        // a pixel of where the stroke ended. They say nothing about how hard it was drawn.
        let drawn = (0..=20).map(|i| (Vector2::new(100.0 + 5.0 * i as f64, 100.0), 0.8));
        let rest = (0..100).map(|i| (Vector2::new(200.0, 100.0) + jitter(i, 0.5), 0.1));
        let path = pressured_path(drawn.chain(rest));

        let pressure = mean_width(&path, 1.0, |pressure| pressure).unwrap();

        assert!(
            pressure > 0.75,
            "the hold dragged the pressure to {pressure}"
        );
    }

    #[test]
    fn mean_width_of_a_stroke_that_never_moved_is_still_something() {
        let path =
            pressured_path((0..10).map(|i| (Vector2::new(100.0, 100.0) + jitter(i, 0.2), 0.6)));

        let pressure = mean_width(&path, 1.0, |pressure| pressure).unwrap();

        assert!((pressure - 0.6).abs() < 1e-9, "{pressure}");
    }

    #[test]
    fn recognize_straight_line() {
        let points = (0..=50)
            .map(|i| Vector2::new(20.0 + 4.0 * i as f64, 30.0 + 2.0 * i as f64) + jitter(i, 1.5));

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        assert!(
            matches!(shape, Some(Shape::Line(_))),
            "expected line, got {shape:?}"
        );
    }

    /// A stroke along the given points, followed by the pen resting at the last point
    /// for a while, jittering slightly, like when it is held still to trigger the recognition.
    fn with_resting_tail(points: impl IntoIterator<Item = Vector2>) -> Vec<Vector2> {
        let mut points: Vec<Vector2> = points.into_iter().collect();
        let rest = *points.last().unwrap();
        points.extend((0..100).map(|i| rest + jitter(1000 + i, 0.1)));
        points
    }

    #[test]
    fn recognize_short_line_with_resting_pen() {
        // A fraction bar: a short horizontal line, then the pen held still.
        let points = with_resting_tail(
            (0..=15).map(|i| Vector2::new(100.0 + 2.0 * i as f64, 200.0) + jitter(i, 0.4)),
        );

        match recognize_shape(&pen_path_from_points(points), 1.0) {
            Some(Shape::Line(line)) => {
                assert!((line.end - Vector2::new(130.0, 200.0)).length() < 1.0);
            }
            other => panic!("expected line, got {other:?}"),
        }
    }

    #[test]
    fn recognize_short_line_when_zoomed_in() {
        // At 250% zoom, a line of 12 document units is 30 pixels long on screen.
        let points = with_resting_tail(
            (0..=12).map(|i| Vector2::new(100.0 + i as f64, 200.0) + jitter(i, 0.15)),
        );

        let shape = recognize_shape(&pen_path_from_points(points), 2.5);
        assert!(
            matches!(shape, Some(Shape::Line(_))),
            "expected line, got {shape:?}"
        );
    }

    #[test]
    fn reject_short_arc() {
        // A parenthesis is short and nearly straight, but must not become a line.
        let points = with_resting_tail((0..=20).map(|i| {
            let t = i as f64 / 20.0;
            Vector2::new(
                100.0 - 5.0 * (t * std::f64::consts::PI).sin(),
                200.0 + 30.0 * t,
            )
        }));

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        assert!(shape.is_none(), "expected no shape, got {shape:?}");
    }

    #[test]
    fn recognize_long_line_with_resting_pen() {
        let points = with_resting_tail(
            (0..=50).map(|i| Vector2::new(100.0 + 4.0 * i as f64, 200.0) + jitter(i, 0.4)),
        );

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        assert!(
            matches!(shape, Some(Shape::Line(_))),
            "expected line, got {shape:?}"
        );
    }

    #[test]
    fn recognize_circle() {
        let points = (0..=80).map(|i| {
            let angle = (i as f64 / 80.0) * std::f64::consts::TAU;
            Vector2::new(100.0 + 50.0 * angle.cos(), 100.0 + 50.0 * angle.sin()) + jitter(i, 1.5)
        });

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        match shape {
            Some(Shape::Ellipse(ellipse)) => {
                assert!((ellipse.radii.x - 50.0).abs() < 5.0);
                assert!((ellipse.radii.y - 50.0).abs() < 5.0);
            }
            other => panic!("expected ellipse, got {other:?}"),
        }
    }

    #[test]
    fn recognize_imperfect_circle_snaps_to_circle() {
        // A slightly squashed circle (radii ratio 0.8) should still snap to a circle.
        let points = (0..=80).map(|i| {
            let angle = (i as f64 / 80.0) * std::f64::consts::TAU;
            Vector2::new(100.0 + 50.0 * angle.cos(), 100.0 + 40.0 * angle.sin()) + jitter(i, 1.5)
        });

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        match shape {
            Some(Shape::Ellipse(ellipse)) => {
                assert!(
                    (ellipse.radii.x - ellipse.radii.y).abs() < f64::EPSILON,
                    "expected circle, got radii {:?}",
                    ellipse.radii
                );
            }
            other => panic!("expected ellipse, got {other:?}"),
        }
    }

    #[test]
    fn recognize_flat_ellipse() {
        let points = (0..=80).map(|i| {
            let angle = (i as f64 / 80.0) * std::f64::consts::TAU;
            Vector2::new(100.0 + 80.0 * angle.cos(), 100.0 + 30.0 * angle.sin()) + jitter(i, 1.5)
        });

        let shape = recognize_shape(&pen_path_from_points(points), 1.0);
        match shape {
            Some(Shape::Ellipse(ellipse)) => {
                // The fit underestimates the major radius of this jittered ellipse by about 10%.
                assert!((ellipse.radii.x - 80.0).abs() < 9.0);
                assert!((ellipse.radii.y - 30.0).abs() < 5.0);
            }
            other => panic!("expected ellipse, got {other:?}"),
        }
    }

    fn closed_polygon_points(corners: &[Vector2], per_edge: usize) -> Vec<Vector2> {
        let mut points = Vec::new();
        for i in 0..corners.len() {
            let a = corners[i];
            let b = corners[(i + 1) % corners.len()];
            for j in 0..per_edge {
                let t = j as f64 / per_edge as f64;
                points.push(a + (b - a) * t + jitter(i * per_edge + j, 1.0));
            }
        }
        points.push(corners[0]);
        points
    }

    #[test]
    fn recognize_axis_aligned_rectangle() {
        let corners = [
            Vector2::new(50.0, 50.0),
            Vector2::new(250.0, 50.0),
            Vector2::new(250.0, 150.0),
            Vector2::new(50.0, 150.0),
        ];

        let shape = recognize_shape(
            &pen_path_from_points(closed_polygon_points(&corners, 20)),
            1.0,
        );
        match shape {
            Some(Shape::Rectangle(rect)) => {
                assert!((rect.cuboid.half_extents.x - 100.0).abs() < 8.0);
                assert!((rect.cuboid.half_extents.y - 50.0).abs() < 8.0);
            }
            other => panic!("expected rectangle, got {other:?}"),
        }
    }

    #[test]
    fn recognize_rotated_rectangle() {
        let rot = DAffine2::from_angle_translation(0.4, Vector2::new(150.0, 150.0));
        let corners = [
            rot.transform_point2(Vector2::new(-100.0, -50.0)),
            rot.transform_point2(Vector2::new(100.0, -50.0)),
            rot.transform_point2(Vector2::new(100.0, 50.0)),
            rot.transform_point2(Vector2::new(-100.0, 50.0)),
        ];

        let shape = recognize_shape(
            &pen_path_from_points(closed_polygon_points(&corners, 20)),
            1.0,
        );
        match shape {
            Some(Shape::Rectangle(rect)) => {
                let mut half_extents = [rect.cuboid.half_extents.x, rect.cuboid.half_extents.y];
                half_extents.sort_by(f64::total_cmp);
                assert!((half_extents[0] - 50.0).abs() < 8.0);
                assert!((half_extents[1] - 100.0).abs() < 8.0);
            }
            other => panic!("expected rectangle, got {other:?}"),
        }
    }

    #[test]
    fn recognize_slightly_rotated_rectangle_snaps_to_axis() {
        // A rectangle drawn ~4 degrees off-axis should be snapped to be axis-aligned.
        let rot = DAffine2::from_angle_translation(
            4.0 * std::f64::consts::PI / 180.0,
            Vector2::new(150.0, 150.0),
        );
        let corners = [
            rot.transform_point2(Vector2::new(-100.0, -50.0)),
            rot.transform_point2(Vector2::new(100.0, -50.0)),
            rot.transform_point2(Vector2::new(100.0, 50.0)),
            rot.transform_point2(Vector2::new(-100.0, 50.0)),
        ];

        let shape = recognize_shape(
            &pen_path_from_points(closed_polygon_points(&corners, 20)),
            1.0,
        );
        match shape {
            Some(Shape::Rectangle(rect)) => {
                let orientation = rect.affine.to_scale_angle_translation().1;
                assert!(
                    orientation.abs() < 1e-9,
                    "expected axis-aligned rectangle, got orientation {orientation}"
                );
            }
            other => panic!("expected rectangle, got {other:?}"),
        }
    }

    #[test]
    fn recognize_triangle() {
        let corners = [
            Vector2::new(100.0, 40.0),
            Vector2::new(180.0, 180.0),
            Vector2::new(20.0, 180.0),
        ];

        let shape = recognize_shape(
            &pen_path_from_points(closed_polygon_points(&corners, 25)),
            1.0,
        );
        match shape {
            Some(Shape::Polygon(polygon)) => {
                assert_eq!(polygon.path.len(), 2);
            }
            other => panic!("expected polygon, got {other:?}"),
        }
    }

    #[test]
    fn reject_scribble() {
        // An open s-shaped curve is neither a line nor a closed shape.
        let points = (0..=100).map(|i| {
            let t = i as f64 / 100.0;
            Vector2::new(
                50.0 + 200.0 * t,
                100.0 + 80.0 * (t * std::f64::consts::TAU).sin(),
            )
        });

        assert!(recognize_shape(&pen_path_from_points(points), 1.0).is_none());
    }

    #[test]
    fn reject_tiny_stroke() {
        let points = (0..=20).map(|i| {
            let angle = (i as f64 / 20.0) * std::f64::consts::TAU;
            Vector2::new(100.0 + 4.0 * angle.cos(), 100.0 + 4.0 * angle.sin())
        });

        assert!(recognize_shape(&pen_path_from_points(points), 1.0).is_none());
    }

    /// A long line as a hand draws it: bowed a little, trembling, with a hook where the pen landed
    /// and one where it lifted, and then held still.
    fn crooked_line(length: f64, bow: f64, tremor: f64, hook: f64) -> Vec<Vector2> {
        let n = (length / 2.0) as usize;
        let body = (0..=n).map(|i| {
            let t = i as f64 / n as f64;
            Vector2::new(
                100.0 + length * t,
                200.0 + bow * (t * std::f64::consts::PI).sin(),
            ) + jitter(i, tremor)
        });
        let landing =
            (0..6).map(|i| Vector2::new(100.0 + i as f64, 200.0 - hook * (1.0 - i as f64 / 6.0)));
        let lifting = (0..6).map(|i| {
            Vector2::new(
                100.0 + length - i as f64,
                200.0 + hook * (1.0 - i as f64 / 6.0),
            )
        });
        with_resting_tail(
            landing
                .chain(body)
                .chain(lifting.collect::<Vec<_>>().into_iter().rev()),
        )
    }

    #[test]
    fn a_long_crooked_line_is_a_line() {
        for (length, bow, tremor, hook) in [
            (300.0, 15.0, 1.5, 8.0),
            (500.0, 30.0, 1.5, 12.0),
            (800.0, 45.0, 2.0, 12.0),
            (300.0, 0.0, 2.5, 0.0),
        ] {
            let path = pen_path_from_points(crooked_line(length, bow, tremor, hook));
            let shape = recognize_shape(&path, 1.0);
            assert!(
                matches!(shape, Some(Shape::Line(_))),
                "{length} long, bowed {bow}: {shape:?}"
            );
        }
    }

    #[test]
    fn a_long_arc_is_no_line() {
        let path = pen_path_from_points(crooked_line(300.0, 60.0, 0.5, 0.0));
        assert!(recognize_shape(&path, 1.0).is_none());
    }

    #[test]
    fn a_line_is_not_tilted_by_its_hooks() {
        let path = pen_path_from_points(crooked_line(400.0, 0.0, 0.5, 12.0));
        let Some(Shape::Line(line)) = recognize_shape(&path, 1.0) else {
            panic!("expected a line");
        };
        let tilt = (line.end.y - line.start.y).abs() / (line.end.x - line.start.x).abs();
        assert!(tilt < 0.01, "tilted by {tilt}");
    }

    #[test]
    fn short_strokes_have_to_be_held_longer() {
        let stroke = |length: f64| {
            pen_path_from_points(
                (0..=20).map(move |i| Vector2::new(100.0, 100.0 + length * i as f64 / 20.0)),
            )
        };
        assert_eq!(hold_factor(&stroke(25.0), 1.0), HOLD_SHORT_FACTOR);
        assert_eq!(hold_factor(&stroke(300.0), 1.0), 1.0);
        let between = hold_factor(&stroke(80.0), 1.0);
        assert!(1.0 < between && between < HOLD_SHORT_FACTOR, "{between}");
        // What counts is how large it is on screen
        assert_eq!(hold_factor(&stroke(25.0), 8.0), 1.0);
    }
}
