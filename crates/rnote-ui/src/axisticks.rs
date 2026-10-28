// Imports
use crate::{RnAppWindow, RnCanvas};
use adw::prelude::*;
use gettextrs::gettext;
use gtk4::{
    Align, Box, Button, Entry, Label, Orientation, Popover, PositionType, ScrolledWindow, Switch,
    gdk, glib, glib::clone, graphene,
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

    let ticks_switch = Switch::builder()
        .active(settings.ticks)
        .valign(Align::Center)
        .tooltip_text(gettext("Ticks on the axes"))
        .build();
    let count_row = spin_row(
        &gettext("Number of Ticks"),
        &if settings.constant_spacing {
            gettext("On the shortest half-axis (+/−)")
        } else {
            gettext("On each half-axis (+/−)")
        },
        AxisTicks::COUNT_MIN as f64,
        AxisTicks::COUNT_MAX as f64,
        settings.count as f64,
    );
    let tick_size_row = spin_row(
        &gettext("Tick Size"),
        "",
        ShaperConfig::AXIS_TICK_SIZE_MIN,
        ShaperConfig::AXIS_TICK_SIZE_MAX,
        settings.tick_style.tick_size,
    );
    let constant_spacing_row = switch_row(
        &gettext("Same Spacing"),
        &gettext("On axes of different length"),
        settings.constant_spacing,
    );
    let numbering_row = switch_row(&gettext("Numbers"), "", settings.tick_style.numbering);
    let label_every_row = spin_row(
        &gettext("Number Every"),
        &gettext("Ticks from one number to the next"),
        AxisTicks::LABEL_EVERY_MIN as f64,
        AxisTicks::LABEL_EVERY_MAX as f64,
        settings.tick_style.label_every as f64,
    );
    let font_size_row = spin_row(
        &gettext("Text Size"),
        &gettext("The names are a little larger"),
        AxisTicks::FONT_SIZE_MIN,
        AxisTicks::FONT_SIZE_MAX,
        settings.tick_style.font_size,
    );
    let arrows_row = switch_row(&gettext("Arrows"), "", settings.arrows);
    let arrow_size_row = spin_row(
        &gettext("Arrow Size"),
        "",
        AxisEnd::ARROW_SIZE_MIN,
        AxisEnd::ARROW_SIZE_MAX,
        settings.arrow_size,
    );
    let labels_row = switch_row(&gettext("Axis Names"), "", settings.labels);
    // The 3D coordinate system has a third axis to name
    let name_count = if edit.three_d { 3 } else { 2 };
    let name_entries = settings.label_names[..name_count]
        .iter()
        .map(|name| {
            Entry::builder()
                .text(name.as_str())
                .width_chars(2)
                .max_width_chars(3)
                .xalign(0.5)
                .valign(Align::Center)
                .build()
        })
        .collect::<Vec<Entry>>();
    let names_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .valign(Align::Center)
        .build();
    for entry in &name_entries {
        names_box.append(entry);
    }
    let names_row = adw::ActionRow::builder()
        .title(gettext("Names"))
        .subtitle(if edit.three_d {
            gettext("Of x, y and z")
        } else {
            gettext("Of x and y")
        })
        .build();
    names_row.add_suffix(&names_box);
    let third_into_page_row = switch_row(
        &gettext("Depth Axis Into Page"),
        &gettext("Instead of out of it"),
        settings.third_into_page,
    );
    let right_handed_row = switch_row(
        &gettext("Right-Handed"),
        &gettext("Decides whether x or y is the depth axis"),
        settings.right_handed,
    );

    // Two columns side by side, the ticks on the left and the ends of the axes on the right, so
    // that the popup is not too tall to fit next to the coordinate system
    let ticks_group = adw::PreferencesGroup::builder()
        .title(gettext("Ticks"))
        .header_suffix(&ticks_switch)
        .valign(Align::Start)
        .width_request(300)
        .build();
    ticks_group.add(&count_row);
    ticks_group.add(&tick_size_row);
    ticks_group.add(&constant_spacing_row);
    ticks_group.add(&numbering_row);
    ticks_group.add(&label_every_row);
    ticks_group.add(&font_size_row);

    let axes_group = adw::PreferencesGroup::builder()
        .title(gettext("Axes"))
        .build();
    axes_group.add(&arrows_row);
    axes_group.add(&arrow_size_row);
    axes_group.add(&labels_row);
    axes_group.add(&names_row);
    let axes_column = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .valign(Align::Start)
        .width_request(300)
        .build();
    axes_column.append(&axes_group);
    if edit.three_d {
        let three_d_group = adw::PreferencesGroup::builder()
            .title(gettext("3D"))
            .build();
        three_d_group.add(&third_into_page_row);
        three_d_group.add(&right_handed_row);
        axes_column.append(&three_d_group);
    }
    let columns = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(18)
        .build();
    columns.append(&ticks_group);
    columns.append(&axes_column);

    let title = Label::builder()
        .label(gettext("Coordinate System"))
        .halign(Align::Start)
        .hexpand(true)
        .css_classes(["title-4"])
        .build();
    let done_button = Button::builder()
        .label(gettext("Done"))
        .tooltip_text(gettext("Close (Enter)"))
        .valign(Align::Center)
        .css_classes(["suggested-action", "pill"])
        .build();
    let header = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .build();
    header.append(&title);
    header.append(&done_button);

    let content = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&header);
    content.append(&columns);

    let popover = Popover::builder()
        .name(POPOVER_NAME)
        .child(&content)
        .position(position)
        .pointing_to(&rect)
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.set_parent(canvas);

    // Every control applies all of them, so the coordinate system always shows what they say
    let controls = Rc::new(Controls {
        ticks: ticks_switch.downgrade(),
        count: count_row.downgrade(),
        tick_size: tick_size_row.downgrade(),
        constant_spacing: constant_spacing_row.downgrade(),
        numbering: numbering_row.downgrade(),
        label_every: label_every_row.downgrade(),
        font_size: font_size_row.downgrade(),
        arrows: arrows_row.downgrade(),
        arrow_size: arrow_size_row.downgrade(),
        labels: labels_row.downgrade(),
        names: name_entries.iter().map(|entry| entry.downgrade()).collect(),
        third_into_page: third_into_page_row.downgrade(),
        right_handed: right_handed_row.downgrade(),
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
    for row in [
        &count_row,
        &tick_size_row,
        &label_every_row,
        &font_size_row,
        &arrow_size_row,
    ] {
        row.connect_value_notify(clone!(
            #[strong]
            apply,
            move |_| apply()
        ));
    }
    ticks_switch.connect_active_notify(clone!(
        #[strong]
        apply,
        move |_| apply()
    ));
    for row in [
        &constant_spacing_row,
        &numbering_row,
        &arrows_row,
        &labels_row,
        &third_into_page_row,
        &right_handed_row,
    ] {
        row.connect_active_notify(clone!(
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
        count_row,
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
                    count_row.set_value(count_row.value() + 1.0);
                    glib::Propagation::Stop
                }
                gdk::Key::minus | gdk::Key::KP_Subtract | gdk::Key::underscore if !typing_name => {
                    count_row.set_value(count_row.value() - 1.0);
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
    count_row.grab_focus();
}

/// The controls of the popup, held weakly so the handlers connected to them do not keep them alive.
struct Controls {
    ticks: glib::WeakRef<Switch>,
    count: glib::WeakRef<adw::SpinRow>,
    tick_size: glib::WeakRef<adw::SpinRow>,
    constant_spacing: glib::WeakRef<adw::SwitchRow>,
    numbering: glib::WeakRef<adw::SwitchRow>,
    label_every: glib::WeakRef<adw::SpinRow>,
    font_size: glib::WeakRef<adw::SpinRow>,
    arrows: glib::WeakRef<adw::SwitchRow>,
    arrow_size: glib::WeakRef<adw::SpinRow>,
    labels: glib::WeakRef<adw::SwitchRow>,
    names: Vec<glib::WeakRef<Entry>>,
    third_into_page: glib::WeakRef<adw::SwitchRow>,
    right_handed: glib::WeakRef<adw::SwitchRow>,
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

/// A row with a switch.
fn switch_row(title: &str, subtitle: &str, active: bool) -> adw::SwitchRow {
    adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .active(active)
        .build()
}

/// A row with a spin button for a whole number in the given range.
fn spin_row(title: &str, subtitle: &str, min: f64, max: f64, value: f64) -> adw::SpinRow {
    let row = adw::SpinRow::with_range(min, max, 1.0);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_digits(0);
    row.set_numeric(true);
    row.set_value(value);
    row
}
