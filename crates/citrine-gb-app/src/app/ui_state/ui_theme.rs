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

pub fn brand(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode { BRAND_DARK } else { BRAND }
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

fn branded(visuals: &mut egui::Visuals) {
    visuals.selection.bg_fill = brand(visuals);
    visuals.selection.stroke.color = ON_BRAND;
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
    pub fn apply(&self, ctx: &egui::Context, phone: bool) {
        self.apply_base(ctx);
        if phone {
            ctx.style_mut(|style| branded(&mut style.visuals));
        }
    }

    fn apply_base(&self, ctx: &egui::Context) {
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
}

const DARK_ERROR: egui::Color32 = egui::Color32::from_rgb(255, 150, 150);

fn dark() -> egui::Visuals {
    egui::Visuals {
        error_fg_color: DARK_ERROR,
        ..egui::Visuals::dark()
    }
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
