use crate::app::ui_state::ui_theme::UiTheme;
use crate::audio::Audio;
use crate::emulator::Emulator;
use crate::icons;
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub ui_scale: f32,
    pub ui_theme: UiTheme,
    pub dmg_theme: DmgTheme,
    pub matrix: bool,
    pub ghosting: bool,
    pub matrix_edge_darkness: f32,
    pub matrix_corner_darkness: f32,
    pub ghosting_strength: f32,
    pub volume: f32,
    pub current_tab: SettingsTab,
    pub dev_mode: bool,
    pub focus_mode: bool,
    pub track_pc: bool,
    #[serde(skip, default = "default_dirty")]
    pub dirty: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: Self::DEFAULT_UI_SCALE,
            ui_theme: Default::default(),
            dmg_theme: DmgTheme::default(),
            matrix: false,
            ghosting: false,
            matrix_edge_darkness: Self::DEFAULT_MATRIX_EDGE_DARKNESS,
            matrix_corner_darkness: Self::DEFAULT_MATRIX_CORNER_DARKNESS,
            ghosting_strength: Self::DEFAULT_GHOSTING_STRENGTH,
            volume: Self::DEFAULT_VOLUME,
            current_tab: SettingsTab::default(),
            dev_mode: false,
            focus_mode: false,
            track_pc: false,
            dirty: default_dirty(),
        }
    }
}

fn default_dirty() -> bool {
    true
}

impl Settings {
    pub const DEFAULT_VOLUME: f32 = 0.25;
    #[cfg(target_arch = "wasm32")]
    pub const DEFAULT_UI_SCALE: f32 = 2.0;
    #[cfg(not(target_arch = "wasm32"))]
    pub const DEFAULT_UI_SCALE: f32 = 1.5;
    pub const DEFAULT_MATRIX_EDGE_DARKNESS: f32 = 0.15;
    pub const DEFAULT_MATRIX_CORNER_DARKNESS: f32 = 0.25;
    pub const DEFAULT_GHOSTING_STRENGTH: f32 = 0.3;

    pub fn apply(
        &mut self,
        ctx: &egui::Context,
        audio: &mut Option<Audio>,
        emulator: &mut Emulator,
    ) {
        if !self.dirty {
            return;
        }

        ctx.set_pixels_per_point(self.ui_scale);
        self.ui_theme.apply(ctx);
        emulator.gb.ppu.dmg_theme = self.dmg_theme;
        emulator.enable_matrix = self.matrix;
        emulator.enable_ghosting = self.ghosting;
        emulator.matrix_edge_brightness = 1.0 - self.matrix_edge_darkness;
        emulator.matrix_corner_brightness = 1.0 - self.matrix_corner_darkness;
        emulator.ghosting_blend = 1.0 - self.ghosting_strength;

        if let Some(audio) = audio {
            audio.set_volume(self.volume);
        };

        self.dirty = false;
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum SettingsTab {
    #[default]
    General,
    Sound,
    Style,
    Developer,
}

impl SettingsTab {
    pub fn title(&self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Sound => "Sound",
            SettingsTab::Style => "GB Style",
            SettingsTab::Developer => "Developer",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SettingsTab::General => icons::GEAR_SIX,
            SettingsTab::Sound => icons::SPEAKER_HIGH,
            SettingsTab::Style => icons::PAINT_BRUSH_HOUSEHOLD,
            SettingsTab::Developer => icons::BRACKETS_CURLY,
        }
    }

    pub fn iter_with_dev_mode(dev_mode: bool) -> impl Iterator<Item = SettingsTab> {
        SettingsTab::iter().filter(move |tab| dev_mode || *tab != SettingsTab::Developer)
    }
}
