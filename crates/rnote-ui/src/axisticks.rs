// Imports
use crate::{RnAppWindow, RnCanvas};
use gettextrs::gettext;
use gtk4::{
    Align, Box, Button, Entry, Grid, Label, Orientation, Popover, PositionType, ScrolledWindow,
    Separator, SpinButton, Switch, gdk, glib, glib::clone, graphene, prelude::*,
};
use rnote_engine::pens::CoordSystemSettings;
use rnote_engine::pens::pensconfig::ShaperConfig;
use rnote_engine::strokes::{AxisEnd, AxisTicks, AxisTicksStyle};
use std::rc::Rc;

const POPOVER_NAME: &str = "axis-ticks-popover";

fn find_popover(canvas: &RnCanvas) -> Option<Popover> {
    let mut child = canvas.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == POPOVER_NAME
            && let Ok(popover) = widget.clone().downcast::<Popover>()
        {
            return Some(popover);
        }
        child = widget.next_sibling();
    }
    None
}

/// Hide the popup for the coordinate system.
pub(crate) fn hide_axis_ticks_popover(canvas: &RnCanvas) {
    if let Some(popover) = find_popover(canvas) {
        popover.popdown();
    }
}

/// Show a popup next to the coordinate system that was just drawn, to set how it is marked: the
/// number of ticks on each half-axis (also with plus and minus on the keyboard), their size,
/// spacing and numbers, the arrow heads and names at the ends of the axes, and for 3D coordinate
/// systems the direction of the depth axis and the handedness.
pub(crate) fn show_axis_ticks_popover(appwindow: &RnAppWindow, canvas: &RnCanvas) {
    let Some(edit) = canvas.engine_ref().coord_system_edit() else {
        return;
    };
    let settings = &edit.settings;

    // The anchor is in document coordinates
    let (mins, maxs) = (edit.anchor.mins, edit.anchor.maxs);
    let transform = canvas.engine_ref().camera.transform();
    let (top_left, bottom_right) = (
        transform.transform_point2(mins),
        transform.transform_point2(maxs),
    );
    let (rect, position) = popover_anchor(
        canvas,
        (top_left.x, top_left.y),
        (bottom_right.x, bottom_right.y),
    );

    let ticks_switch = switch(settings.ticks, &gettext("Ticks on the axes"));
    let count_spin = spin_button(
        AxisTicks::COUNT_MIN as f64,
        AxisTicks::COUNT_MAX as f64,
        settings.count as f64,
        &if settings.constant_spacing {
            gettext("Number of ticks on the shortest half-axis (+/−)")
        } else {
            gettext("Number of ticks on each half-axis (+/−)")
        },
    );
    let tick_size_spin = spin_button(
        ShaperConfig::AXIS_TICK_SIZE_MIN,
        ShaperConfig::AXIS_TICK_SIZE_MAX,
        settings.tick_style.tick_size,
        &gettext("How far the ticks reach to each side of the axis"),
    );
    let constant_spacing_switch = switch(
        settings.constant_spacing,
        &gettext(
            "On axes of different length, keep the distance between ticks the same instead of their number",
        ),
    );
    let numbering_switch = switch(settings.tick_style.numbering, &gettext("Number the ticks"));
    let label_every_spin = spin_button(
        AxisTicks::LABEL_EVERY_MIN as f64,
        AxisTicks::LABEL_EVERY_MAX as f64,
        settings.tick_style.label_every as f64,
        &gettext("Number only every this many ticks"),
    );
    let font_size_spin = spin_button(
        AxisTicks::FONT_SIZE_MIN,
        AxisTicks::FONT_SIZE_MAX,
        settings.tick_style.font_size,
        &gettext("The size of the numbers. The names of the axes are a little larger"),
    );
    let arrows_switch = switch(
        settings.arrows,
        &gettext("Arrow heads at the ends of the axes"),
    );
    let arrow_size_spin = spin_button(
        AxisEnd::ARROW_SIZE_MIN,
        AxisEnd::ARROW_SIZE_MAX,
        settings.arrow_size,
        &gettext("The size of the arrow heads"),
    );
    let labels_switch = switch(settings.labels, &gettext("Names at the ends of the axes"));
    // The 3D coordinate system has a third axis to name
    let name_count = if edit.three_d { 3 } else { 2 };
    let name_entries = settings.label_names[..name_count]
        .iter()
        .zip([
            gettext("Name of x"),
            gettext("Name of y"),
            gettext("Name of z"),
        ])
        .map(|(name, tooltip)| {
            Entry::builder()
                .text(name.as_str())
                .width_chars(1)
                .max_width_chars(2)
                .xalign(0.5)
                .tooltip_text(tooltip)
                .build()
        })
        .collect::<Vec<Entry>>();
    let names_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(4)
        .halign(Align::End)
        .build();
    for entry in &name_entries {
        names_box.append(entry);
    }
    let third_into_page_switch = switch(
        settings.third_into_page,
        &gettext(
            "Whether the depth axis across the page goes into it, towards the upper right, or comes out of it, towards the lower left",
        ),
    );
    let right_handed_switch = switch(
        settings.right_handed,
        &gettext(
            "Whether the coordinate system is right-handed or left-handed, which decides whether x or y is the depth axis",
        ),
    );

    // Two compact columns side by side, the ticks on the left and the axes on the right, so that
    // the popup is small enough to fit next to the coordinate system
    let grid = Grid::builder()
        .row_spacing(4)
        .column_spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(10)
        .margin_end(10)
        .build();
    let attach_row = |title: &str, control: &gtk4::Widget, column: i32, row: i32| {
        let label = Label::builder()
            .label(title)
            .halign(Align::Start)
            .valign(Align::Center)
            .build();
        grid.attach(&label, column, row, 1, 1);
        grid.attach(control, column + 1, row, 1, 1);
    };
    let attach_heading = |title: &str, control: Option<&gtk4::Widget>, column: i32, row: i32| {
        let label = Label::builder()
            .label(title)
            .halign(Align::Start)
            .valign(Align::Center)
            .css_classes(["heading"])
            .build();
        grid.attach(&label, column, row, 1, 1);
        if let Some(control) = control {
            grid.attach(control, column + 1, row, 1, 1);
        }
    };

    attach_heading(&gettext("Ticks"), Some(ticks_switch.upcast_ref()), 0, 0);
    let ticks_rows: [(String, &gtk4::Widget); 6] = [
        (gettext("Count"), count_spin.upcast_ref()),
        (gettext("Size"), tick_size_spin.upcast_ref()),
        (
            gettext("Same spacing"),
            constant_spacing_switch.upcast_ref(),
        ),
        (gettext("Numbers"), numbering_switch.upcast_ref()),
        (gettext("Number every"), label_every_spin.upcast_ref()),
        (gettext("Text size"), font_size_spin.upcast_ref()),
    ];
    for (row, (title, control)) in ticks_rows.iter().enumerate() {
        attach_row(title, control, 0, row as i32 + 1);
    }

    attach_heading(&gettext("Axes"), None, 3, 0);
    let axes_rows: [(String, &gtk4::Widget); 4] = [
        (gettext("Arrows"), arrows_switch.upcast_ref()),
        (gettext("Arrow size"), arrow_size_spin.upcast_ref()),
        (gettext("Names"), labels_switch.upcast_ref()),
        (String::new(), names_box.upcast_ref()),
    ];
    for (row, (title, control)) in axes_rows.iter().enumerate() {
        attach_row(title, control, 3, row as i32 + 1);
    }
    let mut rows = ticks_rows.len() as i32 + 1;
    if edit.three_d {
        let first = axes_rows.len() as i32 + 1;
        attach_heading(&gettext("3D"), None, 3, first);
        attach_row(
            &gettext("Into page"),
            third_into_page_switch.upcast_ref(),
            3,
            first + 1,
        );
        attach_row(
            &gettext("Right-handed"),
            right_handed_switch.upcast_ref(),
            3,
            first + 2,
        );
        rows = rows.max(first + 3);
    }
    grid.attach(
        &Separator::builder()
            .orientation(Orientation::Vertical)
            .margin_start(4)
            .margin_end(4)
            .build(),
        2,
        0,
        1,
        rows,
    );

    let done_button = Button::builder()
        .label(gettext("Done"))
        .tooltip_text(gettext("Close (Enter)"))
        .halign(Align::End)
        .margin_top(4)
        .css_classes(["suggested-action"])
        .build();
    grid.attach(&done_button, 0, rows, 5, 1);

    let popover = Popover::builder()
        .name(POPOVER_NAME)
        .child(&grid)
        .position(position)
        .pointing_to(&rect)
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.set_parent(canvas);

    // Every control applies all of them, so the coordinate system always shows what they say
    let controls = Rc::new(Controls {
        ticks: ticks_switch.downgrade(),
        count: count_spin.downgrade(),
        tick_size: tick_size_spin.downgrade(),
        constant_spacing: constant_spacing_switch.downgrade(),
        numbering: numbering_switch.downgrade(),
        label_every: label_every_spin.downgrade(),
        font_size: font_size_spin.downgrade(),
        arrows: arrows_switch.downgrade(),
        arrow_size: arrow_size_spin.downgrade(),
        labels: labels_switch.downgrade(),
        names: name_entries.iter().map(|entry| entry.downgrade()).collect(),
        third_into_page: third_into_page_switch.downgrade(),
        right_handed: right_handed_switch.downgrade(),
        // Names that have no entry stay as they were
        default_names: settings.label_names.clone(),
    });
    controls.update_sensitivity();
    let apply = Rc::new(clone!(
        #[weak]
        appwindow,
        #[weak]
        canvas,
        #[strong]
        controls,
        move || {
            controls.update_sensitivity();
            let Some(settings) = controls.settings() else {
                return;
            };
            let widget_flags = canvas.engine_mut().coord_system_update(settings);
            appwindow.handle_widget_flags(widget_flags, &canvas);
        }
    ));
    for spin in [
        &count_spin,
        &tick_size_spin,
        &label_every_spin,
        &font_size_spin,
        &arrow_size_spin,
    ] {
        spin.connect_value_changed(clone!(
            #[strong]
            apply,
            move |_| apply()
        ));
    }
    for switch in [
        &ticks_switch,
        &constant_spacing_switch,
        &numbering_switch,
        &arrows_switch,
        &labels_switch,
        &third_into_page_switch,
        &right_handed_switch,
    ] {
        switch.connect_active_notify(clone!(
            #[strong]
            apply,
            move |_| apply()
        ));
    }
    for entry in &name_entries {
        entry.connect_changed(clone!(
            #[strong]
            apply,
            move |_| apply()
        ));
    }

    done_button.connect_clicked(clone!(
        #[weak]
        popover,
        move |_| popover.popdown()
    ));

    // Keyboard: plus and minus change the count, enter confirms. Not while typing the name of an
    // axis, which may well have a minus in it.
    let key_controller = gtk4::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let name_entries_weak = name_entries
        .iter()
        .map(|entry| entry.downgrade())
        .collect::<Vec<_>>();
    key_controller.connect_key_pressed(clone!(
        #[weak]
        count_spin,
        #[weak]
        popover,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            let typing_name = name_entries_weak
                .iter()
                .filter_map(|entry| entry.upgrade())
                .any(|entry| entry.state_flags().contains(gtk4::StateFlags::FOCUS_WITHIN));
            match key {
                gdk::Key::plus | gdk::Key::KP_Add | gdk::Key::equal if !typing_name => {
                    count_spin.set_value(count_spin.value() + 1.0);
                    glib::Propagation::Stop
                }
                gdk::Key::minus | gdk::Key::KP_Subtract | gdk::Key::underscore if !typing_name => {
                    count_spin.set_value(count_spin.value() - 1.0);
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    popover.popdown();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        }
    ));
    popover.add_controller(key_controller);

    popover.connect_closed(clone!(
        #[weak]
        appwindow,
        #[weak]
        canvas,
        move |popover| {
            let widget_flags = canvas.engine_mut().coord_system_finish();
            appwindow.handle_widget_flags(widget_flags, &canvas);
            // Unparenting inside the signal handler can crash GTK, so defer it
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        }
    ));

    popover.popup();
    count_spin.grab_focus();
}

/// The controls of the popup, held weakly so the handlers connected to them do not keep them alive.
struct Controls {
    ticks: glib::WeakRef<Switch>,
    count: glib::WeakRef<SpinButton>,
    tick_size: glib::WeakRef<SpinButton>,
    constant_spacing: glib::WeakRef<Switch>,
    numbering: glib::WeakRef<Switch>,
    label_every: glib::WeakRef<SpinButton>,
    font_size: glib::WeakRef<SpinButton>,
    arrows: glib::WeakRef<Switch>,
    arrow_size: glib::WeakRef<SpinButton>,
    labels: glib::WeakRef<Switch>,
    names: Vec<glib::WeakRef<Entry>>,
    third_into_page: glib::WeakRef<Switch>,
    right_handed: glib::WeakRef<Switch>,
    default_names: [String; 3],
}

impl Controls {
    /// The settings the controls are set to.
    fn settings(&self) -> Option<CoordSystemSettings> {
        let mut label_names = self.default_names.clone();
        for (name, entry) in label_names.iter_mut().zip(&self.names) {
            *name = entry.upgrade()?.text().trim().to_string();
        }
        Some(CoordSystemSettings {
            ticks: self.ticks.upgrade()?.is_active(),
            count: self.count.upgrade()?.value().round() as u32,
            constant_spacing: self.constant_spacing.upgrade()?.is_active(),
            tick_style: AxisTicksStyle {
                tick_size: self.tick_size.upgrade()?.value(),
                numbering: self.numbering.upgrade()?.is_active(),
                label_every: self.label_every.upgrade()?.value().round() as u32,
                font_size: self.font_size.upgrade()?.value(),
            },
            arrows: self.arrows.upgrade()?.is_active(),
            arrow_size: self.arrow_size.upgrade()?.value(),
            labels: self.labels.upgrade()?.is_active(),
            label_names,
            third_into_page: self.third_into_page.upgrade()?.is_active(),
            right_handed: self.right_handed.upgrade()?.is_active(),
        })
    }

    /// Grey out what does not matter as the others are set: the tick settings without ticks, and
    /// so on.
    fn update_sensitivity(&self) {
        let Some(settings) = self.settings() else {
            return;
        };
        let ticks = settings.ticks;
        let numbers = ticks && settings.tick_style.numbering;
        let rows: [(Option<gtk4::Widget>, bool); 7] = [
            (self.count.upgrade().map(|row| row.upcast()), ticks),
            (self.tick_size.upgrade().map(|row| row.upcast()), ticks),
            (
                self.constant_spacing.upgrade().map(|row| row.upcast()),
                ticks,
            ),
            (self.numbering.upgrade().map(|row| row.upcast()), ticks),
            (self.label_every.upgrade().map(|row| row.upcast()), numbers),
            (
                self.font_size.upgrade().map(|row| row.upcast()),
                numbers || settings.labels,
            ),
            (
                self.arrow_size.upgrade().map(|row| row.upcast()),
                settings.arrows,
            ),
        ];
        for (row, sensitive) in rows {
            if let Some(row) = row {
                row.set_sensitive(sensitive);
            }
        }
        for entry in self.names.iter().filter_map(|entry| entry.upgrade()) {
            entry.set_sensitive(settings.labels);
        }
    }
}

/// Where the popup points to, in canvas coordinates, and on which side of it it opens, for a
/// coordinate system spanning the given corners in canvas coordinates.
///
/// It points to the part of the coordinate system that is in view: the canvas can be larger than
/// what is shown of it, and a popup pointing outside the window ends up off screen, or is not
/// shown at all. It opens on the side with more room.
fn popover_anchor(
    canvas: &RnCanvas,
    top_left: (f64, f64),
    bottom_right: (f64, f64),
) -> (gdk::Rectangle, PositionType) {
    let visible = canvas
        .ancestor(ScrolledWindow::static_type())
        .and_then(|scroller| {
            let corner = |x: f64, y: f64| {
                scroller.compute_point(canvas, &graphene::Point::new(x as f32, y as f32))
            };
            let start = corner(0.0, 0.0)?;
            let end = corner(scroller.width() as f64, scroller.height() as f64)?;
            Some((
                (start.x() as f64, start.y() as f64),
                (end.x() as f64, end.y() as f64),
            ))
        })
        .unwrap_or(((0.0, 0.0), (canvas.width() as f64, canvas.height() as f64)));
    let ((view_left, view_top), (view_right, view_bottom)) = visible;

    // The part in view, or the point in view closest to it when none of it is
    let left = top_left.0.clamp(view_left, view_right);
    let right = bottom_right.0.clamp(view_left, view_right);
    let top = top_left.1.clamp(view_top, view_bottom);
    let bottom = bottom_right.1.clamp(view_top, view_bottom);
    let rect = gdk::Rectangle::new(
        left.floor() as i32,
        top.floor() as i32,
        ((right - left).ceil() as i32).max(1),
        ((bottom - top).ceil() as i32).max(1),
    );
    let position = if view_bottom - bottom >= top - view_top {
        PositionType::Bottom
    } else {
        PositionType::Top
    };
    (rect, position)
}

/// A switch for the popup, explained by its tooltip.
fn switch(active: bool, tooltip: &str) -> Switch {
    Switch::builder()
        .active(active)
        .halign(Align::End)
        .valign(Align::Center)
        .tooltip_text(tooltip)
        .build()
}

/// A spin button for a whole number in the given range, explained by its tooltip.
fn spin_button(min: f64, max: f64, value: f64, tooltip: &str) -> SpinButton {
    let spin = SpinButton::with_range(min, max, 1.0);
    spin.set_digits(0);
    spin.set_numeric(true);
    spin.set_value(value);
    spin.set_halign(Align::End);
    spin.set_valign(Align::Center);
    spin.set_tooltip_text(Some(tooltip));
    spin
}
