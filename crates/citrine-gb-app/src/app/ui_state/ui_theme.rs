use citrine_gb::debug::code::{InstrClass, OperandKind};
use egui::Color32;
use std::fmt::Display;
use strum_macros::EnumIter;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum UiTheme {
    #[default]
    System,
    Dark,
    Light,
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
}

pub const BRAND: egui::Color32 = egui::Color32::from_rgb(0xF2, 0xCE, 0x44);
pub const BRAND_DARK: egui::Color32 = egui::Color32::from_rgb(0xF5, 0xAD, 0x33);
pub const ON_BRAND: egui::Color32 = egui::Color32::from_rgb(0x2B, 0x22, 0x07);
pub const BRAND_LOCKED: egui::Color32 = egui::Color32::from_rgb(0xDA, 0x95, 0x0B);
pub const BRAND_MARK: egui::Color32 = egui::Color32::from_rgb(0x8C, 0x5F, 0x00);
pub const BRAND_EDGE: egui::Color32 = egui::Color32::from_rgb(0xB0, 0x78, 0x00);
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(0xC6, 0x28, 0x28);
pub const DANGER_DARK: egui::Color32 = egui::Color32::from_rgb(0xD3, 0x2F, 0x2F);
pub const ON_DANGER: egui::Color32 = egui::Color32::WHITE;

#[cfg(feature = "nds")]
pub const METER: egui::Color32 = egui::Color32::from_rgb(0x2E, 0x8B, 0x57);
#[cfg(feature = "nds")]
pub const METER_DARK: egui::Color32 = egui::Color32::from_rgb(0x4C, 0xC4, 0x6A);

#[cfg(feature = "nds")]
pub fn meter(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode { METER_DARK } else { METER }
}

pub fn brand(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode { BRAND_DARK } else { BRAND }
}

pub fn danger(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        DANGER_DARK
    } else {
        DANGER
    }
}

pub fn danger_text(visuals: &egui::Visuals) -> egui::Color32 {
    visuals.error_fg_color
}

pub fn brand_mark(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        BRAND_DARK
    } else {
        BRAND_MARK
    }
}

pub fn own_value(visuals: &egui::Visuals) -> egui::Color32 {
    brand_mark(visuals)
}

pub fn ink_on(fill: egui::Color32) -> egui::Color32 {
    let [r, g, b, _] = fill.to_normalized_gamma_f32();
    if 0.299 * r + 0.587 * g + 0.114 * b > 0.55 {
        ON_BRAND
    } else {
        egui::Color32::WHITE
    }
}

pub fn focus_ring(visuals: &egui::Visuals) -> egui::Color32 {
    brand_mark(visuals)
}

pub fn paint_focus_ring(ctx: &egui::Context) {
    if !ctx.wants_keyboard_input() {
        return;
    }
    let Some(response) = ctx
        .memory(|m| m.focused())
        .and_then(|id| ctx.read_response(id))
    else {
        return;
    };
    let style = ctx.style();
    let active = &style.visuals.widgets.active;
    let stroke = focus_ring_stroke(ctx);
    ctx.layer_painter(response.layer_id)
        .with_clip_rect(response.interact_rect.expand(active.expansion))
        .rect_stroke(
            response.rect.expand(active.expansion),
            active.corner_radius,
            stroke,
            egui::StrokeKind::Inside,
        );
}

pub fn focus_ring_stroke(ctx: &egui::Context) -> egui::Stroke {
    let style = ctx.style();
    egui::Stroke::new(
        whole_pixels(style.visuals.selection.stroke.width, ctx.pixels_per_point()),
        focus_ring(&style.visuals),
    )
}

fn whole_pixels(width: f32, pixels_per_point: f32) -> f32 {
    (width * pixels_per_point).ceil().max(1.0) / pixels_per_point
}

fn branded(visuals: &mut egui::Visuals) {
    visuals.selection.bg_fill = brand(visuals);
    visuals.selection.stroke.color = ON_BRAND;
    visuals.slider_trailing_fill = true;
    let field = if visuals.dark_mode {
        visuals.panel_fill.lerp_to_gamma(egui::Color32::WHITE, 0.13)
    } else {
        visuals
            .panel_fill
            .lerp_to_gamma(egui::Color32::from_gray(226), 0.7)
    };
    visuals.text_edit_bg_color = Some(field);
}

impl UiTheme {
    pub fn apply(&self, ctx: &egui::Context) {
        self.apply_base(ctx);
        ctx.all_styles_mut(|style| branded(&mut style.visuals));
    }

    fn apply_base(&self, ctx: &egui::Context) {
        self.set_base(ctx);
        ctx.all_styles_mut(|style| readable_weak(&mut style.visuals));
    }

    pub fn resolve(self, ctx: &egui::Context) -> Self {
        match (self, ctx.theme()) {
            (Self::System, egui::Theme::Light) => Self::Light,
            (Self::System, egui::Theme::Dark) => Self::Dark,
            (theme, _) => theme,
        }
    }

    fn is_dark(self) -> bool {
        !matches!(self, Self::Light | Self::CatppuccinLatte)
    }

    fn set_base(&self, ctx: &egui::Context) {
        if *self == Self::System {
            ctx.set_theme(egui::ThemePreference::System);
            ctx.set_visuals_of(egui::Theme::Dark, dark());
            ctx.set_visuals_of(egui::Theme::Light, light());
            return;
        }
        ctx.set_theme(if self.is_dark() {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        match self {
            Self::System => {}
            Self::Dark => ctx.set_visuals(dark()),
            Self::Light => ctx.set_visuals(light()),
            Self::CatppuccinLatte => catppuccin_egui::set_theme(ctx, catppuccin_egui::LATTE),
            Self::CatppuccinFrappe => catppuccin_egui::set_theme(ctx, catppuccin_egui::FRAPPE),
            Self::CatppuccinMacchiato => {
                catppuccin_egui::set_theme(ctx, catppuccin_egui::MACCHIATO)
            }
            Self::CatppuccinMocha => catppuccin_egui::set_theme(ctx, catppuccin_egui::MOCHA),
        }
    }

    pub fn code_colours(&self) -> CodeColours {
        match self {
            Self::System | Self::Dark => CodeColours(CODE_DARK),
            Self::Light => CodeColours(CODE_LIGHT),
            Self::CatppuccinLatte => catppuccin_code(catppuccin_egui::LATTE),
            Self::CatppuccinFrappe => catppuccin_code(catppuccin_egui::FRAPPE),
            Self::CatppuccinMacchiato => catppuccin_code(catppuccin_egui::MACCHIATO),
            Self::CatppuccinMocha => catppuccin_code(catppuccin_egui::MOCHA),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeColours([Color32; 15]);

impl CodeColours {
    pub fn class(&self, class: InstrClass) -> Color32 {
        self.0[class as usize]
    }

    pub fn operand(&self, kind: OperandKind) -> Color32 {
        self.0[InstrClass::ALL.len() + kind as usize]
    }
}

const CODE_DARK: [Color32; 15] = [
    Color32::from_rgb(150, 255, 150),
    Color32::from_rgb(150, 200, 255),
    Color32::from_rgb(255, 150, 150),
    Color32::from_rgb(220, 150, 255),
    Color32::from_rgb(120, 255, 255),
    Color32::from_rgb(255, 200, 120),
    Color32::from_rgb(255, 150, 200),
    Color32::from_rgb(150, 150, 150),
    Color32::RED,
    Color32::from_rgb(255, 180, 140),
    Color32::from_rgb(220, 220, 160),
    Color32::from_rgb(160, 220, 255),
    Color32::from_rgb(180, 255, 180),
    Color32::from_rgb(240, 180, 255),
    Color32::from_rgb(160, 255, 230),
];

const CODE_LIGHT: [Color32; 15] = [
    Color32::from_rgb(0, 105, 0),
    Color32::from_rgb(0, 0, 200),
    Color32::from_rgb(187, 0, 0),
    Color32::from_rgb(120, 0, 150),
    Color32::from_rgb(0, 103, 104),
    Color32::from_rgb(146, 68, 0),
    Color32::from_rgb(180, 0, 120),
    Color32::from_rgb(100, 100, 100),
    Color32::from_rgb(214, 0, 0),
    Color32::from_rgb(163, 85, 1),
    Color32::from_rgb(110, 109, 0),
    Color32::from_rgb(0, 111, 170),
    Color32::from_rgb(19, 123, 21),
    Color32::from_rgb(140, 30, 160),
    Color32::from_rgb(0, 120, 101),
];

fn catppuccin_code(theme: catppuccin_egui::Theme) -> CodeColours {
    CodeColours([
        theme.green,
        theme.blue,
        theme.maroon,
        theme.mauve,
        theme.teal,
        theme.peach,
        theme.pink,
        theme.overlay2,
        theme.red,
        theme.flamingo,
        theme.yellow,
        theme.sapphire,
        theme.green,
        theme.lavender,
        theme.sky,
    ])
}

const DARK_ERROR: egui::Color32 = egui::Color32::from_rgb(255, 150, 150);
const LIGHT_ERROR: egui::Color32 = egui::Color32::from_rgb(190, 20, 20);

fn light() -> egui::Visuals {
    egui::Visuals {
        error_fg_color: LIGHT_ERROR,
        ..egui::Visuals::light()
    }
}

fn dark() -> egui::Visuals {
    let mut visuals = egui::Visuals {
        error_fg_color: DARK_ERROR,
        ..egui::Visuals::dark()
    };
    visuals.widgets.noninteractive.fg_stroke.color = DARK_TEXT;
    visuals
}

const DARK_TEXT: Color32 = Color32::from_gray(170);
const WEAK_CONTRAST: f32 = 4.5;
const EDGE_CONTRAST: f32 = 3.0;
pub(crate) const TRACK_CONTRAST: f32 = 1.5;

fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

pub fn box_edge(visuals: &egui::Visuals, fill: Color32) -> Color32 {
    let ground = visuals.panel_fill;
    let ink = visuals.text_color();
    (0..=100)
        .map(|step| ground.lerp_to_gamma(ink, step as f32 / 100.0))
        .find(|edge| {
            contrast(*edge, ground) >= EDGE_CONTRAST && contrast(*edge, fill) >= EDGE_CONTRAST
        })
        .unwrap_or(ink)
}

pub fn track(visuals: &egui::Visuals) -> Color32 {
    let ground = visuals.panel_fill;
    let ink = visuals.text_color();
    (0..=100)
        .map(|step| ground.lerp_to_gamma(ink, step as f32 / 100.0))
        .find(|track| contrast(*track, ground) >= TRACK_CONTRAST)
        .unwrap_or(ink)
}

pub(crate) fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

pub fn pc_fill(visuals: &egui::Visuals, colours: &CodeColours) -> Color32 {
    let ground = visuals.panel_fill;
    let inks: Vec<Color32> = colours
        .0
        .iter()
        .copied()
        .chain([visuals.text_color(), visuals.weak_text_color()])
        .collect();
    (0..=PC_TINT_STEPS)
        .rev()
        .map(|step| ground.lerp_to_gamma(brand(visuals), step as f32 / 100.0))
        .find(|fill| {
            inks.iter().all(|ink| {
                contrast(*ink, *fill) >= (PC_KEEP * contrast(*ink, ground)).min(WEAK_CONTRAST)
            })
        })
        .unwrap_or(ground)
}

pub fn weak_on(visuals: &egui::Visuals, fill: Color32) -> Color32 {
    let weak = visuals.weak_text_color();
    if fill.a() < u8::MAX {
        return weak;
    }
    (0..=50)
        .map(|step| weak.lerp_to_gamma(visuals.text_color(), step as f32 / 50.0))
        .find(|ink| contrast(*ink, fill) >= WEAK_CONTRAST)
        .unwrap_or(visuals.text_color())
}

pub fn hover_fill(visuals: &egui::Visuals, colours: &CodeColours) -> Color32 {
    let ground = visuals.panel_fill;
    let hover = visuals.widgets.hovered.weak_bg_fill;
    let inks: Vec<Color32> = colours
        .0
        .iter()
        .copied()
        .chain([visuals.text_color()])
        .collect();
    (0..=HOVER_STEPS)
        .map(|step| hover.lerp_to_gamma(ground, f32::from(step) / f32::from(HOVER_STEPS)))
        .find(|fill| {
            inks.iter().all(|ink| {
                contrast(*ink, *fill) >= (PC_KEEP * contrast(*ink, ground)).min(WEAK_CONTRAST)
            })
        })
        .unwrap_or(ground)
}

const HOVER_STEPS: u8 = 20;
const PC_TINT_STEPS: u8 = 25;
const PC_KEEP: f32 = 0.9;

fn stripe(visuals: &egui::Visuals) -> Color32 {
    let faint = visuals.faint_bg_color;
    if faint.a() == 0 {
        let [r, g, b, _] = visuals.panel_fill.to_array();
        Color32::from_rgb(
            r.saturating_add(faint.r()),
            g.saturating_add(faint.g()),
            b.saturating_add(faint.b()),
        )
    } else {
        faint
    }
}

fn text_grounds(visuals: &egui::Visuals) -> [Color32; 4] {
    [
        visuals.panel_fill,
        visuals.window_fill,
        stripe(visuals),
        crate::app::mobile::kit::card_fill(visuals),
    ]
}

fn readable_weak(visuals: &mut egui::Visuals) {
    visuals.weak_text_color = None;
    let text = visuals.text_color();
    let grounds = text_grounds(visuals);
    let weak = (0..=30)
        .rev()
        .map(|step| text.lerp_to_gamma(visuals.panel_fill, step as f32 / 50.0))
        .find(|weak| grounds.iter().all(|g| contrast(*weak, *g) >= WEAK_CONTRAST))
        .unwrap_or(text);
    visuals.weak_text_color = Some(weak);
}

impl Display for UiTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UiTheme::System => write!(f, "Same as system"),
            UiTheme::Dark => write!(f, "Dark"),
            UiTheme::Light => write!(f, "Light"),
            UiTheme::CatppuccinLatte => write!(f, "Catppuccin Latte"),
            UiTheme::CatppuccinFrappe => write!(f, "Catppuccin Frappé"),
            UiTheme::CatppuccinMacchiato => write!(f, "Catppuccin Macchiato"),
            UiTheme::CatppuccinMocha => write!(f, "Catppuccin Mocha"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BRAND, BRAND_DARK, BRAND_LOCKED, BRAND_MARK, DARK_ERROR, LIGHT_ERROR, ON_BRAND, ON_DANGER,
        UiTheme, brand, contrast, danger, dark, focus_ring, hover_fill, light, pc_fill,
        text_grounds, weak_on, whole_pixels,
    };
    use citrine_gb::debug::code::{InstrClass, OperandKind};
    use egui::Color32;
    use strum::IntoEnumIterator;

    #[test]
    fn red_text_reads_on_dark_buttons() {
        let visuals = dark();
        assert_eq!(visuals.error_fg_color, DARK_ERROR);
        for fill in [
            visuals.panel_fill,
            visuals.widgets.inactive.weak_bg_fill,
            visuals.widgets.hovered.weak_bg_fill,
        ] {
            assert!(contrast(DARK_ERROR, fill) >= 4.5, "{fill:?}");
        }
    }

    #[test]
    fn red_text_reads_on_light_buttons_and_cards() {
        let visuals = light();
        assert_eq!(visuals.error_fg_color, LIGHT_ERROR);
        for fill in [
            visuals.panel_fill,
            visuals.widgets.inactive.weak_bg_fill,
            visuals.widgets.hovered.weak_bg_fill,
            crate::app::mobile::kit::card_fill(&visuals),
        ] {
            assert!(contrast(LIGHT_ERROR, fill) >= 4.5, "{fill:?}");
        }
    }

    #[test]
    fn the_dark_red_keeps_the_red_hue() {
        assert_eq!(DARK_ERROR.r(), 255);
        assert_eq!(DARK_ERROR.g(), DARK_ERROR.b());
    }

    #[test]
    fn every_theme_tells_code_classes_and_operands_apart() {
        for theme in UiTheme::iter() {
            let colours = theme.code_colours();
            let classes: Vec<_> = InstrClass::ALL.map(|c| colours.class(c)).into();
            let operands: Vec<_> = OperandKind::ALL.map(|k| colours.operand(k)).into();
            for list in [classes, operands] {
                for (i, a) in list.iter().enumerate() {
                    assert!(!list[i + 1..].contains(a), "{theme} repeats {a:?}");
                }
            }
        }
    }

    #[test]
    fn weak_text_reads_in_every_theme() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let weak = visuals.weak_text_color();
            for ground in text_grounds(&visuals) {
                let ratio = contrast(weak, ground);
                assert!(ratio >= 4.5, "{theme} weak {weak:?} on {ground:?}: {ratio}");
            }
            assert!(
                contrast(visuals.text_color(), visuals.panel_fill)
                    > contrast(weak, visuals.panel_fill) + 0.5,
                "{theme} weak text looks like body text"
            );
        }
    }

    fn themed(theme: UiTheme) -> egui::Visuals {
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        ctx.style().visuals.clone()
    }

    #[test]
    fn every_theme_selects_slides_and_toggles_in_the_brand_colour() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let wanted = if visuals.dark_mode { BRAND_DARK } else { BRAND };
            assert_eq!(visuals.selection.bg_fill, wanted, "{theme} selection");
            assert_eq!(brand(&visuals), wanted, "{theme} buttons");
            assert!(visuals.slider_trailing_fill, "{theme} slider fill");
        }
    }

    #[test]
    fn selected_text_and_the_focus_ring_read_in_every_theme() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let ink = visuals.selection.stroke.color;
            assert_eq!(ink, ON_BRAND, "{theme}");
            let ratio = contrast(ink, visuals.selection.bg_fill);
            assert!(ratio >= 4.5, "{theme} selected text: {ratio}");
            let field = visuals.text_edit_bg_color();
            let wanted = if visuals.dark_mode {
                BRAND_DARK
            } else {
                BRAND_MARK
            };
            assert_eq!(focus_ring(&visuals), wanted, "{theme} focus ring colour");
            let ring = contrast(focus_ring(&visuals), field);
            assert!(ring >= 3.0, "{theme} focus ring: {ring}");
        }
    }

    #[test]
    fn phone_row_values_read_on_cards() {
        for (theme, least) in [(super::UiTheme::Dark, 4.5), (super::UiTheme::Light, 3.0)] {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let visuals = ctx.style().visuals.clone();
            let card = crate::app::mobile::kit::card_fill(&visuals);
            let value = crate::app::mobile::kit::value_color(&visuals);
            assert!(
                contrast(value, card) >= least,
                "{theme}: {value:?} on {card:?}"
            );
        }
    }

    #[test]
    fn destructive_buttons_read_in_every_theme() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let ratio = contrast(ON_DANGER, danger(&visuals));
            assert!(ratio >= 4.5, "{theme} danger label: {ratio}");
        }
    }

    #[test]
    fn the_pc_row_keeps_every_code_colour_readable_and_still_shows() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let colours = theme.code_colours();
            let fill = pc_fill(&visuals, &colours);
            assert_ne!(fill, visuals.panel_fill, "{theme} PC row has no tint");
            let inks = InstrClass::ALL
                .map(|c| colours.class(c))
                .into_iter()
                .chain(OperandKind::ALL.map(|k| colours.operand(k)))
                .chain([visuals.text_color(), visuals.weak_text_color()]);
            for ink in inks {
                let plain = contrast(ink, visuals.panel_fill);
                let on_pc = contrast(ink, fill);
                assert!(
                    on_pc >= (0.9 * plain).min(4.5),
                    "{theme} {ink:?} on the PC row {fill:?}: {on_pc} from {plain}"
                );
            }
            assert!(
                contrast(super::brand_mark(&visuals), visuals.panel_fill) >= 3.0,
                "{theme} PC outline fades"
            );
        }
    }

    #[test]
    fn light_code_reads_on_plain_pc_and_hovered_rows() {
        let theme = UiTheme::Light;
        let visuals = themed(theme);
        let colours = theme.code_colours();
        let code: Vec<_> = InstrClass::ALL
            .map(|c| colours.class(c))
            .into_iter()
            .chain(OperandKind::ALL.map(|k| colours.operand(k)))
            .chain([visuals.text_color()])
            .collect();
        let grounds = text_grounds(&visuals)
            .into_iter()
            .chain([pc_fill(&visuals, &colours), hover_fill(&visuals, &colours)]);
        for ground in grounds {
            let inks = code.iter().copied().chain([weak_on(&visuals, ground)]);
            for ink in inks {
                let ratio = contrast(ink, ground);
                assert!(ratio >= 4.5, "{ink:?} on {ground:?}: {ratio}");
            }
        }
    }

    #[test]
    fn weak_text_on_a_plain_row_is_the_theme_weak_text() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let weak = visuals.weak_text_color();
            assert_eq!(weak_on(&visuals, visuals.panel_fill), weak, "{theme}");
            assert_eq!(weak_on(&visuals, Color32::TRANSPARENT), weak, "{theme}");
        }
    }

    #[test]
    fn the_hovered_row_keeps_code_readable_and_still_shows() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let colours = theme.code_colours();
            let fill = hover_fill(&visuals, &colours);
            assert!(
                contrast(fill, visuals.panel_fill) >= 1.05,
                "{theme} hovered row {fill:?} fades into the panel"
            );
            let inks = InstrClass::ALL
                .map(|c| colours.class(c))
                .into_iter()
                .chain(OperandKind::ALL.map(|k| colours.operand(k)))
                .chain([visuals.text_color(), weak_on(&visuals, fill)]);
            for ink in inks {
                let plain = contrast(ink, visuals.panel_fill);
                let hovered = contrast(ink, fill);
                assert!(
                    hovered >= (0.9 * plain).min(4.5),
                    "{theme} {ink:?} on the hovered row {fill:?}: {hovered} from {plain}"
                );
            }
        }
    }

    #[test]
    fn same_as_system_follows_the_system_theme_and_a_picked_theme_does_not() {
        let cases = [
            (UiTheme::System, egui::Theme::Light, false),
            (UiTheme::System, egui::Theme::Dark, true),
            (UiTheme::Dark, egui::Theme::Light, true),
            (UiTheme::Light, egui::Theme::Dark, false),
        ];
        for (theme, system, dark) in cases {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                system_theme: Some(system),
                ..Default::default()
            };
            let _ = ctx.run(input.clone(), |_| {});
            theme.apply(&ctx);
            let _ = ctx.run(input, |_| {});
            let visuals = ctx.style().visuals.clone();
            assert_eq!(visuals.dark_mode, dark, "{theme} on a {system:?} system");
            assert_eq!(visuals.selection.bg_fill, brand(&visuals), "{theme}");
            assert_eq!(theme.resolve(&ctx).is_dark(), dark, "{theme}");
            assert_ne!(theme.resolve(&ctx), UiTheme::System);
        }
    }

    #[test]
    fn the_focus_ring_fills_whole_pixels_at_every_scale() {
        for pixels_per_point in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let pixels = whole_pixels(1.0, pixels_per_point) * pixels_per_point;
            assert!(
                (pixels - pixels.round()).abs() < 1e-4 && pixels >= pixels_per_point - 1e-4,
                "{pixels_per_point}: {pixels} px"
            );
        }
    }

    fn oklch(color: Color32) -> (f32, f32, f32) {
        let linear = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let [r, g, b, _] = color.to_array().map(linear);
        let l = (0.412_221 * r + 0.536_333 * g + 0.051_446 * b).cbrt();
        let m = (0.211_903 * r + 0.680_7 * g + 0.107_397 * b).cbrt();
        let s = (0.088_302 * r + 0.281_719 * g + 0.629_979 * b).cbrt();
        let lightness = 0.210_454 * l + 0.793_618 * m - 0.004_072 * s;
        let a = 1.977_998 * l - 2.428_592 * m + 0.450_594 * s;
        let b = 0.025_904 * l + 0.782_772 * m - 0.808_676 * s;
        (lightness, a.hypot(b), b.atan2(a).to_degrees())
    }

    #[test]
    fn a_locked_switch_keeps_the_amber() {
        let (brand_l, brand_c, brand_h) = oklch(BRAND_DARK);
        let (l, c, h) = oklch(BRAND_LOCKED);
        assert!(l < brand_l - 0.05, "locked reads dimmer: {l} vs {brand_l}");
        assert!((h - brand_h).abs() < 2.0, "same hue: {h} vs {brand_h}");
        assert!(c > brand_c * 0.9, "no grey-brown: chroma {c} vs {brand_c}");
    }
}
