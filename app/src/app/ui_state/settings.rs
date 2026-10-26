use crate::app::ui_state::hotkeys::Hotkeys;
use crate::app::ui_state::keybinds::KeyBindings;
use crate::app::ui_state::palette::{PaletteChoice, PaletteLibrary};
use crate::app::ui_state::touch::TouchSettings;
use crate::app::ui_state::ui_theme::UiTheme;
use crate::audio::{Audio, DEFAULT_LATENCY_MS};
use crate::autosave::{self, AutosaveInterval};
use crate::emulator::Emulator;
use crate::icons;
use crate::media::recording;
use citrine_gb::ReadMemory;
use citrine_gb::gb::GameBoy;
use citrine_gb::gb::GbModel;
use citrine_gb::gb::ppu::compat::CompatPalette;
use citrine_gb::gb::ppu::types::color::{ColorCorrection, DmgColors};
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use citrine_gb::rom::Rom;
use std::collections::BTreeMap;
use std::fmt::Display;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum LayoutMode {
    #[default]
    Auto,
    Desktop,
    Mobile,
}

impl Display for LayoutMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Auto => "Automatic",
            Self::Desktop => "Desktop (tabs)",
            Self::Mobile => "Touch",
        };
        f.write_str(name)
    }
}

pub const MOBILE_DESIGN_POINTS: f32 = 400.0;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum ConsoleChoice {
    #[default]
    Auto,
    GameBoy,
    GameBoyColor,
}

impl ConsoleChoice {
    pub fn model_for(&self, rom: &Rom) -> GbModel {
        self.resolve(GbModel::for_rom(rom).unwrap_or_default())
    }

    pub fn resolve(&self, game_model: GbModel) -> GbModel {
        match self {
            Self::Auto => game_model,
            Self::GameBoy => GbModel::Dmg,
            Self::GameBoyColor => GbModel::Cgb,
        }
    }
}

impl Display for ConsoleChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Auto => "Automatic",
            Self::GameBoy => "Game Boy",
            Self::GameBoyColor => "Game Boy Color",
        };
        f.write_str(name)
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum ScalingMode {
    #[default]
    Integer,
    Fit,
    Stretch,
}

impl Display for ScalingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Integer => "Integer (sharpest)",
            Self::Fit => "Fit",
            Self::Stretch => "Stretch",
        };
        f.write_str(name)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ui_scale: f32,
    pub ui_theme: UiTheme,
    pub layout: LayoutMode,
    pub console: ConsoleChoice,

    pub dmg_theme: DmgTheme,
    pub custom_palette: Option<String>,
    pub gbc_palette: Option<CompatPalette>,
    pub palettes: PaletteLibrary,
    pub game_palettes: BTreeMap<String, PaletteChoice>,
    pub color_correction: ColorCorrection,
    pub scaling: ScalingMode,
    pub matrix: bool,
    pub ghosting: bool,
    pub matrix_edge_darkness: f32,
    pub matrix_corner_darkness: f32,
    pub ghosting_strength: f32,

    pub volume: f32,
    pub muted: bool,
    pub audio_queue_ms: u32,
    pub audio_device: Option<String>,
    pub channels: [bool; 4],

    pub bindings: KeyBindings,
    pub hotkeys: Hotkeys,
    pub touch: TouchSettings,

    pub current_tab: SettingsTab,
    pub quick_slot: usize,
    pub autosave_interval: AutosaveInterval,
    pub autosave_keep: usize,
    pub screenshot_scale: usize,
    pub recording_scale: usize,
    pub randomized_ram: bool,
    pub dev_mode: bool,
    pub focus_mode: bool,
    pub track_pc: bool,
    #[serde(skip, default = "default_dirty")]
    pub dirty: bool,
    #[serde(skip, default)]
    pub unsaved: bool,
    #[serde(skip, default)]
    pub scale_drag: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: Self::DEFAULT_UI_SCALE,
            ui_theme: UiTheme::default(),
            layout: LayoutMode::default(),
            console: ConsoleChoice::default(),

            dmg_theme: DmgTheme::default(),
            custom_palette: None,
            gbc_palette: None,
            palettes: PaletteLibrary::default(),
            game_palettes: BTreeMap::new(),
            color_correction: ColorCorrection::Screen,
            scaling: ScalingMode::default(),
            matrix: false,
            ghosting: false,
            matrix_edge_darkness: Self::DEFAULT_MATRIX_EDGE_DARKNESS,
            matrix_corner_darkness: Self::DEFAULT_MATRIX_CORNER_DARKNESS,
            ghosting_strength: Self::DEFAULT_GHOSTING_STRENGTH,

            volume: Self::DEFAULT_VOLUME,
            muted: false,
            audio_queue_ms: DEFAULT_LATENCY_MS,
            audio_device: None,
            channels: [true; 4],

            bindings: KeyBindings::default(),
            hotkeys: Hotkeys::default(),
            touch: TouchSettings::default(),

            current_tab: SettingsTab::default(),
            quick_slot: 0,
            autosave_interval: AutosaveInterval::default(),
            autosave_keep: autosave::DEFAULT_KEEP,
            screenshot_scale: recording::DEFAULT_SCALE,
            recording_scale: recording::DEFAULT_SCALE,
            randomized_ram: false,
            dev_mode: false,
            focus_mode: false,
            track_pc: false,
            dirty: default_dirty(),
            unsaved: false,
            scale_drag: false,
        }
    }
}

pub fn rom_header(gb: &GameBoy) -> Vec<u8> {
    (0..0x150)
        .map(|addr| gb.cartridge.read_naive(addr))
        .collect()
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

    pub fn effective_theme(&self) -> DmgTheme {
        self.custom_palette
            .as_deref()
            .and_then(|name| self.palettes.get(name))
            .map(|palette| palette.to_theme())
            .unwrap_or(self.dmg_theme)
    }

    pub fn global_choice(&self) -> PaletteChoice {
        if let Some(palette) = self.gbc_palette {
            return PaletteChoice::Gbc(palette);
        }
        match &self.custom_palette {
            Some(name) => PaletteChoice::Custom(name.clone()),
            None => PaletteChoice::Builtin(self.dmg_theme),
        }
    }

    pub fn set_global_choice(&mut self, choice: PaletteChoice) {
        self.gbc_palette = None;
        match choice {
            PaletteChoice::Builtin(theme) => {
                self.dmg_theme = theme;
                self.custom_palette = None;
            }
            PaletteChoice::Custom(name) => self.custom_palette = Some(name),
            PaletteChoice::Gbc(palette) => self.gbc_palette = Some(palette),
        }
        self.dirty = true;
    }

    pub fn choice_for(&self, rom_key: Option<&str>) -> PaletteChoice {
        self.game_palette(rom_key)
            .cloned()
            .unwrap_or_else(|| self.global_choice())
    }

    pub fn theme_for(&self, rom_key: Option<&str>) -> DmgTheme {
        let Some(choice) = self.game_palette(rom_key) else {
            return self.effective_theme();
        };
        match choice {
            PaletteChoice::Builtin(theme) => *theme,
            PaletteChoice::Custom(name) => self
                .palettes
                .get(name)
                .map(|palette| palette.to_theme())
                .unwrap_or_else(|| self.effective_theme()),
            PaletteChoice::Gbc(_) => self.effective_theme(),
        }
    }

    fn apply_colors(&self, gb: &mut GameBoy, rom_key: Option<&str>) {
        gb.ppu.color_correction = self.color_correction;
        match self.choice_for(rom_key) {
            PaletteChoice::Gbc(palette) => gb.ppu.set_compat_palette(palette, &rom_header(gb)),
            _ => {
                gb.ppu.dmg_theme = self.theme_for(rom_key);
                gb.ppu.dmg_colors = DmgColors::Theme;
            }
        }
    }

    pub fn game_palette(&self, rom_key: Option<&str>) -> Option<&PaletteChoice> {
        self.game_palettes.get(rom_key?)
    }

    pub fn set_game_palette(&mut self, rom_key: &str, choice: Option<PaletteChoice>) {
        match choice {
            Some(choice) => {
                self.game_palettes.insert(rom_key.to_string(), choice);
            }
            None => {
                self.game_palettes.remove(rom_key);
            }
        }
        self.dirty = true;
    }

    pub fn rename_palette(&mut self, from: &str, to: &str) -> bool {
        if !self.palettes.rename(from, to) {
            return false;
        }
        let to = to.trim();
        if self.custom_palette.as_deref() == Some(from) {
            self.custom_palette = Some(to.to_string());
        }
        for choice in self.game_palettes.values_mut() {
            if let PaletteChoice::Custom(name) = choice
                && name == from
            {
                *name = to.to_string();
            }
        }
        self.dirty = true;
        true
    }

    pub fn sanitize(&mut self) {
        self.hotkeys.bind_new_defaults();
        if self
            .custom_palette
            .as_deref()
            .is_some_and(|name| !self.palettes.contains(name))
        {
            self.custom_palette = None;
        }
        if matches!(self.dmg_theme, DmgTheme::Custom(_)) {
            self.dmg_theme = DmgTheme::default();
        }
        self.ui_scale = self.ui_scale.clamp(0.5, 5.0);
        self.volume = self.volume.clamp(0.0, 1.0);
        self.audio_queue_ms = self
            .audio_queue_ms
            .clamp(crate::audio::MIN_LATENCY_MS, crate::audio::MAX_LATENCY_MS);
        self.autosave_keep = self.autosave_keep.clamp(1, autosave::MAX_KEEP);
        self.screenshot_scale = self.screenshot_scale.clamp(1, recording::MAX_SCALE);
        self.recording_scale = recording::even_scale(self.recording_scale);
        self.dirty = true;
    }

    pub fn note_scale_drag(&mut self, response: &egui::Response) {
        if response.dragged() {
            self.scale_drag = true;
        }
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            self.dirty = true;
        }
    }

    pub fn pixels_per_point(&self, ctx: &egui::Context, mobile: bool) -> f32 {
        if !mobile {
            return self.ui_scale;
        }

        let physical = ctx.content_rect().size() * ctx.pixels_per_point();
        let short_edge = physical.x.min(physical.y);
        if short_edge <= 0.0 {
            return ctx.native_pixels_per_point().unwrap_or(1.0);
        }

        let scale = self.touch.clamped_ui_scale();
        (short_edge * scale / MOBILE_DESIGN_POINTS).clamp(0.5, 6.0)
    }

    pub fn apply(
        &mut self,
        ctx: &egui::Context,
        audio: &mut Option<Audio>,
        emulator: &mut Emulator,
        mobile: bool,
    ) -> Option<String> {
        if !std::mem::take(&mut self.scale_drag) {
            let wanted_ppp = self.pixels_per_point(ctx, mobile);
            if (ctx.pixels_per_point() - wanted_ppp).abs() > f32::EPSILON {
                ctx.set_pixels_per_point(wanted_ppp);
            }
        }
        emulator.scaling = if mobile {
            self.touch.scaling
        } else {
            self.scaling
        };

        if !self.dirty {
            return None;
        }
        self.dirty = false;
        self.unsaved = true;

        self.ui_theme.apply(ctx);

        let rom_key = emulator.rom_key().map(str::to_owned);
        self.apply_colors(&mut emulator.gb, rom_key.as_deref());
        emulator.gb.ram_init = if self.randomized_ram {
            citrine_gb::gb::ram_init::RamInit::random()
        } else {
            citrine_gb::gb::ram_init::RamInit::Zeroed
        };
        emulator.enable_matrix = self.matrix;
        emulator.enable_ghosting = self.ghosting;
        emulator.matrix_edge_brightness = 1.0 - self.matrix_edge_darkness;
        emulator.matrix_corner_brightness = 1.0 - self.matrix_corner_darkness;
        emulator.ghosting_blend = 1.0 - self.ghosting_strength;
        emulator.console = self.console;
        emulator.autosave_interval = self.autosave_interval;
        emulator.autosave_keep = self.autosave_keep;

        emulator.gb.debugger.ch1_disabled = !self.channels[0];
        emulator.gb.debugger.ch2_disabled = !self.channels[1];
        emulator.gb.debugger.ch3_disabled = !self.channels[2];
        emulator.gb.debugger.ch4_disabled = !self.channels[3];

        let audio = audio.as_mut()?;
        audio.set_volume(self.volume);
        audio.set_muted(self.muted);

        let reopen = audio.is_running()
            && (audio.latency_ms() != self.audio_queue_ms
                || audio.requested_device() != self.audio_device.as_deref());

        if reopen {
            match audio.start(self.audio_device.as_deref(), self.audio_queue_ms) {
                Ok(link) => emulator.attach_audio(link),
                Err(err) => return Some(format!("Could not reopen the audio device: {err}")),
            }
        }

        None
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum SettingsTab {
    #[default]
    General,
    Sound,
    Controls,
    Style,
    Developer,
}

impl SettingsTab {
    pub fn title(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Sound => "Sound",
            Self::Controls => "Controls",
            Self::Style => "GB style",
            Self::Developer => "Developer",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::General => icons::GEAR_SIX,
            Self::Sound => icons::SPEAKER_HIGH,
            Self::Controls => icons::GAME_CONTROLLER,
            Self::Style => icons::PAINT_BRUSH_HOUSEHOLD,
            Self::Developer => icons::BRACKETS_CURLY,
        }
    }

    pub fn iter_with_dev_mode(dev_mode: bool) -> impl Iterator<Item = SettingsTab> {
        SettingsTab::iter().filter(move |tab| dev_mode || *tab != SettingsTab::Developer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::palette::CustomPalette;

    #[test]
    fn a_custom_palette_wins_over_the_builtin_choice() {
        let mut settings = Settings::default();
        let mine = CustomPalette {
            name: "Mine".into(),
            colors: [[9, 9, 9], [8, 8, 8], [7, 7, 7], [6, 6, 6]],
        };
        settings.palettes.upsert(mine.clone());
        settings.custom_palette = Some("Mine".into());

        assert_eq!(settings.effective_theme(), mine.to_theme());
    }

    #[test]
    fn a_deleted_palette_falls_back_to_the_builtin() {
        let mut settings = Settings {
            dmg_theme: DmgTheme::Pocket,
            custom_palette: Some("Gone".into()),
            ..Default::default()
        };

        assert_eq!(
            settings.effective_theme(),
            DmgTheme::Pocket,
            "a name with nothing behind it must not blank the screen"
        );

        settings.sanitize();
        assert_eq!(
            settings.custom_palette, None,
            "the dangling name is dropped"
        );
    }

    #[test]
    fn sanitize_clears_the_unusable_custom_theme_from_older_builds() {
        let mut settings = Settings {
            dmg_theme: DmgTheme::Custom(Default::default()),
            ..Default::default()
        };

        settings.sanitize();

        assert_eq!(settings.dmg_theme, DmgTheme::default());
        assert_eq!(settings.effective_theme(), DmgTheme::default());
    }

    #[test]
    fn sanitize_pulls_out_of_range_values_back_in() {
        let mut settings = Settings {
            ui_scale: 99.0,
            volume: -3.0,
            audio_queue_ms: 5000,
            autosave_keep: 0,
            ..Default::default()
        };

        settings.sanitize();
        assert_eq!(settings.autosave_keep, 1);
        settings.autosave_keep = 900;
        settings.sanitize();
        assert_eq!(settings.autosave_keep, autosave::MAX_KEEP);

        assert_eq!(settings.ui_scale, 5.0);
        assert_eq!(settings.volume, 0.0);
        assert_eq!(settings.audio_queue_ms, crate::audio::MAX_LATENCY_MS);
    }

    #[test]
    fn renaming_a_palette_keeps_everything_that_points_at_it() {
        let mine = CustomPalette {
            name: "Mine".into(),
            colors: [[9, 9, 9], [8, 8, 8], [7, 7, 7], [6, 6, 6]],
        };
        let mut settings = Settings {
            dmg_theme: DmgTheme::Pocket,
            custom_palette: Some("Mine".into()),
            ..Default::default()
        };
        settings.palettes.upsert(mine.clone());
        settings.set_game_palette("game", Some(PaletteChoice::Custom("Mine".into())));
        settings.set_game_palette("other", Some(PaletteChoice::Builtin(DmgTheme::Aqua)));

        assert!(settings.rename_palette("Mine", "  Ours  "));

        assert_eq!(settings.custom_palette.as_deref(), Some("Ours"));
        assert_eq!(
            settings.game_palette(Some("game")),
            Some(&PaletteChoice::Custom("Ours".into())),
            "the game follows the palette to its new name"
        );
        assert_eq!(settings.theme_for(Some("game")), mine.to_theme());
        assert_eq!(
            settings.game_palette(Some("other")),
            Some(&PaletteChoice::Builtin(DmgTheme::Aqua)),
            "games on other palettes are untouched"
        );
    }

    #[test]
    fn a_refused_rename_leaves_the_games_alone() {
        let mut settings = Settings::default();
        for name in ["Mine", "Taken"] {
            settings.palettes.upsert(CustomPalette {
                name: name.into(),
                colors: [[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4]],
            });
        }
        settings.set_game_palette("game", Some(PaletteChoice::Custom("Mine".into())));

        assert!(!settings.rename_palette("Mine", "Taken"));

        assert_eq!(
            settings.game_palette(Some("game")),
            Some(&PaletteChoice::Custom("Mine".into()))
        );
    }

    #[test]
    fn settings_saved_by_an_older_build_still_load() {
        let stored = r#"{
            "ui_scale": 2.5,
            "ui_theme": "CatppuccinFrappe",
            "dmg_theme": "Pocket",
            "matrix": true,
            "ghosting": true,
            "matrix_edge_darkness": 0.2,
            "matrix_corner_darkness": 0.3,
            "ghosting_strength": 0.4,
            "volume": 0.8,
            "current_tab": "Sound",
            "quick_slot": 3,
            "randomized_ram": true,
            "dev_mode": true,
            "focus_mode": false,
            "track_pc": true
        }"#;

        let settings: Settings = serde_json::from_str(stored).expect("old settings still load");

        assert_eq!(settings.ui_scale, 2.5);
        assert_eq!(settings.dmg_theme, DmgTheme::Pocket);
        assert_eq!(settings.quick_slot, 3);
        assert!(settings.dev_mode);
        assert_eq!(settings.audio_queue_ms, DEFAULT_LATENCY_MS);
        assert_eq!(settings.channels, [true; 4]);
        assert_eq!(settings.layout, LayoutMode::Auto);
        assert!(settings.bindings.is_default());
        assert_eq!(settings.autosave_interval, AutosaveInterval::FiveMinutes);
        assert_eq!(settings.autosave_keep, autosave::DEFAULT_KEEP);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn autosave_choices_persist() {
        let root =
            std::env::temp_dir().join(format!("citrine-autosave-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = crate::storage::SettingsStore::at(root.clone());
        let settings = Settings {
            autosave_interval: AutosaveInterval::FifteenMinutes,
            autosave_keep: 25,
            ..Default::default()
        };
        store.store(&settings).expect("store");

        let back: Settings = store.load().expect("load");
        assert_eq!(back.autosave_interval, AutosaveInterval::FifteenMinutes);
        assert_eq!(back.autosave_keep, 25);
        let _ = std::fs::remove_dir_all(root);
    }
}
