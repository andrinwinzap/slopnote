// Imports
use crate::pens::shaper::CoordSystemSettings;
use crate::strokes::AxisTicksStyle;
use rand::{RngExt, SeedableRng};
use rnote_compose::Constraints;
use rnote_compose::Style;
use rnote_compose::builders::ShapeBuilderType;
use rnote_compose::constraints::ConstraintRatio;
use rnote_compose::style::rough::RoughOptions;
use rnote_compose::style::smooth::SmoothOptions;
use serde::{Deserialize, Serialize};

#[derive(
    Copy, Clone, Debug, Serialize, Deserialize, num_derive::FromPrimitive, num_derive::ToPrimitive,
)]
#[serde(rename = "shaper_style")]
pub enum ShaperStyle {
    #[serde(rename = "smooth")]
    Smooth = 0,
    #[serde(rename = "rough")]
    Rough,
}

impl Default for ShaperStyle {
    fn default() -> Self {
        Self::Smooth
    }
}

impl TryFrom<u32> for ShaperStyle {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        num_traits::FromPrimitive::from_u32(value).ok_or_else(|| {
            anyhow::anyhow!("ShaperStyle try_from::<u32>() for value {} failed", value)
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename = "shaper_config")]
pub struct ShaperConfig {
    #[serde(rename = "builder_type")]
    pub builder_type: ShapeBuilderType,
    #[serde(rename = "style")]
    pub style: ShaperStyle,
    #[serde(rename = "smooth_options")]
    pub smooth_options: SmoothOptions,
    #[serde(rename = "rough_options")]
    pub rough_options: RoughOptions,
    #[serde(rename = "highlight_mode")]
    pub highlight_mode: bool,
    #[serde(
        rename = "highlight_opacity",
        with = "rnote_compose::serialize::f64_dp3"
    )]
    pub highlight_opacity: f64,
    #[serde(rename = "constraints")]
    pub constraints: Constraints,
    /// Whether the pen pressure changes the width of drawn shapes.
    #[serde(rename = "pen_pressure")]
    pub pen_pressure: bool,
    /// Whether coordinate systems get unit markings on their axes.
    #[serde(rename = "axis_ticks_enabled")]
    pub axis_ticks_enabled: bool,
    /// The distance between two unit markings that the default tick count is derived from.
    #[serde(
        rename = "axis_tick_spacing",
        with = "rnote_compose::serialize::f64_dp3"
    )]
    pub axis_tick_spacing: f64,
    /// How far a unit marking reaches to each side of the axis.
    #[serde(rename = "axis_tick_size", with = "rnote_compose::serialize::f64_dp3")]
    pub axis_tick_size: f64,
    /// Whether the unit markings are numbered.
    #[serde(rename = "axis_tick_numbering")]
    pub axis_tick_numbering: bool,
    /// Whether all axes get the same distance between unit markings, instead of the same number
    /// of markings, when they differ in length.
    #[serde(rename = "axis_tick_constant_spacing")]
    pub axis_tick_constant_spacing: bool,
    /// Only every this many unit markings is numbered.
    #[serde(rename = "axis_tick_label_every")]
    pub axis_tick_label_every: u32,
    /// The font size of the numbers of the unit markings.
    #[serde(
        rename = "axis_tick_font_size",
        with = "rnote_compose::serialize::f64_dp3"
    )]
    pub axis_tick_font_size: f64,
    /// Whether the positive half-axes of coordinate systems end in an arrow head.
    #[serde(rename = "axis_arrows")]
    pub axis_arrows: bool,
    /// The length of the sides of the arrow heads of coordinate systems.
    #[serde(rename = "axis_arrow_size", with = "rnote_compose::serialize::f64_dp3")]
    pub axis_arrow_size: f64,
    /// Whether the positive half-axes of coordinate systems are named at their tip.
    #[serde(rename = "axis_labels")]
    pub axis_labels: bool,
    /// The names of the axes: the x, y and z axis. In 3D coordinate systems z is up, and which of
    /// x and y is the depth axis depends on its direction and the handedness.
    #[serde(rename = "axis_label_names")]
    pub axis_label_names: [String; 3],
    /// Whether the depth axis of 3D coordinate systems grows into the page, towards the upper
    /// right, instead of out of it, towards the lower left.
    #[serde(rename = "axis_third_into_page")]
    pub axis_third_into_page: bool,
    /// Whether 3D coordinate systems are right-handed, rather than left-handed.
    #[serde(rename = "axis_right_handed")]
    pub axis_right_handed: bool,
}

impl Default for ShaperConfig {
    fn default() -> Self {
        let mut constraints = Constraints::default();
        constraints.ratios.insert(ConstraintRatio::OneToOne);
        constraints.ratios.insert(ConstraintRatio::Horizontal);
        constraints.ratios.insert(ConstraintRatio::Vertical);

        Self {
            builder_type: ShapeBuilderType::default(),
            style: ShaperStyle::default(),
            smooth_options: SmoothOptions::default(),
            rough_options: RoughOptions::default(),
            highlight_mode: false,
            highlight_opacity: 0.45,
            constraints,
            pen_pressure: true,
            axis_ticks_enabled: true,
            axis_tick_spacing: Self::AXIS_TICK_SPACING_DEFAULT,
            axis_tick_size: Self::AXIS_TICK_SIZE_DEFAULT,
            axis_tick_numbering: true,
            axis_tick_constant_spacing: false,
            axis_tick_label_every: 1,
            axis_tick_font_size: Self::AXIS_TICK_FONT_SIZE_DEFAULT,
            axis_arrows: true,
            axis_arrow_size: Self::AXIS_ARROW_SIZE_DEFAULT,
            axis_labels: true,
            axis_label_names: ["x".to_string(), "y".to_string(), "z".to_string()],
            axis_third_into_page: false,
            axis_right_handed: true,
        }
    }
}

impl ShaperConfig {
    pub const STROKE_WIDTH_MIN: f64 = 0.1;
    pub const STROKE_WIDTH_MAX: f64 = 500.0;
    pub const AXIS_TICK_SPACING_MIN: f64 = 5.0;
    pub const AXIS_TICK_SPACING_MAX: f64 = 1000.0;
    pub const AXIS_TICK_SPACING_DEFAULT: f64 = 50.0;
    pub const AXIS_TICK_SIZE_MIN: f64 = 1.0;
    pub const AXIS_TICK_SIZE_MAX: f64 = 50.0;
    pub const AXIS_TICK_SIZE_DEFAULT: f64 = 6.0;
    pub const AXIS_TICK_FONT_SIZE_DEFAULT: f64 = 12.0;
    pub const AXIS_ARROW_SIZE_DEFAULT: f64 = 5.0;

    /// The look of the unit markings of new coordinate systems.
    pub fn axis_ticks_style(&self) -> AxisTicksStyle {
        AxisTicksStyle {
            tick_size: self.axis_tick_size,
            numbering: self.axis_tick_numbering,
            label_every: self.axis_tick_label_every,
            font_size: self.axis_tick_font_size,
        }
    }

    /// Make the given look of unit markings the one new coordinate systems get.
    pub fn set_axis_ticks_style(&mut self, style: AxisTicksStyle) {
        self.axis_tick_size = style.tick_size;
        self.axis_tick_numbering = style.numbering;
        self.axis_tick_label_every = style.label_every;
        self.axis_tick_font_size = style.font_size;
    }

    /// What a new coordinate system gets, with the given number of ticks on each half-axis.
    pub fn coord_system_settings(&self, count: u32) -> CoordSystemSettings {
        CoordSystemSettings {
            ticks: self.axis_ticks_enabled,
            count,
            constant_spacing: self.axis_tick_constant_spacing,
            tick_style: self.axis_ticks_style(),
            arrows: self.axis_arrows,
            arrow_size: self.axis_arrow_size,
            labels: self.axis_labels,
            label_names: self.axis_label_names.clone(),
            third_into_page: self.axis_third_into_page,
            right_handed: self.axis_right_handed,
        }
    }

    /// Make the given settings, but for the number of ticks, the ones new coordinate systems get.
    pub fn set_coord_system_settings(&mut self, settings: &CoordSystemSettings) {
        self.axis_ticks_enabled = settings.ticks;
        self.axis_tick_constant_spacing = settings.constant_spacing;
        self.set_axis_ticks_style(settings.tick_style);
        self.axis_arrows = settings.arrows;
        self.axis_arrow_size = settings.arrow_size;
        self.axis_labels = settings.labels;
        self.axis_label_names = settings.label_names.clone();
        self.axis_third_into_page = settings.third_into_page;
        self.axis_right_handed = settings.right_handed;
    }

    /// The factor the stroke width is scaled with for the given mean pen pressure.
    ///
    /// Uses the pressure curve of the smooth options and is normalized so that input without
    /// pressure information (mouse, touch) leaves the width unchanged.
    pub(crate) fn pressure_width_factor(&self, pressure: f64) -> f64 {
        if !self.pen_pressure {
            return 1.0;
        }
        self.smooth_options
            .pressure_curve
            .relative_width_factor(pressure)
    }

    /// The style for the current options with the width scaled by the pressure.
    pub(crate) fn gen_style_with_pressure(&self, pressure: f64) -> Style {
        let mut style = self.gen_style_for_current_options();
        style.set_stroke_width(style.stroke_width() * self.pressure_width_factor(pressure));
        style
    }

    /// A new seed for new shapes
    pub(crate) fn new_style_seeds(&mut self) {
        let seed = Some(rand_pcg::Pcg64::from_rng(&mut rand::rng()).random());
        self.rough_options.seed = seed;
    }

    pub(crate) fn gen_style_for_current_options(&self) -> Style {
        match &self.style {
            ShaperStyle::Smooth => {
                let mut options = self.smooth_options.clone();
                if self.highlight_mode {
                    if let Some(ref mut color) = options.stroke_color {
                        color.a = self.highlight_opacity;
                    }
                    if let Some(ref mut color) = options.fill_color
                        && color.a > 0.0
                    {
                        color.a = self.highlight_opacity;
                    }
                }
                Style::Smooth(options)
            }
            ShaperStyle::Rough => {
                let mut options = self.rough_options.clone();
                if self.highlight_mode {
                    if let Some(ref mut color) = options.stroke_color {
                        color.a = self.highlight_opacity;
                    }
                    if let Some(ref mut color) = options.fill_color
                        && color.a > 0.0
                    {
                        color.a = self.highlight_opacity;
                    }
                }
                Style::Rough(options)
            }
        }
    }
}
