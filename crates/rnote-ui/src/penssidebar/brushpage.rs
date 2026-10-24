// Imports
use crate::{RnAppWindow, RnStrokeWidthPicker};
use adw::prelude::*;
use gtk4::{
    Button, CompositeTemplate, ListBox, MenuButton, Popover, ToggleButton, Widget, glib,
    glib::clone, subclass::prelude::*,
};
use num_traits::cast::ToPrimitive;
use rnote_compose::builders::PenPathBuilderType;
use rnote_compose::scribblerecognition::ScribbleRecognitionOptions;
use rnote_compose::style::PressureCurve;
use rnote_compose::style::textured::{TexturedDotsDistribution, TexturedOptions};
use rnote_engine::pens::pensconfig::BrushConfig;
use rnote_engine::pens::pensconfig::brushconfig::{BrushStyle, SolidOptions};
use std::time::Duration;

mod imp {
    use super::*;

    #[derive(Default, Debug, CompositeTemplate)]
    #[template(resource = "/com/github/flxzt/rnote/ui/penssidebar/brushpage.ui")]
    pub(crate) struct RnBrushPage {
        #[template_child]
        pub(crate) brushstyle_solid_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brushstyle_marker_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brushstyle_textured_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) brushconfig_menubutton: TemplateChild<MenuButton>,
        #[template_child]
        pub(crate) brushconfig_popover: TemplateChild<Popover>,
        #[template_child]
        pub(crate) brushconfig_popover_close_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) brush_buildertype_listbox: TemplateChild<ListBox>,
        #[template_child]
        pub(crate) brush_buildertype_simple: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub(crate) brush_buildertype_curved: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub(crate) brush_buildertype_modeled: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub(crate) shaperecognition_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) shaperecognition_delay_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) scribbleerase_row: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub(crate) scribbleerase_min_turns_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) scribbleerase_min_turn_rate_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) scribbleerase_pass_height_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) scribbleerase_pass_length_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) scribbleerase_min_size_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) scribbleerase_min_length_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) scribbleerase_reset_row: TemplateChild<adw::ButtonRow>,
        #[template_child]
        pub(crate) solidstyle_pressure_curves_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub(crate) texturedstyle_density_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) texturedstyle_distribution_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub(crate) stroke_width_picker: TemplateChild<RnStrokeWidthPicker>,
        #[template_child]
        pub(crate) ruler_toggle: TemplateChild<ToggleButton>,
        #[template_child]
        pub(crate) ruler_menubutton: TemplateChild<MenuButton>,
        #[template_child]
        pub(crate) ruler_popover: TemplateChild<Popover>,
        #[template_child]
        pub(crate) ruler_popover_close_button: TemplateChild<Button>,
        #[template_child]
        pub(crate) ruler_snap_distance_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) ruler_angle_snap_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) ruler_show_dial_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub(crate) ruler_width_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) ruler_tick_spacing_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) ruler_body_opacity_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub(crate) ruler_scroll_step_row: TemplateChild<adw::SpinRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RnBrushPage {
        const NAME: &'static str = "RnBrushPage";
        type Type = super::RnBrushPage;
        type ParentType = Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for RnBrushPage {
        fn constructed(&self) {
            self.parent_constructed();
        }

        fn dispose(&self) {
            self.dispose_template();
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RnBrushPage {}
}

glib::wrapper! {
    pub(crate) struct RnBrushPage(ObjectSubclass<imp::RnBrushPage>)
        @extends Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for RnBrushPage {
    fn default() -> Self {
        Self::new()
    }
}

impl RnBrushPage {
    pub(crate) fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn brushconfig_menubutton(&self) -> MenuButton {
        self.imp().brushconfig_menubutton.get()
    }

    pub(crate) fn brush_style(&self) -> Option<BrushStyle> {
        if self.imp().brushstyle_marker_toggle.is_active() {
            Some(BrushStyle::Marker)
        } else if self.imp().brushstyle_solid_toggle.is_active() {
            Some(BrushStyle::Solid)
        } else if self.imp().brushstyle_textured_toggle.is_active() {
            Some(BrushStyle::Textured)
        } else {
            None
        }
    }

    pub(crate) fn set_brush_style(&self, brush_style: BrushStyle) {
        match brush_style {
            BrushStyle::Marker => self.imp().brushstyle_marker_toggle.set_active(true),
            BrushStyle::Solid => self.imp().brushstyle_solid_toggle.set_active(true),
            BrushStyle::Textured => self.imp().brushstyle_textured_toggle.set_active(true),
        }
    }

    fn apply_brush_style_selection(&self, appwindow: &RnAppWindow, brush_style: BrushStyle) {
        appwindow
            .engine_config()
            .write()
            .pens_config
            .brush_config
            .style = brush_style;
        self.stroke_width_picker().deselect_setters();

        match brush_style {
            BrushStyle::Marker => {
                let stroke_width = appwindow
                    .engine_config()
                    .read()
                    .pens_config
                    .brush_config
                    .marker_options
                    .stroke_width;
                self.imp()
                    .stroke_width_picker
                    .set_stroke_width(stroke_width);
            }
            BrushStyle::Solid => {
                let stroke_width = appwindow
                    .engine_config()
                    .read()
                    .pens_config
                    .brush_config
                    .solid_options
                    .stroke_width;
                self.imp()
                    .stroke_width_picker
                    .set_stroke_width(stroke_width);
            }
            BrushStyle::Textured => {
                let stroke_width = appwindow
                    .engine_config()
                    .read()
                    .pens_config
                    .brush_config
                    .textured_options
                    .stroke_width;
                self.imp()
                    .stroke_width_picker
                    .set_stroke_width(stroke_width);
            }
        }
    }

    pub(crate) fn buildertype(&self) -> Option<PenPathBuilderType> {
        PenPathBuilderType::try_from(
            self.imp().brush_buildertype_listbox.selected_row()?.index() as u32
        )
        .ok()
    }

    pub(crate) fn set_buildertype(&self, buildertype: PenPathBuilderType) {
        match buildertype {
            PenPathBuilderType::Simple => self
                .imp()
                .brush_buildertype_listbox
                .select_row(Some(&*self.imp().brush_buildertype_simple)),
            PenPathBuilderType::Curved => self
                .imp()
                .brush_buildertype_listbox
                .select_row(Some(&*self.imp().brush_buildertype_curved)),
            PenPathBuilderType::Modeled => self
                .imp()
                .brush_buildertype_listbox
                .select_row(Some(&*self.imp().brush_buildertype_modeled)),
        }
    }

    pub(crate) fn solidstyle_pressure_curve(&self) -> PressureCurve {
        PressureCurve::try_from(self.imp().solidstyle_pressure_curves_row.get().selected()).unwrap()
    }

    pub(crate) fn set_solidstyle_pressure_curve(&self, pressure_curve: PressureCurve) {
        let position = pressure_curve.to_u32().unwrap();

        self.imp()
            .solidstyle_pressure_curves_row
            .get()
            .set_selected(position);
    }

    pub(crate) fn texturedstyle_dots_distribution(&self) -> TexturedDotsDistribution {
        TexturedDotsDistribution::try_from(
            self.imp().texturedstyle_distribution_row.get().selected(),
        )
        .unwrap()
    }

    pub(crate) fn set_texturedstyle_distribution_variant(
        &self,
        distribution: TexturedDotsDistribution,
    ) {
        let position = distribution.to_u32().unwrap();

        self.imp()
            .texturedstyle_distribution_row
            .get()
            .set_selected(position);
    }

    pub(crate) fn stroke_width_picker(&self) -> RnStrokeWidthPicker {
        self.imp().stroke_width_picker.get()
    }

    pub(crate) fn init(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();
        let brushconfig_popover = imp.brushconfig_popover.get();

        // Popovers
        imp.brushconfig_popover_close_button.connect_clicked(clone!(
            #[weak]
            brushconfig_popover,
            move |_| {
                brushconfig_popover.popdown();
            }
        ));

        // Stroke width
        imp.stroke_width_picker
            .spinbutton()
            .set_range(BrushConfig::STROKE_WIDTH_MIN, BrushConfig::STROKE_WIDTH_MAX);
        // set value after the range!
        imp.stroke_width_picker
            .set_stroke_width(SolidOptions::default().stroke_width);

        imp.stroke_width_picker.connect_notify_local(
            Some("stroke-width"),
            clone!(
                #[weak]
                appwindow,
                move |picker, _| {
                    let stroke_width = picker.stroke_width();
                    let brush_style = appwindow
                        .engine_config()
                        .read()
                        .pens_config
                        .brush_config
                        .style;

                    match brush_style {
                        BrushStyle::Marker => {
                            appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .brush_config
                                .marker_options
                                .stroke_width = stroke_width;
                        }
                        BrushStyle::Solid => {
                            appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .brush_config
                                .solid_options
                                .stroke_width = stroke_width;
                        }
                        BrushStyle::Textured => {
                            appwindow
                                .engine_config()
                                .write()
                                .pens_config
                                .brush_config
                                .textured_options
                                .stroke_width = stroke_width;
                        }
                    }
                }
            ),
        );

        // Style
        imp.brushstyle_marker_toggle.connect_toggled(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(brush_style) = brushpage.brush_style() else {
                    return;
                };

                brushpage.apply_brush_style_selection(&appwindow, brush_style);
            }
        ));

        imp.brushstyle_solid_toggle.connect_toggled(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(brush_style) = brushpage.brush_style() else {
                    return;
                };

                brushpage.apply_brush_style_selection(&appwindow, brush_style);
            }
        ));

        imp.brushstyle_textured_toggle.connect_toggled(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |toggle| {
                if !toggle.is_active() {
                    return;
                }

                let Some(brush_style) = brushpage.brush_style() else {
                    return;
                };

                brushpage.apply_brush_style_selection(&appwindow, brush_style);
            }
        ));

        // Builder type
        imp.brush_buildertype_listbox.connect_row_selected(clone!(
            #[weak(rename_to=brushpage)]
            self,
            #[weak]
            appwindow,
            move |_, _| {
                let Some(buildertype) = brushpage.buildertype() else {
                    return;
                };
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .builder_type = buildertype;
            }
        ));

        // Shape recognition
        imp.shaperecognition_row.connect_active_notify(clone!(
            #[weak]
            appwindow,
            #[weak(rename_to=brushpage)]
            self,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .shape_recognition_enabled = row.is_active();
                brushpage
                    .imp()
                    .shaperecognition_delay_row
                    .set_sensitive(row.is_active());
            }
        ));

        imp.shaperecognition_delay_row.get().set_range(
            BrushConfig::SHAPE_RECOGNITION_DELAY_MIN.as_secs_f64(),
            BrushConfig::SHAPE_RECOGNITION_DELAY_MAX.as_secs_f64(),
        );
        // set value after the range!
        imp.shaperecognition_delay_row
            .get()
            .set_value(BrushConfig::SHAPE_RECOGNITION_DELAY_DEFAULT.as_secs_f64());

        imp.shaperecognition_delay_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .shape_recognition_delay = Duration::from_secs_f64(row.value());
            }
        ));

        // Scratch to erase
        imp.scribbleerase_row
            .connect_enable_expansion_notify(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .scribble_erase_enabled = row.enables_expansion();
                }
            ));

        imp.scribbleerase_min_turns_row.get().set_range(
            ScribbleRecognitionOptions::MIN_REVERSALS_MIN as f64,
            ScribbleRecognitionOptions::MIN_REVERSALS_MAX as f64,
        );
        imp.scribbleerase_min_turns_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .scribble_recognition_options
                        .min_reversals = row.value().round() as u32;
                }
            ));

        imp.scribbleerase_min_turn_rate_row.get().set_range(
            ScribbleRecognitionOptions::MIN_TURN_RATE_MIN,
            ScribbleRecognitionOptions::MIN_TURN_RATE_MAX,
        );
        imp.scribbleerase_min_turn_rate_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .scribble_recognition_options
                        .min_turn_rate = row.value();
                }
            ));

        // The ratio is shown in percent
        imp.scribbleerase_pass_height_row.get().set_range(
            ScribbleRecognitionOptions::REVERSAL_PROMINENCE_RATIO_MIN * 100.0,
            ScribbleRecognitionOptions::REVERSAL_PROMINENCE_RATIO_MAX * 100.0,
        );
        imp.scribbleerase_pass_height_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .scribble_recognition_options
                        .reversal_prominence_ratio = row.value() / 100.0;
                }
            ));

        imp.scribbleerase_pass_length_row.get().set_range(
            ScribbleRecognitionOptions::REVERSAL_MIN_PROMINENCE_SURFACE_MIN,
            ScribbleRecognitionOptions::REVERSAL_MIN_PROMINENCE_SURFACE_MAX,
        );
        imp.scribbleerase_pass_length_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .scribble_recognition_options
                        .reversal_min_prominence_surface = row.value();
                }
            ));

        imp.scribbleerase_min_size_row.get().set_range(
            ScribbleRecognitionOptions::MIN_EXTENT_SURFACE_MIN,
            ScribbleRecognitionOptions::MIN_EXTENT_SURFACE_MAX,
        );
        imp.scribbleerase_min_size_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .scribble_recognition_options
                    .min_extent_surface = row.value();
            }
        ));

        imp.scribbleerase_min_length_row.get().set_range(
            ScribbleRecognitionOptions::MIN_LENGTH_DIAG_RATIO_MIN,
            ScribbleRecognitionOptions::MIN_LENGTH_DIAG_RATIO_MAX,
        );
        imp.scribbleerase_min_length_row
            .get()
            .connect_changed(clone!(
                #[weak]
                appwindow,
                move |row| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .scribble_recognition_options
                        .min_length_diag_ratio = row.value();
                }
            ));

        imp.scribbleerase_reset_row.get().connect_activated(clone!(
            #[weak(rename_to=brushpage)]
            self,
            move |_| {
                // Setting the rows writes the defaults back to the config.
                brushpage.set_scribble_recognition_options(ScribbleRecognitionOptions::default());
            }
        ));

        // Solid style
        // Pressure curve
        imp.solidstyle_pressure_curves_row
            .get()
            .connect_selected_notify(clone!(
                #[weak(rename_to=brushpage)]
                self,
                #[weak]
                appwindow,
                move |_| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .solid_options
                        .pressure_curve = brushpage.solidstyle_pressure_curve();
                }
            ));

        // Textured style
        // Density
        imp.texturedstyle_density_row
            .get()
            .set_range(TexturedOptions::DENSITY_MIN, TexturedOptions::DENSITY_MAX);
        // set value after the range!
        imp.texturedstyle_density_row
            .get()
            .set_value(TexturedOptions::default().density);

        imp.texturedstyle_density_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .textured_options
                    .density = row.value();
            }
        ));

        // dots distribution
        imp.texturedstyle_distribution_row
            .get()
            .connect_selected_notify(clone!(
                #[weak(rename_to=brushpage)]
                self,
                #[weak]
                appwindow,
                move |_| {
                    appwindow
                        .engine_config()
                        .write()
                        .pens_config
                        .brush_config
                        .textured_options
                        .distribution = brushpage.texturedstyle_dots_distribution();
                }
            ));

        // Ruler toggle: enables/disables the on-canvas ruler. The first time the
        // user enables it, seed the ruler position to the current viewport center.
        imp.ruler_toggle.connect_toggled(clone!(
            #[weak]
            appwindow,
            move |toggle| {
                let visible = toggle.is_active();
                let needs_seed = {
                    let mut config = appwindow.engine_config().write();
                    let ruler = &mut config.pens_config.brush_config.ruler_config;
                    ruler.visible = visible;
                    visible && ruler.anchor == p2d::math::Vector2::ZERO
                };
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    if needs_seed {
                        // The ruler position is in window coordinates — seed it to the center
                        // of the visible area, which is what the canvas wrapper covers.
                        let center_window = appwindow
                            .active_tab_wrapper()
                            .map(|w| {
                                p2d::math::Vector2::new(w.width() as f64, w.height() as f64) * 0.5
                            })
                            .unwrap_or_else(|| canvas.engine_ref().camera.size() * 0.5);
                        let mut c = appwindow.engine_config().write();
                        let r = &mut c.pens_config.brush_config.ruler_config;
                        r.anchor = center_window;
                        r.dial_pos = center_window;
                    }
                    canvas.queue_draw();
                }
            }
        ));

        let ruler_popover = imp.ruler_popover.get();
        imp.ruler_popover_close_button.connect_clicked(clone!(
            #[weak]
            ruler_popover,
            move |_| {
                ruler_popover.popdown();
            }
        ));

        imp.ruler_snap_distance_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .snap_distance = row.value();
            }
        ));

        imp.ruler_angle_snap_row.get().connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .angle_snap_enabled = row.is_active();
            }
        ));

        imp.ruler_show_dial_row.get().connect_active_notify(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .show_dial = row.is_active();
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
        ));

        // Ruler width: the UI exposes the full width; storage uses the half-width.
        imp.ruler_width_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .body_half_width = row.value() * 0.5;
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
        ));

        imp.ruler_tick_spacing_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .tick_spacing = row.value();
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
        ));

        imp.ruler_body_opacity_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .body_opacity = row.value();
                if let Some(canvas) = appwindow.active_tab_canvas() {
                    canvas.queue_draw();
                }
            }
        ));

        imp.ruler_scroll_step_row.get().connect_changed(clone!(
            #[weak]
            appwindow,
            move |row| {
                appwindow
                    .engine_config()
                    .write()
                    .pens_config
                    .brush_config
                    .ruler_config
                    .scroll_rotation_step_deg = row.value();
            }
        ));
    }

    fn set_scribble_recognition_options(&self, options: ScribbleRecognitionOptions) {
        let imp = self.imp();
        imp.scribbleerase_min_turns_row
            .set_value(options.min_reversals as f64);
        imp.scribbleerase_min_turn_rate_row
            .set_value(options.min_turn_rate);
        imp.scribbleerase_pass_height_row
            .set_value(options.reversal_prominence_ratio * 100.0);
        imp.scribbleerase_pass_length_row
            .set_value(options.reversal_min_prominence_surface);
        imp.scribbleerase_min_size_row
            .set_value(options.min_extent_surface);
        imp.scribbleerase_min_length_row
            .set_value(options.min_length_diag_ratio);
    }

    pub(crate) fn refresh_ui(&self, appwindow: &RnAppWindow) {
        let imp = self.imp();
        let brush_config = appwindow
            .engine_config()
            .read()
            .pens_config
            .brush_config
            .clone();

        self.set_solidstyle_pressure_curve(brush_config.solid_options.pressure_curve);
        imp.texturedstyle_density_row
            .set_value(brush_config.textured_options.density);
        self.set_texturedstyle_distribution_variant(brush_config.textured_options.distribution);

        self.set_brush_style(brush_config.style);
        self.set_buildertype(brush_config.builder_type);
        imp.shaperecognition_row
            .set_active(brush_config.shape_recognition_enabled);
        imp.shaperecognition_delay_row
            .set_sensitive(brush_config.shape_recognition_enabled);
        imp.shaperecognition_delay_row
            .set_value(brush_config.shape_recognition_delay.as_secs_f64());
        imp.scribbleerase_row
            .set_enable_expansion(brush_config.scribble_erase_enabled);
        self.set_scribble_recognition_options(brush_config.scribble_recognition_options);

        match brush_config.style {
            BrushStyle::Marker => {
                imp.stroke_width_picker
                    .set_stroke_width(brush_config.marker_options.stroke_width);
            }
            BrushStyle::Solid => {
                imp.stroke_width_picker
                    .set_stroke_width(brush_config.solid_options.stroke_width);
            }
            BrushStyle::Textured => {
                imp.stroke_width_picker
                    .set_stroke_width(brush_config.textured_options.stroke_width);
            }
        }

        imp.ruler_toggle
            .set_active(brush_config.ruler_config.visible);
        imp.ruler_snap_distance_row
            .set_value(brush_config.ruler_config.snap_distance);
        imp.ruler_angle_snap_row
            .set_active(brush_config.ruler_config.angle_snap_enabled);
        imp.ruler_show_dial_row
            .set_active(brush_config.ruler_config.show_dial);
        imp.ruler_width_row
            .set_value(brush_config.ruler_config.body_half_width * 2.0);
        imp.ruler_tick_spacing_row
            .set_value(brush_config.ruler_config.tick_spacing);
        imp.ruler_body_opacity_row
            .set_value(brush_config.ruler_config.body_opacity);
        imp.ruler_scroll_step_row
            .set_value(brush_config.ruler_config.scroll_rotation_step_deg);
    }
}
