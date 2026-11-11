// Imports
use crate::document::Layout;
use crate::engine::snapshot::Snapshotable;
use crate::engine::{EngineTask, EngineTaskSender};
use crate::tasks::{OneOffTaskError, OneOffTaskHandle};
use crate::{Document, WidgetFlags};
use p2d::bounding_volume::Aabb;
use p2d::glamx::DAffine2;
use p2d::math::Vector2;
use rnote_compose::ext::{AabbExt, DAffine2Ext};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NudgeDirection {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "camera")]
pub struct Camera {
    /// The offset in surface coordinates.
    #[serde(rename = "offset")]
    offset: Vector2,
    /// The dimensions in surface coordinates.
    #[serde(rename = "size")]
    size: Vector2,
    /// The camera zoom, origin at (0.0, 0.0).
    #[serde(rename = "zoom")]
    zoom: f64,
    /// The camera rotation in radians.
    #[serde(skip)]
    rotation: f64,
    /// The rotation that was asked for, before snapping.
    ///
    /// Further turns start from this, so that a turn smaller than the snap distance adds up
    /// and gets away from the angle it is snapped to.
    #[serde(skip)]
    rotation_unsnapped: f64,
    /// The temporary zoom. Is used to overlay the "permanent" zoom.
    #[serde(skip)]
    temporary_zoom: f64,

    /// The origin of the canvas in window coordinates.
    ///
    /// In the bounded layouts the canvas is only as large as the document and is centered in the
    /// window, so the two coordinate systems are offset by a margin that changes with the zoom.
    /// Overlays that are placed on the window rather than on the document need it to find their
    /// place on the document.
    #[serde(skip)]
    surface_origin: Vector2,

    /// The scale factor of the surface, usually 1.0 or 2.0 for high-dpi screens.
    ///
    /// This value could become a non-integer value in the future, so it is stored as float.
    #[serde(skip)]
    scale_factor: f64,

    /// Whether the view can move freely over pages of limited width, instead of being locked to them.
    #[serde(skip)]
    free_panning: bool,

    /// Whether the view may be rotated. Bookmark jumps leave the rotation alone when it may not.
    #[serde(skip)]
    rotation_enabled: bool,

    #[serde(skip)]
    zoom_task_handle: Option<crate::tasks::OneOffTaskHandle>,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            offset: Vector2::new(-Self::OVERSHOOT_HORIZONTAL, -Self::OVERSHOOT_VERTICAL),
            size: Vector2::new(800.0, 600.0),
            zoom: 1.0,
            rotation: 0.0,
            rotation_unsnapped: 0.0,
            temporary_zoom: 1.0,
            surface_origin: Vector2::ZERO,
            scale_factor: 1.0,
            free_panning: false,
            rotation_enabled: true,
            zoom_task_handle: None,
        }
    }
}

impl Snapshotable for Camera {
    fn extract_snapshot_data(&self) -> Self {
        Self {
            offset: self.offset,
            size: self.size,
            zoom: self.zoom,
            rotation: self.rotation,
            rotation_unsnapped: self.rotation_unsnapped,
            ..Default::default()
        }
    }
}

impl Camera {
    pub const ZOOM_MIN: f64 = 0.2;
    pub const ZOOM_MAX: f64 = 6.0;
    pub const ZOOM_DEFAULT: f64 = 1.0;
    // The zoom timeout time.
    pub const ZOOM_TIMEOUT: Duration = Duration::from_millis(400);
    // when performing a drag - zoom 0.5% zoom for every pixel in y dir
    pub const DRAG_ZOOM_MAGN_ZOOM_FACTOR: f64 = 0.005;
    pub const OVERSHOOT_HORIZONTAL: f64 = 96.0;
    pub const OVERSHOOT_VERTICAL: f64 = 96.0;

    pub fn with_zoom(mut self, zoom: f64) -> Self {
        self.zoom = zoom.clamp(Self::ZOOM_MIN, Self::ZOOM_MAX);
        self
    }

    pub fn with_offset(mut self, offset: Vector2) -> Self {
        self.offset = offset;
        self
    }
    pub fn with_size(mut self, size: Vector2) -> Self {
        self.size = size;
        self
    }

    pub fn with_rotation(mut self, rotation: f64) -> Self {
        self.rotation = rotation;
        self.rotation_unsnapped = rotation;
        self
    }

    /// The current viewport offset in surface coordinate space.
    pub fn offset(&self) -> Vector2 {
        self.offset
    }

    /// The origin of the canvas in window coordinates.
    pub fn surface_origin(&self) -> Vector2 {
        self.surface_origin
    }

    /// Set the origin of the canvas in window coordinates. Updated by the UI when the canvas is
    /// laid out, which is also when the zoom changes its size and with it the margin around it.
    pub fn set_surface_origin(&mut self, surface_origin: Vector2) {
        self.surface_origin = surface_origin;
    }

    pub fn set_offset(&mut self, offset: Vector2, doc: &Document) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        self.offset = if let Some((center_mins, center_maxs)) = self.page_center_range(doc) {
            let doc_to_view = self.document_to_view_transform();
            let center = doc_to_view
                .inverse()
                .transform_point2(offset + self.size * 0.5)
                .clamp(center_mins, center_maxs);
            doc_to_view.transform_point2(center) - self.size * 0.5
        } else {
            let (mins, maxs) = self.surface_mins_maxs(doc);
            let offset_maxs = Vector2::new(
                (maxs.x - self.size.x).max(mins.x),
                (maxs.y - self.size.y).max(mins.y),
            );

            Vector2::new(
                offset.x.clamp(mins.x, offset_maxs.x),
                offset.y.clamp(mins.y, offset_maxs.y),
            )
        };

        widget_flags.view_modified = true;
        widget_flags
    }

    /// Whether the view can move freely over pages of limited width.
    pub fn free_panning(&self) -> bool {
        self.free_panning
    }

    pub fn rotation_enabled(&self) -> bool {
        self.rotation_enabled
    }

    pub fn set_rotation_enabled(&mut self, rotation_enabled: bool) {
        self.rotation_enabled = rotation_enabled;
    }

    pub fn set_free_panning(&mut self, free_panning: bool) {
        self.free_panning = free_panning;
    }

    /// The range of the viewport center in document coordinates, along the axes of the page.
    ///
    /// Limiting the view in screen axes would make the limit jump open as soon as the page is
    /// rotated, because the upright box around a rotated page is a lot larger than the page.
    ///
    /// None when the view is limited in screen axes: in layouts that aren't limited, and when level
    /// and locked, where both ways are the same.
    fn page_center_range(&self, doc: &Document) -> Option<(Vector2, Vector2)> {
        let limited_width = matches!(
            doc.config.layout,
            Layout::FixedSize | Layout::ContinuousVertical
        );
        let free_panning = self.free_panning && doc.config.layout != Layout::Infinite;
        if !free_panning && (!limited_width || self.rotation == 0.0) {
            return None;
        }

        let zoom = self.total_zoom();
        let overshoot = Vector2::new(Self::OVERSHOOT_HORIZONTAL, Self::OVERSHOOT_VERTICAL);
        let (cos, sin) = (self.rotation.cos().abs(), self.rotation.sin().abs());
        let (cos2, sin2) = (cos * cos, sin * sin);
        let half_size = self.size * 0.5;
        // How far the view reaches along the axes of the page, in surface pixels.
        let mut reach = Vector2::new(
            half_size.x * cos2 + half_size.y * sin2,
            half_size.x * sin2 + half_size.y * cos2,
        );

        if free_panning {
            // An edge of the page can go up to the far side of the view, so any part of the page can
            // be moved to where it is comfortable to write. A bit of the page stays in view, so
            // the view can't get lost in empty space.
            let past_edge = (reach - overshoot).max(Vector2::ZERO) / zoom;
            let page = doc.bounds();
            return Some((page.mins - past_edge, page.maxs + past_edge));
        }

        let bounds = doc.bounds().extend_by(overshoot / zoom);
        let page_half = bounds.extents() * 0.5 * zoom;
        let lock_x = reach.x >= page_half.x;
        let lock_y = reach.y >= page_half.y;

        // Reduced so that the corners of a tilted page can still be brought into view. An axis
        // that is locked keeps its lock, only the other one gets the room for it.
        let free_along = |locked: f64, reach: f64, sin: f64, cos: f64| {
            let limit = ((half_size.x - cos * locked) / sin.max(1e-9))
                .min((half_size.y - sin * locked) / cos.max(1e-9));
            limit.clamp(0.0, reach)
        };
        match (lock_x, lock_y) {
            (true, false) => reach.y = free_along(page_half.x, reach.y, sin, cos),
            (false, true) => reach.x = free_along(page_half.y, reach.x, cos, sin),
            (false, false) => {
                reach /= ((cos * reach.x + sin * reach.y) / half_size.x)
                    .max((sin * reach.x + cos * reach.y) / half_size.y)
                    .max(1.0);
            }
            (true, true) => {}
        }

        // A page that fits stays in the middle, as it does when level, where the canvas widget is
        // only as wide as the page and gets centered in the window.
        let center = bounds.center();
        let travel = (bounds.extents() * 0.5 - reach / zoom).max(Vector2::ZERO);
        let travel = Vector2::new(
            if lock_x { 0.0 } else { travel.x },
            if lock_y { 0.0 } else { travel.y },
        );
        Some((center - travel, center + travel))
    }

    /// The minimum and maximum surface bounds (document including overshoot) in surface coordinate space.
    pub fn surface_mins_maxs(&self, doc: &Document) -> (Vector2, Vector2) {
        let document_bounds = self
            .document_to_view_transform()
            .transform_aabb(doc.bounds());

        let bounds = match doc.config.layout {
            Layout::FixedSize | Layout::ContinuousVertical | Layout::SemiInfinite => {
                document_bounds.extend_by(Vector2::new(
                    Self::OVERSHOOT_HORIZONTAL,
                    Self::OVERSHOOT_VERTICAL,
                ))
            }
            Layout::Infinite => document_bounds,
        };
        let (mut mins, mut maxs) = (bounds.mins, bounds.maxs);

        // The scrollable area has to include everywhere the view can go.
        if let Some((center_mins, center_maxs)) = self.page_center_range(doc) {
            let doc_to_view = self.document_to_view_transform();
            let half_size = self.size * 0.5;
            for corner in [
                center_mins,
                Vector2::new(center_maxs.x, center_mins.y),
                Vector2::new(center_mins.x, center_maxs.y),
                center_maxs,
            ] {
                let center = doc_to_view.transform_point2(corner);
                mins = mins.min(center - half_size);
                maxs = maxs.max(center + half_size);
            }
        }

        (mins, maxs)
    }

    /// The current viewport size in surface coordinate space.
    pub fn size(&self) -> Vector2 {
        self.size
    }

    pub fn set_size(&mut self, size: Vector2) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        self.size = size;

        widget_flags.view_modified = true;
        widget_flags.resize = true;
        widget_flags
    }

    /// The permanent zoom.
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// Set the permanent zoom.
    pub fn zoom_to(&mut self, zoom: f64) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        self.zoom = zoom.clamp(Self::ZOOM_MIN, Self::ZOOM_MAX);
        widget_flags.view_modified = true;
        widget_flags.resize = true;
        widget_flags.refresh_canvasmenu = true;
        widget_flags.update_old_viewport = true;
        widget_flags
    }

    /// The camera rotation in radians.
    pub fn rotation(&self) -> f64 {
        self.rotation
    }

    fn snap_angle(angle: f64, step: f64) -> f64 {
        const SNAP_EPS: f64 = 1_f64.to_radians();

        let k = (angle / step).round();
        let snapped_angle = k * step;

        if (angle - snapped_angle).abs() <= SNAP_EPS {
            snapped_angle
        } else {
            angle
        }
    }

    /// The rotation in radians before snapping, which further turns should start from.
    pub fn rotation_unsnapped(&self) -> f64 {
        self.rotation_unsnapped
    }

    /// Normalizes angle to (-pi, pi].
    pub fn normalize_angle(angle: f64) -> f64 {
        std::f64::consts::PI - (std::f64::consts::PI - angle).rem_euclid(std::f64::consts::TAU)
    }

    /// Set the rotation in radians.
    ///
    /// `level_snap` is how close to level, in radians, the rotation has to be to snap to zero.
    pub fn set_rotation(&mut self, rotation: f64, level_snap: f64) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        let rotation = Self::normalize_angle(rotation);
        self.rotation_unsnapped = rotation;

        // snap angle to nearest 45 degrees and normalize it
        // angle must not be close to zero, because it causes major rendering issues in GTK
        let snapped =
            Self::normalize_angle(Self::snap_angle(rotation, std::f64::consts::FRAC_PI_4));
        self.rotation = if snapped.abs() <= level_snap {
            0.0
        } else {
            snapped
        };

        widget_flags.view_modified = true;
        widget_flags.resize = true;
        widget_flags.refresh_canvasmenu = true;
        widget_flags
    }

    /// The temporary zoom, to be overlaid on the surface when zooming with a timeout.
    pub fn temporary_zoom(&self) -> f64 {
        self.temporary_zoom
    }

    /// Set the temporary zoom.
    pub fn zoom_temporarily_to(&mut self, temporary_zoom: f64) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        self.temporary_zoom =
            temporary_zoom.clamp(Camera::ZOOM_MIN / self.zoom, Camera::ZOOM_MAX / self.zoom);
        widget_flags.view_modified = true;
        widget_flags.resize = true;
        widget_flags.refresh_canvasmenu = true;
        widget_flags
    }

    /// First zoom temporarily and then permanently after a timeout.
    ///
    /// Repeated calls to this function reset the timeout.
    pub(crate) fn zoom_w_timeout(&mut self, zoom: f64, tasks_tx: EngineTaskSender) -> WidgetFlags {
        let old_zoom = self.zoom();
        let mut reinstall_zoom_task = false;

        // zoom temporarily immediately
        let new_temporary_zoom = zoom / old_zoom;
        let widget_flags = self.zoom_temporarily_to(new_temporary_zoom);

        let zoom_task = move || {
            tasks_tx.send(EngineTask::Zoom(zoom));
        };
        if let Some(handle) = self.zoom_task_handle.as_mut() {
            match handle.replace_task(zoom_task.clone()) {
                Ok(()) => {}
                Err(OneOffTaskError::TimeoutReached) => {
                    reinstall_zoom_task = true;
                }
                Err(e) => {
                    error!("Could not replace task for one off zoom task, Err: {e:?}");
                    reinstall_zoom_task = true;
                }
            }
        } else {
            reinstall_zoom_task = true;
        }

        if reinstall_zoom_task {
            self.zoom_task_handle = Some(OneOffTaskHandle::new(zoom_task, Self::ZOOM_TIMEOUT));
        }

        widget_flags
    }

    /// The total zoom of the camera, including the temporary zoom.
    pub fn total_zoom(&self) -> f64 {
        self.zoom * self.temporary_zoom
    }

    /// The scaling factor for generating bitmap images with the current permanent zoom.
    ///
    /// Takes the scale factor in account
    pub fn image_scale(&self) -> f64 {
        self.zoom * self.scale_factor
    }

    /// The scale factor that gets set according to the toolkit hi-dpi settings.
    ///
    /// For Gtk it currently is 1.0 for scaling < 150%, 2.0 for >= 150% and < 250%, ..
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    pub fn set_scale_factor(&mut self, scale_factor: f64) -> WidgetFlags {
        self.scale_factor = scale_factor;
        let mut widget_flags = WidgetFlags::default();
        widget_flags.redraw = true;
        widget_flags
    }

    /// The viewport in document coordinate space.
    ///
    /// Returns the Aabb enclosing the (potentially rotated) viewport.
    pub fn viewport(&self) -> Aabb {
        let viewport_surface = Aabb::new(Vector2::ZERO, self.size);
        self.transform().inverse().transform_aabb(viewport_surface)
    }

    /// The current viewport center in document coordinate space.
    pub fn viewport_center(&self) -> Vector2 {
        self.transform().inverse().transform_point2(self.size * 0.5)
    }

    /// Set the viewport center.
    ///
    /// `center` must be in document coordinate space.
    pub fn set_viewport_center(&mut self, center: Vector2) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        self.offset = self.document_to_view_transform().transform_point2(center) - self.size * 0.5;

        widget_flags.view_modified = true;
        widget_flags
    }

    /// The transform from document coords to view coords (rotation + scale only).
    ///
    /// To get the inverse, call `.inverse()`.
    pub fn document_to_view_transform(&self) -> DAffine2 {
        // LHS is applied onto RHS: rotate -> scale
        DAffine2::from_scale(Vector2::splat(self.total_zoom()))
            * DAffine2::from_angle(self.rotation)
    }

    /// The transform from view coords to surface coords (translation only).
    ///
    /// To get the inverse, call `.inverse()`.
    pub fn view_to_surface_transform(&self) -> DAffine2 {
        DAffine2::from_translation(-self.offset)
    }

    /// The transform from document coords to surface coords (full).
    ///
    /// To get the inverse, call `.inverse()`.
    pub fn transform(&self) -> DAffine2 {
        // LHS is applied onto RHS: document -> view -> surface
        self.view_to_surface_transform() * self.document_to_view_transform()
    }

    /// The gsk transform for the GTK snapshot function.
    ///
    /// GTKs transformations are applied on its coordinate system,
    /// so we need to reverse the transformation order (translate, then scale, then rotate).
    /// Small rotation angles seem to cause major rendering issues.
    ///
    /// To get the inverse, call .invert().
    #[cfg(feature = "ui")]
    pub fn transform_for_gtk_snapshot(&self) -> gtk4::gsk::Transform {
        let total_zoom = self.total_zoom();

        gtk4::gsk::Transform::new()
            .translate(&gtk4::graphene::Point::new(
                -self.offset[0] as f32,
                -self.offset[1] as f32,
            ))
            .scale(total_zoom as f32, total_zoom as f32)
            .rotate(self.rotation.to_degrees() as f32)
    }

    /// Detects if a nudge is needed, meaning: the position is close to an edge of the current viewport.
    pub fn detect_nudge_needed(&self, pos: Vector2) -> Option<NudgeDirection> {
        const NUDGE_VIEWPORT_DIST: f64 = 10.0;

        // Transform position into surface coordinates and compare against the surface viewport.
        let pos_surface = self.transform().transform_point2(pos);

        let nudge_north = pos_surface.y <= NUDGE_VIEWPORT_DIST;
        let nudge_east = pos_surface.x >= self.size[0] - NUDGE_VIEWPORT_DIST;
        let nudge_south = pos_surface.y >= self.size[1] - NUDGE_VIEWPORT_DIST;
        let nudge_west = pos_surface.x <= NUDGE_VIEWPORT_DIST;

        match (nudge_north, nudge_east, nudge_south, nudge_west) {
            (true, false, _, false) => Some(NudgeDirection::North),
            (true, true, _, _) => Some(NudgeDirection::NorthEast),
            (false, true, false, _) => Some(NudgeDirection::East),
            (_, true, true, _) => Some(NudgeDirection::SouthEast),
            (_, false, true, false) => Some(NudgeDirection::South),
            (_, _, true, true) => Some(NudgeDirection::SouthWest),
            (false, _, false, true) => Some(NudgeDirection::West),
            (true, _, _, true) => Some(NudgeDirection::NorthWest),
            (false, false, false, false) => None,
        }
    }

    pub fn nudge_by(
        &mut self,
        amount: f64,
        direction: NudgeDirection,
        doc: &Document,
    ) -> WidgetFlags {
        let nudge_offset = match direction {
            NudgeDirection::North => Vector2::new(0., -amount),
            NudgeDirection::NorthEast => Vector2::new(amount, -amount),
            NudgeDirection::East => Vector2::new(amount, 0.),
            NudgeDirection::SouthEast => Vector2::new(amount, amount),
            NudgeDirection::South => Vector2::new(0., amount),
            NudgeDirection::SouthWest => Vector2::new(-amount, amount),
            NudgeDirection::West => Vector2::new(-amount, 0.),
            NudgeDirection::NorthWest => Vector2::new(-amount, -amount),
        };
        self.set_offset(self.offset() + nudge_offset, doc)
    }

    pub fn nudge(&mut self, direction: NudgeDirection, doc: &Document) -> WidgetFlags {
        const NUDGE_AMOUNT: f64 = 20.0;
        self.nudge_by(NUDGE_AMOUNT, direction, doc)
    }

    pub fn nudge_w_pos(&mut self, pos: Vector2, doc: &Document) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        if let Some(nudge_direction) = self.detect_nudge_needed(pos) {
            widget_flags |= self.nudge(nudge_direction, doc);
        }
        widget_flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn transform_vec() {
        let offset = Vector2::new(4.0, 2.0);
        let zoom = 1.5;
        let camera = Camera::default().with_zoom(zoom).with_offset(offset);

        // Point in document coordinates
        let p0 = Vector2::new(10.0, 2.0);

        // first zoom, then scale
        assert_relative_eq!(
            camera.transform().transform_point2(p0),
            (p0 * zoom) - offset
        );
    }

    #[test]
    fn rotation_snaps_to_level_within_the_given_distance() {
        let snap = 3_f64.to_radians();
        let mut camera = Camera::default();

        for (degrees, expected) in [(2.9, 0.0), (-2.9, 0.0), (3.1, 3.1), (-3.1, -3.1)] {
            let _ = camera.set_rotation(f64::to_radians(degrees), snap);
            assert_relative_eq!(camera.rotation().to_degrees(), expected, epsilon = 1e-9);
        }

        // Only level snaps this far: other angles keep the small snap to 45 degree steps.
        let _ = camera.set_rotation(f64::to_radians(178.0), snap);
        assert_relative_eq!(camera.rotation().to_degrees(), 178.0, epsilon = 1e-9);
        let _ = camera.set_rotation(f64::to_radians(45.5), snap);
        assert_relative_eq!(camera.rotation().to_degrees(), 45.0, epsilon = 1e-9);
    }

    #[test]
    fn small_turns_add_up_past_the_level_snap() {
        let snap = 10_f64.to_radians();
        let step = 5_f64.to_radians();
        let mut camera = Camera::default();

        let _ = camera.set_rotation(camera.rotation_unsnapped() + step, snap);
        assert_relative_eq!(camera.rotation(), 0.0);

        let _ = camera.set_rotation(camera.rotation_unsnapped() + step, snap);
        let _ = camera.set_rotation(camera.rotation_unsnapped() + step, snap);
        assert_relative_eq!(camera.rotation().to_degrees(), 15.0, epsilon = 1e-9);
    }

    #[test]
    fn transform_applies_rotation_before_zoom_and_offset() {
        let offset = Vector2::new(4.0, 2.0);
        let zoom = 2.0;
        let camera = Camera::default()
            .with_zoom(zoom)
            .with_offset(offset)
            .with_rotation(std::f64::consts::FRAC_PI_2);

        // A quarter turn takes (1, 0) to (0, 1).
        assert_relative_eq!(
            camera.transform().transform_point2(Vector2::new(1.0, 0.0)),
            Vector2::new(0.0, zoom) - offset,
            epsilon = 1e-9
        );
    }

    #[test]
    fn rotated_viewport_encloses_the_surface() {
        let camera = Camera::default()
            .with_offset(Vector2::ZERO)
            .with_size(Vector2::new(20.0, 10.0))
            .with_rotation(std::f64::consts::FRAC_PI_2);
        let viewport = camera.viewport();

        assert_relative_eq!(
            viewport.maxs - viewport.mins,
            Vector2::new(10.0, 20.0),
            epsilon = 1e-9
        );
    }

    #[test]
    fn viewport_center_round_trips_when_rotated() {
        let mut camera = Camera::default()
            .with_zoom(1.5)
            .with_size(Vector2::new(300.0, 200.0))
            .with_rotation(0.7);
        let center = Vector2::new(123.0, -45.0);
        let _ = camera.set_viewport_center(center);

        assert_relative_eq!(camera.viewport_center(), center, epsilon = 1e-9);
    }

    fn limited_doc() -> Document {
        let mut doc = Document::default();
        doc.config.layout = Layout::ContinuousVertical;
        doc.width = 1000.0;
        doc.height = 8000.0;
        doc
    }

    /// Where the view ends up when trying to move it far in a direction.
    fn pushed_center(camera: &mut Camera, doc: &Document, direction: Vector2) -> Vector2 {
        let _ = camera.set_offset(camera.offset() + direction * 1e6, doc);
        camera.viewport_center()
    }

    #[test]
    fn level_and_locked_is_limited_as_before() {
        let doc = limited_doc();
        let mut camera = Camera::default().with_size(Vector2::new(1400.0, 900.0));
        let _ = camera.set_offset(Vector2::new(5000.0, -5000.0), &doc);
        assert_relative_eq!(
            camera.offset(),
            Vector2::splat(-Camera::OVERSHOOT_HORIZONTAL)
        );
    }

    #[test]
    fn rotated_page_that_fits_stays_locked_in_the_middle() {
        let doc = limited_doc();
        for degrees in [2.0_f64, 10.0, 30.0] {
            let mut camera = Camera::default()
                .with_size(Vector2::new(1400.0, 900.0))
                .with_rotation(degrees.to_radians());
            let (mins, maxs) = camera.page_center_range(&doc).unwrap();
            assert_relative_eq!(mins.x, doc.x + doc.width * 0.5, epsilon = 1e-9);
            assert_relative_eq!(maxs.x, doc.x + doc.width * 0.5, epsilon = 1e-9);
            let right = pushed_center(&mut camera, &doc, Vector2::X);
            let left = pushed_center(&mut camera, &doc, Vector2::NEG_X);
            // Pushing sideways only slides the view along the page, never across it.
            let across = |c: Vector2| c.x - (doc.x + doc.width * 0.5);
            assert!(across(right).abs() < 1e-6 && across(left).abs() < 1e-6);
        }
    }

    #[test]
    fn corners_of_a_page_that_fits_are_reachable_when_rotated() {
        let doc = limited_doc();
        let size = Vector2::new(1400.0, 900.0);
        for degrees in (0..360).step_by(15).filter(|d| d % 90 != 0).map(f64::from) {
            let mut camera = Camera::default()
                .with_size(size)
                .with_rotation(degrees.to_radians());
            let (mins, maxs) = camera.page_center_range(&doc).unwrap();
            for (center_y, corner_y) in [(mins.y, doc.y), (maxs.y, doc.y + doc.height)] {
                for (center_x, corner_x) in [(mins.x, doc.x), (maxs.x, doc.x + doc.width)] {
                    let _ = camera.set_viewport_center(Vector2::new(center_x, center_y));
                    let on_surface = camera
                        .transform()
                        .transform_point2(Vector2::new(corner_x, corner_y));
                    assert!(
                        on_surface.x > -1.0
                            && on_surface.x < size.x + 1.0
                            && on_surface.y > -1.0
                            && on_surface.y < size.y + 1.0,
                        "corner ({corner_x}, {corner_y}) at {degrees} degrees is at {on_surface}"
                    );
                }
            }
        }
    }

    #[test]
    fn rotated_page_still_moves_along_its_length() {
        let doc = limited_doc();
        let mut camera = Camera::default()
            .with_size(Vector2::new(1400.0, 900.0))
            .with_rotation(30_f64.to_radians());
        let start = pushed_center(&mut camera, &doc, Vector2::NEG_Y);
        let end = pushed_center(&mut camera, &doc, Vector2::Y);
        assert!((end.y - start.y).abs() > 5000.0, "{start} to {end}");
    }

    #[test]
    fn corners_are_reachable_when_rotated() {
        let doc = limited_doc();
        let size = Vector2::new(800.0, 600.0);
        for degrees in (0..360).step_by(15).map(f64::from) {
            let mut camera = Camera::default()
                .with_size(size)
                .with_zoom(3.0)
                .with_rotation(degrees.to_radians());
            let (mins, maxs) = camera.page_center_range(&doc).unwrap_or((
                doc.bounds().mins + size / 6.0,
                doc.bounds().maxs - size / 6.0,
            ));
            for corner_center in [
                mins,
                maxs,
                Vector2::new(mins.x, maxs.y),
                Vector2::new(maxs.x, mins.y),
            ] {
                let _ = camera.set_viewport_center(corner_center);
                let page_corner = Vector2::new(
                    if corner_center.x == mins.x {
                        doc.x
                    } else {
                        doc.x + doc.width
                    },
                    if corner_center.y == mins.y {
                        doc.y
                    } else {
                        doc.y + doc.height
                    },
                );
                let on_surface = camera.transform().transform_point2(page_corner);
                assert!(
                    on_surface.x > -1.0
                        && on_surface.x < size.x + 1.0
                        && on_surface.y > -1.0
                        && on_surface.y < size.y + 1.0,
                    "corner {page_corner} at {degrees} degrees is at {on_surface}"
                );
            }
        }
    }

    #[test]
    fn scrollable_area_contains_every_position() {
        let doc = limited_doc();
        for (free, degrees) in [(false, 30.0), (true, 0.0), (true, 45.0_f64)] {
            let mut camera = Camera::default()
                .with_size(Vector2::new(1400.0, 900.0))
                .with_rotation(degrees.to_radians());
            camera.set_free_panning(free);
            for direction in [Vector2::X, Vector2::NEG_X, Vector2::Y, Vector2::NEG_Y] {
                let _ = camera.set_offset(camera.offset() + direction * 1e6, &doc);
                let (mins, maxs) = camera.surface_mins_maxs(&doc);
                let offset = camera.offset();
                assert!(offset.x >= mins.x - 1e-6 && offset.y >= mins.y - 1e-6);
                assert!(offset.x + 1400.0 <= maxs.x + 1e-6 && offset.y + 900.0 <= maxs.y + 1e-6);
            }
        }
    }

    #[test]
    fn free_panning_moves_a_page_edge_to_the_far_side_of_the_view() {
        let size = Vector2::new(1400.0, 900.0);
        for layout in [
            Layout::FixedSize,
            Layout::ContinuousVertical,
            Layout::SemiInfinite,
        ] {
            let mut doc = limited_doc();
            doc.config.layout = layout;
            let mut camera = Camera::default().with_size(size);
            camera.set_free_panning(true);

            let right = pushed_center(&mut camera, &doc, Vector2::NEG_X);
            let left_edge = camera.transform().transform_point2(Vector2::ZERO).x;
            assert_relative_eq!(
                left_edge,
                size.x - Camera::OVERSHOOT_HORIZONTAL,
                epsilon = 1e-6
            );
            assert!(right.x < 0.0, "{right}");

            let _ = pushed_center(&mut camera, &doc, Vector2::X);
            let right_edge = camera
                .transform()
                .transform_point2(Vector2::new(doc.width, 0.0))
                .x;
            assert_relative_eq!(right_edge, Camera::OVERSHOOT_HORIZONTAL, epsilon = 1e-6);
        }
    }

    #[test]
    fn free_panning_leaves_the_infinite_layout_alone() {
        let mut doc = limited_doc();
        doc.config.layout = Layout::Infinite;
        let mut camera = Camera::default().with_size(Vector2::new(1400.0, 900.0));
        camera.set_free_panning(true);
        assert!(camera.page_center_range(&doc).is_none());
    }

    #[test]
    fn locked_semi_infinite_stays_limited_in_screen_axes() {
        let mut doc = limited_doc();
        doc.config.layout = Layout::SemiInfinite;
        let camera = Camera::default()
            .with_size(Vector2::new(1400.0, 900.0))
            .with_rotation(30_f64.to_radians());
        assert!(camera.page_center_range(&doc).is_none());
    }

    #[test]
    fn viewport() {
        let zoom = 2.0;
        let offset = Vector2::splat(10.0);
        let size = Vector2::new(20.0, 30.0);

        let camera = Camera::default()
            .with_zoom(zoom)
            .with_offset(offset)
            .with_size(size);

        let mins = offset / zoom;
        let maxs = (offset + size) / zoom;
        let viewport = camera.viewport();

        assert_relative_eq!(viewport.mins, mins);
        assert_relative_eq!(viewport.maxs, maxs);
    }
}
