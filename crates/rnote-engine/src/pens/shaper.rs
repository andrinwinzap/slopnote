// Imports
use super::PenBehaviour;
use super::PenStyle;
use crate::engine::{EngineView, EngineViewMut};
use crate::store::StrokeKey;
use crate::strokes::Stroke;
use crate::strokes::{AxisEnd, AxisTicks, AxisTicksStyle, ShapeStroke};
use crate::{DrawableOnDoc, WidgetFlags};
use p2d::bounding_volume::Aabb;
use p2d::math::Vector2;
use piet::RenderContext;
use rnote_compose::Shape;
use rnote_compose::builders::buildable::{Buildable, BuilderCreator, BuilderProgress};
use rnote_compose::builders::{ArrowBuilder, GridBuilder, PolygonBuilder, PolylineBuilder};
use rnote_compose::builders::{
    CoordSystem2DBuilder, CoordSystem3DBuilder, CubBezBuilder, EllipseBuilder, FociEllipseBuilder,
    LineBuilder, QuadBezBuilder, QuadrantCoordSystem2DBuilder, RectangleBuilder, ShapeBuilderType,
};
use rnote_compose::eventresult::{EventPropagation, EventResult};
use rnote_compose::penevent::{KeyboardKey, ModifierKey, PenEvent, PenProgress};
use rnote_compose::penpath::Element;
use std::time::Instant;

#[derive(Debug)]
enum ShaperState {
    Idle,
    BuildShape {
        builder: Box<dyn Buildable<Emit = Shape>>,
        builder_type: ShapeBuilderType,
        /// The sum of the pen pressure of all pressed events, for the mean pressure of the shape.
        pressure_sum: f64,
        pressure_count: u32,
    },
    /// A coordinate system was just drawn, and its ticks and the ends of its axes can still be
    /// changed.
    EditCoordSystem {
        keys: Vec<StrokeKey>,
        builder_type: ShapeBuilderType,
        settings: CoordSystemSettings,
        anchor: Aabb,
    },
}

/// How a coordinate system is marked: its ticks, and the ends of its axes.
#[derive(Debug, Clone, PartialEq)]
pub struct CoordSystemSettings {
    /// Whether the axes have ticks.
    pub ticks: bool,
    /// The number of ticks on each half-axis, or on the shortest one with constant spacing.
    pub count: u32,
    /// Whether all half-axes have the same distance between ticks.
    pub constant_spacing: bool,
    /// What the ticks look like.
    pub tick_style: AxisTicksStyle,
    /// Whether the positive half-axes end in an arrow head.
    pub arrows: bool,
    /// Whether the positive half-axes are named at their tip.
    pub labels: bool,
    /// The names of the x, y and z axis.
    pub label_names: [String; 3],
    /// Whether the third axis of a 3D coordinate system grows into the page.
    pub third_into_page: bool,
}

/// The coordinate system that can currently be edited.
#[derive(Debug, Clone)]
pub struct CoordSystemEdit {
    /// How it is marked.
    pub settings: CoordSystemSettings,
    /// Whether it is a 3D coordinate system, which has a third axis.
    pub three_d: bool,
    /// The bounds of the coordinate system.
    pub anchor: Aabb,
}

fn is_coord_system(builder_type: ShapeBuilderType) -> bool {
    matches!(
        builder_type,
        ShapeBuilderType::CoordSystem2D
            | ShapeBuilderType::QuadrantCoordSystem2D
            | ShapeBuilderType::CoordSystem3D
    )
}

/// Which axis a half-axis running from the origin in the given direction belongs to, as an index
/// into the axis names (x, y, z), and whether it is the negative half.
///
/// This follows the usual convention, regardless of the direction the axis was drawn in: x grows
/// to the right and y upwards, and in 3D coordinate systems the x axis runs across the page,
/// towards the lower left out of it (or the upper right into it), y to the right and z upwards.
fn half_axis_role(
    builder_type: ShapeBuilderType,
    dir: Vector2,
    third_into_page: bool,
) -> (usize, bool) {
    let three_d = matches!(builder_type, ShapeBuilderType::CoordSystem3D);
    let (dx, dy) = (dir.x.abs(), dir.y.abs());
    if three_d && (dx - dy).abs() <= 1e-6 * (dx + dy) {
        let towards_lower_left = dir.x < 0.0;
        (0, towards_lower_left == third_into_page)
    } else if dx > dy {
        (if three_d { 1 } else { 0 }, dir.x < 0.0)
    } else {
        // Surface coordinates grow downwards
        (if three_d { 2 } else { 1 }, dir.y > 0.0)
    }
}

/// The lengths of the half-axes of a coordinate system.
fn half_axis_lengths(store: &crate::store::StrokeStore, keys: &[StrokeKey]) -> Vec<f64> {
    keys.iter()
        .map(|&key| match store.get_stroke_ref(key) {
            Some(Stroke::ShapeStroke(ShapeStroke {
                shape: Shape::Line(line),
                ..
            })) => (line.end - line.start).length(),
            _ => 0.0,
        })
        .collect()
}

/// The gap between the last tick of an axis and its arrow head.
const ARROW_TICK_GAP: f64 = 4.0;

/// Mark the half-axes of a coordinate system as the settings say: their ticks, and the arrow
/// head and name at the end of the positive ones.
fn apply_coord_system(
    store: &mut crate::store::StrokeStore,
    keys: &[StrokeKey],
    builder_type: ShapeBuilderType,
    settings: &CoordSystemSettings,
) {
    // With arrows the ticks stop short of the arrow heads, on every half-axis alike, so that the
    // last one does not sit on the arrow and the spacing stays the same on all of them.
    let lengths = half_axis_lengths(store, keys);
    let arrow_size = settings.tick_style.tick_size * AxisEnd::ARROW_SIZE_FACTOR;
    let margin = if settings.arrows {
        let stroke_width = keys
            .first()
            .and_then(|&key| match store.get_stroke_ref(key) {
                Some(Stroke::ShapeStroke(shapestroke)) => Some(shapestroke.style.stroke_width()),
                _ => None,
            })
            .unwrap_or(1.0);
        AxisEnd::arrow_reach(arrow_size + stroke_width) + ARROW_TICK_GAP
    } else {
        0.0
    };
    let tick_lengths = lengths
        .iter()
        .map(|length| (length - margin).max(length * 0.5))
        .collect::<Vec<f64>>();
    let layouts = AxisTicks::layouts(&tick_lengths, settings.count, settings.constant_spacing)
        .into_iter()
        .zip(lengths.iter().zip(&tick_lengths))
        .map(|((count, fraction), (&length, &tick_length))| {
            // The fractions are of the shorter length the ticks are spread over
            let fraction = fraction.unwrap_or(1.0 / count.max(1) as f64);
            let fraction = if length > 0.0 {
                fraction * tick_length / length
            } else {
                fraction
            };
            (count, Some(fraction))
        })
        .collect::<Vec<_>>();
    let label_font_size = settings.tick_style.font_size * AxisEnd::LABEL_FONT_FACTOR;
    for (&key, (axis_count, spacing_fraction)) in keys.iter().zip(layouts) {
        let Some(Stroke::ShapeStroke(shapestroke)) = store.get_stroke_mut(key) else {
            continue;
        };
        let Shape::Line(line) = &shapestroke.shape else {
            continue;
        };
        let (name, negative) = half_axis_role(
            builder_type,
            line.end - line.start,
            settings.third_into_page,
        );

        let ticks = shapestroke.axis_ticks.take();
        shapestroke.axis_ticks = settings.ticks.then(|| {
            let mut ticks =
                ticks.unwrap_or_else(|| AxisTicks::new(axis_count, settings.tick_style, negative));
            ticks.negative = negative;
            ticks.set_layout(axis_count, spacing_fraction);
            ticks.set_style(settings.tick_style);
            ticks
        });
        shapestroke.axis_end =
            (!negative && (settings.arrows || settings.labels)).then(|| AxisEnd {
                arrow: settings.arrows,
                arrow_size,
                label: if settings.labels {
                    settings.label_names[name].clone()
                } else {
                    String::new()
                },
                font_size: label_font_size,
            });
    }
    store.update_geometry_for_strokes(keys);
}

fn mean_pressure(sum: f64, count: u32) -> f64 {
    if count == 0 {
        Element::PRESSURE_DEFAULT
    } else {
        sum / count as f64
    }
}

#[derive(Debug)]
pub struct Shaper {
    state: ShaperState,
}

impl Default for Shaper {
    fn default() -> Self {
        Self {
            state: ShaperState::Idle,
        }
    }
}

impl PenBehaviour for Shaper {
    fn init(&mut self, _engine_view: &EngineView) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn deinit(&mut self) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        if matches!(self.state, ShaperState::EditCoordSystem { .. }) {
            self.state = ShaperState::Idle;
            widget_flags.axis_ticks_popup = Some(false);
        }
        widget_flags
    }

    fn style(&self) -> PenStyle {
        PenStyle::Shaper
    }

    fn update_state(&mut self, _engine_view: &mut EngineViewMut) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn handle_event(
        &mut self,
        event: PenEvent,
        now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();

        // A new pen press ends the editing of the last coordinate system.
        if matches!(self.state, ShaperState::EditCoordSystem { .. })
            && matches!(event, PenEvent::Down { .. })
        {
            widget_flags |= self.finish_coord_system_edit(engine_view);
        }

        let event_result = match (&mut self.state, event) {
            (ShaperState::EditCoordSystem { .. }, _) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (ShaperState::Idle, PenEvent::Down { mut element, .. }) => {
                engine_view
                    .config
                    .pens_config
                    .shaper_config
                    .new_style_seeds();
                element.pos = engine_view
                    .document
                    .snap_position(element.pos, engine_view.config);

                let builder_type = engine_view.config.pens_config.shaper_config.builder_type;
                self.state = ShaperState::BuildShape {
                    builder: new_builder(builder_type, element, now),
                    builder_type,
                    pressure_sum: element.pressure,
                    pressure_count: 1,
                };

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            (ShaperState::Idle, _) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (ShaperState::BuildShape { .. }, PenEvent::Cancel) => {
                self.state = ShaperState::Idle;

                EventResult {
                    handled: false,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (
                ShaperState::BuildShape {
                    builder,
                    builder_type,
                    pressure_sum,
                    pressure_count,
                },
                mut event,
            ) => {
                if let PenEvent::Down { element, .. } = &event {
                    *pressure_sum += element.pressure;
                    *pressure_count += 1;
                }
                let pressure = mean_pressure(*pressure_sum, *pressure_count);
                let builder_type = *builder_type;

                // Use Ctrl to temporarily enable/disable constraints when the switch is off/on
                let mut constraints = engine_view
                    .config
                    .pens_config
                    .shaper_config
                    .constraints
                    .clone();
                constraints.enabled = match event {
                    PenEvent::Down {
                        ref modifier_keys, ..
                    }
                    | PenEvent::Up {
                        ref modifier_keys, ..
                    }
                    | PenEvent::Proximity {
                        ref modifier_keys, ..
                    }
                    | PenEvent::KeyPressed {
                        ref modifier_keys, ..
                    } => constraints.enabled ^ modifier_keys.contains(&ModifierKey::KeyboardCtrl),
                    PenEvent::Text { .. } | PenEvent::Cancel => false,
                };
                match &mut event {
                    PenEvent::Down { element, .. } | PenEvent::Up { element, .. } => {
                        element.pos = engine_view
                            .document
                            .snap_position(element.pos, engine_view.config);
                    }
                    _ => {}
                }

                let builder_result = builder.handle_event(event.clone(), now, constraints);
                let handled = builder_result.handled;
                let propagate = builder_result.propagate;

                let mut progress = match builder_result.progress {
                    BuilderProgress::InProgress => PenProgress::InProgress,
                    BuilderProgress::EmitContinue(shapes) => {
                        let mut style = engine_view
                            .config
                            .pens_config
                            .shaper_config
                            .gen_style_with_pressure(pressure);
                        let shapes_emitted = !shapes.is_empty();

                        for shape in shapes {
                            let key = engine_view.store.insert_stroke(
                                Stroke::ShapeStroke(ShapeStroke::new(shape, style.clone())),
                                None,
                            );
                            style.advance_seed();
                            engine_view.store.regenerate_rendering_for_stroke(
                                key,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }

                        if shapes_emitted {
                            widget_flags |= engine_view.store.record(Instant::now());
                            widget_flags.store_modified = true;
                        }
                        PenProgress::InProgress
                    }
                    BuilderProgress::Finished(shapes) => {
                        let shaper_config = &engine_view.config.pens_config.shaper_config;
                        let mut style = shaper_config.gen_style_with_pressure(pressure);
                        let coord_system = is_coord_system(builder_type)
                            && shapes.iter().all(|shape| matches!(shape, Shape::Line(_)));
                        let tick_spacing = shaper_config.axis_tick_spacing;
                        // The default count gives the shortest half-axis about the default spacing
                        let tick_count = shapes
                            .iter()
                            .filter_map(|shape| match shape {
                                Shape::Line(line) => Some((line.end - line.start).length()),
                                _ => None,
                            })
                            .fold(f64::INFINITY, f64::min);
                        let tick_count = if tick_count.is_finite() {
                            ((tick_count / tick_spacing).round() as u32)
                                .clamp(AxisTicks::COUNT_MIN, AxisTicks::COUNT_MAX)
                        } else {
                            AxisTicks::COUNT_MIN
                        };

                        let settings = shaper_config.coord_system_settings(tick_count);

                        let shapes_emitted = !shapes.is_empty();
                        let mut keys = Vec::with_capacity(shapes.len());
                        for shape in shapes {
                            let stroke = ShapeStroke::new(shape, style.clone());
                            let key = engine_view
                                .store
                                .insert_stroke(Stroke::ShapeStroke(stroke), None);
                            keys.push(key);
                            style.advance_seed();
                            engine_view.store.regenerate_rendering_for_stroke(
                                key,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }

                        if coord_system {
                            apply_coord_system(engine_view.store, &keys, builder_type, &settings);
                            engine_view.store.regenerate_rendering_for_strokes(
                                &keys,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }

                        self.state = ShaperState::Idle;

                        if shapes_emitted {
                            widget_flags |= engine_view
                                .document
                                .resize_autoexpand(engine_view.store, engine_view.camera)
                                | engine_view.store.record(Instant::now());
                            widget_flags.store_modified = true;

                            if coord_system
                                && let Some(anchor) = engine_view.store.bounds_for_strokes(&keys)
                            {
                                self.state = ShaperState::EditCoordSystem {
                                    keys,
                                    builder_type,
                                    settings,
                                    anchor,
                                };
                                widget_flags.axis_ticks_popup = Some(true);
                            }
                        }
                        PenProgress::Finished
                    }
                };

                // When esc is pressed, reset state
                if let PenEvent::KeyPressed {
                    keyboard_key,
                    modifier_keys,
                } = event
                    && keyboard_key == KeyboardKey::Escape
                    && modifier_keys.is_empty()
                {
                    self.state = ShaperState::Idle;
                    progress = PenProgress::Finished;
                }

                EventResult {
                    handled,
                    propagate,
                    progress,
                }
            }
        };

        (event_result, widget_flags)
    }
}

impl Shaper {
    /// The coordinate system that can currently be edited.
    pub fn coord_system_edit(&self) -> Option<CoordSystemEdit> {
        match &self.state {
            ShaperState::EditCoordSystem {
                builder_type,
                settings,
                anchor,
                ..
            } => Some(CoordSystemEdit {
                settings: settings.clone(),
                three_d: matches!(builder_type, ShapeBuilderType::CoordSystem3D),
                anchor: *anchor,
            }),
            _ => None,
        }
    }

    /// Change how the coordinate system being edited is marked.
    ///
    /// Everything but the number of ticks also becomes what the next coordinate system gets, as
    /// the shaper settings show it.
    pub fn update_coord_system(
        &mut self,
        mut new_settings: CoordSystemSettings,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        let ShaperState::EditCoordSystem {
            keys,
            builder_type,
            settings,
            anchor,
            ..
        } = &mut self.state
        else {
            return widget_flags;
        };
        new_settings.count = new_settings
            .count
            .clamp(AxisTicks::COUNT_MIN, AxisTicks::COUNT_MAX);
        if new_settings == *settings {
            return widget_flags;
        }
        // The number of ticks is the coordinate system's own, everything else becomes the default
        let defaults_changed = CoordSystemSettings {
            count: settings.count,
            ..new_settings.clone()
        } != *settings;
        if defaults_changed {
            engine_view
                .config
                .pens_config
                .shaper_config
                .set_coord_system_settings(&new_settings);
            widget_flags.refresh_ui = true;
        }
        *settings = new_settings;

        apply_coord_system(engine_view.store, keys, *builder_type, settings);
        engine_view.store.regenerate_rendering_for_strokes(
            keys,
            engine_view.camera.viewport(),
            engine_view.camera.image_scale(),
        );
        if let Some(bounds) = engine_view.store.bounds_for_strokes(keys) {
            *anchor = bounds;
        }

        widget_flags |= engine_view
            .document
            .resize_autoexpand(engine_view.store, engine_view.camera);
        widget_flags.store_modified = true;
        widget_flags.redraw = true;
        widget_flags
    }

    /// End editing the coordinate system. The changes become part of its history entry.
    pub fn finish_coord_system_edit(&mut self, engine_view: &mut EngineViewMut) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        if matches!(self.state, ShaperState::EditCoordSystem { .. }) {
            self.state = ShaperState::Idle;
            widget_flags |= engine_view
                .store
                .update_latest_history_entry(Instant::now());
            widget_flags.axis_ticks_popup = Some(false);
        }
        widget_flags
    }

    /// The mean pen pressure of the shape being built.
    fn current_pressure(&self) -> f64 {
        match &self.state {
            ShaperState::BuildShape {
                pressure_sum,
                pressure_count,
                ..
            } => mean_pressure(*pressure_sum, *pressure_count),
            _ => Element::PRESSURE_DEFAULT,
        }
    }
}

impl DrawableOnDoc for Shaper {
    fn bounds_on_doc(&self, engine_view: &EngineView) -> Option<Aabb> {
        let style = engine_view
            .config
            .pens_config
            .shaper_config
            .gen_style_with_pressure(self.current_pressure());

        match &self.state {
            ShaperState::Idle | ShaperState::EditCoordSystem { .. } => None,
            ShaperState::BuildShape { builder, .. } => {
                builder.bounds(&style, engine_view.camera.total_zoom())
            }
        }
    }

    fn draw_on_doc(
        &self,
        cx: &mut piet_cairo::CairoRenderContext,
        engine_view: &EngineView,
    ) -> anyhow::Result<()> {
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let style = engine_view
            .config
            .pens_config
            .shaper_config
            .gen_style_with_pressure(self.current_pressure());

        match &self.state {
            ShaperState::Idle | ShaperState::EditCoordSystem { .. } => {}
            ShaperState::BuildShape { builder, .. } => {
                builder.draw_styled(cx, &style, engine_view.camera.total_zoom())
            }
        }

        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

fn new_builder(
    builder_type: ShapeBuilderType,
    element: Element,
    now: Instant,
) -> Box<dyn Buildable<Emit = Shape>> {
    match builder_type {
        ShapeBuilderType::Arrow => Box::new(ArrowBuilder::start(element, now)),
        ShapeBuilderType::Line => Box::new(LineBuilder::start(element, now)),
        ShapeBuilderType::Rectangle => Box::new(RectangleBuilder::start(element, now)),
        ShapeBuilderType::Grid => Box::new(GridBuilder::start(element, now)),
        ShapeBuilderType::CoordSystem2D => Box::new(CoordSystem2DBuilder::start(element, now)),
        ShapeBuilderType::CoordSystem3D => Box::new(CoordSystem3DBuilder::start(element, now)),
        ShapeBuilderType::QuadrantCoordSystem2D => {
            Box::new(QuadrantCoordSystem2DBuilder::start(element, now))
        }
        ShapeBuilderType::Ellipse => Box::new(EllipseBuilder::start(element, now)),
        ShapeBuilderType::FociEllipse => Box::new(FociEllipseBuilder::start(element, now)),
        ShapeBuilderType::QuadBez => Box::new(QuadBezBuilder::start(element, now)),
        ShapeBuilderType::CubBez => Box::new(CubBezBuilder::start(element, now)),
        ShapeBuilderType::Polyline => Box::new(PolylineBuilder::start(element, now)),
        ShapeBuilderType::Polygon => Box::new(PolygonBuilder::start(element, now)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_axes_of_a_2d_coordinate_system() {
        let role = |x: f64, y: f64| {
            half_axis_role(ShapeBuilderType::CoordSystem2D, Vector2::new(x, y), false)
        };
        assert_eq!(role(10.0, 0.0), (0, false));
        assert_eq!(role(-10.0, 0.0), (0, true));
        // Surface coordinates grow downwards, so up is the positive y axis
        assert_eq!(role(0.0, -10.0), (1, false));
        assert_eq!(role(0.0, 10.0), (1, true));
    }

    #[test]
    fn half_axes_of_a_3d_coordinate_system() {
        let role = |x: f64, y: f64, into_page: bool| {
            half_axis_role(
                ShapeBuilderType::CoordSystem3D,
                Vector2::new(x, y),
                into_page,
            )
        };
        assert_eq!(role(10.0, 0.0, false), (1, false));
        assert_eq!(role(0.0, -10.0, false), (2, false));
        // The third axis comes out of the page towards the lower left
        assert_eq!(role(-7.0, 7.0, false), (0, false));
        assert_eq!(role(7.0, -7.0, false), (0, true));
        // Or goes into it towards the upper right
        assert_eq!(role(-7.0, 7.0, true), (0, true));
        assert_eq!(role(7.0, -7.0, true), (0, false));
    }
}
