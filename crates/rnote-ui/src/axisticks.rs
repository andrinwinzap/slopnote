// Imports
use crate::{RnAppWindow, RnCanvas};
use gettextrs::gettext;
use gtk4::{
    Align, Box, Button, Entry, Grid, Label, Orientation, Overlay, Separator, SpinButton, Switch,
    gdk, glib, glib::clone, graphene, prelude::*,
};
use rnote_engine::pens::CoordSystemSettings;
use rnote_engine::pens::pensconfig::ShaperConfig;
use rnote_engine::strokes::{AxisTicks, AxisTicksStyle};
use std::rc::Rc;

const CARD_NAME: &str = "coord-system-card";
/// The space kept between the card and the coordinate system or the edges of the window.
const CARD_MARGIN: i32 = 8;

fn find_card(overlay: &Overlay) -> Option<gtk4::Widget> {
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == CARD_NAME {
            return Some(widget);
        }
        child = widget.next_sibling();
    }
    None
}

/// Hide the card for the coordinate system.
pub(crate) fn hide_axis_ticks_popover(appwindow: &RnAppWindow) {
    let overlay = appwindow.overlays().toolbar_overlay();
    if let Some(card) = find_card(&overlay) {
        overlay.remove_overlay(&card);
    }
}

/// Show a card next to the coordinate system that was just drawn, to set how it is marked: the
/// number of ticks on each half-axis (also with plus and minus on the keyboard), their size,
/// spacing and numbers, the arrow heads and names at the ends of the axes, and for 3D coordinate
/// systems whether the third axis goes into the page or comes out of it.
///
/// It is a card over the canvas rather than a popover: a popover closes on any press next to it,
/// a palm resting on the screen included, and one that does not cannot take the keyboard.
pub(crate) fn show_axis_ticks_popover(appwindow: &RnAppWindow, canvas: &RnCanvas) {
    let Some(edit) = canvas.engine_ref().coord_system_edit() else {
        return;
    };
    hide_axis_ticks_popover(appwindow);
    let settings = &edit.settings;

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

    let ticks_switch = switch(settings.ticks);
    let count_spin = spin_button(
        AxisTicks::COUNT_MIN as f64,
        AxisTicks::COUNT_MAX as f64,
        settings.count as f64,
    );
    count_spin.set_tooltip_text(Some(&if settings.constant_spacing {
        gettext("Number of ticks on the shortest half-axis (+/-)")
    } else {
        gettext("Number of ticks on each half-axis (+/-)")
    }));
    let tick_size_spin = spin_button(
        ShaperConfig::AXIS_TICK_SIZE_MIN,
        ShaperConfig::AXIS_TICK_SIZE_MAX,
        settings.tick_style.tick_size,
    );
    let constant_spacing_switch = switch(settings.constant_spacing);
    constant_spacing_switch.set_tooltip_text(Some(&gettext(
        "On axes of different length, keep the distance between ticks the same instead of their number",
    )));
    let numbering_switch = switch(settings.tick_style.numbering);
    let label_every_spin = spin_button(
        AxisTicks::LABEL_EVERY_MIN as f64,
        AxisTicks::LABEL_EVERY_MAX as f64,
        settings.tick_style.label_every as f64,
    );
    label_every_spin.set_tooltip_text(Some(&gettext("Number only every this many ticks")));
    let font_size_spin = spin_button(
        AxisTicks::FONT_SIZE_MIN,
        AxisTicks::FONT_SIZE_MAX,
        settings.tick_style.font_size,
    );
    font_size_spin.set_tooltip_text(Some(&gettext(
        "The size of the numbers. The names of the axes are a little larger",
    )));
    let arrows_switch = switch(settings.arrows);
    let labels_switch = switch(settings.labels);
    // The 3D coordinate system has a third axis to name
    let name_count = if edit.three_d { 3 } else { 2 };
    let name_entries = settings.label_names[..name_count]
        .iter()
        .map(|name| {
            Entry::builder()
                .text(name.as_str())
                .width_chars(3)
                .max_width_chars(3)
                .xalign(0.5)
                .build()
        })
        .collect::<Vec<Entry>>();
    let names_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .halign(Align::End)
        .build();
    for entry in &name_entries {
        names_box.append(entry);
    }
    let third_into_page_switch = switch(settings.third_into_page);
    third_into_page_switch.set_tooltip_text(Some(&gettext(
        "Whether the axis across the page grows into it, towards the upper right, or comes out of it, towards the lower left",
    )));

    let grid = Grid::builder()
        .row_spacing(6)
        .column_spacing(18)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let mut sections: Vec<Vec<(String, gtk4::Widget)>> = vec![
        vec![
            (gettext("Ticks"), ticks_switch.clone().upcast()),
            (gettext("Number of Ticks"), count_spin.clone().upcast()),
            (gettext("Tick Size"), tick_size_spin.clone().upcast()),
            (
                gettext("Same Spacing"),
                constant_spacing_switch.clone().upcast(),
            ),
            (gettext("Numbers"), numbering_switch.clone().upcast()),
            (gettext("Number Every"), label_every_spin.clone().upcast()),
        ],
        vec![(gettext("Text Size"), font_size_spin.clone().upcast())],
        vec![
            (gettext("Arrows"), arrows_switch.clone().upcast()),
            (gettext("Axis Names"), labels_switch.clone().upcast()),
            (gettext("Names"), names_box.clone().upcast()),
        ],
    ];
    if edit.three_d {
        sections.push(vec![(
            gettext("Third Axis Into Page"),
            third_into_page_switch.clone().upcast(),
        )]);
    }
    let mut row = 0;
    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            grid.attach(&Separator::new(Orientation::Horizontal), 0, row, 2, 1);
            row += 1;
        }
        for (title, control) in section {
            let label = Label::builder()
                .label(title.as_str())
                .halign(Align::Start)
                .valign(Align::Center)
                .build();
            grid.attach(&label, 0, row, 1, 1);
            grid.attach(control, 1, row, 1, 1);
            row += 1;
        }
    }

    let done_button = Button::builder()
        .label(gettext("Done"))
        .tooltip_text(gettext("Close (Enter)"))
        .halign(Align::End)
        .margin_top(6)
        .css_classes(["suggested-action"])
        .build();
    grid.attach(&done_button, 0, row, 2, 1);

    let card = Box::builder()
        .name(CARD_NAME)
        .orientation(Orientation::Vertical)
        .halign(Align::Start)
        .valign(Align::Start)
        .css_classes(["card"])
        .build();
    card.append(&grid);
    let overlay = appwindow.overlays().toolbar_overlay();
    overlay.add_overlay(&card);
    place_card(&card, &overlay, canvas, &rect);

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
        labels: labels_switch.downgrade(),
        names: name_entries.iter().map(|entry| entry.downgrade()).collect(),
        third_into_page: third_into_page_switch.downgrade(),
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

    // Closing the card finishes editing the coordinate system
    let close = Rc::new(clone!(
        #[weak]
        appwindow,
        #[weak]
        canvas,
        move || {
            hide_axis_ticks_popover(&appwindow);
            let widget_flags = canvas.engine_mut().coord_system_finish();
            appwindow.handle_widget_flags(widget_flags, &canvas);
        }
    ));
    done_button.connect_clicked(clone!(
        #[strong]
        close,
        move |_| close()
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
        #[strong]
        close,
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
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::Escape => {
                    close();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        }
    ));
    card.add_controller(key_controller);

    count_spin.grab_focus();
}

/// Put the card below the coordinate system, or above it when there is no room below, and inside
/// the window. `rect` is the coordinate system in canvas coordinates.
fn place_card(card: &Box, overlay: &Overlay, canvas: &RnCanvas, rect: &gdk::Rectangle) {
    let corner = |x: i32, y: i32| {
        canvas
            .compute_point(overlay, &graphene::Point::new(x as f32, y as f32))
            .map(|point| (point.x() as i32, point.y() as i32))
            .unwrap_or((x, y))
    };
    let (left, top) = corner(rect.x(), rect.y());
    let (right, bottom) = corner(rect.x() + rect.width(), rect.y() + rect.height());
    let (_, width, _, _) = card.measure(Orientation::Horizontal, -1);
    let (_, height, _, _) = card.measure(Orientation::Vertical, width);
    let (overlay_width, overlay_height) = (overlay.width(), overlay.height());

    let x = ((left + right) / 2 - width / 2)
        .min(overlay_width - width - CARD_MARGIN)
        .max(CARD_MARGIN);
    let y = if bottom + CARD_MARGIN + height <= overlay_height - CARD_MARGIN {
        bottom + CARD_MARGIN
    } else {
        (top - CARD_MARGIN - height).max(CARD_MARGIN)
    };
    card.set_margin_start(x);
    card.set_margin_top(y);
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
    labels: glib::WeakRef<Switch>,
    names: Vec<glib::WeakRef<Entry>>,
    third_into_page: glib::WeakRef<Switch>,
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
            labels: self.labels.upgrade()?.is_active(),
            label_names,
            third_into_page: self.third_into_page.upgrade()?.is_active(),
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
        for spin in [&self.count, &self.tick_size] {
            if let Some(spin) = spin.upgrade() {
                spin.set_sensitive(ticks);
            }
        }
        for switch in [&self.constant_spacing, &self.numbering] {
            if let Some(switch) = switch.upgrade() {
                switch.set_sensitive(ticks);
            }
        }
        if let Some(spin) = self.label_every.upgrade() {
            spin.set_sensitive(numbers);
        }
        if let Some(spin) = self.font_size.upgrade() {
            spin.set_sensitive(numbers || settings.labels);
        }
        for entry in self.names.iter().filter_map(|entry| entry.upgrade()) {
            entry.set_sensitive(settings.labels);
        }
    }
}

/// A switch for a row of the popup.
fn switch(active: bool) -> Switch {
    Switch::builder()
        .active(active)
        .halign(Align::End)
        .valign(Align::Center)
        .build()
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
