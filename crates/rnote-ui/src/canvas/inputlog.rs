//! A recorder of the last moments of pen input and drawing, saved on request.
//!
//! For finding input problems that can't be reproduced on purpose: the last
//! [`KEEP`] of pointer events and frame timings are kept in memory, and are only written
//! to a file when asked to, right after something went wrong.

use gtk4::gdk;
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
    },
    Frame {
        at: Instant,
        drawing: Duration,
        /// Where the view was: offset, zoom and rotation of the camera.
        view: (f64, f64, f64, f64),
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
pub(super) struct Frame(Instant, (f64, f64, f64, f64));

impl Frame {
    /// `camera` is where the view is in this frame, to see whether it moved while writing.
    pub(super) fn begin(camera: &rnote_engine::Camera) -> Self {
        let offset = camera.offset();
        Self(
            Instant::now(),
            (offset.x, offset.y, camera.total_zoom(), camera.rotation()),
        )
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        push(Entry::Frame {
            at: self.0,
            drawing: self.0.elapsed(),
            view: self.1,
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
                } => writeln!(
                    text,
                    "{:9.1} {kind:?} tool={tool:?} pos={pos:?} pressure={pressure:?} mods={modifiers:?} pen {:?}->{:?} stylus {}->{} handled_in={:.2}ms",
                    ms(*at),
                    pen_state.0,
                    pen_state.1,
                    stylus_active.0,
                    stylus_active.1,
                    handling.as_secs_f64() * 1e3,
                ),
                Entry::Frame { at, drawing, view } => {
                    let since_last = last_frame
                        .map_or(0.0, |last| at.duration_since(last).as_secs_f64() * 1e3);
                    last_frame = Some(*at);
                    writeln!(
                        text,
                        "{:9.1} frame since_last={since_last:.1}ms drawn_in={:.2}ms view_offset=({:.1},{:.1}) zoom={:.3} rotation={:.3}",
                        ms(*at),
                        drawing.as_secs_f64() * 1e3,
                        view.0,
                        view.1,
                        view.2,
                        view.3,
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
