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

impl UiTheme {
    pub fn apply(&self, ctx: &egui::Context) {
        match self {
            Self::Dark => ctx.set_visuals(egui::Visuals::dark()),
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
