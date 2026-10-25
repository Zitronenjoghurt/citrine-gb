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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TouchSettings {
    pub show_controls: bool,
    pub opacity: f32,
    pub scale: f32,
    pub haptics: bool,
    pub landscape: LandscapeControls,
    pub mirrored: bool,
    pub ui_scale: f32,
    pub scaling: ScalingMode,
}

impl Default for TouchSettings {
    fn default() -> Self {
        Self {
            show_controls: true,
            opacity: Self::DEFAULT_OPACITY,
            scale: 1.0,
            haptics: true,
            landscape: LandscapeControls::default(),
            mirrored: false,
            ui_scale: 1.0,
            scaling: ScalingMode::Fit,
        }
    }
}

impl TouchSettings {
    pub const DEFAULT_OPACITY: f32 = 0.75;
    pub const MIN_OPACITY: f32 = 0.15;
    pub const MIN_SCALE: f32 = 0.6;
    pub const MAX_SCALE: f32 = 1.6;
    pub const MIN_UI_SCALE: f32 = 0.6;
    pub const MAX_UI_SCALE: f32 = 2.0;

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
