// Imports
use super::Content;
use crate::{Drawable, strokes::content};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use piet::TextLayout as _;
use piet::TextLayoutBuilder as _;
use rnote_compose::Style;
use rnote_compose::Transformable;
use rnote_compose::ext::AabbExt;
use rnote_compose::shapes::Line;
use rnote_compose::shapes::Shape;
use rnote_compose::shapes::Shapeable;
use rnote_compose::style::Composer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Unit markings along an axis, drawn on top of a line shape that runs from the origin to the axis tip.
///
/// The ticks are derived from the line geometry, so they follow every transformation of the stroke.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "axis_ticks")]
pub struct AxisTicks {
    /// The number of ticks on the axis. The last one sits at the tip.
    #[serde(rename = "count")]
    pub count: u32,
    /// The distance a tick reaches to each side of the axis.
    #[serde(rename = "tick_size")]
    pub tick_size: f64,
    /// Whether the ticks are numbered.
    #[serde(rename = "numbering")]
    pub numbering: bool,
    /// Whether this is a negative half-axis, in which case the numbers are negative.
    #[serde(rename = "negative")]
    pub negative: bool,
    /// The font size of the numbers.
    #[serde(rename = "font_size")]
    pub font_size: f64,
    /// Only every this many ticks is numbered, counting from the origin.
    #[serde(rename = "label_every")]
    pub label_every: u32,
    /// The distance between two ticks, relative to the axis length.
    ///
    /// When `None`, the ticks are spread evenly with the last one at the tip.
    #[serde(rename = "spacing_fraction", skip_serializing_if = "Option::is_none")]
    pub spacing_fraction: Option<f64>,
    /// The indices (starting at 1) of the ticks that were erased.
    #[serde(rename = "erased_ticks", skip_serializing_if = "BTreeSet::is_empty")]
    pub erased_ticks: BTreeSet<u32>,
    /// The indices (starting at 1) of the numbers that were erased.
    #[serde(rename = "erased_labels", skip_serializing_if = "BTreeSet::is_empty")]
    pub erased_labels: BTreeSet<u32>,
}

impl Default for AxisTicks {
    fn default() -> Self {
        Self {
            count: 5,
            tick_size: 6.0,
            numbering: true,
            negative: false,
            font_size: 12.0,
            label_every: 1,
            spacing_fraction: None,
            erased_ticks: BTreeSet::new(),
            erased_labels: BTreeSet::new(),
        }
    }
}

/// What the unit markings of a coordinate system look like, the same on all of its axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisTicksStyle {
    /// The distance a tick reaches to each side of the axis.
    pub tick_size: f64,
    /// Whether the ticks are numbered.
    pub numbering: bool,
    /// Only every this many ticks is numbered.
    pub label_every: u32,
    /// The font size of the numbers.
    pub font_size: f64,
}

/// The geometry of a single unit marking.
struct TickPart {
    index: u32,
    tick: Line,
    label: Option<(String, kurbo::Point, Aabb)>,
}

impl AxisTicks {
    pub const COUNT_MIN: u32 = 1;
    pub const COUNT_MAX: u32 = 50;
    /// The most ticks one axis can have, which can be more than [Self::COUNT_MAX] with constant
    /// spacing on axes of different length.
    const AXIS_COUNT_MAX: u32 = 1000;
    pub const LABEL_EVERY_MIN: u32 = 1;
    pub const LABEL_EVERY_MAX: u32 = 20;
    pub const FONT_SIZE_MIN: f64 = 4.0;
    pub const FONT_SIZE_MAX: f64 = 72.0;
    /// The gap between a tick and its number.
    const LABEL_GAP: f64 = 2.0;

    pub fn new(count: u32, style: AxisTicksStyle, negative: bool) -> Self {
        let mut ticks = Self {
            count: count.clamp(Self::COUNT_MIN, Self::COUNT_MAX),
            negative,
            ..Default::default()
        };
        ticks.set_style(style);
        ticks
    }

    /// The look of the markings.
    pub fn style(&self) -> AxisTicksStyle {
        AxisTicksStyle {
            tick_size: self.tick_size,
            numbering: self.numbering,
            label_every: self.label_every,
            font_size: self.font_size,
        }
    }

    /// Change the look of the markings.
    pub fn set_style(&mut self, style: AxisTicksStyle) {
        self.tick_size = style.tick_size;
        self.numbering = style.numbering;
        self.label_every = style
            .label_every
            .clamp(Self::LABEL_EVERY_MIN, Self::LABEL_EVERY_MAX);
        self.font_size = style
            .font_size
            .clamp(Self::FONT_SIZE_MIN, Self::FONT_SIZE_MAX);
    }

    /// Set the number of ticks and their spacing relative to the axis length (`None` to spread
    /// them evenly). Erased ticks and numbers come back, since they all move.
    pub fn set_layout(&mut self, count: u32, spacing_fraction: Option<f64>) {
        self.count = count.clamp(Self::COUNT_MIN, Self::AXIS_COUNT_MAX);
        self.spacing_fraction = spacing_fraction.filter(|f| f.is_finite() && *f > 0.0);
        self.erased_ticks.clear();
        self.erased_labels.clear();
    }

    /// The tick layouts `(count, spacing_fraction)` for half-axes of the given lengths, so that the
    /// shortest one has `count` ticks.
    ///
    /// With constant spacing, all axes get the spacing of the shortest one, so longer axes get more
    /// ticks. Otherwise every axis gets `count` ticks, spread over its length.
    pub fn layouts(lengths: &[f64], count: u32, constant_spacing: bool) -> Vec<(u32, Option<f64>)> {
        let count = count.clamp(Self::COUNT_MIN, Self::COUNT_MAX);
        let shortest = lengths.iter().copied().fold(f64::INFINITY, f64::min);
        if !constant_spacing || !shortest.is_finite() || shortest <= 0.0 {
            return lengths.iter().map(|_| (count, None)).collect();
        }
        let spacing = shortest / count as f64;
        lengths
            .iter()
            .map(|&length| {
                if length <= 0.0 {
                    return (count, None);
                }
                // Tolerate rounding, so that a tick lands on the tip of axes of the same length
                let axis_count = ((length / spacing) + 1e-6).floor() as u32;
                (axis_count.max(Self::COUNT_MIN), Some(spacing / length))
            })
            .collect()
    }

    /// How far the ticks and numbers reach beyond the axis line.
    fn extent(&self) -> f64 {
        if self.numbering {
            self.tick_size + Self::LABEL_GAP + 2.0 * self.font_size
        } else {
            self.tick_size
        }
    }

    fn label_text(&self, index: u32) -> String {
        if self.negative {
            format!("-{index}")
        } else {
            index.to_string()
        }
    }

    fn build_label_layout<T: piet::Text>(
        &self,
        text: &mut T,
        label: String,
        color: piet::Color,
    ) -> Option<T::TextLayout> {
        text.new_text_layout(label)
            .font(piet::FontFamily::SYSTEM_UI, self.font_size)
            .text_color(color)
            .build()
            .ok()
    }

    /// The ticks and numbers that were not erased.
    fn parts(&self, axis: &Line) -> Vec<TickPart> {
        let dir = axis.end - axis.start;
        let len = dir.length();
        if len < 1e-6 {
            return vec![];
        }
        let unit = dir / len;
        let normal = Vector2::new(-unit.y, unit.x);
        // Numbers go below horizontal and left of vertical axes.
        let horizontal = unit.x.abs() >= unit.y.abs();
        let count = self.count.max(1);
        let spacing_fraction = self.spacing_fraction.unwrap_or(1.0 / count as f64);
        let reach = self.tick_size + Self::LABEL_GAP;
        let mut text = piet_cairo::CairoText::new();

        (1..=count)
            .map(|index| {
                let pos = axis.start + dir * (index as f64 * spacing_fraction);
                let tick = Line {
                    start: pos - normal * self.tick_size,
                    end: pos + normal * self.tick_size,
                };
                let label = (self.numbering
                    && index % self.label_every.max(1) == 0
                    && !self.erased_labels.contains(&index))
                .then(|| {
                    let label = self.label_text(index);
                    let layout =
                        self.build_label_layout(&mut text, label.clone(), piet::Color::BLACK)?;
                    let size = layout.size();
                    let top_left = if horizontal {
                        kurbo::Point::new(pos.x - size.width * 0.5, pos.y + reach)
                    } else {
                        kurbo::Point::new(pos.x - reach - size.width, pos.y - size.height * 0.5)
                    };
                    let rect = Aabb::new(
                        Vector2::new(top_left.x, top_left.y),
                        Vector2::new(top_left.x + size.width, top_left.y + size.height),
                    );
                    Some((label, top_left, rect))
                })
                .flatten();
                TickPart { index, tick, label }
            })
            .collect()
    }

    fn draw(&self, cx: &mut impl piet::RenderContext, axis: &Line, style: &Style) {
        let color = style.stroke_color().map(piet::Color::from);

        for part in self.parts(axis) {
            if !self.erased_ticks.contains(&part.index) {
                part.tick.draw_composed(cx, style);
            }
            if let (Some((label, top_left, _)), Some(color)) = (part.label, color)
                && let Some(layout) = self.build_label_layout(cx.text(), label, color)
            {
                cx.draw_text(&layout, top_left);
            }
        }
    }

    /// The hitboxes of the ticks and numbers that were not erased.
    fn hitboxes(&self, axis: &Line, stroke_width: f64) -> Vec<Aabb> {
        self.parts(axis)
            .into_iter()
            .flat_map(|part| {
                let tick = (!self.erased_ticks.contains(&part.index))
                    .then(|| part.tick.bounds().loosened(stroke_width * 0.5));
                tick.into_iter().chain(part.label.map(|(_, _, rect)| rect))
            })
            .collect()
    }

    /// Erase the ticks and numbers for which `hit` returns true. Returns whether anything was erased.
    fn erase_parts(&mut self, axis: &Line, stroke_width: f64, hit: impl Fn(&Aabb) -> bool) -> bool {
        let mut erased = false;
        for part in self.parts(axis) {
            if !self.erased_ticks.contains(&part.index)
                && hit(&part.tick.bounds().loosened(stroke_width * 0.5))
            {
                erased |= self.erased_ticks.insert(part.index);
            }
            if let Some((_, _, rect)) = part.label
                && hit(&rect)
            {
                erased |= self.erased_labels.insert(part.index);
            }
        }
        erased
    }
}

/// What erasing at a place did to a coordinate system axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AxisErase {
    /// The place does not touch the axis.
    Miss,
    /// Only ticks or numbers were erased.
    Parts,
    /// The axis line itself was hit.
    Axis,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename = "shapestroke")]
pub struct ShapeStroke {
    #[serde(rename = "shape")]
    pub shape: Shape,
    #[serde(rename = "style")]
    pub style: Style,
    /// Unit markings, for shapes that are the half-axis of a coordinate system.
    #[serde(rename = "axis_ticks", skip_serializing_if = "Option::is_none")]
    pub axis_ticks: Option<AxisTicks>,
    #[serde(skip)]
    // since the shape can have many hitboxes, we store them and update them when the stroke geometry changes
    hitboxes: Vec<Aabb>,
}

impl Content for ShapeStroke {
    fn draw_highlight(
        &self,
        cx: &mut impl piet::RenderContext,
        total_zoom: f64,
    ) -> anyhow::Result<()> {
        const PATH_HIGHLIGHT_MIN_STROKE_WIDTH: f64 = 5.0;
        const DRAW_BOUNDS_THRESHOLD_AREA: f64 = 10_u32.pow(2) as f64;
        let bounds = self.bounds();
        let bez_path = self.shape.outline_path();

        if bounds.scale(total_zoom).volume() < DRAW_BOUNDS_THRESHOLD_AREA {
            cx.fill(bounds.to_kurbo_rect(), &content::CONTENT_HIGHLIGHT_COLOR);
        } else {
            cx.stroke_styled(
                bez_path,
                &content::CONTENT_HIGHLIGHT_COLOR,
                (PATH_HIGHLIGHT_MIN_STROKE_WIDTH / total_zoom)
                    .max(self.style.stroke_width() + 10.0 / total_zoom),
                &piet::StrokeStyle::new()
                    .line_join(piet::LineJoin::Round)
                    .line_cap(piet::LineCap::Round),
            );
        }

        Ok(())
    }

    fn update_geometry(&mut self) {
        self.hitboxes = self.gen_hitboxes_int();
    }
}

impl Drawable for ShapeStroke {
    fn draw(&self, cx: &mut impl piet::RenderContext, _image_scale: f64) -> anyhow::Result<()> {
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        self.shape.draw_composed(cx, &self.style);
        if let (Some(ticks), Shape::Line(axis)) = (&self.axis_ticks, &self.shape) {
            ticks.draw(cx, axis, &self.style);
        }

        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

impl Shapeable for ShapeStroke {
    fn bounds(&self) -> Aabb {
        let bounds = match &self.style {
            Style::Smooth(options) => self.shape.composed_bounds(options),
            Style::Rough(options) => self.shape.composed_bounds(options),
            Style::Textured(_) => self.shape.bounds(),
        };
        match &self.axis_ticks {
            Some(ticks) => bounds.loosened(ticks.extent()),
            None => bounds,
        }
    }

    fn hitboxes(&self) -> Vec<Aabb> {
        self.hitboxes.clone()
    }

    fn outline_path(&self) -> kurbo::BezPath {
        self.shape.outline_path()
    }
}

impl Transformable for ShapeStroke {
    fn translate(&mut self, offset: Vector2) {
        self.shape.translate(offset);
    }
    fn rotate(&mut self, angle: f64, center: Vector2) {
        self.shape.rotate(angle, center);
    }
    fn scale(&mut self, scale: Vector2) {
        self.shape.scale(scale);
        // Using the geometric mean behaves the best when scaling non-uniformly.
        let scale_scalar = (scale[0] * scale[1]).sqrt();
        self.style
            .set_stroke_width(self.style.stroke_width() * scale_scalar);
        if let Some(ticks) = &mut self.axis_ticks {
            ticks.tick_size *= scale_scalar;
            ticks.font_size *= scale_scalar;
        }
    }
}

impl ShapeStroke {
    pub fn new(shape: Shape, style: Style) -> Self {
        let mut shapestroke = Self {
            shape,
            style,
            axis_ticks: None,
            hitboxes: vec![],
        };
        shapestroke.update_geometry();

        shapestroke
    }

    /// A new shape stroke that is a half-axis carrying unit markings.
    pub fn new_axis(shape: Shape, style: Style, axis_ticks: AxisTicks) -> Self {
        let mut shapestroke = Self::new(shape, style);
        shapestroke.axis_ticks = Some(axis_ticks);
        shapestroke
    }

    /// The hitboxes of the shape itself, without unit markings.
    fn shape_hitboxes(&self) -> Vec<Aabb> {
        let width = self.style.stroke_width();

        self.shape
            .hitboxes()
            .into_iter()
            .map(|hitbox| hitbox.loosened(width * 0.5))
            .collect()
    }

    fn gen_hitboxes_int(&self) -> Vec<Aabb> {
        let mut hitboxes = self.shape_hitboxes();
        if let (Some(ticks), Shape::Line(axis)) = (&self.axis_ticks, &self.shape) {
            hitboxes.extend(ticks.hitboxes(axis, self.style.stroke_width()));
        }
        hitboxes
    }

    /// For an axis with unit markings, erase the ticks and numbers for which `hit` returns true.
    ///
    /// Ticks and numbers take precedence: the axis line only counts as hit when no tick or number
    /// was, since every tick crosses the axis. Returns `None` for strokes without unit markings.
    pub(crate) fn erase_axis_parts(&mut self, hit: impl Fn(&Aabb) -> bool) -> Option<AxisErase> {
        let stroke_width = self.style.stroke_width();
        let shape_hit = self.shape_hitboxes().iter().any(&hit);
        let (Some(ticks), Shape::Line(axis)) = (&mut self.axis_ticks, &self.shape) else {
            return None;
        };
        if ticks.erase_parts(axis, stroke_width, &hit) {
            self.update_geometry();
            Some(AxisErase::Parts)
        } else if shape_hit {
            Some(AxisErase::Axis)
        } else {
            Some(AxisErase::Miss)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_count_spreads_the_same_number_of_ticks_on_every_axis() {
        assert_eq!(
            AxisTicks::layouts(&[100.0, 200.0], 4, false),
            vec![(4, None), (4, None)]
        );
    }

    #[test]
    fn constant_spacing_gives_longer_axes_more_ticks() {
        let layouts = AxisTicks::layouts(&[100.0, 250.0, 100.0], 4, true);
        let counts = layouts.iter().map(|(c, _)| *c).collect::<Vec<_>>();
        assert_eq!(counts, vec![4, 10, 4]);
        // Same absolute spacing of 25 on every axis
        for ((_, fraction), length) in layouts.iter().zip([100.0, 250.0, 100.0]) {
            assert!((fraction.unwrap() * length - 25.0).abs() < 1e-9);
        }
    }

    #[test]
    fn only_every_nth_tick_is_numbered() {
        let style = AxisTicksStyle {
            tick_size: 6.0,
            numbering: true,
            label_every: 2,
            font_size: 12.0,
        };
        let ticks = AxisTicks::new(6, style, false);
        let axis = Line {
            start: Vector2::ZERO,
            end: Vector2::new(300.0, 0.0),
        };

        let numbered = ticks
            .parts(&axis)
            .into_iter()
            .filter(|part| part.label.is_some())
            .map(|part| part.index)
            .collect::<Vec<u32>>();
        assert_eq!(numbered, vec![2, 4, 6]);
    }
}
