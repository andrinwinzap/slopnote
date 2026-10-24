// Imports
use crate::{RnAppWindow, RnCanvas};
use gettextrs::gettext;
use gtk4::{
    Align, Button, Grid, Label, Popover, PositionType, SpinButton, Switch, gdk, glib, glib::clone,
    prelude::*,
};
use rnote_engine::pens::pensconfig::ShaperConfig;
use rnote_engine::strokes::{AxisTicks, AxisTicksStyle};
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

/// Hide the popup for the number of unit markings.
pub(crate) fn hide_axis_ticks_popover(canvas: &RnCanvas) {
    if let Some(popover) = find_popover(canvas) {
        popover.popdown();
    }
}

/// Show a popup next to the coordinate system that was just drawn, to set its unit markings: their
/// number on each half-axis (also with plus and minus on the keyboard), their size and spacing, and
/// how they are numbered.
pub(crate) fn show_axis_ticks_popover(appwindow: &RnAppWindow, canvas: &RnCanvas) {
    let Some(edit) = canvas.engine_ref().axis_ticks_edit() else {
        return;
    };

    // The anchor is in document coordinates
    let (mins, maxs) = (edit.anchor.mins, edit.anchor.maxs);
    let transform = canvas.engine_ref().camera.transform();
    let (top_left, bottom_right) = (
        transform.transform_point2(mins),
        transform.transform_point2(maxs),
    );
    let rect = gdk::Rectangle::new(
        top_left.x.floor() as i32,
        top_left.y.floor() as i32,
        ((bottom_right.x - top_left.x).ceil() as i32).max(1),
        ((bottom_right.y - top_left.y).ceil() as i32).max(1),
    );

    let count_spin = spin_button(
        AxisTicks::COUNT_MIN as f64,
        AxisTicks::COUNT_MAX as f64,
        edit.count as f64,
    );
    count_spin.set_tooltip_text(Some(&if edit.constant_spacing {
        gettext("Number of ticks on the shortest half-axis (+/-)")
    } else {
        gettext("Number of ticks on each half-axis (+/-)")
    }));
    let tick_size_spin = spin_button(
        ShaperConfig::AXIS_TICK_SIZE_MIN,
        ShaperConfig::AXIS_TICK_SIZE_MAX,
        edit.style.tick_size,
    );
    let constant_spacing_switch = Switch::builder()
        .active(edit.constant_spacing)
        .halign(Align::End)
        .valign(Align::Center)
        .tooltip_text(gettext(
            "On axes of different length, keep the distance between ticks the same instead of their number",
        ))
        .build();
    let numbering_switch = Switch::builder()
        .active(edit.style.numbering)
        .halign(Align::End)
        .valign(Align::Center)
        .build();
    let label_every_spin = spin_button(
        AxisTicks::LABEL_EVERY_MIN as f64,
        AxisTicks::LABEL_EVERY_MAX as f64,
        edit.style.label_every as f64,
    );
    label_every_spin.set_tooltip_text(Some(&gettext("Number only every this many ticks")));
    let font_size_spin = spin_button(
        AxisTicks::FONT_SIZE_MIN,
        AxisTicks::FONT_SIZE_MAX,
        edit.style.font_size,
    );
    // The number settings only matter while there are numbers
    for spin in [&label_every_spin, &font_size_spin] {
        numbering_switch
            .bind_property("active", spin, "sensitive")
            .sync_create()
            .build();
    }

    let grid = Grid::builder()
        .row_spacing(6)
        .column_spacing(18)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    let rows: [(String, &gtk4::Widget); 6] = [
        (gettext("Ticks"), count_spin.upcast_ref()),
        (gettext("Tick Size"), tick_size_spin.upcast_ref()),
        (
            gettext("Same Spacing"),
            constant_spacing_switch.upcast_ref(),
        ),
        (gettext("Numbers"), numbering_switch.upcast_ref()),
        (gettext("Number Every"), label_every_spin.upcast_ref()),
        (gettext("Number Size"), font_size_spin.upcast_ref()),
    ];
    for (row, (title, control)) in rows.iter().enumerate() {
        let label = Label::builder()
            .label(title.as_str())
            .halign(Align::Start)
            .valign(Align::Center)
            .build();
        grid.attach(&label, 0, row as i32, 1, 1);
        grid.attach(*control, 1, row as i32, 1, 1);
    }

    let done_button = Button::builder()
        .label(gettext("Done"))
        .tooltip_text(gettext("Close (Enter)"))
        .halign(Align::End)
        .margin_top(6)
        .css_classes(["suggested-action"])
        .build();
    grid.attach(&done_button, 0, rows.len() as i32, 2, 1);

    let popover = Popover::builder()
        .name(POPOVER_NAME)
        .child(&grid)
        .position(PositionType::Bottom)
        .pointing_to(&rect)
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.set_parent(canvas);

    // Every control applies all of them, so the coordinate system always shows what they say
    let controls = Rc::new(Controls {
        count: count_spin.downgrade(),
        tick_size: tick_size_spin.downgrade(),
        constant_spacing: constant_spacing_switch.downgrade(),
        numbering: numbering_switch.downgrade(),
        label_every: label_every_spin.downgrade(),
        font_size: font_size_spin.downgrade(),
    });
    let apply = Rc::new(clone!(
        #[weak]
        appwindow,
        #[weak]
        canvas,
        #[strong]
        controls,
        move || {
            let Some((count, constant_spacing, style)) = controls.values() else {
                return;
            };
            let widget_flags =
                canvas
                    .engine_mut()
                    .axis_ticks_update(count, constant_spacing, style);
            appwindow.handle_widget_flags(widget_flags, &canvas);
        }
    ));
    for spin in [
        &count_spin,
        &tick_size_spin,
        &label_every_spin,
        &font_size_spin,
    ] {
        spin.connect_value_changed(clone!(
            #[strong]
            apply,
            move |_| apply()
        ));
    }
    for switch in [&constant_spacing_switch, &numbering_switch] {
        switch.connect_active_notify(clone!(
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

    // Keyboard: plus and minus change the count, enter confirms
    let key_controller = gtk4::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    key_controller.connect_key_pressed(clone!(
        #[weak]
        count_spin,
        #[weak]
        popover,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| match key {
            gdk::Key::plus | gdk::Key::KP_Add | gdk::Key::equal => {
                count_spin.set_value(count_spin.value() + 1.0);
                glib::Propagation::Stop
            }
            gdk::Key::minus | gdk::Key::KP_Subtract | gdk::Key::underscore => {
                count_spin.set_value(count_spin.value() - 1.0);
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                popover.popdown();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    ));
    popover.add_controller(key_controller);

    popover.connect_closed(clone!(
        #[weak]
        appwindow,
        #[weak]
        canvas,
        move |popover| {
            let widget_flags = canvas.engine_mut().axis_ticks_finish();
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
    count: glib::WeakRef<SpinButton>,
    tick_size: glib::WeakRef<SpinButton>,
    constant_spacing: glib::WeakRef<Switch>,
    numbering: glib::WeakRef<Switch>,
    label_every: glib::WeakRef<SpinButton>,
    font_size: glib::WeakRef<SpinButton>,
}

impl Controls {
    /// The count, whether the spacing is constant and the look of the ticks they are set to.
    fn values(&self) -> Option<(u32, bool, AxisTicksStyle)> {
        Some((
            self.count.upgrade()?.value().round() as u32,
            self.constant_spacing.upgrade()?.is_active(),
            AxisTicksStyle {
                tick_size: self.tick_size.upgrade()?.value(),
                numbering: self.numbering.upgrade()?.is_active(),
                label_every: self.label_every.upgrade()?.value().round() as u32,
                font_size: self.font_size.upgrade()?.value(),
            },
        ))
    }
}

/// A spin button for a whole number in the given range.
fn spin_button(min: f64, max: f64, value: f64) -> SpinButton {
    let spin = SpinButton::with_range(min, max, 1.0);
    spin.set_digits(0);
    spin.set_numeric(true);
    spin.set_value(value);
    spin.set_halign(Align::End);
    spin.set_valign(Align::Center);
    spin
}
