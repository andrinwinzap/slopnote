// Imports
use crate::Camera;
use p2d::math::Vector2;
use serde::{Deserialize, Serialize};

/// The mapping between the window coordinates the ruler lives in and document coordinates.
///
/// The ruler stays where it is on screen while the canvas is panned or zoomed, so its position is
/// kept in window coordinates. Those are not the canvas surface coordinates the camera works in:
/// in the bounded layouts the canvas is only as large as the document and is centered in the
/// window, so the two are offset by a margin that changes with the zoom.
#[derive(Clone, Copy, Debug)]
pub struct RulerView {
    surface_origin: Vector2,
    camera_offset: Vector2,
    total_zoom: f64,
    /// The camera rotation in radians. The ruler stays put on screen when the canvas turns too.
    rotation: f64,
}

impl RulerView {
    pub fn from_camera(camera: &Camera) -> Self {
        Self {
            surface_origin: camera.surface_origin(),
            camera_offset: camera.offset(),
            total_zoom: camera.total_zoom(),
            rotation: camera.rotation(),
        }
    }

    pub fn total_zoom(&self) -> f64 {
        self.total_zoom
    }

    /// The camera rotation in radians.
    pub fn rotation(&self) -> f64 {
        self.rotation
    }

    /// Convert a direction from window coordinates to document coordinates.
    pub fn direction_to_doc(&self, window_dir: Vector2) -> Vector2 {
        Vector2::from_angle(-self.rotation).rotate(window_dir)
    }

    /// Convert a position from window coordinates to document coordinates.
    pub fn to_doc(&self, window_pos: Vector2) -> Vector2 {
        self.direction_to_doc(window_pos - self.surface_origin + self.camera_offset)
            / self.total_zoom
    }

    /// Convert a position from document coordinates to window coordinates.
    pub fn from_doc(&self, doc_pos: Vector2) -> Vector2 {
        Vector2::from_angle(self.rotation).rotate(doc_pos) * self.total_zoom - self.camera_offset
            + self.surface_origin
    }
}

/// Turning the ruler, with the pen on its angle dial or with two fingers.
///
/// The turn itself is kept apart from the angle the ruler shows, because the common angles draw the
/// ruler in: around them the ruler answers a turn with a smaller change than the turn itself, and
/// right at them it does not move at all.
#[derive(Clone, Copy, Debug)]
pub struct RulerTurn {
    /// The angle the turn has reached, which the common angles are applied to.
    raw_angle: f64,
    /// The angle the ruler is showing.
    angle: f64,
}

impl RulerTurn {
    /// Start turning a ruler that is currently at `angle`.
    pub fn new(angle: f64) -> Self {
        Self {
            raw_angle: angle,
            angle,
        }
    }

    /// The angle the ruler is showing.
    pub fn angle(&self) -> f64 {
        self.angle
    }

    /// Turn by `delta` radians, and return the angle the ruler takes.
    pub fn update(&mut self, ruler: &RulerConfig, delta: f64) -> f64 {
        self.raw_angle += delta;
        self.angle = ruler.attract_angle(self.raw_angle);

        self.angle
    }

    /// Turn by a drag of `drag` surface pixels of the pen inside the angle dial.
    pub fn update_from_drag(&mut self, ruler: &RulerConfig, drag: f64) -> f64 {
        self.update(ruler, drag * RulerConfig::dial_radians_per_pixel())
    }
}

/// The midpoint of the bounding box of the given touch points, at the precision they have.
///
/// This is what the two-finger gesture follows. It cannot be had from the gesture itself: GTK works
/// it out through an integer rectangle, so the midpoint of two fingers moves in whole pixels and
/// flickers by one whenever a finger sits near the edge between two of them, which moved the whole
/// ruler in small jumps while it was turned. The points themselves are exact, and following their
/// midpoint is as smooth as following one finger.
///
/// `None` when there are no points.
pub fn touch_midpoint(points: impl IntoIterator<Item = Vector2>) -> Option<Vector2> {
    let mut points = points.into_iter();
    let first = points.next()?;
    let (min, max) = points.fold((first, first), |(min, max), point| {
        (min.min(point), max.max(point))
    });

    Some((min + max) * 0.5)
}

/// How far the line between two fingers turned, from the vector between them before to the one
/// after, in radians.
///
/// The ruler takes its rotation from this rather than from GTK's rotate gesture, so that its turn
/// and its midpoint come from the same touch points. The two gestures each keep their own copy of
/// the points and are updated one after the other, so taking the turn from one and the midpoint
/// from the other let them disagree, and each only reported when its own value changed: the ruler
/// held still while the fingers moved, then caught up all at once.
///
/// Which finger comes first is not something the gesture keeps to, and a line between two fingers
/// is the same line either way round, so the turn is folded into a quarter turn either side.
pub fn finger_turn(before: Vector2, after: Vector2) -> f64 {
    let turn = before.perp_dot(after).atan2(before.dot(after));
    if turn > std::f64::consts::FRAC_PI_2 {
        turn - std::f64::consts::PI
    } else if turn <= -std::f64::consts::FRAC_PI_2 {
        turn + std::f64::consts::PI
    } else {
        turn
    }
}

/// Configuration and runtime state for the on-canvas ruler.
///
/// The ruler is a translucent straight-edge spanning the viewport. Position is
/// stored in **window coordinates**, so the ruler stays where it is on screen
/// while the canvas is panned or zoomed, like a straight-edge held against the
/// screen. Conversions to document coordinates go through [`RulerView`], which
/// accounts for where the canvas sits inside the window.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename = "ruler_config")]
pub struct RulerConfig {
    /// Whether the ruler is currently shown and active for snapping.
    /// In-session only — not persisted.
    #[serde(skip)]
    pub visible: bool,
    /// Angle of the ruler's long axis in radians (0 = horizontal). In-session only.
    #[serde(skip)]
    pub angle: f64,
    /// A point in window coordinates the ruler centerline passes through.
    /// Defines the origin for tick marks. In-session only.
    #[serde(skip)]
    pub anchor: Vector2,
    /// Where the angle dial / rotation pivot is rendered, in window
    /// coordinates. Always lies on the centerline. Distinct from `anchor` so
    /// the dial can move (e.g., to the finger centroid) without shifting the
    /// tick origin. In-session only.
    #[serde(skip)]
    pub dial_pos: Vector2,
    /// Snap distance beyond the ruler's edge, as a percentage of the ruler's
    /// full width (`2 × BODY_HALF_WIDTH_PX`). Stored as a preference, e.g.
    /// `25.0` means 25%, i.e. half a body-width beyond each edge.
    pub snap_distance: f64,
    /// Whether to snap the ruler's angle to common values (0°, ±45°, ±90°)
    /// during two-finger rotation. Persisted.
    pub angle_snap_enabled: bool,
    /// Whether to render the angle dial in the center of rotation. Persisted.
    pub show_dial: bool,
    /// Half-width of the ruler body in surface pixels. Persisted.
    pub body_half_width: f64,
    /// On-screen spacing between minor long-edge ticks, in surface pixels.
    /// Persisted.
    pub tick_spacing: f64,
    /// Opacity of the ruler body fill, in percent (`0.0` = fully transparent,
    /// `100.0` = fully opaque). Persisted.
    pub body_opacity: f64,
    /// Degrees of ruler rotation per scroll-wheel unit (`dy`). Persisted.
    pub scroll_rotation_step_deg: f64,
}

impl Default for RulerConfig {
    fn default() -> Self {
        Self {
            visible: false,
            angle: 0.0,
            anchor: Vector2::ZERO,
            dial_pos: Vector2::ZERO,
            snap_distance: Self::SNAP_DISTANCE_DEFAULT,
            angle_snap_enabled: true,
            show_dial: true,
            body_half_width: Self::BODY_HALF_WIDTH_DEFAULT,
            tick_spacing: Self::TICK_SPACING_DEFAULT,
            body_opacity: Self::BODY_OPACITY_DEFAULT,
            scroll_rotation_step_deg: Self::SCROLL_ROTATION_STEP_DEG_DEFAULT,
        }
    }
}

impl RulerConfig {
    pub const SNAP_DISTANCE_DEFAULT: f64 = 25.0;
    pub const SNAP_DISTANCE_MIN: f64 = 0.0;
    pub const SNAP_DISTANCE_MAX: f64 = 200.0;
    /// Default / range for `body_half_width`, in surface pixels (constant on-screen size).
    pub const BODY_HALF_WIDTH_DEFAULT: f64 = 60.0;
    pub const BODY_HALF_WIDTH_MIN: f64 = 20.0;
    pub const BODY_HALF_WIDTH_MAX: f64 = 120.0;
    /// Default / range for `tick_spacing`, in surface pixels.
    pub const TICK_SPACING_DEFAULT: f64 = 5.0;
    pub const TICK_SPACING_MIN: f64 = 2.0;
    pub const TICK_SPACING_MAX: f64 = 20.0;
    /// Default / range for `body_opacity`, in percent.
    pub const BODY_OPACITY_DEFAULT: f64 = 35.0;
    pub const BODY_OPACITY_MIN: f64 = 0.0;
    pub const BODY_OPACITY_MAX: f64 = 100.0;
    /// Default / range for `scroll_rotation_step_deg`, in degrees per scroll-wheel unit.
    pub const SCROLL_ROTATION_STEP_DEG_DEFAULT: f64 = 2.0;
    pub const SCROLL_ROTATION_STEP_DEG_MIN: f64 = 0.1;
    pub const SCROLL_ROTATION_STEP_DEG_MAX: f64 = 15.0;
    /// Length of major tick marks in surface pixels.
    pub const TICK_MAJOR_LEN_PX: f64 = 14.0;
    /// Outer radius of the angle dial, in surface pixels (constant on-screen size).
    pub const DIAL_OUTER_RADIUS_PX: f64 = 32.0;
    /// How far the ruler turns per surface pixel the pen is dragged up or down inside the dial.
    pub const DIAL_DEG_PER_PIXEL: f64 = 0.4;
    /// Length of minor tick marks in surface pixels.
    pub const TICK_MINOR_LEN_PX: f64 = 7.0;
    /// How far from a common angle (0, ±45 and ±90 degrees) the ruler starts being drawn towards
    /// it.
    pub const ANGLE_SNAP_ZONE_DEG: f64 = 6.0;
    /// How close to a common angle the ruler sits exactly on it, however the fingers wobble.
    ///
    /// Wide enough to hold the ruler steady against the jitter of fingers on glass, which is about
    /// a degree. Turning past it moves the ruler on from the common angle without any jump, so the
    /// angles next to it are still set by turning a little further.
    pub const ANGLE_SNAP_PIN_DEG: f64 = 1.5;
    /// Half-width of the snap-to-angle window, in degrees, when *approaching*
    /// a target. Within this many degrees of `0`, `±45`, or `±90`, the angle
    /// gets pulled in to the target.
    pub const ANGLE_SNAP_THRESHOLD_DEG: f64 = 3.0;
    /// Half-width of the snap-to-angle window when the ruler is **already
    /// locked** to a target, in degrees. Smaller than the enter threshold so
    /// a single small scroll-wheel click is enough to break out of a snap.
    pub const ANGLE_SNAP_LEAVE_THRESHOLD_DEG: f64 = 0.5;

    /// Unit direction vector along the ruler's long axis.
    pub fn direction(&self) -> Vector2 {
        Vector2::new(self.angle.cos(), self.angle.sin())
    }

    /// Unit normal vector perpendicular to the ruler's long axis (left-hand side).
    pub fn normal(&self) -> Vector2 {
        Vector2::new(-self.angle.sin(), self.angle.cos())
    }

    /// Half-width of the ruler body in document coordinates at the given zoom.
    pub fn body_half_width_doc(&self, total_zoom: f64) -> f64 {
        self.body_half_width / total_zoom
    }

    /// Whether a dark-on-light palette should be used, given the page's
    /// background color. Uses BT.601 relative luminance with a 0.5 threshold:
    /// dark backgrounds (luminance < 0.5) get a light ruler palette and vice
    /// versa.
    pub fn dark_mode_for_background(bg: &rnote_compose::Color) -> bool {
        let luminance = 0.299 * bg.r + 0.587 * bg.g + 0.114 * bg.b;
        luminance < 0.5
    }

    /// Body fill color computed from `body_opacity` and the dark-mode decision.
    ///
    /// The body's brightness is interpolated against `body_opacity`: at low
    /// opacity we want the body to contrast with the canvas background (so the
    /// ruler is visible at all), and at high opacity we want it to contrast
    /// with the marking colors (so the ticks / text stay readable).
    pub fn body_fill_color(&self, dark_mode: bool) -> piet::Color {
        let opacity = (self.body_opacity / 100.0).clamp(0.0, 1.0);
        let alpha = (opacity * 255.0).round() as u8;
        let (low, high) = if dark_mode {
            // Dark mode: low opacity ≈ near-white (contrast against dark canvas);
            // high opacity ≈ dark gray (contrast against white markings/text).
            (220.0, 60.0)
        } else {
            // Light mode: low opacity ≈ dark gray (contrast against light canvas);
            // high opacity ≈ light gray (contrast against black markings/text).
            (80.0, 200.0)
        };
        let brightness = (low + (high - low) * opacity).round().clamp(0.0, 255.0) as u8;
        piet::Color::rgba8(brightness, brightness, brightness, alpha)
    }

    /// Color used for the body outline.
    pub fn body_stroke_color(dark_mode: bool) -> piet::Color {
        if dark_mode {
            piet::Color::rgba8(255, 255, 255, 220)
        } else {
            piet::Color::rgba8(0, 0, 0, 220)
        }
    }

    /// Color used for the tick marks (long edges + dial ticks).
    pub fn tick_color(dark_mode: bool) -> piet::Color {
        if dark_mode {
            piet::Color::rgba8(255, 255, 255, 240)
        } else {
            piet::Color::rgba8(0, 0, 0, 240)
        }
    }

    /// Color used for the angle text in the dial.
    pub fn angle_text_color(dark_mode: bool) -> piet::Color {
        if dark_mode {
            piet::Color::rgba8(255, 255, 255, 255)
        } else {
            piet::Color::rgba8(0, 0, 0, 255)
        }
    }

    /// Ruler centerline anchor in document coordinates.
    pub fn anchor_doc(&self, view: RulerView) -> Vector2 {
        view.to_doc(self.anchor)
    }

    /// Dial position in document coordinates.
    pub fn dial_pos_doc(&self, view: RulerView) -> Vector2 {
        view.to_doc(self.dial_pos)
    }

    /// Signed distance of a position in window coordinates from the centerline.
    fn perp_distance(&self, window_pos: Vector2) -> f64 {
        (window_pos - self.anchor).dot(self.normal())
    }

    /// Whether `window_pos` (in window coordinates) lies within the ruler body strip.
    pub fn hit_body_window(&self, window_pos: Vector2) -> bool {
        if !self.visible {
            return false;
        }

        self.perp_distance(window_pos).abs() <= self.body_half_width
    }

    /// Whether `window_pos` (in window coordinates) lies inside the angle dial, the knob that
    /// rotates the ruler. Always false while the dial is not shown.
    pub fn hit_dial_window(&self, window_pos: Vector2) -> bool {
        if !self.visible || !self.show_dial {
            return false;
        }

        (window_pos - self.dial_pos).length() <= Self::DIAL_OUTER_RADIUS_PX
    }

    /// Whether `pos_doc` (in document coordinates) lies inside the angle dial.
    pub fn hit_dial(&self, pos_doc: Vector2, view: RulerView) -> bool {
        self.hit_dial_window(view.from_doc(pos_doc))
    }

    /// The angle (in radians) the ruler turns per surface pixel the pen is dragged inside the dial.
    ///
    /// Dragging down turns the ruler clockwise, the same way scrolling down does.
    pub fn dial_radians_per_pixel() -> f64 {
        Self::DIAL_DEG_PER_PIXEL.to_radians()
    }

    /// Draw an angle towards the common angles (0, ±45 and ±90 degrees).
    ///
    /// The ruler sits exactly on a common angle while it is turned to within `ANGLE_SNAP_PIN_DEG`
    /// of it, so it stays put however the fingers wobble. Beyond that it moves on at a steady rate
    /// a little faster than the turn, which catches up what the pin held back by the edge of the
    /// zone, where the turn is left alone: nothing jumps and nothing moves out of step.
    ///
    /// The rate matters as much as the pin. A turn that is answered by a steeper change of the ruler
    /// than itself makes the ruler shake with the fingers, a half degree wobble of which comes out
    /// as a degree or more, so it is kept as near to the turn as the catching up allows: a third
    /// more. The angles next to a common one are reached by turning a little further than they are.
    pub fn attract_angle(&self, angle_rad: f64) -> f64 {
        const TARGETS_DEG: [f64; 5] = [-90.0, -45.0, 0.0, 45.0, 90.0];

        if !self.angle_snap_enabled {
            return angle_rad;
        }
        let normalized_deg = Self::normalize_angle(angle_rad).to_degrees();
        let Some(distance_deg) = TARGETS_DEG
            .iter()
            .map(|target| normalized_deg - target)
            .min_by(|a, b| a.abs().total_cmp(&b.abs()))
        else {
            return angle_rad;
        };
        let away = distance_deg.abs();
        if away >= Self::ANGLE_SNAP_ZONE_DEG {
            return angle_rad;
        }
        // How far from the common angle the ruler is: none while pinned, and then growing steadily
        // so that it has caught up with the turn at the edge of the zone.
        let shown = if away <= Self::ANGLE_SNAP_PIN_DEG {
            0.0
        } else {
            (away - Self::ANGLE_SNAP_PIN_DEG) * Self::ANGLE_SNAP_ZONE_DEG
                / (Self::ANGLE_SNAP_ZONE_DEG - Self::ANGLE_SNAP_PIN_DEG)
        };
        // The angle grows against the normalized one, so what the ruler holds back is added.
        angle_rad + (distance_deg - shown.copysign(distance_deg)).to_radians()
    }

    /// Rotate a position around the pivot by the given angle, both in the same coordinate space.
    pub fn rotate_around(pos: Vector2, pivot: Vector2, angle: f64) -> Vector2 {
        let v = pos - pivot;
        let (sin, cos) = angle.sin_cos();

        pivot + Vector2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
    }

    /// If `pos_doc` lies within the snap zone of one of the ruler's long
    /// edges, return the sign of the perpendicular (`+1.0` or `-1.0`) that
    /// identifies that edge. `None` means no snap.
    pub fn snap_side(&self, pos_doc: Vector2, view: RulerView) -> Option<f64> {
        if !self.visible {
            return None;
        }
        let half_w = self.body_half_width;
        let snap_dist = (self.snap_distance / 100.0) * 2.0 * half_w;
        let perp = self.perp_distance(view.from_doc(pos_doc));
        if perp.abs() - half_w > snap_dist {
            None
        } else {
            Some(if perp >= 0.0 { 1.0 } else { -1.0 })
        }
    }

    /// Project `pos_doc` onto the long edge identified by `side` (`+1.0` or
    /// `-1.0`), regardless of distance. Used to keep a stroke locked to the
    /// ruler once it has snapped.
    pub fn project_to_edge(&self, pos_doc: Vector2, side: f64, view: RulerView) -> Vector2 {
        let pos_window = view.from_doc(pos_doc);
        let dir = self.direction();
        let normal = self.normal();
        let along = (pos_window - self.anchor).dot(dir);
        let snapped_window = self.anchor + along * dir + side * self.body_half_width * normal;

        view.to_doc(snapped_window)
    }

    /// Normalize an angle (radians) to the displayable principal angle in
    /// `[-π/2, π/2)`, with sign flipped so positive is CCW as the user sees
    /// it on screen (the stored `angle` uses screen-y-down radians where
    /// positive rotation is visually clockwise).
    pub fn normalize_angle(angle_rad: f64) -> f64 {
        let half_pi = std::f64::consts::FRAC_PI_2;
        let pi = std::f64::consts::PI;
        -(((angle_rad + half_pi).rem_euclid(pi)) - half_pi)
    }

    /// Whether `angle_rad` is essentially equal to one of the snap targets
    /// (0°, ±45°, ±90° — modulo π for line symmetry). Used by the
    /// hysteretic snap to know whether to use the "enter" or "leave" window.
    pub fn is_at_snap_target(angle_rad: f64) -> bool {
        const EPS_DEG: f64 = 0.001;
        const TARGETS_DEG: [f64; 5] = [-90.0, -45.0, 0.0, 45.0, 90.0];
        let normalized_deg = Self::normalize_angle(angle_rad).to_degrees();
        TARGETS_DEG
            .iter()
            .any(|t| (normalized_deg - t).abs() < EPS_DEG)
    }

    /// If `angle_rad` is within `threshold_deg` of one of the target angles
    /// (0°, ±45°, ±90°), return the snapped angle in the same
    /// (screen-radian) convention as the input. Otherwise return the input
    /// unchanged.
    pub fn snap_angle_with_threshold(angle_rad: f64, threshold_deg: f64) -> f64 {
        const TARGETS_DEG: [f64; 5] = [-90.0, -45.0, 0.0, 45.0, 90.0];
        let normalized_deg = Self::normalize_angle(angle_rad).to_degrees();
        for t in TARGETS_DEG {
            if (normalized_deg - t).abs() < threshold_deg {
                // The ruler line is symmetric: `base + k*π` for any integer k
                // describes the same physical line direction. We pick the k
                // that keeps the returned value closest to the input — this
                // avoids a π-jump in `ruler.angle` when the user rotates
                // across a snap boundary (which would rotate the anchor
                // around the pivot by π and visibly shift the tick pattern
                // along the ruler's axis).
                let base = -t.to_radians();
                let pi = std::f64::consts::PI;
                let k = ((angle_rad - base) / pi).round();
                return base + k * pi;
            }
        }
        angle_rad
    }

    /// Hysteretic snap: the narrow "leave" window applies only to the target
    /// the ruler is *currently* locked on, so it's easy to escape that one
    /// without making it hard to engage any of the others.
    pub fn snap_angle_hysteretic(raw_angle: f64, prev_angle: f64) -> f64 {
        const TARGETS_DEG: [f64; 5] = [-90.0, -45.0, 0.0, 45.0, 90.0];
        const EPS_DEG: f64 = 0.001;
        let normalized_deg = Self::normalize_angle(raw_angle).to_degrees();
        let prev_normalized_deg = Self::normalize_angle(prev_angle).to_degrees();
        for t in TARGETS_DEG {
            let was_at_this_target = (prev_normalized_deg - t).abs() < EPS_DEG;
            let threshold = if was_at_this_target {
                Self::ANGLE_SNAP_LEAVE_THRESHOLD_DEG
            } else {
                Self::ANGLE_SNAP_THRESHOLD_DEG
            };
            if (normalized_deg - t).abs() < threshold {
                let base = -t.to_radians();
                let pi = std::f64::consts::PI;
                let k = ((raw_angle - base) / pi).round();
                return base + k * pi;
            }
        }
        raw_angle
    }

    /// Back-compat wrapper using the enter-threshold only.
    pub fn snap_angle(angle_rad: f64) -> f64 {
        Self::snap_angle_with_threshold(angle_rad, Self::ANGLE_SNAP_THRESHOLD_DEG)
    }

    /// Whether `pos_doc` (in document coordinates) lies within the ruler body
    /// strip (infinite along the long axis, finite across).
    pub fn hit_body(&self, pos_doc: Vector2, view: RulerView) -> bool {
        self.hit_body_window(view.from_doc(pos_doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn horizontal_ruler() -> RulerConfig {
        RulerConfig {
            visible: true,
            angle: 0.0,
            anchor: Vector2::new(200.0, 150.0),
            dial_pos: Vector2::new(200.0, 150.0),
            body_half_width: 60.0,
            ..RulerConfig::default()
        }
    }

    /// A view with the canvas offset inside the window, as in the bounded layouts where it is
    /// only as large as the document and centered.
    fn view(total_zoom: f64, surface_origin: Vector2) -> RulerView {
        RulerView {
            surface_origin,
            camera_offset: Vector2::new(40.0, 25.0),
            total_zoom,
            rotation: 0.0,
        }
    }

    #[test]
    fn conversions_are_inverse_when_the_canvas_is_rotated() {
        let view = RulerView {
            rotation: 0.7,
            ..view(2.5, Vector2::new(120.0, 0.0))
        };
        let window_pos = Vector2::new(310.0, 180.0);

        assert!((view.from_doc(view.to_doc(window_pos)) - window_pos).length() < 1e-9);
    }

    #[test]
    fn project_to_edge_lands_on_the_edge_when_the_canvas_is_rotated() {
        let ruler = horizontal_ruler();
        let view = RulerView {
            rotation: 0.7,
            ..view(2.5, Vector2::new(120.0, 0.0))
        };
        let pos_window = Vector2::new(320.0, 190.0);

        let projected = view.from_doc(ruler.project_to_edge(view.to_doc(pos_window), 1.0, view));

        assert!((projected.x - pos_window.x).abs() < 1e-9);
        assert!((ruler.perp_distance(projected) - ruler.body_half_width).abs() < 1e-9);
    }

    #[test]
    fn window_and_document_conversions_are_inverse() {
        let view = view(2.5, Vector2::new(120.0, 0.0));
        let window_pos = Vector2::new(310.0, 180.0);

        assert!((view.from_doc(view.to_doc(window_pos)) - window_pos).length() < 1e-9);
    }

    #[test]
    fn body_stays_under_the_same_screen_position() {
        let ruler = horizontal_ruler();
        // 50 pixels below the centerline on screen is inside the 60 pixel half width,
        // no matter the zoom or where the canvas sits inside the window.
        let window_pos = ruler.anchor + Vector2::new(0.0, 50.0);

        for total_zoom in [0.2, 1.0, 6.0] {
            for surface_origin in [Vector2::ZERO, Vector2::new(180.0, 40.0)] {
                let view = view(total_zoom, surface_origin);
                assert!(
                    ruler.hit_body(view.to_doc(window_pos), view),
                    "expected a hit at zoom {total_zoom} with origin {surface_origin:?}"
                );
            }
        }
    }

    #[test]
    fn the_midpoint_of_two_fingers_keeps_the_precision_of_the_points() {
        let midpoint = touch_midpoint([Vector2::new(619.25, 462.5), Vector2::new(700.75, 500.0)]);

        // Not a whole number of pixels, which is what going through an integer rectangle gave.
        assert_eq!(midpoint, Some(Vector2::new(660.0, 481.25)));
    }

    #[test]
    fn the_midpoint_moves_smoothly_while_a_finger_does() {
        // One finger moving a tenth of a pixel at a time moves the midpoint by half of that, with no
        // steps and no flicker.
        let mut last: Option<f64> = None;
        for i in 0..20 {
            let moved = i as f64 * 0.1;
            let midpoint = touch_midpoint([
                Vector2::new(600.0 + moved, 460.0),
                Vector2::new(700.0, 500.0),
            ])
            .unwrap();
            if let Some(last) = last {
                let step = midpoint.x - last;
                assert!((step - 0.05).abs() < 1e-9, "stepped by {step}");
            }
            last = Some(midpoint.x);
        }
    }

    #[test]
    fn no_fingers_have_no_midpoint() {
        assert_eq!(touch_midpoint([]), None);
    }

    #[test]
    fn one_finger_is_its_own_midpoint() {
        assert_eq!(
            touch_midpoint([Vector2::new(12.5, 40.25)]),
            Some(Vector2::new(12.5, 40.25))
        );
    }

    #[test]
    fn the_dial_is_grabbed_at_the_same_screen_size() {
        let ruler = horizontal_ruler();
        // Just inside and just outside the dial on screen, at any zoom and canvas position.
        let inside = ruler.dial_pos + Vector2::new(0.0, 30.0);
        let outside = ruler.dial_pos + Vector2::new(0.0, 34.0);

        for total_zoom in [0.2, 1.0, 6.0] {
            let view = view(total_zoom, Vector2::new(180.0, 40.0));
            assert!(
                ruler.hit_dial(view.to_doc(inside), view),
                "zoom {total_zoom}"
            );
            assert!(
                !ruler.hit_dial(view.to_doc(outside), view),
                "zoom {total_zoom}"
            );
        }
    }

    #[test]
    fn a_hidden_dial_is_not_grabbed() {
        let ruler = RulerConfig {
            show_dial: false,
            ..horizontal_ruler()
        };

        assert!(!ruler.hit_dial_window(ruler.dial_pos));
    }

    /// Turn the ruler by `degrees` per step and report every angle it showed.
    fn turn(ruler: &RulerConfig, from: f64, degrees: f64, steps: usize) -> Vec<f64> {
        let mut turn = RulerTurn::new(from.to_radians());

        (0..steps)
            .map(|_| turn.update(ruler, degrees.to_radians()).to_degrees())
            .collect()
    }

    /// How close the turn got to the wanted angle.
    fn closest_to(angles: &[f64], wanted: f64) -> f64 {
        angles
            .iter()
            .map(|angle| (angle - wanted).abs())
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn dragging_down_turns_clockwise() {
        // The stored angle grows clockwise, and a downwards drag is a positive one, the same way
        // scrolling down turns the ruler.
        assert!(RulerConfig::dial_radians_per_pixel() > 0.0);
    }

    #[test]
    fn the_ruler_sits_exactly_on_a_common_angle() {
        let ruler = horizontal_ruler();

        for degrees in [90.0f64, 89.5, 90.5, 45.0, 0.0] {
            let angle = ruler.attract_angle(degrees.to_radians()).to_degrees();
            let nearest = [0.0f64, 45.0, 90.0]
                .into_iter()
                .min_by(|a, b| (a - degrees).abs().total_cmp(&(b - degrees).abs()))
                .unwrap();
            assert!(
                (angle - nearest).abs() < 1e-9,
                "{degrees} degrees should sit on {nearest}, got {angle}"
            );
        }
    }

    #[test]
    fn the_ruler_does_not_move_while_the_fingers_wobble_around_a_common_angle() {
        let ruler = horizontal_ruler();

        // Fingers on glass wobble by about a degree: that must not reach the ruler at all.
        let shown = [88.6f64, 89.2, 90.0, 90.7, 91.4]
            .map(|degrees| ruler.attract_angle(degrees.to_radians()).to_degrees());

        for degrees in shown {
            assert!(
                (degrees - 90.0).abs() < 1e-9,
                "the ruler moved to {degrees}"
            );
        }
    }

    #[test]
    fn the_ruler_never_moves_much_faster_than_the_fingers_do() {
        let ruler = horizontal_ruler();

        // A ruler that answers a turn with a steeper change of its own makes a wobble of the
        // fingers shake the ruler: this is what the first version of the snap did, with a rate of
        // up to three, and what made the ruler hard to place by two fingers.
        let step = 0.01f64;
        let mut fastest = 0.0f64;
        let mut degrees = 82.0f64;
        while degrees < 98.0 {
            let here = ruler.attract_angle(degrees.to_radians()).to_degrees();
            let next = ruler
                .attract_angle((degrees + step).to_radians())
                .to_degrees();
            fastest = fastest.max((next - here) / step);
            degrees += step;
        }

        assert!(
            fastest < 1.4,
            "the ruler moves {fastest} times as fast as the fingers"
        );
    }

    #[test]
    fn a_wobble_next_to_a_common_angle_is_not_made_larger() {
        let ruler = horizontal_ruler();

        // The same half degree wobble, at every distance from the common angle that is inside the
        // zone, must come out as a wobble of at most about that size.
        for away in [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0, 5.9] {
            let low = ruler
                .attract_angle((90.0 - away - 0.25).to_radians())
                .to_degrees();
            let high = ruler
                .attract_angle((90.0 - away + 0.25).to_radians())
                .to_degrees();

            assert!(
                (high - low).abs() < 0.7,
                "{away} degrees away: a wobble of 0.5 came out as {}",
                (high - low).abs()
            );
        }
    }

    #[test]
    fn the_angles_next_to_a_common_one_are_reachable() {
        let ruler = horizontal_ruler();

        // Turning through the zone in small steps, as a hand does.
        let angles = turn(&ruler, 84.0, 0.05, 240);

        for wanted in [88.0, 89.0, 91.0, 92.0] {
            let closest = closest_to(&angles, wanted);
            assert!(
                closest < 0.1,
                "closest to {wanted} degrees was {closest} off"
            );
        }
    }

    #[test]
    fn the_pull_reaches_as_far_as_its_zone_and_no_further() {
        let ruler = horizontal_ruler();
        let outside = 90.0 - RulerConfig::ANGLE_SNAP_ZONE_DEG - 0.5;
        let inside = 90.0 - RulerConfig::ANGLE_SNAP_ZONE_DEG + 0.5;

        let left_alone = ruler.attract_angle(outside.to_radians()).to_degrees();
        let drawn_in = ruler.attract_angle(inside.to_radians()).to_degrees();

        assert!((left_alone - outside).abs() < 1e-9);
        assert!(
            drawn_in > inside,
            "{inside} should be drawn towards 90, got {drawn_in}"
        );
    }

    #[test]
    fn the_pull_does_not_jump_anywhere() {
        let ruler = horizontal_ruler();

        // Turning in even steps must never move the ruler by more than the zone is wide, and in
        // particular not jump when the zone is entered or left.
        let angles = turn(&ruler, 80.0, 0.1, 200);
        let biggest_step = angles
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f64::max);

        assert!(
            biggest_step < 0.5,
            "the ruler jumped by {biggest_step} degrees"
        );
    }

    #[test]
    fn nothing_is_pulled_when_the_snap_is_off() {
        let ruler = RulerConfig {
            angle_snap_enabled: false,
            ..horizontal_ruler()
        };
        let raw = 89f64.to_radians();

        assert!((ruler.attract_angle(raw) - raw).abs() < 1e-9);
    }

    #[test]
    fn rotating_the_anchor_keeps_it_around_the_dial() {
        let ruler = horizontal_ruler();
        let anchor = ruler.anchor + Vector2::new(80.0, 0.0);
        let angle = std::f64::consts::FRAC_PI_2;

        let rotated = RulerConfig::rotate_around(anchor, ruler.dial_pos, angle);

        // A quarter turn clockwise on screen: the point to the right ends up below the dial.
        assert!((rotated - (ruler.dial_pos + Vector2::new(0.0, 80.0))).length() < 1e-9);
    }

    #[test]
    fn project_to_edge_lands_on_the_edge() {
        let ruler = horizontal_ruler();
        let view = view(2.5, Vector2::new(120.0, 0.0));
        let pos_window = Vector2::new(320.0, 190.0);

        let projected = view.from_doc(ruler.project_to_edge(view.to_doc(pos_window), 1.0, view));

        // Keeps its position along the ruler, and sits exactly one half width off the centerline.
        assert!((projected.x - pos_window.x).abs() < 1e-9);
        assert!((ruler.perp_distance(projected) - ruler.body_half_width).abs() < 1e-9);
    }

    #[test]
    fn snap_zone_reaches_past_the_edge_in_screen_pixels() {
        let ruler = horizontal_ruler();
        // The default snap distance is 25% of the full width, so the zone ends half a body
        // half-width past the edge: 60 + 30 = 90 pixels on screen.
        for total_zoom in [0.2, 1.0, 6.0] {
            let view = view(total_zoom, Vector2::new(180.0, 40.0));
            let at = |offset: f64| {
                ruler.snap_side(view.to_doc(ruler.anchor + Vector2::new(0.0, offset)), view)
            };

            assert_eq!(at(85.0), Some(1.0), "zoom {total_zoom}");
            assert_eq!(at(-85.0), Some(-1.0), "zoom {total_zoom}");
            assert_eq!(at(95.0), None, "zoom {total_zoom}");
        }
    }

    #[test]
    fn finger_turn_follows_the_fingers() {
        let before = Vector2::new(200.0, 0.0);
        let turned = |deg: f64| {
            let rad = deg.to_radians();
            Vector2::new(rad.cos(), rad.sin()) * 200.0
        };

        for deg in [-60.0, -2.0, -0.01, 0.0, 0.01, 2.0, 60.0] {
            let turn = finger_turn(before, turned(deg)).to_degrees();
            assert!((turn - deg).abs() < 1e-9, "{deg}: {turn}");
        }
    }

    #[test]
    fn finger_turn_ignores_which_finger_comes_first() {
        let before = Vector2::new(200.0, 0.0);
        let rad = 3.0_f64.to_radians();
        let swapped = -Vector2::new(rad.cos(), rad.sin()) * 200.0;

        let turn = finger_turn(before, swapped).to_degrees();
        assert!((turn - 3.0).abs() < 1e-9, "{turn}");
    }

    #[test]
    fn finger_turn_matches_the_screen_direction_of_rotation() {
        // Window coordinates point down, so turning from right towards down is a positive angle,
        // as the ruler's own angle is.
        let turn = finger_turn(Vector2::new(1.0, 0.0), Vector2::new(1.0, 0.1));
        assert!(turn > 0.0);
    }
}
