use crate::app::ui_state::keybinds::{GbButton, KeyBindings, key_label};
use crate::held::{Change, HeldButtons, InputSource};
use crate::storage::{SaveStore, now_unix};
use citrine_gb::error::GbResult;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::netplay::LocalCable;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

const SAVE_COOLDOWN: Duration = Duration::from_secs(5);
const PLAYED_FOR_AUTOSAVE: Duration = Duration::from_secs(5);
const QUIET: f32 = 0.35;
const MAX_QUEUED: usize = 8_192;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Game {
    #[default]
    First,
    Second,
}

impl Game {
    pub fn other(self) -> Self {
        match self {
            Game::First => Game::Second,
            Game::Second => Game::First,
        }
    }
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    strum_macros::EnumIter,
)]
pub enum Controls {
    #[default]
    Shared,
    KeyboardAndController,
    TwoControllers,
    SplitKeyboard,
}

impl Controls {
    pub fn label(self) -> &'static str {
        match self {
            Controls::Shared => "One for both",
            Controls::KeyboardAndController => "Keyboard and controller",
            Controls::TwoControllers => "Two controllers",
            Controls::SplitKeyboard => "Keyboard split in two",
        }
    }

    pub fn sides(self) -> Option<[&'static str; 2]> {
        match self {
            Controls::Shared => None,
            Controls::KeyboardAndController => Some(["Keyboard", "Controller"]),
            Controls::TwoControllers => Some(["Controller 1", "Controller 2"]),
            Controls::SplitKeyboard => Some(["Left keys", "Right keys"]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Half {
    Left,
    Right,
}

impl Half {
    pub const BOTH: [Half; 2] = [Half::Left, Half::Right];

    pub fn label(self) -> &'static str {
        match self {
            Half::Left => "Left keys",
            Half::Right => "Right keys",
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LinkSettings {
    pub controls: Controls,
    pub swapped: bool,
    pub second_sound: bool,
    left_keys: Vec<(egui::Key, GbButton)>,
    right_keys: Vec<(egui::Key, GbButton)>,
}

impl Default for LinkSettings {
    fn default() -> Self {
        use egui::Key as K;
        Self {
            controls: Controls::default(),
            swapped: false,
            second_sound: true,
            left_keys: vec![
                (K::W, GbButton::Up),
                (K::S, GbButton::Down),
                (K::A, GbButton::Left),
                (K::D, GbButton::Right),
                (K::G, GbButton::A),
                (K::F, GbButton::B),
                (K::Num2, GbButton::Start),
                (K::Num1, GbButton::Select),
            ],
            right_keys: vec![
                (K::ArrowUp, GbButton::Up),
                (K::ArrowDown, GbButton::Down),
                (K::ArrowLeft, GbButton::Left),
                (K::ArrowRight, GbButton::Right),
                (K::L, GbButton::A),
                (K::K, GbButton::B),
                (K::Enter, GbButton::Start),
                (K::Backspace, GbButton::Select),
            ],
        }
    }
}

impl LinkSettings {
    fn keys(&self, half: Half) -> &[(egui::Key, GbButton)] {
        match half {
            Half::Left => &self.left_keys,
            Half::Right => &self.right_keys,
        }
    }

    fn keys_mut(&mut self, half: Half) -> &mut Vec<(egui::Key, GbButton)> {
        match half {
            Half::Left => &mut self.left_keys,
            Half::Right => &mut self.right_keys,
        }
    }

    pub fn keys_for(&self, half: Half, button: GbButton) -> Vec<egui::Key> {
        self.keys(half)
            .iter()
            .filter(|(_, bound)| *bound == button)
            .map(|(key, _)| *key)
            .collect()
    }

    pub fn bind_key(&mut self, half: Half, key: egui::Key, button: GbButton) {
        self.left_keys.retain(|(bound, _)| *bound != key);
        self.right_keys.retain(|(bound, _)| *bound != key);
        self.keys_mut(half).push((key, button));
    }

    pub fn unbind_key(&mut self, half: Half, key: egui::Key) {
        self.keys_mut(half).retain(|(bound, _)| *bound != key);
    }

    pub fn reset_keys(&mut self) {
        let fresh = Self::default();
        self.left_keys = fresh.left_keys;
        self.right_keys = fresh.right_keys;
    }

    pub fn keys_are_default(&self) -> bool {
        let fresh = Self::default();
        self.left_keys == fresh.left_keys && self.right_keys == fresh.right_keys
    }

    pub fn key_names(&self, half: Half) -> String {
        let dpad = [
            GbButton::Up,
            GbButton::Left,
            GbButton::Down,
            GbButton::Right,
        ];
        let mut parts = Vec::new();
        let moves: Vec<String> = dpad
            .iter()
            .filter_map(|b| self.keys_for(half, *b).first().map(|k| key_label(*k)))
            .collect();
        if !moves.is_empty() {
            parts.push(moves.join(""));
        }
        for button in [GbButton::A, GbButton::B] {
            if let Some(key) = self.keys_for(half, button).first() {
                parts.push(format!("{button} {}", key_label(*key)));
            }
        }
        parts.join(", ")
    }

    fn half_for(&self, key: egui::Key) -> Option<(Half, GbButton)> {
        Half::BOTH.into_iter().find_map(|half| {
            self.keys(half)
                .iter()
                .find(|(bound, _)| *bound == key)
                .map(|(_, button)| (half, *button))
        })
    }

    pub fn game_of_side(&self, side: usize) -> Game {
        match (side == 0) != self.swapped {
            true => Game::First,
            false => Game::Second,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Pad(usize),
    Touch,
}

pub struct LocalLink {
    pub gb: GameBoy,
    cable: LocalCable,
    held: HeldButtons,
    control: Game,
    pads: Vec<usize>,
    rom_key: String,
    save_key: String,
    title: String,
    settings: LinkSettings,
    queued: VecDeque<f32>,
    last_save: Option<Instant>,
    started_at: Instant,
}

impl LocalLink {
    pub fn start(
        rom: &Rom,
        model: GbModel,
        title: String,
        store: &SaveStore,
        sample_rate: u32,
        first_key: Option<&str>,
    ) -> GbResult<Self> {
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom_with_model(rom, model)?;
        gb.apu.set_sample_rate(sample_rate);
        let rom_key = gb.cartridge.header.sha256_hex_string();
        let save_key = if first_key == Some(rom_key.as_str()) {
            format!("{rom_key}-link2")
        } else {
            rom_key.clone()
        };
        if let Some(data) = store.load_battery(&save_key) {
            gb.put_sram_dump(SramDump::from_slice(&data), now_unix());
        }
        let mut cable = LocalCable::new();
        cable.set_plugged(true);
        Ok(Self {
            gb,
            cable,
            held: HeldButtons::default(),
            control: Game::First,
            pads: Vec::new(),
            rom_key,
            save_key,
            title,
            settings: LinkSettings::default(),
            queued: VecDeque::new(),
            last_save: None,
            started_at: Instant::now(),
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn rom_key(&self) -> &str {
        &self.rom_key
    }

    #[cfg(test)]
    pub fn save_key(&self) -> &str {
        &self.save_key
    }

    pub fn model(&self) -> GbModel {
        self.gb.model
    }

    pub fn plugged(&self) -> bool {
        self.cable.plugged()
    }

    pub fn set_plugged(&mut self, plugged: bool) {
        self.cable.set_plugged(plugged);
    }

    pub fn control(&self) -> Game {
        self.control
    }

    pub fn settings(&self) -> &LinkSettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: &LinkSettings) {
        if self.settings.controls != settings.controls {
            self.pads.clear();
        }
        self.settings = settings.clone();
    }

    pub fn set_control(&mut self, game: Game) -> bool {
        let moved = self.control != game;
        self.control = game;
        moved
    }

    pub fn held(&self) -> JoypadState {
        self.held.held()
    }

    pub fn held_of(&self, source: InputSource) -> JoypadState {
        self.held.of(source)
    }

    pub fn set_held(&mut self, source: InputSource, mask: JoypadState, down: JoypadState) {
        self.held.set(source, mask, down);
    }

    pub fn forget_pad(&mut self, pad: usize) {
        self.held.forget_pad(pad);
        self.pads.retain(|p| *p != pad);
    }

    pub fn release_all(&mut self) -> Change {
        self.held.clear()
    }

    pub fn run_frame(&mut self, first: &mut GameBoy, first_buttons: JoypadState) {
        self.cable
            .run_frame([first, &mut self.gb], [first_buttons, self.held.held()]);
        self.held.take_owed();
    }

    pub fn unplug(&mut self, first: &mut GameBoy) {
        self.cable.unplug([first, &mut self.gb]);
    }

    pub fn key(&self, key: egui::Key, bindings: &KeyBindings) -> Option<(Game, GbButton)> {
        if self.settings.controls == Controls::SplitKeyboard {
            let (half, button) = self.settings.half_for(key)?;
            let side = match half {
                Half::Left => 0,
                Half::Right => 1,
            };
            return Some((self.settings.game_of_side(side), button));
        }
        let button = bindings.button_for_key(key)?;
        let game = match self.settings.controls {
            Controls::KeyboardAndController => self.settings.game_of_side(0),
            _ => self.control,
        };
        Some((game, button))
    }

    pub fn game_for(&mut self, source: Source) -> Game {
        match (self.settings.controls, source) {
            (Controls::KeyboardAndController, Source::Pad(_)) => self.settings.game_of_side(1),
            (Controls::TwoControllers, Source::Pad(pad)) => {
                if !self.pads.contains(&pad) && self.pads.len() < 2 {
                    self.pads.push(pad);
                }
                match self.pads.iter().position(|p| *p == pad) {
                    Some(side) => self.settings.game_of_side(side),
                    None => self.control,
                }
            }
            _ => self.control,
        }
    }

    pub fn takes_clicks(&self) -> bool {
        self.settings.controls == Controls::Shared
    }

    pub fn driver(&self, game: Game) -> Option<&'static str> {
        match self.settings.controls.sides() {
            None => (self.control == game).then_some("Playing here"),
            Some(sides) => {
                let side = usize::from(self.settings.game_of_side(0) != game);
                Some(sides[side])
            }
        }
    }

    pub fn mix(&mut self, samples: &mut [f32]) {
        let mut fresh = std::mem::take(&mut self.gb.apu.audio_buffer);
        if self.settings.second_sound {
            self.queued.extend(fresh.drain(..));
        } else {
            fresh.clear();
            self.queued.clear();
        }
        self.gb.apu.audio_buffer = fresh;
        if !self.settings.second_sound {
            return;
        }
        for sample in samples.iter_mut() {
            let Some(other) = self.queued.pop_front() else {
                break;
            };
            *sample = (*sample + other * QUIET).clamp(-1.0, 1.0);
        }
        while self.queued.len() > MAX_QUEUED {
            self.queued.pop_front();
            self.queued.pop_front();
        }
    }

    pub fn clear_audio(&mut self) {
        self.gb.apu.audio_buffer.clear();
        self.queued.clear();
    }

    pub fn handle_save(&mut self, store: &SaveStore) -> GbResult<()> {
        if self
            .last_save
            .is_some_and(|at| at.elapsed() < SAVE_COOLDOWN)
        {
            return Ok(());
        }
        self.flush_save(store)
    }

    pub fn flush_save(&mut self, store: &SaveStore) -> GbResult<()> {
        let Some(dump) = self.gb.poll_sram_dump(false, now_unix()) else {
            return Ok(());
        };
        store.store_battery(&self.save_key, dump.as_slice())?;
        self.last_save = Some(Instant::now());
        Ok(())
    }

    pub fn finish(mut self, first: &mut GameBoy, store: &SaveStore, keep: usize) -> GbResult<()> {
        self.unplug(first);
        self.flush_save(store)?;
        if self.started_at.elapsed() >= PLAYED_FOR_AUTOSAVE {
            let state = self.gb.dump_full(now_unix())?;
            let frame = self.gb.frame().as_slice().to_vec();
            store.store_autosave(&self.save_key, &state, &frame, self.gb.model, keep)?;
        }
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
