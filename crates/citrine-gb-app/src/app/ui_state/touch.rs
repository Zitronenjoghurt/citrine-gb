use crate::app::ui_state::settings::ScalingMode;
use std::fmt::Display;
use strum_macros::EnumIter;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum LandscapeControls {
    #[default]
    Beside,
    Overlay,
}

impl Display for LandscapeControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Beside => "Beside the screen",
            Self::Overlay => "Over the screen",
        };
        f.write_str(name)
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum HiddenButtons {
    #[default]
    Auto,
    Show,
    Hide,
}

impl HiddenButtons {
    pub const NOTE: &str = "Whether the on-screen buttons show while the menus are hidden. Auto hides them when you press a controller button or a key, and shows them again when you tap beside the game.";
}

impl Display for HiddenButtons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Auto => "Auto",
            Self::Show => "Show",
            Self::Hide => "Hide",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TouchSettings {
    pub show_controls: bool,
    pub opacity: f32,
    pub scale: f32,
    pub haptics: bool,
    pub landscape: LandscapeControls,
    pub hidden_buttons: HiddenButtons,
    pub mirrored: bool,
    pub ui_scale: f32,
    pub scaling: ScalingMode,
    #[serde(default)]
    pub opacity_rechecked: bool,
}

impl Default for TouchSettings {
    fn default() -> Self {
        Self {
            show_controls: true,
            opacity: Self::DEFAULT_OPACITY,
            scale: 1.0,
            haptics: true,
            landscape: LandscapeControls::default(),
            hidden_buttons: HiddenButtons::default(),
            mirrored: false,
            ui_scale: 1.0,
            scaling: ScalingMode::Fit,
            opacity_rechecked: true,
        }
    }
}

impl TouchSettings {
    pub const DEFAULT_OPACITY: f32 = 0.5;
    const OLD_DEFAULT_OPACITY: f32 = 0.75;
    pub const MIN_OPACITY: f32 = 0.15;
    pub const MIN_SCALE: f32 = 0.6;
    pub const MAX_SCALE: f32 = 1.6;
    pub const MIN_UI_SCALE: f32 = 0.6;
    pub const MAX_UI_SCALE: f32 = 2.0;

    pub fn recheck_opacity(&mut self) {
        if !self.opacity_rechecked && (self.opacity - Self::OLD_DEFAULT_OPACITY).abs() < 0.001 {
            self.opacity = Self::DEFAULT_OPACITY;
        }
        self.opacity_rechecked = true;
    }

    pub fn clamped_opacity(&self) -> f32 {
        self.opacity.clamp(Self::MIN_OPACITY, 1.0)
    }

    pub fn clamped_scale(&self) -> f32 {
        self.scale.clamp(Self::MIN_SCALE, Self::MAX_SCALE)
    }

    pub fn clamped_ui_scale(&self) -> f32 {
        self.ui_scale.clamp(Self::MIN_UI_SCALE, Self::MAX_UI_SCALE)
    }
}

#[cfg(test)]
mod tests {
    use super::TouchSettings;

    #[test]
    fn the_old_default_opacity_moves_to_the_new_one_once_and_a_chosen_one_stays() {
        let mut old: TouchSettings = serde_json::from_str(r#"{"opacity":0.75}"#).unwrap();
        old.recheck_opacity();
        assert_eq!(old.opacity, TouchSettings::DEFAULT_OPACITY);
        old.opacity = 0.75;
        old.recheck_opacity();
        assert_eq!(old.opacity, 0.75);
        let mut chosen: TouchSettings = serde_json::from_str(r#"{"opacity":0.9}"#).unwrap();
        chosen.recheck_opacity();
        assert_eq!(chosen.opacity, 0.9);
    }
}
