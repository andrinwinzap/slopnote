// Imports
use super::PenBehaviour;
use super::PenStyle;
use crate::engine::{EngineView, EngineViewMut};
use crate::store::StrokeKey;
use crate::strokes::Stroke;
use crate::strokes::{AxisTicks, AxisTicksStyle, ShapeStroke};
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
    /// A coordinate system was just drawn and the number of its unit markings can still be changed.
    EditAxisTicks {
        keys: Vec<StrokeKey>,
        count: u32,
        constant_spacing: bool,
        style: AxisTicksStyle,
        anchor: Aabb,
    },
}

/// The coordinate system the unit markings of which can currently be edited.
#[derive(Debug, Clone, Copy)]
pub struct AxisTicksEdit {
    /// The number of markings on each half-axis, or on the shortest one with constant spacing.
    pub count: u32,
    /// Whether all half-axes have the same distance between markings.
    pub constant_spacing: bool,
    /// What the markings look like.
    pub style: AxisTicksStyle,
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

/// Whether a half-axis running from the origin in the given direction is negative, by the usual
/// convention and regardless of the direction it was drawn in: x grows to the right, y grows
/// upwards, and the z axis of 3D coordinate systems grows towards the lower left.
fn is_negative_half_axis(dir: Vector2) -> bool {
    let (dx, dy) = (dir.x.abs(), dir.y.abs());
    if (dx - dy).abs() <= 1e-6 * (dx + dy) {
        // Diagonal z axis
        dir.x > 0.0
    } else if dx > dy {
        dir.x < 0.0
    } else {
        // Surface coordinates grow downwards
        dir.y > 0.0
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

/// Lay out and style the ticks of all half-axes of a coordinate system for the given count.
fn apply_axis_ticks(
    store: &mut crate::store::StrokeStore,
    keys: &[StrokeKey],
    count: u32,
    constant_spacing: bool,
    style: AxisTicksStyle,
) {
    let layouts = AxisTicks::layouts(&half_axis_lengths(store, keys), count, constant_spacing);
    for (&key, (axis_count, spacing_fraction)) in keys.iter().zip(layouts) {
        if let Some(Stroke::ShapeStroke(shapestroke)) = store.get_stroke_mut(key)
            && let Some(ticks) = &mut shapestroke.axis_ticks
        {
            ticks.set_layout(axis_count, spacing_fraction);
            ticks.set_style(style);
        }
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
        if matches!(self.state, ShaperState::EditAxisTicks { .. }) {
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

        // A new pen press ends the editing of the unit markings of the last coordinate system.
        if matches!(self.state, ShaperState::EditAxisTicks { .. })
            && matches!(event, PenEvent::Down { .. })
        {
            widget_flags |= self.finish_axis_ticks_edit(engine_view);
        }

        let event_result = match (&mut self.state, event) {
            (ShaperState::EditAxisTicks { .. }, _) => EventResult {
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
                        let axis_ticks = shaper_config.axis_ticks_enabled
                            && is_coord_system(builder_type)
                            && shapes.iter().all(|shape| matches!(shape, Shape::Line(_)));
                        let (tick_style, tick_spacing, constant_spacing) = (
                            shaper_config.axis_ticks_style(),
                            shaper_config.axis_tick_spacing,
                            shaper_config.axis_tick_constant_spacing,
                        );
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

                        let shapes_emitted = !shapes.is_empty();
                        let mut keys = Vec::with_capacity(shapes.len());
                        for shape in shapes {
                            let stroke = match &shape {
                                Shape::Line(line) if axis_ticks => {
                                    let negative = is_negative_half_axis(line.end - line.start);
                                    ShapeStroke::new_axis(
                                        shape,
                                        style.clone(),
                                        AxisTicks::new(tick_count, tick_style, negative),
                                    )
                                }
                                _ => ShapeStroke::new(shape, style.clone()),
                            };
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

                        if axis_ticks {
                            apply_axis_ticks(
                                engine_view.store,
                                &keys,
                                tick_count,
                                constant_spacing,
                                tick_style,
                            );
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

                            if axis_ticks
                                && let Some(anchor) = engine_view.store.bounds_for_strokes(&keys)
                            {
                                self.state = ShaperState::EditAxisTicks {
                                    keys,
                                    count: tick_count,
                                    constant_spacing,
                                    style: tick_style,
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
    /// The coordinate system whose unit markings can currently be edited.
    pub fn axis_ticks_edit(&self) -> Option<AxisTicksEdit> {
        match &self.state {
            ShaperState::EditAxisTicks {
                count,
                constant_spacing,
                style,
                anchor,
                ..
            } => Some(AxisTicksEdit {
                count: *count,
                constant_spacing: *constant_spacing,
                style: *style,
                anchor: *anchor,
            }),
            _ => None,
        }
    }

    /// Change the unit markings of the coordinate system being edited.
    ///
    /// Everything but the count also becomes what the next coordinate system gets, as the shaper
    /// settings show it.
    pub fn update_axis_ticks(
        &mut self,
        new_count: u32,
        new_constant_spacing: bool,
        new_style: AxisTicksStyle,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        let ShaperState::EditAxisTicks {
            keys,
            count,
            constant_spacing,
            style,
            anchor,
        } = &mut self.state
        else {
            return widget_flags;
        };
        let new_count = new_count.clamp(AxisTicks::COUNT_MIN, AxisTicks::COUNT_MAX);
        if new_count == *count && new_constant_spacing == *constant_spacing && new_style == *style {
            return widget_flags;
        }
        if new_constant_spacing != *constant_spacing || new_style != *style {
            let shaper_config = &mut engine_view.config.pens_config.shaper_config;
            shaper_config.axis_tick_constant_spacing = new_constant_spacing;
            shaper_config.set_axis_ticks_style(new_style);
            widget_flags.refresh_ui = true;
        }
        *count = new_count;
        *constant_spacing = new_constant_spacing;
        *style = new_style;

        apply_axis_ticks(
            engine_view.store,
            keys,
            new_count,
            new_constant_spacing,
            new_style,
        );
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

    /// End editing the unit markings. The changes become part of the history entry of the coordinate system.
    pub fn finish_axis_ticks_edit(&mut self, engine_view: &mut EngineViewMut) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();
        if matches!(self.state, ShaperState::EditAxisTicks { .. }) {
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
            ShaperState::Idle | ShaperState::EditAxisTicks { .. } => None,
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
            ShaperState::Idle | ShaperState::EditAxisTicks { .. } => {}
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
    fn half_axis_sign_follows_convention_not_drawing_direction() {
        // Right and up are positive, left and down negative
        assert!(!is_negative_half_axis(Vector2::new(10.0, 0.0)));
        assert!(is_negative_half_axis(Vector2::new(-10.0, 0.0)));
        assert!(!is_negative_half_axis(Vector2::new(0.0, -10.0)));
        assert!(is_negative_half_axis(Vector2::new(0.0, 10.0)));
        // The 3D z axis grows towards the lower left
        assert!(!is_negative_half_axis(Vector2::new(-5.0, 5.0)));
        assert!(is_negative_half_axis(Vector2::new(5.0, -5.0)));
    }
}
