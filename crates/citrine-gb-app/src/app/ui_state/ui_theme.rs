use citrine_gb::debug::code::{InstrClass, OperandKind};
use egui::Color32;
use std::fmt::Display;
use strum_macros::EnumIter;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum UiTheme {
    #[default]
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
pub const BRAND_MARK: egui::Color32 = egui::Color32::from_rgb(0x8C, 0x5F, 0x00);
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(0xC6, 0x28, 0x28);
pub const DANGER_DARK: egui::Color32 = egui::Color32::from_rgb(0xD3, 0x2F, 0x2F);
pub const ON_DANGER: egui::Color32 = egui::Color32::WHITE;

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

pub fn brand_mark(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        BRAND_DARK
    } else {
        BRAND_MARK
    }
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
    let stroke = egui::Stroke::new(
        style.visuals.selection.stroke.width,
        focus_ring(&style.visuals),
    );
    ctx.layer_painter(response.layer_id)
        .with_clip_rect(response.interact_rect.expand(active.expansion))
        .rect_stroke(
            response.rect.expand(active.expansion),
            active.corner_radius,
            stroke,
            egui::StrokeKind::Inside,
        );
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
        ctx.style_mut(|style| branded(&mut style.visuals));
    }

    fn apply_base(&self, ctx: &egui::Context) {
        self.set_base(ctx);
        ctx.style_mut(|style| readable_weak(&mut style.visuals));
    }

    fn set_base(&self, ctx: &egui::Context) {
        match self {
            Self::Dark => ctx.set_visuals(dark()),
            Self::Light => ctx.set_visuals(egui::Visuals::light()),
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
            Self::Dark => CodeColours(CODE_DARK),
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
    Color32::from_rgb(0, 150, 0),
    Color32::from_rgb(0, 0, 200),
    Color32::from_rgb(200, 0, 0),
    Color32::from_rgb(120, 0, 150),
    Color32::from_rgb(0, 120, 120),
    Color32::from_rgb(160, 80, 0),
    Color32::from_rgb(180, 0, 120),
    Color32::from_rgb(100, 100, 100),
    Color32::RED,
    Color32::from_rgb(180, 100, 30),
    Color32::from_rgb(130, 130, 0),
    Color32::from_rgb(0, 120, 180),
    Color32::from_rgb(30, 130, 30),
    Color32::from_rgb(140, 30, 160),
    Color32::from_rgb(0, 140, 120),
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

fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

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

fn text_grounds(visuals: &egui::Visuals) -> [Color32; 3] {
    [visuals.panel_fill, visuals.window_fill, stripe(visuals)]
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
mod tests;
