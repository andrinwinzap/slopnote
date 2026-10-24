// Imports
use super::PenBehaviour;
use super::PenStyle;
use super::pensconfig::brushconfig::BrushStyle;
use super::pensconfig::rulerconfig::{RulerConfig, RulerTurn, RulerView};
use super::shapeadjust::ShapeAdjust;
use crate::engine::{EngineTask, EngineTaskSender, EngineView, EngineViewMut};
use crate::store::StrokeKey;
use crate::strokes::BrushStroke;
use crate::strokes::ShapeStroke;
use crate::strokes::Stroke;
use crate::tasks::OneOffTaskHandle;
use crate::{DrawableOnDoc, WidgetFlags};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use p2d::math::Vector2;
use piet::RenderContext;
use rnote_compose::Constraints;
use rnote_compose::Style;
use rnote_compose::builders::buildable::{Buildable, BuilderCreator, BuilderProgress};
use rnote_compose::builders::{
    PenPathBuilderType, PenPathCurvedBuilder, PenPathModeledBuilder, PenPathSimpleBuilder,
};
use rnote_compose::eventresult::{EventPropagation, EventResult};
use rnote_compose::penevent::{PenEvent, PenProgress};
use rnote_compose::penpath::{Element, Segment};
use rnote_compose::scribblerecognition;
use rnote_compose::shaperecognition;
use rnote_compose::shapes::Shape;
use std::time::{Duration, Instant};

#[derive(Debug)]
enum BrushState {
    Idle,
    Drawing {
        path_builder: Box<dyn Buildable<Emit = Segment>>,
        current_stroke_key: StrokeKey,
        preview_style: Style,
        /// If the stroke started snapped to the ruler, the side of the edge
        /// (`+1.0` or `-1.0`) it locked to. All subsequent input is projected
        /// onto this edge for the duration of the stroke. `None` means the
        /// stroke started unsnapped and will never snap.
        ruler_snap_side: Option<f64>,
        /// When the pen went down, to tell how quickly the stroke was drawn.
        begin: Instant,
    },
    DraggingRuler {
        anchor_begin: Vector2,
        dial_pos_begin: Vector2,
        start_pos: Vector2,
    },
    /// The pen went down on the angle dial and turns the ruler while it is dragged up and down.
    RotatingRuler {
        /// The dial position, which the ruler turns around and which stays where it is.
        pivot: Vector2,
        /// The anchor and angle the ruler had when the drag began, which the turn is applied to.
        anchor_begin: Vector2,
        angle_begin: f64,
        /// The turn itself, which follows how far and how fast the pen is moved.
        turn: RulerTurn,
        /// The position of the last event, to measure the movement since it.
        last_pos: Vector2,
    },
    /// The drawn stroke was replaced by a recognized shape while the pen is still down.
    ///
    /// The pen keeps adjusting the shape until it is lifted.
    AdjustingShape {
        stroke_key: StrokeKey,
        adjust: ShapeAdjust,
        /// Whether the pen moved and actually adjusted the shape.
        adjusted: bool,
    },
}

#[derive(Debug)]
pub struct Brush {
    state: BrushState,
    /// The position where the pen last came (approximately) to rest, while drawing.
    hold_anchor: Vector2,
    /// The time the pen came to rest at the current hold anchor.
    hold_begin: Instant,
    /// Task that fires when the pen was held still long enough to trigger shape recognition.
    hold_task_handle: Option<OneOffTaskHandle>,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            state: BrushState::Idle,
            hold_anchor: Vector2::ZERO,
            hold_begin: Instant::now(),
            hold_task_handle: None,
        }
    }
}

impl PenBehaviour for Brush {
    fn init(&mut self, _engine_view: &EngineView) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn deinit(&mut self) -> WidgetFlags {
        self.hold_task_handle = None;
        WidgetFlags::default()
    }

    fn style(&self) -> PenStyle {
        PenStyle::Brush
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

        let total_zoom = engine_view.camera.total_zoom();
        let ruler_view = RulerView::from_camera(engine_view.camera);
        let event_result = match (&mut self.state, event) {
            (BrushState::Idle, PenEvent::Down { mut element, .. }) => {
                // If the input lands on the ruler, move it instead of drawing: on its angle dial
                // it turns the ruler, anywhere else on its body it drags the ruler along.
                let ruler = &engine_view.config.pens_config.brush_config.ruler_config;
                if ruler.hit_dial(element.pos, ruler_view) {
                    self.state = BrushState::RotatingRuler {
                        pivot: ruler.dial_pos,
                        anchor_begin: ruler.anchor,
                        angle_begin: ruler.angle,
                        turn: RulerTurn::new(ruler.angle),
                        last_pos: element.pos,
                    };
                    widget_flags.redraw = true;
                    return (
                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress: PenProgress::InProgress,
                        },
                        widget_flags,
                    );
                }
                if ruler.visible && ruler.hit_body(element.pos, ruler_view) {
                    self.state = BrushState::DraggingRuler {
                        anchor_begin: ruler.anchor,
                        dial_pos_begin: ruler.dial_pos,
                        start_pos: element.pos,
                    };
                    widget_flags.redraw = true;
                    return (
                        EventResult {
                            handled: true,
                            propagate: EventPropagation::Stop,
                            progress: PenProgress::InProgress,
                        },
                        widget_flags,
                    );
                }
                // Decide once at the start of the stroke whether to snap to
                // the ruler. The decision is locked for the remainder of the
                // stroke (sticky).
                let ruler_snap_side = engine_view
                    .config
                    .pens_config
                    .brush_config
                    .ruler_config
                    .snap_side(element.pos, ruler_view);
                if let Some(side) = ruler_snap_side {
                    element.pos = engine_view
                        .config
                        .pens_config
                        .brush_config
                        .ruler_config
                        .project_to_edge(element.pos, side, ruler_view);
                }
                if !element.filter_by_bounds(
                    engine_view
                        .document
                        .bounds()
                        .loosened(Self::INPUT_OVERSHOOT),
                ) {
                    #[cfg(feature = "ui")]
                    {
                        if engine_view.config.pens_config.brush_config.style == BrushStyle::Marker {
                            play_marker_sound(engine_view);
                        } else {
                            trigger_brush_sound(engine_view);
                        }
                    }

                    engine_view
                        .config
                        .pens_config
                        .brush_config
                        .new_style_seeds();

                    let preview_style = Self::get_preview_style(&engine_view.as_im());
                    let brushstroke =
                        Stroke::BrushStroke(BrushStroke::new(element, preview_style.clone()));

                    let current_stroke_key = engine_view.store.insert_stroke(
                        brushstroke,
                        Some(
                            engine_view
                                .config
                                .pens_config
                                .brush_config
                                .layer_for_current_options(),
                        ),
                    );

                    engine_view.store.regenerate_rendering_for_stroke(
                        current_stroke_key,
                        engine_view.camera.viewport(),
                        engine_view.camera.image_scale(),
                    );

                    self.state = BrushState::Drawing {
                        path_builder: new_builder(
                            engine_view.config.pens_config.brush_config.builder_type,
                            element,
                            now,
                        ),
                        current_stroke_key,
                        preview_style,
                        ruler_snap_side,
                        begin: now,
                    };

                    self.hold_anchor = element.pos;
                    self.hold_begin = now;
                    if engine_view
                        .config
                        .pens_config
                        .brush_config
                        .shape_recognition_enabled
                    {
                        reset_hold_task(
                            &mut self.hold_task_handle,
                            engine_view.tasks_tx.clone(),
                            engine_view
                                .config
                                .pens_config
                                .brush_config
                                .shape_recognition_delay,
                        );
                    } else {
                        self.hold_task_handle = None;
                    }

                    EventResult {
                        handled: true,
                        propagate: EventPropagation::Stop,
                        progress: PenProgress::InProgress,
                    }
                } else {
                    EventResult {
                        handled: false,
                        propagate: EventPropagation::Proceed,
                        progress: PenProgress::Idle,
                    }
                }
            }
            (BrushState::Idle, _) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::Idle,
            },
            (
                BrushState::DraggingRuler {
                    anchor_begin,
                    dial_pos_begin,
                    start_pos,
                },
                PenEvent::Down { element, .. },
            ) => {
                // Drag delta in doc coords -> in scroller pixels (= doc * zoom).
                let delta_scroller = (element.pos - *start_pos) * total_zoom;
                let ruler = &mut engine_view.config.pens_config.brush_config.ruler_config;
                ruler.anchor = *anchor_begin + delta_scroller;
                ruler.dial_pos = *dial_pos_begin + delta_scroller;
                widget_flags.redraw = true;
                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            (
                BrushState::RotatingRuler {
                    pivot,
                    anchor_begin,
                    angle_begin,
                    turn,
                    last_pos,
                },
                PenEvent::Down { element, .. },
            ) => {
                // Only the vertical part of the drag turns the ruler, in surface pixels.
                let drag_y = (element.pos.y - last_pos.y) * total_zoom;
                *last_pos = element.pos;

                let ruler = &mut engine_view.config.pens_config.brush_config.ruler_config;
                let angle = turn.update_from_drag(ruler, drag_y);
                ruler.anchor =
                    RulerConfig::rotate_around(*anchor_begin, *pivot, angle - *angle_begin);
                ruler.angle = angle;
                widget_flags.redraw = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            (BrushState::RotatingRuler { .. }, PenEvent::Up { .. } | PenEvent::Cancel) => {
                self.state = BrushState::Idle;
                widget_flags.redraw = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (BrushState::RotatingRuler { .. }, _) => EventResult {
                handled: true,
                propagate: EventPropagation::Stop,
                progress: PenProgress::InProgress,
            },
            (BrushState::DraggingRuler { .. }, PenEvent::Up { .. } | PenEvent::Cancel) => {
                self.state = BrushState::Idle;
                widget_flags.redraw = true;
                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (BrushState::DraggingRuler { .. }, _) => EventResult {
                handled: false,
                propagate: EventPropagation::Proceed,
                progress: PenProgress::InProgress,
            },
            (
                BrushState::Drawing {
                    current_stroke_key,
                    begin,
                    ..
                },
                PenEvent::Cancel,
            ) => {
                if let Some(Stroke::BrushStroke(brushstroke)) =
                    engine_view.store.get_stroke_mut(*current_stroke_key)
                {
                    brushstroke.style = engine_view
                        .config
                        .pens_config
                        .brush_config
                        .style_for_current_options();
                }

                // Finish up the last stroke
                engine_view
                    .store
                    .update_geometry_for_stroke(*current_stroke_key);
                let (erased, wf) = erase_with_scribble(
                    *current_stroke_key,
                    now.saturating_duration_since(*begin),
                    engine_view,
                );
                widget_flags |= wf;
                if !erased {
                    engine_view.store.regenerate_rendering_for_stroke_threaded(
                        engine_view.tasks_tx.clone(),
                        *current_stroke_key,
                        engine_view.camera.viewport(),
                        engine_view.camera.image_scale(),
                    );
                }
                widget_flags |= engine_view
                    .document
                    .resize_autoexpand(engine_view.store, engine_view.camera);

                self.state = BrushState::Idle;
                self.hold_task_handle = None;

                widget_flags |= engine_view.store.record(Instant::now());
                widget_flags.store_modified = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (
                BrushState::Drawing {
                    path_builder,
                    current_stroke_key,
                    ruler_snap_side,
                    begin,
                    ..
                },
                mut pen_event,
            ) => {
                // Track whether the pen is being held still,
                // which after a timeout triggers recognizing the drawn stroke as a shape.
                if engine_view
                    .config
                    .pens_config
                    .brush_config
                    .shape_recognition_enabled
                    && let PenEvent::Down { element, .. } = &pen_event
                {
                    let hold_radius =
                        shaperecognition::HOLD_RADIUS_SURFACE / engine_view.camera.total_zoom();
                    if (element.pos - self.hold_anchor).length() > hold_radius {
                        self.hold_anchor = element.pos;
                        self.hold_begin = now;
                        reset_hold_task(
                            &mut self.hold_task_handle,
                            engine_view.tasks_tx.clone(),
                            engine_view
                                .config
                                .pens_config
                                .brush_config
                                .shape_recognition_delay,
                        );
                    }
                }

                // If the stroke started snapped, every subsequent point is
                // projected onto the same edge — the stroke stays on the
                // ruler until release, regardless of how far the input moves
                // perpendicular to it.
                if let Some(side) = *ruler_snap_side {
                    match &mut pen_event {
                        PenEvent::Down { element, .. } | PenEvent::Up { element, .. } => {
                            element.pos = engine_view
                                .config
                                .pens_config
                                .brush_config
                                .ruler_config
                                .project_to_edge(element.pos, side, ruler_view);
                        }
                        _ => {}
                    }
                }
                let builder_result =
                    path_builder.handle_event(pen_event, now, Constraints::default());
                let handled = builder_result.handled;
                let propagate = builder_result.propagate;

                let progress = match builder_result.progress {
                    BuilderProgress::InProgress => {
                        #[cfg(feature = "ui")]
                        {
                            if engine_view.config.pens_config.brush_config.style
                                != BrushStyle::Marker
                            {
                                trigger_brush_sound(engine_view);
                            }
                        }

                        PenProgress::InProgress
                    }
                    BuilderProgress::EmitContinue(segments) => {
                        #[cfg(feature = "ui")]
                        {
                            if engine_view.config.pens_config.brush_config.style
                                != BrushStyle::Marker
                            {
                                trigger_brush_sound(engine_view);
                            }
                        }

                        let n_segments = segments.len();

                        if n_segments != 0 {
                            if let Some(Stroke::BrushStroke(brushstroke)) =
                                engine_view.store.get_stroke_mut(*current_stroke_key)
                            {
                                brushstroke.extend_w_segments(segments);
                                widget_flags.store_modified = true;
                            }

                            engine_view.store.append_rendering_last_segments(
                                engine_view.tasks_tx.clone(),
                                *current_stroke_key,
                                n_segments,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }

                        PenProgress::InProgress
                    }
                    BuilderProgress::Finished(segments) => {
                        let n_segments = segments.len();

                        if n_segments != 0 {
                            if let Some(Stroke::BrushStroke(brushstroke)) =
                                engine_view.store.get_stroke_mut(*current_stroke_key)
                            {
                                brushstroke.extend_w_segments(segments);
                                widget_flags.store_modified = true;
                            }

                            engine_view.store.append_rendering_last_segments(
                                engine_view.tasks_tx.clone(),
                                *current_stroke_key,
                                n_segments,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }

                        if let Some(Stroke::BrushStroke(brushstroke)) =
                            engine_view.store.get_stroke_mut(*current_stroke_key)
                        {
                            brushstroke.style = engine_view
                                .config
                                .pens_config
                                .brush_config
                                .style_for_current_options();
                        }

                        // Finish up the last stroke
                        engine_view
                            .store
                            .update_geometry_for_stroke(*current_stroke_key);
                        let (erased, wf) = erase_with_scribble(
                            *current_stroke_key,
                            now.saturating_duration_since(*begin),
                            engine_view,
                        );
                        widget_flags |= wf;
                        if !erased {
                            engine_view.store.regenerate_rendering_for_stroke_threaded(
                                engine_view.tasks_tx.clone(),
                                *current_stroke_key,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }
                        widget_flags |= engine_view
                            .document
                            .resize_autoexpand(engine_view.store, engine_view.camera);

                        self.state = BrushState::Idle;
                        self.hold_task_handle = None;

                        widget_flags |= engine_view.store.record(Instant::now());
                        widget_flags.store_modified = true;

                        PenProgress::Finished
                    }
                };

                EventResult {
                    handled,
                    propagate,
                    progress,
                }
            }
            (
                BrushState::AdjustingShape {
                    stroke_key,
                    adjust,
                    adjusted,
                },
                PenEvent::Down { element, .. },
            ) => {
                *adjusted = true;
                let shape = adjust.adjusted(element.pos);
                widget_flags |= update_shape_stroke(*stroke_key, shape, engine_view);

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::InProgress,
                }
            }
            (
                BrushState::AdjustingShape {
                    stroke_key,
                    adjust,
                    adjusted,
                },
                PenEvent::Up { element, .. },
            ) => {
                if *adjusted {
                    let shape = adjust.adjusted(element.pos);
                    widget_flags |= update_shape_stroke(*stroke_key, shape, engine_view);
                    widget_flags |= engine_view.store.record(Instant::now());
                }
                self.state = BrushState::Idle;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (BrushState::AdjustingShape { .. }, PenEvent::Cancel) => {
                self.state = BrushState::Idle;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (BrushState::AdjustingShape { .. }, _) => EventResult {
                handled: true,
                propagate: EventPropagation::Stop,
                progress: PenProgress::InProgress,
            },
        };

        (event_result, widget_flags)
    }
}

impl DrawableOnDoc for Brush {
    fn bounds_on_doc(&self, engine_view: &EngineView) -> Option<Aabb> {
        let style = engine_view
            .config
            .pens_config
            .brush_config
            .style_for_current_options();

        match &self.state {
            BrushState::Idle
            | BrushState::AdjustingShape { .. }
            | BrushState::DraggingRuler { .. }
            | BrushState::RotatingRuler { .. } => None,
            BrushState::Drawing { path_builder, .. } => {
                path_builder.bounds(&style, engine_view.camera.zoom())
            }
        }
    }

    fn draw_on_doc(
        &self,
        cx: &mut piet_cairo::CairoRenderContext,
        engine_view: &EngineView,
    ) -> anyhow::Result<()> {
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        match &self.state {
            BrushState::Idle
            | BrushState::AdjustingShape { .. }
            | BrushState::DraggingRuler { .. }
            | BrushState::RotatingRuler { .. } => {}
            BrushState::Drawing {
                path_builder,
                preview_style,
                ..
            } => {
                match engine_view.config.pens_config.brush_config.style {
                    BrushStyle::Marker => {
                        // Don't draw the marker, as the pen would render on top of other strokes, while the stroke itself would render underneath them.
                    }
                    BrushStyle::Solid | BrushStyle::Textured => {
                        path_builder.draw_styled(
                            cx,
                            preview_style,
                            engine_view.camera.total_zoom(),
                        );
                    }
                }
            }
        }

        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

impl Brush {
    const INPUT_OVERSHOOT: f64 = 30.0;

    /// Attempt to recognize the currently drawn stroke as a shape and replace it,
    /// triggered when the pen was held still at the end of a drawn stroke.
    pub(crate) fn recognize_shape_on_hold(
        &mut self,
        engine_view: &mut EngineViewMut,
    ) -> WidgetFlags {
        let mut widget_flags = WidgetFlags::default();

        if !engine_view
            .config
            .pens_config
            .brush_config
            .shape_recognition_enabled
        {
            return widget_flags;
        }
        let BrushState::Drawing {
            current_stroke_key,
            ruler_snap_side,
            ..
        } = self.state
        else {
            return widget_flags;
        };
        // A line drawn along the ruler is already as straight as it can be, and making a shape of it
        // would let it be dragged off the ruler.
        if ruler_snap_side.is_some() {
            return widget_flags;
        }
        // Guard against the timeout task and the pen resuming movement racing each other:
        // only recognize when the pen actually rested at the hold anchor for the entire hold duration.
        let hold_duration = engine_view
            .config
            .pens_config
            .brush_config
            .shape_recognition_delay;
        if self.hold_begin.elapsed() < hold_duration.mul_f64(0.9) {
            return widget_flags;
        }

        let recognized_shape = if let Some(Stroke::BrushStroke(brushstroke)) =
            engine_view.store.get_stroke_ref(current_stroke_key)
        {
            let zoom = engine_view.camera.total_zoom();
            let brush_config = &engine_view.config.pens_config.brush_config;
            shaperecognition::recognize_shape(&brushstroke.path, zoom).map(|shape| {
                // The shape is as wide as the stroke it replaces was on average, which is what sets
                // how it looks: a firm press is a thick line, a light one a thin line.
                let width = shaperecognition::mean_width(&brushstroke.path, zoom, |pressure| {
                    brush_config.stroke_width_for_pressure(pressure)
                })
                .unwrap_or_else(|| {
                    brush_config.stroke_width_for_pressure(Element::PRESSURE_DEFAULT)
                });

                (shape, width)
            })
        } else {
            None
        };
        let Some((shape, stroke_width)) = recognized_shape else {
            return widget_flags;
        };

        // Replace the drawn stroke with the recognized shape
        engine_view.store.remove_stroke(current_stroke_key);
        let shape_stroke_key = engine_view.store.insert_stroke(
            Stroke::ShapeStroke(ShapeStroke::new(
                shape.clone(),
                engine_view
                    .config
                    .pens_config
                    .brush_config
                    .style_for_recognized_shape(stroke_width),
            )),
            Some(
                engine_view
                    .config
                    .pens_config
                    .brush_config
                    .layer_for_current_options(),
            ),
        );
        engine_view
            .store
            .update_geometry_for_stroke(shape_stroke_key);
        engine_view.store.regenerate_rendering_for_stroke_threaded(
            engine_view.tasks_tx.clone(),
            shape_stroke_key,
            engine_view.camera.viewport(),
            engine_view.camera.image_scale(),
        );
        widget_flags |= engine_view
            .document
            .resize_autoexpand(engine_view.store, engine_view.camera);
        widget_flags |= engine_view.store.record(Instant::now());
        widget_flags.store_modified = true;
        widget_flags.redraw = true;

        // The pen keeps adjusting the recognized shape until it is lifted
        self.state = BrushState::AdjustingShape {
            stroke_key: shape_stroke_key,
            adjust: ShapeAdjust::new(shape, self.hold_anchor),
            adjusted: false,
        };
        self.hold_task_handle = None;

        widget_flags
    }

    fn get_preview_style(engine_view: &EngineView) -> Style {
        let mut style = engine_view
            .config
            .pens_config
            .brush_config
            .style_for_current_options();

        if let Some(mut stroke_color) = style.stroke_color() {
            stroke_color.a = 1.0;
            style.set_stroke_color(stroke_color);
        }

        style
    }
}

/// Replace the shape of the shape stroke with the given key and update the document for it.
fn update_shape_stroke(
    stroke_key: StrokeKey,
    shape: Shape,
    engine_view: &mut EngineViewMut,
) -> WidgetFlags {
    let mut widget_flags = WidgetFlags::default();

    if let Some(Stroke::ShapeStroke(shapestroke)) = engine_view.store.get_stroke_mut(stroke_key) {
        shapestroke.shape = shape;
    } else {
        return widget_flags;
    }

    engine_view.store.update_geometry_for_stroke(stroke_key);
    engine_view.store.regenerate_rendering_for_stroke_threaded(
        engine_view.tasks_tx.clone(),
        stroke_key,
        engine_view.camera.viewport(),
        engine_view.camera.image_scale(),
    );
    widget_flags |= engine_view
        .document
        .resize_autoexpand(engine_view.store, engine_view.camera);
    widget_flags.store_modified = true;
    widget_flags.redraw = true;

    widget_flags
}

#[cfg(feature = "ui")]
fn play_marker_sound(engine_view: &mut EngineViewMut) {
    if let Some(audioplayer) = engine_view.audioplayer {
        audioplayer.play_random_marker_sound();
    }
}

#[cfg(feature = "ui")]
fn trigger_brush_sound(engine_view: &mut EngineViewMut) {
    if let Some(audioplayer) = engine_view.audioplayer.as_mut() {
        audioplayer.trigger_random_brush_sound();
    }
}

/// (Re-)start the hold timeout, (re-)installing the one-off task when it already fired or is not installed yet.
///
/// When the timeout is reached, a task is sent to the engine
/// which triggers recognizing the currently drawn stroke as a shape.
fn reset_hold_task(
    handle: &mut Option<OneOffTaskHandle>,
    tasks_tx: EngineTaskSender,
    hold_duration: Duration,
) {
    if let Some(handle) = handle.as_mut()
        && handle.reset_timeout().is_ok()
    {
        return;
    }

    let hold_task = move || {
        tasks_tx.send(EngineTask::BrushRecognizeShape);
    };
    *handle = Some(OneOffTaskHandle::new(hold_task, hold_duration));
}

/// Erase the strokes a finished stroke was scribbled over.
///
/// When the stroke is the gesture of scratching something out and it actually went over strokes,
/// those strokes and the scribble itself are trashed. A scribble over empty space stays the stroke
/// it is, so that scratching in the margin still draws.
///
/// Returns whether the stroke erased anything, in which case it is gone from the store.
fn erase_with_scribble(
    stroke_key: StrokeKey,
    duration: Duration,
    engine_view: &mut EngineViewMut,
) -> (bool, WidgetFlags) {
    let mut widget_flags = WidgetFlags::default();

    if !engine_view
        .config
        .pens_config
        .brush_config
        .scribble_erase_enabled
    {
        return (false, widget_flags);
    }
    let is_scribble = match engine_view.store.get_stroke_ref(stroke_key) {
        Some(Stroke::BrushStroke(brushstroke)) => scribblerecognition::is_scribble(
            &brushstroke.path,
            engine_view.camera.total_zoom(),
            duration,
            &engine_view
                .config
                .pens_config
                .brush_config
                .scribble_recognition_options,
        ),
        _ => false,
    };
    if !is_scribble {
        return (false, widget_flags);
    }

    let (trashed, modified_keys, wf) = engine_view
        .store
        .trash_strokes_hit_by(stroke_key, engine_view.camera.viewport());
    widget_flags |= wf;
    engine_view.store.regenerate_rendering_for_strokes(
        &modified_keys,
        engine_view.camera.viewport(),
        engine_view.camera.image_scale(),
    );
    if trashed == 0 {
        return (false, widget_flags);
    }

    engine_view.store.remove_stroke(stroke_key);
    widget_flags.store_modified = true;
    widget_flags.redraw = true;

    (true, widget_flags)
}

fn new_builder(
    builder_type: PenPathBuilderType,
    element: Element,
    now: Instant,
) -> Box<dyn Buildable<Emit = Segment>> {
    match builder_type {
        PenPathBuilderType::Simple => Box::new(PenPathSimpleBuilder::start(element, now)),
        PenPathBuilderType::Curved => Box::new(PenPathCurvedBuilder::start(element, now)),
        PenPathBuilderType::Modeled => Box::new(PenPathModeledBuilder::start(element, now)),
    }
}
