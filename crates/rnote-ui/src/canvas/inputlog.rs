//! A recorder of the last moments of pen input and drawing, saved on request.
//!
//! For finding input problems that can't be reproduced on purpose: the last
//! [`KEEP`] of pointer events and frame timings are kept in memory, and are only written
//! to a file when asked to, right after something went wrong.

use gtk4::{gdk, prelude::*};
use rnote_compose::penevent::PenState;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How far back the recording reaches.
const KEEP: Duration = Duration::from_secs(30);

enum Entry {
    Event {
        at: Instant,
        kind: gdk::EventType,
        tool: Option<gdk::DeviceToolType>,
        pos: Option<(f64, f64)>,
        pressure: Option<f64>,
        modifiers: gdk::ModifierType,
        pen_state: (PenState, PenState),
        stylus_active: (bool, bool),
        handling: Duration,
        /// How long ago the compositor stamped the event when the app got it, in ms.
        age: i64,
        /// What was handed to the pen for this event: position in the document, pressure and
        /// how many ms before `at` each element was timed.
        elements: Vec<(f64, f64, f64, f64)>,
    },
    Frame {
        at: Instant,
        drawing: Duration,
        /// Where the view was: offset, zoom and rotation of the camera.
        view: (f64, f64, f64, f64),
        /// For the latest frame the compositor reported as shown: ms from GTK starting it until
        /// it was on screen, and that frame's number counted back from this one.
        shown: Option<(f64, i64)>,
        /// Pixels from the end of the ink of the stroke being drawn to the pen.
        ink_gap: Option<f64>,
    },
}

impl Entry {
    fn at(&self) -> Instant {
        match self {
            Self::Event { at, .. } | Self::Frame { at, .. } => *at,
        }
    }
}

#[derive(Default)]
struct State {
    entries: VecDeque<Entry>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    /// The elements of the event being handled, until the event itself is recorded.
    static ELEMENTS: RefCell<Vec<(f64, f64, f64, f64)>> = const { RefCell::new(Vec::new()) };
}

/// Notes the elements handed to the pen for the event being handled, timed relative to `now`.
pub(super) fn elements(elements: &[(rnote_compose::penpath::Element, Instant)], now: Instant) {
    ELEMENTS.with_borrow_mut(|noted| {
        noted.extend(elements.iter().map(|(element, time)| {
            (
                element.pos.x,
                element.pos.y,
                element.pressure,
                now.saturating_duration_since(*time).as_secs_f64() * 1e3,
            )
        }));
    });
}

fn push(entry: Entry) {
    STATE.with_borrow_mut(|state| {
        let at = entry.at();
        while state
            .entries
            .front()
            .is_some_and(|first| at.duration_since(first.at()) > KEEP)
        {
            state.entries.pop_front();
        }
        state.entries.push_back(entry);
    });
}

/// Times drawing a frame of the canvas, from creation until dropped.
pub(super) struct Frame(
    Instant,
    (f64, f64, f64, f64),
    Option<(f64, i64)>,
    Option<f64>,
);

impl Frame {
    /// `camera` is where the view is in this frame, to see whether it moved while writing.
    /// `ink_gap` is how far the ink of the stroke being drawn ends from the pen.
    pub(super) fn begin(
        camera: &rnote_engine::Camera,
        widget: &impl IsA<gtk4::Widget>,
        ink_gap: Option<f64>,
    ) -> Self {
        let offset = camera.offset();
        Self(
            Instant::now(),
            (offset.x, offset.y, camera.total_zoom(), camera.rotation()),
            widget.frame_clock().and_then(|clock| last_shown(&clock)),
            ink_gap,
        )
    }
}

/// The latest frame the compositor said it put on screen, a few frames back at most.
fn last_shown(clock: &gdk::FrameClock) -> Option<(f64, i64)> {
    let current = clock.frame_counter();
    (1..=8).find_map(|back| {
        let timings = clock.timings(current - back)?;
        let shown = timings.presentation_time();
        (timings.is_complete() && shown != 0)
            .then(|| ((shown - timings.frame_time()) as f64 / 1e3, back))
    })
}

impl Drop for Frame {
    fn drop(&mut self) {
        push(Entry::Frame {
            at: self.0,
            drawing: self.0.elapsed(),
            view: self.1,
            shown: self.2,
            ink_gap: self.3,
        });
    }
}

/// Records a pointer event the canvas handled, `begin` being when handling it started.
pub(super) fn pointer_event(
    begin: Instant,
    event: &gdk::Event,
    pen_state: (PenState, PenState),
    stylus_active: (bool, bool),
) {
    push(Entry::Event {
        at: begin,
        kind: event.event_type(),
        tool: event.device_tool().map(|tool| tool.tool_type()),
        pos: event.position(),
        pressure: event.axes().and_then(|axes| {
            axes.get(crate::utils::axis_use_idx(gdk::AxisUse::Pressure))
                .copied()
        }),
        modifiers: event.modifier_state(),
        pen_state,
        stylus_active,
        handling: begin.elapsed(),
        // Wayland stamps events in ms of the monotonic clock, cut to 32 bits.
        age: ((gtk4::glib::monotonic_time() / 1000) as u32).wrapping_sub(event.time()) as i32
            as i64,
        elements: ELEMENTS.take(),
    });
}

/// Writes what was recorded to a new file in the app's cache directory, with `header` first.
pub(crate) fn save(header: &str) -> anyhow::Result<PathBuf> {
    let text = STATE.with_borrow(|state| {
        let mut text = String::from(header);
        let Some(first) = state.entries.front().map(Entry::at) else {
            text.push_str("nothing recorded\n");
            return text;
        };
        let ms = |at: Instant| at.duration_since(first).as_secs_f64() * 1e3;
        let mut last_frame: Option<Instant> = None;
        for entry in state.entries.iter() {
            let _ = match entry {
                Entry::Event {
                    at,
                    kind,
                    tool,
                    pos,
                    pressure,
                    modifiers,
                    pen_state,
                    stylus_active,
                    handling,
                    age,
                    elements,
                } => writeln!(
                    text,
                    "{:9.1} {kind:?} tool={tool:?} pos={pos:?} pressure={pressure:?} mods={modifiers:?} pen {:?}->{:?} stylus {}->{} handled_in={:.2}ms age={age}ms elements=[{}]",
                    ms(*at),
                    pen_state.0,
                    pen_state.1,
                    stylus_active.0,
                    stylus_active.1,
                    handling.as_secs_f64() * 1e3,
                    elements
                        .iter()
                        .map(|(x, y, pressure, before)| format!(
                            "{x:.2},{y:.2},{pressure:.3},-{before:.1}ms"
                        ))
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                Entry::Frame {
                    at,
                    drawing,
                    view,
                    shown,
                    ink_gap,
                } => {
                    let since_last = last_frame
                        .map_or(0.0, |last| at.duration_since(last).as_secs_f64() * 1e3);
                    last_frame = Some(*at);
                    writeln!(
                        text,
                        "{:9.1} frame since_last={since_last:.1}ms drawn_in={:.2}ms view_offset=({:.1},{:.1}) zoom={:.3} rotation={:.3} shown={}{}",
                        ms(*at),
                        drawing.as_secs_f64() * 1e3,
                        view.0,
                        view.1,
                        view.2,
                        view.3,
                        shown.map_or("unknown".to_string(), |(ms, back)| format!(
                            "{ms:.1}ms(frame -{back})"
                        )),
                        ink_gap.map_or(String::new(), |gap| format!(" ink_gap={gap:.1}px")),
                    )
                }
            };
        }
        text
    });

    let stamp = gtk4::glib::DateTime::now_local()
        .and_then(|now| now.format("%Y-%m-%d_%H-%M-%S"))
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "unknown-time".to_string());
    let path = crate::env::app_cache_dir()?.join(format!("input-log-{stamp}.txt"));
    std::fs::write(&path, text)?;
    Ok(path)
}
