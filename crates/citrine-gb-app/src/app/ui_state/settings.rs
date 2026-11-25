use crate::app::ui_state::hotkeys::{FAST_FORWARD_KEY, HotkeyAction, Hotkeys, QuickLoadKey};
use crate::app::ui_state::keybinds::{GbButton, KeyBindings};
use crate::app::ui_state::palette::{PaletteChoice, PaletteLibrary};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{SettingsTab, Surface};
use crate::app::ui_state::touch::TouchSettings;
use crate::app::ui_state::ui_theme::UiTheme;
use crate::audio::mix::{SoundLevels, level};
use crate::audio::pacer::{FastForwardSound, SlowSound};
use crate::audio::{Audio, DEFAULT_LATENCY_MS};
use crate::autosave::{self, AutosaveInterval};
use crate::emulator::Emulator;
use crate::emulator::fast_forward::{FastForwardKey, FastForwardSpeed};
use crate::emulator::load::ScreenMatch;
use crate::emulator::rewind::{ClipLength, RewindLength};
use crate::media::recording;
use crate::online::OnlineSettings;
use citrine_gb::ReadMemory;
use citrine_gb::console::ConsoleKind;
use citrine_gb::gb::GameBoy;
use citrine_gb::gb::GbModel;
use citrine_gb::gb::ppu::compat::CompatPalette;
use citrine_gb::gb::ppu::types::color::{ColorCorrection, DmgColors};
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use citrine_gb::rom::Rom;
use citrine_gb::rom::header::RomCgbMode;
use std::collections::BTreeMap;
use std::fmt::Display;
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

impl LayoutMode {
    const BREAKPOINT: f32 = 560.0;

    pub fn is_touch(self, ctx: &egui::Context) -> bool {
        self.is_touch_at(device_size(ctx))
    }

    pub fn is_touch_at(self, size: egui::Vec2) -> bool {
        match self {
            Self::Desktop => size.x < Self::BREAKPOINT,
            Self::Mobile => true,
            Self::Auto => size.x.min(size.y) < Self::BREAKPOINT,
        }
    }

    pub fn too_narrow_for_desktop(ctx: &egui::Context) -> bool {
        device_size(ctx).x < Self::BREAKPOINT
    }
}

fn device_size(ctx: &egui::Context) -> egui::Vec2 {
    window_pixels(ctx) / ctx.native_pixels_per_point().unwrap_or(1.0)
}

#[derive(Clone, Copy)]
struct WindowPixels(egui::Vec2);

fn window_id() -> egui::Id {
    egui::Id::new("window_pixels")
}

pub fn note_window(ctx: &egui::Context, raw_input: &egui::RawInput) {
    let Some(screen) = raw_input.screen_rect else {
        return;
    };
    let native = raw_input
        .viewports
        .get(&raw_input.viewport_id)
        .and_then(|viewport| viewport.native_pixels_per_point)
        .or_else(|| ctx.native_pixels_per_point())
        .unwrap_or(1.0);
    let pixels = screen.size() * ctx.zoom_factor() * native;
    ctx.data_mut(|d| d.insert_temp(window_id(), WindowPixels(pixels)));
}

pub fn window_pixels(ctx: &egui::Context) -> egui::Vec2 {
    ctx.data(|d| d.get_temp::<WindowPixels>(window_id()))
        .map(|window| window.0)
        .unwrap_or_else(|| ctx.viewport_rect().size() * ctx.pixels_per_point())
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
    pub fn of(model: GbModel) -> Self {
        match model {
            GbModel::Dmg => Self::GameBoy,
            GbModel::Cgb => Self::GameBoyColor,
        }
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

pub const SAME_SIZE_PIXELS_NOTE: &str =
    "Stops some rows and columns of pixels looking bigger than others. Edges get a little blurry.";

pub const GRID_NOTE: &str = "Draws thin lines between the pixels, like the original screen.";

pub fn grid_note(scaling: ScalingMode, surface: Surface) -> String {
    format!(
        "{GRID_NOTE} While it is on, the game shows at \"{}\" scaling. Turn on \"{}\" to keep \"{scaling}\".",
        ScalingMode::Integer,
        RowId::SameSizePixels.label(surface),
    )
}

pub const MATCH_SCREEN_LABEL: &str = "Match refresh rate";

pub const MATCH_SCREEN_NOTE: &str = "Runs the game at exactly 60 frames a second, a touch faster than the console's 59.7 (59.8 on DS), so every frame stays on screen equally long on 60, 120, 180 and 240 Hz screens. Sound plays under a tenth of a semitone higher. With it off, the picture hitches every few seconds as one frame shows twice. It does nothing on other screens, like 144 Hz, or when you play with others online or by link cable.";

const DISPLAY_SETTINGS: &str = if cfg!(target_os = "macos") {
    "System Settings, Displays"
} else if cfg!(target_os = "windows") {
    "Settings, System, Display, Advanced display"
} else if cfg!(target_os = "android") {
    "your phone's display settings"
} else {
    "your display settings"
};

const MATCH_RATES: &str = if cfg!(target_os = "macos") {
    "ProMotion, 120 or 60 Hz"
} else {
    "60 or 120 Hz"
};

pub fn match_screen_line(status: Option<ScreenMatch>) -> Option<String> {
    match status? {
        ScreenMatch::Unmatched(hz) => Some(format!(
            "Your screen runs at {hz:.0} Hz, which games can't match. Set it to {MATCH_RATES} in {DISPLAY_SETTINGS}."
        )),
        _ => None,
    }
}

pub const GHOSTING_NOTE: &str = "Smears frames together, the way the original LCD did.";

pub const GBC_SCREEN_NOTE: &str =
    "How Game Boy Color colours look. Like a GBC softens them the way its screen did.";

pub const GBA_SCREEN_NOTE: &str =
    "How Game Boy Advance colours look. Like a GBA darkens them the way its unlit screen did.";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GbaScreen(pub ColorCorrection);

impl Settings {
    pub fn gba_screen(&self) -> GbaScreen {
        GbaScreen(self.gba_color_correction.unwrap_or(self.color_correction))
    }

    pub fn gba_jit(&self) -> bool {
        self.gba_jit.unwrap_or(self.ds_jit)
    }

    pub fn gba_jit_mut(&mut self) -> &mut bool {
        self.gba_jit.get_or_insert(self.ds_jit)
    }
}

impl GbaScreen {
    pub const ALL: [Self; 2] = [Self(ColorCorrection::Raw), Self(ColorCorrection::Screen)];
}

impl std::fmt::Display for GbaScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            ColorCorrection::Raw => f.write_str("Raw"),
            ColorCorrection::Screen => f.write_str("Like a GBA"),
        }
    }
}

#[cfg(feature = "nds")]
pub const DS_JIT_HINT: &str = "Translates DS game code for this device's processor, so DS games run faster. Turn it off if a game misbehaves.";
#[cfg(feature = "nds")]
pub const GBA_JIT_HINT: &str = "Translates GBA game code for this device's processor, so GBA games run faster. Turn it off if a game misbehaves.";
pub const DS_JIT_DEFAULT: bool = !cfg!(windows);
#[cfg(feature = "nds")]
pub const GBA_USES_JIT: bool = true;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, remote = "Self")]
pub struct Settings {
    pub ui_scale: f32,
    #[serde(default)]
    pub ui_scale_per_ratio: bool,
    pub ui_theme: UiTheme,
    pub layout: LayoutMode,
    pub console: ConsoleChoice,
    #[serde(skip_serializing)]
    pub game_consoles: BTreeMap<String, GbModel>,
    pub game_names: crate::game_name::GameNames,
    pub cheats: crate::cheats::CheatBook,

    pub dmg_theme: DmgTheme,
    pub custom_palette: Option<String>,
    pub gbc_palette: Option<CompatPalette>,
    pub palettes: PaletteLibrary,
    #[serde(skip_serializing)]
    pub game_palettes: BTreeMap<String, PaletteChoice>,
    pub per_game: super::per_game::PerGame,
    pub per_game_local: super::per_game::PerGame,
    pub color_correction: ColorCorrection,
    #[serde(default)]
    pub gba_color_correction: Option<ColorCorrection>,
    pub scaling: ScalingMode,
    pub same_size_pixels: bool,
    pub ds_screens: crate::app::ui_state::ds_screens::DsScreens,
    pub ds_3d: crate::app::ui_state::ds_3d::Ds3d,
    pub ds_jit: bool,
    #[serde(default)]
    pub gba_jit: Option<bool>,
    pub matrix: bool,
    pub ghosting: bool,
    pub match_screen: bool,
    pub matrix_edge_darkness: f32,
    pub matrix_corner_darkness: f32,
    pub ghosting_strength: f32,

    pub volume: f32,
    pub muted: bool,
    pub audio_queue_ms: u32,
    pub audio_device: Option<String>,
    pub microphone: crate::audio::mic::Microphone,
    pub push_to_talk: bool,
    pub talk_button: bool,
    pub slow_sound: SlowSound,
    pub fast_forward_sound: FastForwardSound,
    pub channels: [bool; 4],
    pub gba_channels: [bool; 6],

    pub bindings: KeyBindings,
    pub hotkeys: Hotkeys,
    pub touch: TouchSettings,
    pub layouts: crate::app::ui_state::screen_layouts::ScreenLayouts,
    pub layout_picks: crate::app::ui_state::layout_picks::LayoutPicks,
    pub saved_layouts: Vec<crate::app::ui_state::layout_picks::SavedLayout>,
    #[serde(default)]
    pub layouts_adopted: bool,
    pub online: OnlineSettings,
    pub local_link: crate::local_link::LinkSettings,
    pub gimmicks: crate::emulator::gimmicks::GimmickSettings,
    pub discord: crate::discord::DiscordSettings,
    pub achievements: crate::achievements::AchievementSettings,
    pub stats_range: crate::app::tabs::stats::Range,

    pub current_tab: SettingsTab,
    pub quick_slot: usize,
    pub autosave_interval: AutosaveInterval,
    pub autosave_keep: usize,
    pub rewind: RewindLength,
    pub fast_forward_speed: FastForwardSpeed,
    pub fast_forward_key: FastForwardKey,
    #[serde(default)]
    pub quick_load: QuickLoadKey,
    #[serde(default)]
    pub background_pause: crate::app::background::InBackground,
    pub screenshot_scale: usize,
    pub clip_length: ClipLength,
    #[serde(default)]
    pub clip_length_adopted: bool,
    #[serde(default)]
    pub capture_folder: Option<String>,
    pub recording_scale: usize,
    pub record_frame_times: bool,
    #[serde(default)]
    pub render_video: crate::media::render::RenderVideo,
    pub randomized_ram: bool,
    pub dev_mode: bool,
    pub check_updates: bool,
    pub covers: bool,
    #[serde(default)]
    pub fullscreen: bool,
    pub hide_pointer: bool,
    #[serde(default)]
    pub library_depth: crate::app::ui_state::library::scan::LibraryDepth,
    pub focus_place: crate::app::focus::ScreenPlace,
    pub hud: crate::app::hud::HudSettings,
    pub big_screen: crate::app::big_screen::BigScreenSettings,
    pub ds_user: crate::app::ui_state::ds_user::DsUser,
    pub ds_online: crate::app::ui_state::ds_online::DsOnline,
    pub walker: crate::app::ui_state::walker_steps::WalkerSteps,
    pub track_pc: bool,
    pub synced: crate::online::synced::SyncDoc,
    #[serde(skip, default)]
    pub playing: super::per_game::Playing,
    #[serde(skip, default = "default_dirty")]
    pub dirty: bool,
    #[serde(skip, default)]
    pub unsaved: bool,
    #[serde(skip, default)]
    pub scale_drag: bool,
    #[serde(skip, default)]
    pub themed_for_phone: Option<bool>,
    #[serde(skip, default)]
    pub find: Option<super::settings_tree::rows::RowId>,
    #[serde(skip, default)]
    pub find_group: Option<super::settings_tree::GroupId>,
    #[serde(skip, default)]
    pub search: String,
    #[serde(skip, default)]
    pub focus_search: bool,
    #[serde(skip, default)]
    pub fullscreen_shown: bool,
    #[serde(skip, default)]
    pub walker_asked: bool,
    #[serde(skip, default)]
    pub mic_test_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: Self::DEFAULT_UI_SCALE,
            ui_scale_per_ratio: true,
            ui_theme: UiTheme::default(),
            layout: LayoutMode::default(),
            console: ConsoleChoice::default(),
            game_consoles: BTreeMap::new(),
            game_names: Default::default(),

            dmg_theme: DmgTheme::default(),
            custom_palette: None,
            gbc_palette: None,
            palettes: PaletteLibrary::default(),
            game_palettes: BTreeMap::new(),
            per_game: Default::default(),
            per_game_local: Default::default(),
            color_correction: ColorCorrection::Screen,
            gba_color_correction: Some(ColorCorrection::Screen),
            scaling: ScalingMode::default(),
            same_size_pixels: false,
            ds_screens: Default::default(),
            ds_3d: Default::default(),
            ds_jit: DS_JIT_DEFAULT,
            gba_jit: Some(DS_JIT_DEFAULT),
            matrix: false,
            ghosting: false,
            match_screen: true,
            matrix_edge_darkness: Self::DEFAULT_MATRIX_EDGE_DARKNESS,
            matrix_corner_darkness: Self::DEFAULT_MATRIX_CORNER_DARKNESS,
            ghosting_strength: Self::DEFAULT_GHOSTING_STRENGTH,

            volume: Self::DEFAULT_VOLUME,
            muted: false,
            audio_queue_ms: DEFAULT_LATENCY_MS,
            audio_device: None,
            microphone: crate::audio::mic::Microphone::default(),
            push_to_talk: false,
            talk_button: true,
            slow_sound: SlowSound::default(),
            fast_forward_sound: FastForwardSound::default(),
            channels: [true; 4],
            gba_channels: [true; 6],

            bindings: KeyBindings::default(),
            hotkeys: Hotkeys::default(),
            touch: TouchSettings::default(),
            layouts: Default::default(),
            layout_picks: Default::default(),
            saved_layouts: Vec::new(),
            layouts_adopted: true,
            online: OnlineSettings::default(),

            cheats: Default::default(),
            current_tab: SettingsTab::default(),
            quick_slot: 0,
            autosave_interval: AutosaveInterval::default(),
            autosave_keep: autosave::DEFAULT_KEEP,
            rewind: RewindLength::default(),
            fast_forward_speed: FastForwardSpeed::default(),
            fast_forward_key: FastForwardKey::default(),
            quick_load: Default::default(),
            background_pause: Default::default(),
            screenshot_scale: recording::DEFAULT_SCALE,
            clip_length: ClipLength::default(),
            clip_length_adopted: true,
            capture_folder: None,
            recording_scale: recording::DEFAULT_SCALE,
            record_frame_times: false,
            render_video: Default::default(),
            randomized_ram: false,
            dev_mode: false,
            check_updates: true,
            covers: true,
            fullscreen: false,
            hide_pointer: false,
            library_depth: Default::default(),
            big_screen: Default::default(),
            focus_place: Default::default(),
            hud: Default::default(),
            ds_user: Default::default(),
            ds_online: Default::default(),
            walker: Default::default(),
            track_pc: false,
            synced: Default::default(),
            playing: Default::default(),
            dirty: default_dirty(),
            unsaved: false,
            scale_drag: false,
            themed_for_phone: None,
            find: None,
            find_group: None,
            search: String::new(),
            focus_search: false,
            fullscreen_shown: false,
            walker_asked: false,
            mic_test_shown: false,
            local_link: crate::local_link::LinkSettings::default(),
            gimmicks: crate::emulator::gimmicks::GimmickSettings::default(),
            discord: crate::discord::DiscordSettings::default(),
            achievements: crate::achievements::AchievementSettings::default(),
            stats_range: Default::default(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn desktop_pixel_ratio(ctx: &egui::Context) -> f32 {
    ctx.native_pixels_per_point().unwrap_or(1.0)
}

#[cfg(not(target_arch = "wasm32"))]
fn desktop_pixel_ratio(_ctx: &egui::Context) -> f32 {
    1.0
}

pub fn plays_on_both(mode: RomCgbMode) -> bool {
    mode == RomCgbMode::CgbAndGb
}

pub fn game_model(mode: RomCgbMode) -> GbModel {
    if mode.is_cgb() {
        GbModel::Cgb
    } else {
        GbModel::Dmg
    }
}

pub fn other_console(model: GbModel) -> GbModel {
    match model {
        GbModel::Dmg => GbModel::Cgb,
        GbModel::Cgb => GbModel::Dmg,
    }
}

pub fn console_name(model: GbModel) -> &'static str {
    match model {
        GbModel::Dmg => "Game Boy",
        GbModel::Cgb => "Game Boy Color",
    }
}

pub fn console_icon(model: GbModel) -> &'static str {
    match model {
        GbModel::Dmg => kind_icon(ConsoleKind::Gb),
        GbModel::Cgb => kind_icon(ConsoleKind::Gbc),
    }
}

pub fn kind_name(kind: ConsoleKind) -> &'static str {
    match kind {
        ConsoleKind::Gba => "Game Boy Advance",
        ConsoleKind::Nds => "Nintendo DS",
        ConsoleKind::Gb => console_name(GbModel::Dmg),
        ConsoleKind::Gbc => console_name(GbModel::Cgb),
    }
}

pub fn kind_icon(kind: ConsoleKind) -> &'static str {
    match kind {
        ConsoleKind::Gb => crate::icons::GAME_BOY,
        ConsoleKind::Gbc => crate::icons::GAME_BOY_COLOR,
        ConsoleKind::Gba => crate::icons::GAME_BOY_ADVANCE,
        ConsoleKind::Nds => crate::icons::NINTENDO_DS,
    }
}

pub const KIND_ICON_16_SIZE: f32 = 16.0;

pub fn kind_icon_16(kind: ConsoleKind) -> &'static str {
    match kind {
        ConsoleKind::Gb => crate::icons::GAME_BOY_16,
        ConsoleKind::Gbc => crate::icons::GAME_BOY_COLOR_16,
        ConsoleKind::Gba => crate::icons::GAME_BOY_ADVANCE_16,
        ConsoleKind::Nds => crate::icons::NINTENDO_DS_16,
    }
}

pub fn kind_short(kind: ConsoleKind) -> &'static str {
    match kind {
        ConsoleKind::Gb => "GB",
        ConsoleKind::Gbc => "GBC",
        ConsoleKind::Gba => "GBA",
        ConsoleKind::Nds => "DS",
    }
}

pub fn kind_label(kind: ConsoleKind) -> String {
    format!("{}  {}", kind_icon(kind), kind_name(kind))
}

pub fn kind_icon_or_cart(kind: Option<ConsoleKind>) -> &'static str {
    kind.map_or(crate::icons::CARTRIDGE, kind_icon)
}

pub fn rom_header(gb: &GameBoy) -> Vec<u8> {
    (0..0x150)
        .map(|addr| gb.cartridge.read_naive(addr))
        .collect()
}

fn choice_of(gbc: Option<CompatPalette>, custom: Option<String>, theme: DmgTheme) -> PaletteChoice {
    if let Some(palette) = gbc {
        return PaletteChoice::Gbc(palette);
    }
    match custom {
        Some(name) => PaletteChoice::Custom(name),
        None => PaletteChoice::Builtin(theme),
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

    pub fn effective_theme(&self) -> DmgTheme {
        self.custom_palette
            .as_deref()
            .and_then(|name| self.palettes.get(name))
            .map(|palette| palette.to_theme())
            .unwrap_or(self.dmg_theme)
    }

    pub fn global_choice(&self) -> PaletteChoice {
        choice_of(
            self.gbc_palette,
            self.custom_palette.clone(),
            self.dmg_theme,
        )
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
        match rom_key.filter(|key| Some(*key) != self.playing_key()) {
            None => self.global_choice(),
            Some(key) => choice_of(
                self.value_for(key, "gbc_palette").flatten(),
                self.value_for(key, "custom_palette").flatten(),
                self.value_for(key, "dmg_theme").unwrap_or(self.dmg_theme),
            ),
        }
    }

    pub fn theme_for(&self, rom_key: Option<&str>) -> DmgTheme {
        match self.choice_for(rom_key) {
            PaletteChoice::Builtin(theme) => theme,
            PaletteChoice::Custom(name) => self
                .palettes
                .get(&name)
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

    pub fn console_for(&self, rom_key: &str, mode: RomCgbMode) -> GbModel {
        let choice = self
            .value_for::<ConsoleChoice>(rom_key, "console")
            .unwrap_or(self.console);
        choice.resolve(game_model(mode))
    }

    pub fn console_for_rom(&self, rom: &Rom) -> GbModel {
        let mode = rom.cgb_mode().unwrap_or_default();
        let key = rom
            .header()
            .map(|header| header.sha256_hex_string())
            .unwrap_or_default();
        self.console_for(&key, mode)
    }

    pub fn global_console(&self) -> ConsoleChoice {
        self.global_value("console")
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or(self.console)
    }

    pub fn set_game_console(&mut self, rom_key: &str, model: GbModel) {
        if model == self.global_console().resolve(GbModel::Cgb) {
            self.drop_own(rom_key, "console");
        } else {
            self.set_own(
                rom_key,
                "console",
                serde_json::to_value(ConsoleChoice::of(model)).unwrap_or_default(),
            );
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
        self.rename_own_palettes(from, to);
        self.dirty = true;
        true
    }

    pub fn sanitize(&mut self) {
        self.gba_color_correction
            .get_or_insert(self.color_correction);
        self.gba_jit.get_or_insert(self.ds_jit);
        self.make_room_for_fast_forward();
        self.hotkeys.bind_new_defaults();
        self.touch.recheck_opacity();
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

    fn make_room_for_fast_forward(&mut self) {
        let action = HotkeyAction::FastForward;
        if self.hotkeys.offers(action) {
            return;
        }
        self.bindings
            .drop_old_default(FAST_FORWARD_KEY, GbButton::Start);
        if self.bindings.binds_key(FAST_FORWARD_KEY) {
            self.hotkeys.offer_unbound(action);
        }
    }

    pub fn note_scale_drag(&mut self, response: &egui::Response) {
        if response.dragged() {
            self.scale_drag = true;
        }
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            self.dirty = true;
        }
    }

    pub fn desktop_pixels_per_point(&self, pixel_ratio: f32) -> f32 {
        self.ui_scale * pixel_ratio
    }

    pub(crate) fn adopt_clip_length(&mut self) {
        if self.clip_length_adopted {
            return;
        }
        self.clip_length_adopted = true;
        self.clip_length = ClipLength::reaching(self.rewind);
    }

    pub fn adopt_pixel_ratio(&mut self, pixel_ratio: f32) {
        if self.ui_scale_per_ratio {
            return;
        }
        self.ui_scale_per_ratio = true;
        self.dirty = true;
        let chosen = (self.ui_scale - Self::DEFAULT_UI_SCALE).abs() > 0.01;
        if chosen && pixel_ratio > 0.0 {
            self.ui_scale = (self.ui_scale / pixel_ratio).clamp(0.5, 5.0);
        }
    }

    pub fn pixels_per_point(&self, ctx: &egui::Context, mobile: bool) -> f32 {
        if !mobile {
            return self.desktop_pixels_per_point(desktop_pixel_ratio(ctx));
        }

        let physical = window_pixels(ctx);
        let short_edge = physical.x.min(physical.y);
        if short_edge <= 0.0 {
            return ctx.native_pixels_per_point().unwrap_or(1.0);
        }

        let scale = self.touch.clamped_ui_scale();
        let design = if self.big_screen.on || crate::platform::is_tv() {
            crate::app::big_screen::DESIGN_POINTS
        } else {
            MOBILE_DESIGN_POINTS
        };
        (short_edge * scale / design).clamp(0.5, 6.0)
    }

    pub fn holds(&self, action: HotkeyAction) -> bool {
        action.is_hold() && self.quick_load == QuickLoadKey::Hold
    }

    pub fn sound_levels(&self) -> SoundLevels {
        SoundLevels {
            mine: level(self.volume, self.muted),
            friend: level(self.online.friend_volume, self.online.friend_muted),
            slow: self.slow_sound,
            fast: self.fast_forward_sound,
        }
    }

    pub fn channels_of(&mut self, kind: ConsoleKind) -> &mut [bool] {
        match kind {
            ConsoleKind::Gb | ConsoleKind::Gbc => &mut self.channels,
            ConsoleKind::Gba => &mut self.gba_channels,
            ConsoleKind::Nds => &mut [],
        }
    }

    pub(crate) fn set_emulator(&self, emulator: &mut Emulator) {
        emulator.enable_matrix = self.matrix;
        emulator.same_size_pixels = self.same_size_pixels;
        emulator.enable_ghosting = self.ghosting;
        emulator.matrix_edge_brightness = 1.0 - self.matrix_edge_darkness;
        emulator.matrix_corner_brightness = 1.0 - self.matrix_corner_darkness;
        emulator.ghosting_blend = 1.0 - self.ghosting_strength;
        emulator.autosave_interval = self.autosave_interval;
        emulator.autosave_keep = self.autosave_keep;
        emulator.rewind_length = self.rewind;
        emulator.clip_length = self.clip_length;
        emulator.fast_forward_speed = self.fast_forward_speed;
        emulator.match_screen = self.match_screen;
    }

    pub fn apply(
        &mut self,
        ctx: &egui::Context,
        audio: &mut Option<Audio>,
        emulator: &mut Emulator,
        mobile: bool,
    ) -> Option<String> {
        let key = emulator.rom_key().filter(|_| !emulator.shows_session());
        self.name_console(key.is_some().then(|| emulator.kind()));
        self.play_game(key);
        self.name_game(emulator.game_name());
        self.adopt_pixel_ratio(desktop_pixel_ratio(ctx));
        if self.bindings.adopt_triggers() {
            self.dirty = true;
        }
        if self.bindings.adopt_ds() {
            self.dirty = true;
        }
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
        emulator.ds_screens = self.ds_screens.with_placements_of(&emulator.ds_screens);
        emulator.ds_3d = self.ds_3d;
        emulator.ds_jit = self.ds_jit;
        emulator.gba_jit = self.gba_jit();
        emulator.hide_pointer = self.hide_pointer && !mobile;
        if !mobile {
            emulator.screen_context = crate::app::ui_state::ds_screens::ScreenContext::Desktop;
        }

        if self.fullscreen != self.fullscreen_shown {
            self.fullscreen_shown = self.fullscreen;
            if cfg!(not(any(target_arch = "wasm32", target_os = "android"))) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
        }
        if self.themed_for_phone != Some(mobile) {
            self.themed_for_phone = Some(mobile);
            self.ui_theme.apply(ctx);
        }
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        self.unsaved = true;

        self.ui_theme.apply(ctx);
        self.apply_emulator(emulator);

        let audio = audio.as_mut()?;

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

    pub fn apply_emulator(&mut self, emulator: &mut Emulator) {
        let rom_key = emulator.rom_key().map(str::to_owned);
        if let Some(gb) = emulator.as_gb_mut() {
            self.apply_colors(gb, rom_key.as_deref());
        }
        #[cfg(feature = "gba")]
        emulator.set_gba_color_correction(self.gba_screen().0);
        if let Some(link) = emulator.link_mut() {
            let key = link.rom_key().to_owned();
            if let Some(gb) = link.machine.as_gb_mut() {
                self.apply_colors(gb, Some(&key));
            }
            link.set_settings(&self.local_link);
        }
        if let Some(gb) = emulator.as_gb_mut() {
            gb.ram_init = if self.randomized_ram {
                citrine_gb::gb::ram_init::RamInit::random()
            } else {
                citrine_gb::gb::ram_init::RamInit::Zeroed
            };
        }
        self.set_emulator(emulator);
        emulator.set_gimmick_settings(&self.gimmicks);
        emulator.follow_cheats(&self.cheats);

        let kind = emulator.kind();
        for (index, enabled) in self.channels_of(kind).to_vec().into_iter().enumerate() {
            emulator.set_channel_enabled(index, enabled);
        }

        emulator.sound = self.sound_levels();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consoles_show_as_icons_not_short_badges() {
        fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("src dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }
        let app = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
        let mut files = Vec::new();
        sources(&app, &mut files);
        let badges: Vec<String> = "GB GBC GBA DS NDS"
            .split(' ')
            .map(|short| format!("\"{short}\""))
            .collect();
        let mut found = Vec::new();
        for file in files {
            if file.ends_with("widgets/registers.rs") || file.ends_with("ui_state/settings.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&file).expect("source");
            for badge in &badges {
                if source.contains(badge.as_str()) {
                    found.push(format!("{}: {badge}", file.display()));
                }
            }
        }
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn every_console_has_its_own_icon() {
        let kinds = [
            ConsoleKind::Gb,
            ConsoleKind::Gbc,
            ConsoleKind::Gba,
            ConsoleKind::Nds,
        ];
        let icons: std::collections::HashSet<_> = kinds.into_iter().map(kind_icon).collect();
        assert_eq!(icons.len(), kinds.len());
        assert_eq!(console_icon(GbModel::Cgb), kind_icon(ConsoleKind::Gbc));
        assert_eq!(kind_icon_or_cart(None), crate::icons::CARTRIDGE);
    }

    #[test]
    fn a_fresh_emulator_shows_what_default_settings_would_set() {
        let fields = |e: &Emulator| {
            (
                (e.enable_matrix, e.same_size_pixels, e.enable_ghosting),
                (
                    e.matrix_edge_brightness,
                    e.matrix_corner_brightness,
                    e.ghosting_blend,
                ),
                (
                    e.autosave_interval,
                    e.autosave_keep,
                    e.rewind_length,
                    e.clip_length,
                ),
                (e.fast_forward_speed, e.match_screen),
            )
        };
        let fresh = Emulator::default();
        let mut set = Emulator::default();
        Settings::default().set_emulator(&mut set);
        assert_eq!(fields(&fresh), fields(&set));
    }
    use crate::app::ui_state::keybinds::GbButton;
    use crate::app::ui_state::palette::CustomPalette;

    #[test]
    fn each_console_keeps_its_own_channel_switches() {
        let mut s = Settings::default();
        s.channels[0] = false;
        s.gba_channels[5] = false;
        assert_eq!(s.channels_of(ConsoleKind::Gb), [false, true, true, true]);
        assert_eq!(s.channels_of(ConsoleKind::Gbc), [false, true, true, true]);
        assert_eq!(
            s.channels_of(ConsoleKind::Gba),
            [true, true, true, true, true, false]
        );
        assert!(s.channels_of(ConsoleKind::Nds).is_empty());
    }

    #[test]
    fn the_friends_volume_is_kept_and_starts_at_half() {
        let fresh = Settings::default();
        assert_eq!(fresh.online.friend_volume, 0.5);
        assert!(!fresh.online.friend_muted);
        let mut settings = Settings::default();
        settings.online.friend_volume = 0.3;
        settings.online.friend_muted = true;
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.online.friend_volume, 0.3);
        assert!(back.online.friend_muted);
        let older: Settings =
            serde_json::from_str(r#"{ "online": { "show_playing": false } }"#).unwrap();
        assert_eq!(older.online.friend_volume, 0.5);
    }

    #[test]
    fn my_mute_and_the_friends_mute_are_separate() {
        let settings = Settings {
            volume: 0.8,
            muted: true,
            ..Default::default()
        };
        assert_eq!(settings.sound_levels().mine, 0.0);
        assert_eq!(settings.sound_levels().friend, 0.25);
        let mut settings = Settings {
            volume: 0.5,
            ..Default::default()
        };
        settings.online.friend_muted = true;
        assert_eq!(settings.sound_levels().mine, 0.25);
        assert_eq!(settings.sound_levels().friend, 0.0);
    }

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
    fn the_desktop_ui_keeps_its_css_size_at_every_pixel_ratio_and_zoom() {
        let settings = Settings::default();
        let one_x = settings.desktop_pixels_per_point(1.0);
        assert_eq!(one_x, Settings::DEFAULT_UI_SCALE);
        for ratio in [1.0, 2.0, 1.25, 2.5, 3.0] {
            let ppp = settings.desktop_pixels_per_point(ratio);
            assert!(
                (ppp / ratio - one_x).abs() < 1e-5,
                "ratio {ratio}: {ppp} points per pixel"
            );
        }
    }

    #[test]
    fn a_ui_scale_from_an_older_build_keeps_its_size_on_the_same_screen() {
        let load = |json: &str| -> Settings { serde_json::from_str(json).expect("loads") };

        let mut chosen = load(r#"{ "ui_scale": 3.0 }"#);
        assert!(!chosen.ui_scale_per_ratio);
        let old_ppp = chosen.ui_scale;
        chosen.adopt_pixel_ratio(2.0);
        assert_eq!(chosen.ui_scale, 1.5);
        assert_eq!(chosen.desktop_pixels_per_point(2.0), old_ppp);
        assert!(chosen.ui_scale_per_ratio);
        chosen.adopt_pixel_ratio(2.0);
        assert_eq!(chosen.ui_scale, 1.5, "adopted once");

        let mut untouched = load(&format!(
            r#"{{ "ui_scale": {} }}"#,
            Settings::DEFAULT_UI_SCALE
        ));
        untouched.adopt_pixel_ratio(2.0);
        assert_eq!(untouched.ui_scale, Settings::DEFAULT_UI_SCALE);

        let mut one_x = load(r#"{ "ui_scale": 3.0 }"#);
        one_x.adopt_pixel_ratio(1.0);
        assert_eq!(one_x.ui_scale, 3.0);

        let mut fresh = Settings {
            ui_scale: 3.0,
            ..Settings::default()
        };
        fresh.adopt_pixel_ratio(2.0);
        assert_eq!(fresh.ui_scale, 3.0);
    }

    #[test]
    fn a_rewind_length_from_an_older_build_becomes_the_clip_length_once() {
        let load = |json: &str| -> Settings { serde_json::from_str(json).expect("loads") };

        let two = load(r#"{ "rewind": "TwoMinutes" }"#);
        assert_eq!(two.clip_length, ClipLength::TwoMinutes);
        assert!(two.clip_length_adopted);
        let ten = load(r#"{ "rewind": "TenSeconds" }"#);
        assert_eq!(ten.clip_length, ClipLength::TenSeconds);
        let off = load(r#"{ "rewind": "Off" }"#);
        assert_eq!(off.rewind, RewindLength::Off);
        assert_eq!(off.clip_length, ClipLength::ThirtySeconds);

        let kept = load(
            r#"{ "rewind": "TwoMinutes", "clip_length": "TenSeconds", "clip_length_adopted": true }"#,
        );
        assert_eq!(kept.clip_length, ClipLength::TenSeconds);
        let turned_off =
            load(r#"{ "rewind": "OneMinute", "clip_length": "Off", "clip_length_adopted": true }"#);
        assert_eq!(turned_off.clip_length, ClipLength::Off);

        let again: Settings =
            serde_json::from_value(serde_json::to_value(&ten).expect("saves")).expect("loads");
        assert_eq!(again.clip_length, ClipLength::TenSeconds);
        let mut changed = again;
        changed.rewind = RewindLength::TwoMinutes;
        let reloaded: Settings =
            serde_json::from_value(serde_json::to_value(&changed).expect("saves")).expect("loads");
        assert_eq!(reloaded.clip_length, ClipLength::TenSeconds, "adopted once");
        assert!(Settings::default().clip_length_adopted);
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
        settings.custom_palette = None;
        settings.set_own("game", "custom_palette", serde_json::json!("Mine"));
        settings.set_own("other", "dmg_theme", serde_json::json!("Aqua"));
        settings.custom_palette = Some("Mine".into());
        settings.set_own("other", "custom_palette", serde_json::Value::Null);

        assert!(settings.rename_palette("Mine", "  Ours  "));

        assert_eq!(settings.custom_palette.as_deref(), Some("Ours"));
        assert_eq!(
            settings.choice_for(Some("game")),
            PaletteChoice::Custom("Ours".into()),
            "the game follows the palette to its new name"
        );
        assert_eq!(settings.theme_for(Some("game")), mine.to_theme());
        assert_eq!(
            settings.choice_for(Some("other")),
            PaletteChoice::Builtin(DmgTheme::Aqua),
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
        settings.set_own("game", "custom_palette", serde_json::json!("Mine"));

        assert!(!settings.rename_palette("Mine", "Taken"));

        assert_eq!(
            settings.choice_for(Some("game")),
            PaletteChoice::Custom("Mine".into())
        );
    }

    #[test]
    fn old_general_and_style_tabs_load_as_app_and_screen() {
        for (old, page) in [
            ("General", SettingsTab::App),
            ("Style", SettingsTab::Screen),
            ("Sound", SettingsTab::Sound),
            ("Controls", SettingsTab::Controls),
            ("Online", SettingsTab::Online),
            ("YourData", SettingsTab::Storage),
            ("Developer", SettingsTab::Developer),
        ] {
            let stored = format!(r#"{{ "current_tab": "{old}", "volume": 0.3 }}"#);
            let settings: Settings = serde_json::from_str(&stored).expect("old tab still loads");
            assert_eq!(settings.current_tab, page, "{old}");
            assert_eq!(settings.volume, 0.3, "{old}");
        }
        let json = serde_json::to_string(&Settings {
            current_tab: SettingsTab::Gba,
            ..Settings::default()
        })
        .unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.current_tab, SettingsTab::Gba);
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
        "focus_mode": true,
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

    #[test]
    fn a_saved_settings_file_never_brings_focus_mode_back() {
        let settings: Settings =
            serde_json::from_str(r#"{ "focus_mode": true }"#).expect("old settings still load");
        let saved = serde_json::to_value(&settings).expect("settings save");
        assert!(saved.get("focus_mode").is_none());

        let mut ui = crate::app::ui_state::UiState {
            settings,
            ..Default::default()
        };
        ui.focus_mode = true;
        let stored = serde_json::to_string(&ui).expect("the ui state saves");
        let back: crate::app::ui_state::UiState =
            serde_json::from_str(&stored).expect("the ui state loads");
        assert!(!back.focus_mode);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn autosave_choices_persist() {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-autosave-settings-{}", std::process::id()));
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

    const BOTH: &str = "BOTH";

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_ds_3d_choices_persist() {
        use crate::app::ui_state::ds_3d::{Resolution, SmoothEdges};
        let root =
            crate::storage::scratch_dir().join(format!("citrine-ds-3d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = crate::storage::SettingsStore::at(root.clone());
        let mut settings = Settings::default();
        assert_eq!(settings.ds_3d.resolution, Resolution::Native);
        settings.ds_3d.resolution = Resolution::Quadruple;
        settings.ds_3d.smooth_edges = SmoothEdges::On;
        store.store(&settings).expect("store");

        let back: Settings = store.load().expect("load");
        assert_eq!(back.ds_3d, settings.ds_3d);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_game_console_persists() {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-game-console-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = crate::storage::SettingsStore::at(root.clone());
        let mut settings = Settings::default();
        settings.set_game_console(BOTH, GbModel::Dmg);
        store.store(&settings).expect("store");

        let back: Settings = store.load().expect("load");
        assert_eq!(back.console_for(BOTH, RomCgbMode::CgbAndGb), GbModel::Dmg);
        assert_eq!(
            back.console_for("OTHER", RomCgbMode::CgbAndGb),
            GbModel::Cgb,
            "another game keeps the default"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn settings_from_before_game_consoles_still_load() {
        let old = r#"{"console":"GameBoy","volume":0.5}"#;
        let settings: Settings = serde_json::from_str(old).expect("old settings load");
        assert_eq!(settings.console, ConsoleChoice::GameBoy);
        assert_eq!(settings.own_values(BOTH).count(), 0);
    }

    #[test]
    fn game_consoles_from_0_12_become_the_games_own_console() {
        let old = format!(r#"{{"console":"Auto","game_consoles":{{"{BOTH}":"Dmg"}}}}"#);
        let settings: Settings = serde_json::from_str(&old).expect("0.12 settings load");
        assert!(settings.game_consoles.is_empty());
        assert_eq!(
            settings.console_for(BOTH, RomCgbMode::CgbAndGb),
            GbModel::Dmg
        );
        assert_eq!(
            settings.console_for("OTHER", RomCgbMode::CgbAndGb),
            GbModel::Cgb
        );
        let saved = serde_json::to_value(&settings).expect("settings save");
        assert!(saved.get("game_consoles").is_none());
        assert_eq!(
            saved["per_game"][BOTH]["console"],
            serde_json::json!("GameBoy")
        );
    }

    #[test]
    fn the_default_applies_without_a_game_choice() {
        let mut settings = Settings::default();
        assert_eq!(
            settings.console_for(BOTH, RomCgbMode::CgbAndGb),
            GbModel::Cgb
        );
        assert_eq!(
            settings.console_for(BOTH, RomCgbMode::CgbOnly),
            GbModel::Cgb
        );
        assert_eq!(settings.console_for(BOTH, RomCgbMode::None), GbModel::Dmg);

        settings.console = ConsoleChoice::GameBoy;
        assert_eq!(
            settings.console_for(BOTH, RomCgbMode::CgbAndGb),
            GbModel::Dmg
        );
        assert_eq!(
            settings.console_for(BOTH, RomCgbMode::CgbOnly),
            GbModel::Dmg,
            "only the default puts a Color-only game on a Game Boy"
        );
    }

    #[test]
    fn a_game_choice_only_counts_for_that_game() {
        let mut settings = Settings::default();
        settings.set_game_console(BOTH, GbModel::Dmg);
        assert_eq!(
            settings.console_for(BOTH, RomCgbMode::CgbAndGb),
            GbModel::Dmg
        );
        assert_eq!(
            settings.console_for("OTHER", RomCgbMode::CgbAndGb),
            GbModel::Cgb
        );
        settings.set_game_console(BOTH, GbModel::Cgb);
        assert_eq!(settings.console_for(BOTH, RomCgbMode::None), GbModel::Dmg);
    }

    #[test]
    fn only_games_that_run_on_both_offer_the_choice() {
        assert!(plays_on_both(RomCgbMode::CgbAndGb));
        assert!(!plays_on_both(RomCgbMode::CgbOnly));
        assert!(!plays_on_both(RomCgbMode::None));
        let shelf: Vec<&str> = crate::homebrew::HOMEBREW_GAMES
            .iter()
            .filter(|game| plays_on_both(game.cgb_mode))
            .map(|game| game.id)
            .collect();
        assert_eq!(shelf, ["libbet", "tobu_deluxe"]);
    }

    #[test]
    fn picking_what_the_default_gives_forgets_the_game_choice() {
        let mut settings = Settings::default();
        settings.set_game_console(BOTH, GbModel::Dmg);
        assert_eq!(settings.own_values(BOTH).count(), 1);
        settings.set_game_console(BOTH, GbModel::Cgb);
        assert_eq!(settings.own_values(BOTH).count(), 0);
        assert_eq!(
            settings.console_for(BOTH, RomCgbMode::CgbAndGb),
            GbModel::Cgb
        );
    }

    #[test]
    fn a_rom_resolves_its_console_from_its_header() {
        let mut data = vec![0u8; 0x8000];
        data[0x143] = 0x80;
        let rom = Rom::new(&data);
        let key = rom.header().unwrap().sha256_hex_string();
        let mut settings = Settings::default();
        assert_eq!(settings.console_for_rom(&rom), GbModel::Cgb);
        settings.set_game_console(&key, GbModel::Dmg);
        assert_eq!(settings.console_for_rom(&rom), GbModel::Dmg);
    }

    #[test]
    fn the_ds_jit_starts_off_on_windows_and_on_elsewhere() {
        assert_eq!(Settings::default().ds_jit, !cfg!(windows));
        let stored: Settings = serde_json::from_str("{}").expect("empty settings load");
        assert_eq!(stored.ds_jit, DS_JIT_DEFAULT);
    }

    #[test]
    fn the_gba_jit_keeps_the_old_shared_switch_and_then_stands_alone() {
        assert_eq!(Settings::default().gba_jit(), DS_JIT_DEFAULT);
        let mut json = serde_json::to_value(Settings {
            ds_jit: false,
            ..Default::default()
        })
        .unwrap();
        json.as_object_mut().unwrap().remove("gba_jit");
        let mut settings: Settings = serde_json::from_value(json).unwrap();
        assert!(!settings.gba_jit());
        settings.sanitize();
        assert_eq!(settings.gba_jit, Some(false));
        settings.ds_jit = true;
        assert!(!settings.gba_jit());
        *settings.gba_jit_mut() = true;
        settings.ds_jit = false;
        let ctx = egui::Context::default();
        let mut emulator = Emulator::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            settings.apply(ctx, &mut None, &mut emulator, false);
        });
        assert!(emulator.gba_jit);
        assert!(!emulator.ds_jit);
    }

    #[test]
    fn match_refresh_rate_starts_on_and_keeps_a_choice() {
        assert!(Settings::default().match_screen);
        let older: Settings =
            serde_json::from_str(r#"{ "matrix": true }"#).expect("old settings load");
        assert!(older.match_screen);
        let settings = Settings {
            match_screen: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(!back.match_screen);
    }

    #[test]
    fn slowdown_keeps_the_pitch_until_changed_and_reaches_the_sound() {
        let mut settings = Settings::default();
        assert_eq!(settings.slow_sound, SlowSound::Tempo);
        assert_eq!(settings.sound_levels().slow, SlowSound::Tempo);
        settings.slow_sound = SlowSound::Pitch;
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sound_levels().slow, SlowSound::Pitch);
    }

    #[test]
    fn settings_saved_before_the_subfolder_depth_read_two_levels_deep() {
        use crate::app::ui_state::library::scan::LibraryDepth;
        let mut json = serde_json::to_value(Settings {
            library_depth: LibraryDepth::All,
            ..Settings::default()
        })
        .unwrap();
        let back: Settings = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.library_depth, LibraryDepth::All);
        json.as_object_mut().unwrap().remove("library_depth");
        let old: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(old.library_depth, LibraryDepth::Two);
    }

    #[test]
    fn quick_load_holds_and_the_pointer_shows_until_changed() {
        use crate::app::background::InBackground;
        let json = serde_json::to_value(Settings::default()).unwrap();
        let mut old = json.clone();
        for field in ["quick_load", "background_pause", "hide_pointer"] {
            old.as_object_mut().unwrap().remove(field);
        }
        let old: Settings = serde_json::from_value(old).unwrap();
        assert_eq!(old.quick_load, QuickLoadKey::Hold);
        assert_eq!(old.background_pause, InBackground::KeepPlaying);
        assert!(!old.hide_pointer);
        assert!(old.holds(HotkeyAction::QuickLoad));
        assert!(!old.holds(HotkeyAction::QuickSave));

        let mut settings: Settings = serde_json::from_value(json).unwrap();
        settings.quick_load = QuickLoadKey::Press;
        assert!(!settings.holds(HotkeyAction::QuickLoad));
        assert_eq!(QuickLoadKey::Hold.to_string(), "Hold 2 s");

        settings.hide_pointer = true;
        let ctx = egui::Context::default();
        let mut emulator = Emulator::default();
        for (mobile, hides) in [(false, true), (true, false)] {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                settings.apply(ctx, &mut None, &mut emulator, mobile);
            });
            assert_eq!(emulator.hide_pointer, hides, "touch layout {mobile}");
        }
    }

    #[test]
    fn fast_forwarding_stays_silent_until_changed_and_reaches_the_sound() {
        let mut settings = Settings::default();
        assert_eq!(settings.sound_levels().fast, FastForwardSound::Silent);
        settings.fast_forward_sound = FastForwardSound::SpedUp;
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sound_levels().fast, FastForwardSound::SpedUp);
        assert_eq!(FastForwardSound::SpedUp.to_string(), "Sped up");
    }

    #[test]
    fn gba_screen_colours_leave_the_gbc_screen_alone() {
        let mut json = serde_json::to_value(Settings {
            color_correction: ColorCorrection::Raw,
            ..Default::default()
        })
        .unwrap();
        json.as_object_mut().unwrap().remove("gba_color_correction");
        let mut settings: Settings = serde_json::from_value(json).unwrap();
        settings.sanitize();
        assert_eq!(settings.gba_screen(), GbaScreen(ColorCorrection::Raw));
        settings.color_correction = ColorCorrection::Screen;
        assert_eq!(settings.gba_screen(), GbaScreen(ColorCorrection::Raw));
        settings.gba_color_correction = Some(ColorCorrection::Screen);
        settings.color_correction = ColorCorrection::Raw;
        assert_eq!(settings.gba_screen(), GbaScreen(ColorCorrection::Screen));
        let mut fresh = Settings::default();
        fresh.sanitize();
        assert_eq!(fresh.gba_screen(), GbaScreen(ColorCorrection::Screen));
    }

    #[test]
    fn sanitize_leaves_the_defaults_alone() {
        let mut fresh = Settings::default();
        fresh.sanitize();
        let fresh = serde_json::to_value(fresh).unwrap();
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        let changed: Vec<&String> = fresh
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, value)| defaults.get(key.as_str()) != Some(*value))
            .map(|(key, _)| key)
            .collect();
        assert!(changed.is_empty(), "sanitize changed {changed:?}");
    }

    fn from_before_fast_forward(bindings: KeyBindings) -> Settings {
        let mut json = serde_json::to_value(Settings {
            bindings,
            ..Default::default()
        })
        .unwrap();
        let hotkeys = json["hotkeys"].as_object_mut().unwrap();
        hotkeys.remove("fast_forward");
        hotkeys["offered"]
            .as_array_mut()
            .unwrap()
            .retain(|action| action != "FastForward");
        let mut settings: Settings = serde_json::from_value(json).unwrap();
        settings.sanitize();
        settings
    }

    fn with_space_on_start() -> KeyBindings {
        let mut bindings = KeyBindings::default();
        bindings.bind_key(egui::Key::Space, GbButton::Start);
        bindings
    }

    #[test]
    fn space_leaves_start_and_fast_forwards_once_the_update_lands() {
        use crate::app::ui_state::hotkeys::HotkeyAction;
        let settings = from_before_fast_forward(with_space_on_start());
        assert_eq!(settings.bindings.button_for_key(egui::Key::Space), None);
        assert_eq!(
            settings.bindings.button_for_key(egui::Key::Enter),
            Some(GbButton::Start)
        );
        assert_eq!(
            settings
                .hotkeys
                .action_for(egui::Key::Space, egui::Modifiers::NONE),
            Some(HotkeyAction::FastForward)
        );
        assert_eq!(settings.bindings, KeyBindings::default());
    }

    #[test]
    fn space_bound_to_another_button_stays_and_fast_forward_gets_no_key() {
        use crate::app::ui_state::hotkeys::HotkeyAction;
        let mut bindings = KeyBindings::default();
        bindings.bind_key(egui::Key::Space, GbButton::A);
        let mut settings = from_before_fast_forward(bindings);
        assert_eq!(
            settings.bindings.button_for_key(egui::Key::Space),
            Some(GbButton::A)
        );
        assert!(
            settings
                .hotkeys
                .keys_for(HotkeyAction::FastForward)
                .is_empty()
        );
        settings.sanitize();
        assert!(
            settings
                .hotkeys
                .keys_for(HotkeyAction::FastForward)
                .is_empty()
        );
    }

    #[test]
    fn space_given_back_to_start_after_the_update_stays_on_start() {
        let mut settings = from_before_fast_forward(with_space_on_start());
        settings
            .bindings
            .bind_key(egui::Key::Space, GbButton::Start);
        settings.sanitize();
        assert_eq!(
            settings.bindings.button_for_key(egui::Key::Space),
            Some(GbButton::Start)
        );
    }

    #[test]
    fn the_match_line_names_the_screen_rate_only_when_it_cannot_match() {
        let line = match_screen_line(Some(ScreenMatch::Unmatched(139.2))).unwrap();
        let expected = if cfg!(target_os = "macos") {
            "Your screen runs at 139 Hz, which games can't match. Set it to ProMotion, 120 or 60 Hz in System Settings, Displays.".to_string()
        } else {
            format!(
                "Your screen runs at 139 Hz, which games can't match. Set it to 60 or 120 Hz in {DISPLAY_SETTINGS}."
            )
        };
        assert_eq!(line, expected);
        for status in [
            ScreenMatch::Off,
            ScreenMatch::Shared,
            ScreenMatch::Measuring,
            ScreenMatch::Matched,
        ] {
            assert_eq!(match_screen_line(Some(status)), None, "{status:?}");
        }
        assert_eq!(match_screen_line(None), None);
    }

    fn blank_game(title: u8) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0134] = title;
        Rom::new(&data)
    }

    fn shown_frame(settings: &mut Settings, emulator: &mut Emulator) {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            settings.apply(ctx, &mut None, emulator, false);
        });
    }

    fn start(settings: &mut Settings, emulator: &mut Emulator, rom: &Rom) {
        emulator
            .load_rom(rom, GbModel::Dmg, None)
            .expect("the game loads");
        shown_frame(settings, emulator);
    }

    #[test]
    fn a_games_own_values_reach_the_emulator_and_skip_other_games() {
        use crate::app::ui_state::ds_3d::{DistantTextures, TextureFilter};
        use crate::app::ui_state::per_game::Scope;
        let seen = |emulator: &Emulator| {
            (
                emulator.ds_3d.texture_filter,
                emulator.ds_3d.distant_textures,
                emulator.sound.mine,
                emulator.scaling,
                emulator.same_size_pixels,
                emulator.gb().apu.channel_enabled(1),
            )
        };
        let all = Settings::default();
        let for_all = (
            all.ds_3d.texture_filter,
            all.ds_3d.distant_textures,
            level(all.volume, false),
            all.scaling,
            all.same_size_pixels,
            true,
        );
        let pinned = (
            TextureFilter::Nearest,
            DistantTextures::Off,
            level(0.8, false),
            ScalingMode::Stretch,
            true,
            false,
        );
        assert_ne!(pinned.0, for_all.0);
        assert_ne!(pinned.1, for_all.1);
        assert_ne!(pinned.3, for_all.3);

        let mut emulator = Emulator::default();
        let mut settings = Settings::default();
        let (first, second) = (blank_game(1), blank_game(2));
        start(&mut settings, &mut emulator, &first);
        assert_eq!(seen(&emulator), for_all);

        settings.set_scope(Scope::ThisGame);
        let frame = settings.begin_scope(BTreeMap::new());
        settings.ds_3d.texture_filter = TextureFilter::Nearest;
        settings.ds_3d.distant_textures = DistantTextures::Off;
        settings.volume = 0.8;
        settings.scaling = ScalingMode::Stretch;
        settings.same_size_pixels = true;
        settings.channels[1] = false;
        settings.dirty = true;
        settings.end_scope(frame);
        shown_frame(&mut settings, &mut emulator);
        assert_eq!(seen(&emulator), pinned, "a pin set mid-game");
        for row in [
            RowId::TextureFilter,
            RowId::DistantTextures,
            RowId::Volume,
            RowId::Scaling,
            RowId::SameSizePixels,
            RowId::GbPulse2,
        ] {
            assert!(settings.owns(row, Surface::Desktop), "{row:?}");
        }

        start(&mut settings, &mut emulator, &second);
        assert_eq!(seen(&emulator), for_all, "another game");
        assert_eq!(settings.volume, all.volume);
        assert_eq!(settings.ds_3d, all.ds_3d);

        start(&mut settings, &mut emulator, &first);
        assert_eq!(seen(&emulator), pinned, "the game again");

        settings.drop_row(RowId::TextureFilter, Surface::Desktop);
        shown_frame(&mut settings, &mut emulator);
        assert_eq!(emulator.ds_3d.texture_filter, for_all.0);
        assert_eq!(emulator.ds_3d.distant_textures, pinned.1);
        assert_eq!(emulator.sound.mine, pinned.2);

        let saved = serde_json::to_value(&settings).expect("settings save");
        assert_eq!(saved["volume"], serde_json::json!(all.volume));
        assert_eq!(saved["ds_3d"], serde_json::to_value(all.ds_3d).unwrap());
    }
}
