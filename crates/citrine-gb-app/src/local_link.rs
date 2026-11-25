use crate::app::ui_state::keybinds::{GameButton, GbButton, KeyBindings, key_label};
use crate::combo::{Bound, Combo};
use crate::held::{Change, HeldButtons, InputSource};
use crate::storage::{SaveStore, Write, Writes, now_unix};
use citrine_gb::console::{Console, ConsoleKind, Keys};
use citrine_gb::error::GbResult;
use citrine_gb::gb::GameBoy;
#[cfg(feature = "gba")]
use citrine_gb::gba::machine::Gba;
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::netplay::{LinkPort, LocalCable};
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

enum Cable {
    Gb(LocalCable<GameBoy>),
    #[cfg(feature = "gba")]
    Gba(LocalCable<Gba>),
}

impl Cable {
    fn new(kind: ConsoleKind) -> Self {
        let mut cable = match kind {
            #[cfg(feature = "gba")]
            ConsoleKind::Gba => Self::Gba(LocalCable::new()),
            _ => Self::Gb(LocalCable::new()),
        };
        cable.set_plugged(true);
        cable
    }

    fn plugged(&self) -> bool {
        match self {
            Self::Gb(cable) => cable.plugged(),
            #[cfg(feature = "gba")]
            Self::Gba(cable) => cable.plugged(),
        }
    }

    fn set_plugged(&mut self, plugged: bool) {
        match self {
            Self::Gb(cable) => cable.set_plugged(plugged),
            #[cfg(feature = "gba")]
            Self::Gba(cable) => cable.set_plugged(plugged),
        }
    }
}

pub struct LocalLink {
    pub machine: Machine,
    cable: Cable,
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
        kind: ConsoleKind,
        firmware: &Firmware,
        file_name: Option<&str>,
        store: &SaveStore,
        sample_rate: u32,
        first_key: Option<&str>,
    ) -> GbResult<Self> {
        let mut machine = Machine::load(rom, kind, firmware)?;
        machine.set_sample_rate(sample_rate);
        let id = machine.rom_id();
        let rom_key = id.sha256_hex_string();
        let title = crate::game_name::game_name(&id.sha1, file_name, &machine.title());
        let save_key = if first_key == Some(rom_key.as_str()) {
            format!("{rom_key}-link2")
        } else {
            rom_key.clone()
        };
        if let Some(save) = store.load_battery_save(&save_key, &machine) {
            machine.put_battery(save, now_unix());
        }
        Ok(Self {
            machine,
            cable: Cable::new(kind),
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

    pub fn kind(&self) -> ConsoleKind {
        self.machine.kind()
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

    pub fn held(&self) -> Keys {
        self.held.held()
    }

    pub fn held_of(&self, source: InputSource) -> Keys {
        self.held.of(source)
    }

    pub fn set_held(&mut self, source: InputSource, mask: Keys, down: Keys) {
        self.held.set(source, mask, down);
    }

    pub fn forget_pad(&mut self, pad: usize) {
        self.held.forget_pad(pad);
        self.pads.retain(|p| *p != pad);
    }

    pub fn release_all(&mut self) -> Change {
        self.held.clear()
    }

    pub fn run_frame(&mut self, first: &mut Machine, first_buttons: Keys) {
        let keys = [first_buttons, self.held.held()];
        match (&mut self.cable, first, &mut self.machine) {
            (Cable::Gb(cable), Machine::Gb(first), Machine::Gb(second)) => {
                if !cable.plugged() && first.has_ir() && second.has_ir() {
                    if first.link_plugged() || second.link_plugged() {
                        cable.unplug([&mut **first, &mut **second]);
                    }
                    infrared_frame([&mut **first, &mut **second], keys);
                } else {
                    cable.run_frame([&mut **first, &mut **second], keys);
                }
            }
            #[cfg(feature = "gba")]
            (Cable::Gba(cable), Machine::Gba(first), Machine::Gba(second)) => {
                cable.run_frame([&mut **first, &mut **second], keys);
            }
            #[cfg(feature = "gba")]
            (_, first, second) => {
                for (console, keys) in [first, second].into_iter().zip(keys) {
                    console.set_keys(keys);
                    console.run_frame();
                }
            }
        }
        self.held.take_owed();
    }

    pub fn unplug(&mut self, first: &mut Machine) {
        match (&mut self.cable, first, &mut self.machine) {
            (Cable::Gb(cable), Machine::Gb(first), Machine::Gb(second)) => {
                cable.unplug([&mut **first, &mut **second]);
            }
            #[cfg(feature = "gba")]
            (Cable::Gba(cable), Machine::Gba(first), Machine::Gba(second)) => {
                cable.unplug([&mut **first, &mut **second]);
            }
            #[cfg(feature = "gba")]
            (cable, _, _) => cable.set_plugged(false),
        }
    }

    pub fn key_table(&self, bindings: &KeyBindings) -> Vec<Bound<egui::Key, (Game, GameButton)>> {
        if self.settings.controls == Controls::SplitKeyboard {
            return Half::BOTH
                .into_iter()
                .enumerate()
                .flat_map(|(side, half)| {
                    let game = self.settings.game_of_side(side);
                    self.settings.keys(half).iter().map(move |(key, button)| {
                        Bound::any_mods(Combo::single(*key), (game, GameButton::Button(*button)))
                    })
                })
                .collect();
        }
        let game = match self.settings.controls {
            Controls::KeyboardAndController => self.settings.game_of_side(0),
            _ => self.control,
        };
        bindings
            .key_table(false)
            .into_iter()
            .filter(|bound| matches!(bound.target, GameButton::Button(_)))
            .map(|bound| bound.map(|button| (game, button)))
            .collect()
    }

    #[cfg(test)]
    pub fn key(&self, key: egui::Key, bindings: &KeyBindings) -> Option<(Game, GbButton)> {
        self.key_table(bindings)
            .into_iter()
            .find(|bound| bound.combo == Combo::single(key))
            .and_then(|bound| match bound.target {
                (game, GameButton::Button(button)) => Some((game, button)),
                _ => None,
            })
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
        let mut fresh = Vec::new();
        self.machine.take_audio(&mut fresh);
        if self.settings.second_sound {
            self.queued.extend(fresh);
        } else {
            self.queued.clear();
        }
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
        self.machine.take_audio(&mut Vec::new());
        self.queued.clear();
    }

    pub fn handle_save(&mut self, store: &SaveStore, writes: &mut Writes) -> GbResult<()> {
        if self
            .last_save
            .is_some_and(|at| at.elapsed() < SAVE_COOLDOWN)
        {
            return Ok(());
        }
        self.flush_save(store, writes)
    }

    pub fn flush_save(&mut self, store: &SaveStore, writes: &mut Writes) -> GbResult<()> {
        let Some(save) = self.machine.battery(false, now_unix()) else {
            return Ok(());
        };
        writes.send(Write::Battery {
            store: store.clone(),
            key: self.save_key.clone(),
            save,
        });
        self.last_save = Some(Instant::now());
        Ok(())
    }

    pub fn finish(
        mut self,
        first: &mut Machine,
        store: &SaveStore,
        writes: &mut Writes,
        keep: usize,
    ) -> GbResult<()> {
        self.unplug(first);
        self.flush_save(store, writes)?;
        if self.started_at.elapsed() >= PLAYED_FOR_AUTOSAVE {
            writes.send(Write::Autosave {
                store: store.clone(),
                key: self.save_key.clone(),
                state: self.machine.state_body(now_unix())?,
                frame: self.machine.frame().to_vec(),
                keep,
            });
        }
        Ok(())
    }
}

pub fn infrared_frame(consoles: [&mut GameBoy; 2], keys: [Keys; 2]) {
    let mut consoles = consoles;
    for (gb, keys) in consoles.iter_mut().zip(keys) {
        gb.set_keys(keys);
        gb.begin_linked_frame();
    }
    let mut clocks = [0u64; 2];
    let mut done = [false; 2];
    while done != [true; 2] {
        let i = match done {
            [false, true] => 0,
            [true, false] => 1,
            _ => usize::from(clocks[1] < clocks[0]),
        };
        clocks[i] += consoles[i].step_linked();
        let led = consoles[i].ir_led();
        consoles[1 - i].set_ir_light(led);
        if consoles[i].linked_frame_done() {
            consoles[i].end_linked_frame();
            done[i] = true;
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::emulator::Emulator;
    use citrine_gb::ReadMemory;
    use citrine_gb::gb::GbModel;
    use egui::Key;
    use std::collections::HashSet;
    use strum::IntoEnumIterator;

    const COUNT: u16 = 300;

    fn trade_program(sc: u8, first: u8, step: u8) -> Vec<u8> {
        let [low, high] = COUNT.to_le_bytes();
        let mut code = vec![0x3E, 0x0A, 0xEA, 0x00, 0x00, 0xAF, 0xEA, 0x00, 0xDF];
        code.extend_from_slice(&[0x3E, 0x10, 0xE0, 0x00, 0xF0, 0x00, 0xCB, 0x47, 0x20, 0xFA]);
        code.extend_from_slice(&[0x21, 0x00, 0xA0, 0x11, low, high, 0x06, first]);
        let top = code.len();
        code.extend_from_slice(&[0x78, 0xE0, 0x01]);
        if sc == 0x81 {
            code.extend_from_slice(&[0x0E, 0x28, 0x0D, 0x20, 0xFD]);
        }
        code.extend_from_slice(&[0x3E, sc, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA]);
        code.extend_from_slice(&[0xF0, 0x01, 0x22, 0x78, 0xC6, step, 0x47]);
        code.extend_from_slice(&[0x1B, 0x7A, 0xB3, 0x20]);
        let back = top as i32 - (code.len() as i32 + 1);
        code.push(back as i8 as u8);
        code.extend_from_slice(&[0x3E, 0xAA, 0xEA, 0x00, 0xDF]);
        code
    }

    fn cart(cgb_flag: u8, kind: u8, code: &[u8]) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        data[0x0143] = cgb_flag;
        data[0x0147] = kind;
        data[0x0149] = 0x02;
        data[0x0150..0x0150 + code.len()].copy_from_slice(code);
        let end = 0x0150 + code.len();
        data[end..end + 2].copy_from_slice(&[0x18, 0xFE]);
        Rom::new(&data)
    }

    fn red() -> Rom {
        cart(0x00, 0x03, &trade_program(0x81, 0x10, 1))
    }

    fn silver() -> Rom {
        cart(0x80, 0x10, &trade_program(0x80, 0x40, 3))
    }

    fn sent(first: u8, step: u8) -> Vec<u8> {
        (0..COUNT)
            .map(|i| first.wrapping_add((i as u8).wrapping_mul(step)))
            .collect()
    }

    fn done(gb: &GameBoy) -> bool {
        gb.memory.read_naive(0xDF00) == 0xAA
    }

    fn emulator(name: &str) -> Emulator {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-link-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut emulator = Emulator::default();
        emulator.store = SaveStore::at(root);
        emulator
    }

    fn linked(name: &str) -> Emulator {
        let mut emulator = emulator(name);
        emulator
            .load_rom(&red(), GbModel::Dmg, None)
            .expect("load red");
        emulator
            .start_link(&silver(), ConsoleKind::Gbc, Some("Silver"))
            .expect("link silver");
        emulator
    }

    fn link(emulator: &mut Emulator) -> &mut LocalLink {
        emulator.link_mut().expect("a linked game")
    }

    fn second(link: &mut LocalLink) -> &mut GameBoy {
        link.machine.as_gb_mut().expect("a game boy")
    }

    fn trade(emulator: &mut Emulator) {
        let start = Instant::now();
        let finished = (1..=200).any(|step| {
            emulator.advance(start + Duration::from_millis(step * 50), None);
            let second = emulator
                .link()
                .and_then(|link| link.machine.as_gb())
                .expect("a linked game");
            done(emulator.gb()) && done(second)
        });
        assert!(finished, "the trade did not finish in 10 seconds");
    }

    fn press_a_on_both(emulator: &mut Emulator) {
        emulator.press(InputSource::Keyboard, Keys::A);
        link(emulator).set_held(InputSource::Keyboard, Keys::A, Keys::A);
    }

    fn store_holds(emulator: &Emulator, key: &str, expected: &[u8]) {
        let battery = emulator.store.load_battery(key).expect("a battery save");
        assert_eq!(&battery[..COUNT as usize], expected);
    }

    #[test]
    fn red_and_silver_trade_and_both_saves_hold_the_other_game_after_the_link_ends() {
        let mut emulator = linked("trade");
        assert_eq!(emulator.gb().model, GbModel::Dmg);
        assert_eq!(link(&mut emulator).kind(), ConsoleKind::Gbc);
        assert!(link(&mut emulator).plugged());
        press_a_on_both(&mut emulator);
        trade(&mut emulator);

        let red_key = emulator.rom_key().expect("red key").to_owned();
        let silver_key = link(&mut emulator).rom_key().to_owned();
        link(&mut emulator).started_at -= Duration::from_secs(10);
        emulator.end_link().expect("end the link");
        emulator.flush_save().expect("save red");

        assert!(!emulator.linked());
        assert!(!emulator.gb().link_plugged());
        store_holds(&emulator, &red_key, &sent(0x40, 3));
        store_holds(&emulator, &silver_key, &sent(0x10, 1));
        assert_eq!(emulator.store.autosave_ids(&silver_key).len(), 1);
    }

    #[test]
    fn the_second_game_loads_its_battery_save_like_it_does_alone() {
        let mut emulator = linked("battery");
        let silver_key = link(&mut emulator).rom_key().to_owned();
        emulator.end_link().expect("end the link");
        let saved: Vec<u8> = (0..0x2000).map(|i| (i % 251) as u8).collect();
        emulator
            .store
            .store_battery(&silver_key, &saved)
            .expect("store");
        emulator
            .start_link(&silver(), ConsoleKind::Gbc, Some("Silver"))
            .expect("link again");
        let dump = second(link(&mut emulator))
            .poll_sram_dump(true, now_unix())
            .expect("a battery save");
        assert_eq!(&dump.as_slice()[..saved.len()], &saved[..]);
    }

    #[test]
    fn a_copy_of_the_same_game_keeps_its_own_save_and_leaves_the_first_one_alone() {
        let mut emulator = emulator("copy");
        emulator
            .load_rom(&red(), GbModel::Dmg, None)
            .expect("load red");
        let red_key = emulator.rom_key().expect("red key").to_owned();
        let saved: Vec<u8> = (0..0x2000).map(|i| (i % 251) as u8).collect();
        emulator
            .store
            .store_battery(&red_key, &saved)
            .expect("store");
        emulator
            .start_link(&red(), ConsoleKind::Gb, Some("Red"))
            .expect("link a copy");
        assert_eq!(link(&mut emulator).rom_key(), red_key);
        let copy_key = link(&mut emulator).save_key().to_owned();
        assert_ne!(copy_key, red_key);
        let dump = second(link(&mut emulator)).poll_sram_dump(true, now_unix());
        assert!(dump.is_none_or(|d| d.as_slice()[..saved.len()] != saved[..]));
        link(&mut emulator).started_at -= Duration::from_secs(10);
        emulator.end_link().expect("end the link");
        assert_eq!(emulator.store.load_battery(&red_key), Some(saved));
    }

    #[test]
    fn with_the_cable_pulled_the_first_game_trades_with_no_one() {
        let mut emulator = linked("cable");
        link(&mut emulator).set_plugged(false);
        press_a_on_both(&mut emulator);
        let start = Instant::now();
        for step in 1..=40 {
            emulator.advance(start + Duration::from_millis(step * 50), None);
        }
        assert!(done(emulator.gb()));
        assert!(!done(second(link(&mut emulator))));
        assert!(!emulator.gb().link_plugged());
    }

    #[test]
    fn closing_the_second_game_keeps_the_first_running_unplugged() {
        let mut emulator = linked("close");
        let red_key = emulator.rom_key().expect("red key").to_owned();
        emulator.end_link().expect("end the link");
        assert_eq!(emulator.rom_key(), Some(red_key.as_str()));
        assert!(emulator.running);
        assert!(!emulator.gb().link_plugged());
    }

    #[test]
    fn the_split_keyboard_halves_cover_every_button_without_sharing_a_key() {
        let settings = LinkSettings::default();
        let mut seen = HashSet::new();
        for half in Half::BOTH {
            for button in GbButton::iter() {
                let keys = settings.keys_for(half, button);
                assert!(!keys.is_empty(), "{} has no {button}", half.label());
                for key in keys {
                    assert!(seen.insert(key), "{key:?} is in both halves");
                }
            }
        }
    }

    #[test]
    fn binding_a_key_to_one_half_takes_it_from_the_other() {
        let mut settings = LinkSettings::default();
        settings.bind_key(Half::Right, Key::W, GbButton::B);
        assert!(settings.keys_for(Half::Left, GbButton::Up).is_empty());
        assert_eq!(
            settings.keys_for(Half::Right, GbButton::B),
            [Key::K, Key::W]
        );
        assert!(!settings.keys_are_default());
        settings.reset_keys();
        assert!(settings.keys_are_default());
    }

    fn link_with(controls: Controls, swapped: bool) -> LocalLink {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-link-routing-{}", std::process::id()));
        let store = SaveStore::at(root);
        let mut link = LocalLink::start(
            &silver(),
            ConsoleKind::Gbc,
            &Firmware::default(),
            Some("Silver"),
            &store,
            48_000,
            None,
        )
        .expect("start");
        link.set_settings(&LinkSettings {
            controls,
            swapped,
            ..LinkSettings::default()
        });
        link
    }

    #[test]
    fn a_split_keyboard_sends_each_half_to_its_own_game() {
        let bindings = KeyBindings::default();
        let link = link_with(Controls::SplitKeyboard, false);
        assert_eq!(
            link.key(Key::W, &bindings),
            Some((Game::First, GbButton::Up))
        );
        assert_eq!(
            link.key(Key::L, &bindings),
            Some((Game::Second, GbButton::A))
        );
        assert_eq!(link.key(Key::Q, &bindings), None);
        let swapped = link_with(Controls::SplitKeyboard, true);
        assert_eq!(
            swapped.key(Key::W, &bindings),
            Some((Game::Second, GbButton::Up))
        );
    }

    #[test]
    fn keyboard_and_controller_each_drive_one_game() {
        let bindings = KeyBindings::default();
        let key = bindings.keys_for(GbButton::A)[0];
        let mut link = link_with(Controls::KeyboardAndController, false);
        assert_eq!(link.key(key, &bindings), Some((Game::First, GbButton::A)));
        assert_eq!(link.game_for(Source::Pad(3)), Game::Second);
        assert_eq!(link.game_for(Source::Touch), Game::First);
        assert_eq!(link.driver(Game::First), Some("Keyboard"));
        assert_eq!(link.driver(Game::Second), Some("Controller"));
    }

    #[test]
    fn two_controllers_take_the_games_in_the_order_they_are_first_used() {
        let mut link = link_with(Controls::TwoControllers, false);
        assert_eq!(link.game_for(Source::Pad(5)), Game::First);
        assert_eq!(link.game_for(Source::Pad(2)), Game::Second);
        assert_eq!(link.game_for(Source::Pad(5)), Game::First);
        assert_eq!(link.game_for(Source::Pad(7)), Game::First);
    }

    #[test]
    fn one_input_for_both_follows_the_game_with_control() {
        let bindings = KeyBindings::default();
        let key = bindings.keys_for(GbButton::A)[0];
        let mut link = link_with(Controls::Shared, false);
        assert!(link.takes_clicks());
        assert_eq!(link.driver(Game::First), Some("Playing here"));
        assert_eq!(link.driver(Game::Second), None);
        assert!(link.set_control(Game::Second));
        assert!(!link.set_control(Game::Second));
        assert_eq!(link.key(key, &bindings), Some((Game::Second, GbButton::A)));
        assert_eq!(link.game_for(Source::Pad(0)), Game::Second);
        assert_eq!(link.game_for(Source::Touch), Game::Second);
        assert_eq!(link.driver(Game::Second), Some("Playing here"));
    }

    #[test]
    fn moving_control_lets_go_of_what_the_old_game_held() {
        let mut emulator = linked("release");
        emulator.press(InputSource::Keyboard, Keys::A);
        assert!(emulator.is_held(Keys::A));
        emulator.set_link_control(Game::Second);
        assert!(!emulator.is_held(Keys::A));
        emulator.press(InputSource::Touch, Keys::B);
        assert!(link(&mut emulator).held().contains(Keys::B));
        assert!(!emulator.is_held(Keys::B));
    }

    #[test]
    fn the_second_sound_mixes_in_quieter_or_not_at_all() {
        let mut link = link_with(Controls::Shared, false);
        second(&mut link).apu.audio_buffer = vec![1.0; 8];
        let mut samples = vec![0.0; 8];
        link.mix(&mut samples);
        assert!(samples.iter().all(|s| *s > 0.0 && *s < 1.0));
        link.set_settings(&LinkSettings {
            second_sound: false,
            ..LinkSettings::default()
        });
        second(&mut link).apu.audio_buffer = vec![1.0; 8];
        let mut samples = vec![0.0; 8];
        link.mix(&mut samples);
        assert!(samples.iter().all(|s| *s == 0.0));
    }

    mod real_trade {
        use super::*;
        use crate::held::InputSource;
        use crate::real_roms::{rom_of, save_from};
        use citrine_gb_gamedata::{GameState, Pokemon, RomTables, Version, read_live, read_save};
        use std::path::PathBuf;

        const CUR_MAP: u16 = 0xD35E;
        const X: u16 = 0xD362;
        const Y: u16 = 0xD361;
        const PARTY: u16 = 0xD163;
        const ENEMY_PARTY: u16 = 0xD89C;
        const BLOCK: u16 = 0x194;
        const TRADE_CENTER: u8 = 0xEF;
        const FRAME: Duration = Duration::from_nanos(16_742_706);

        struct Walk {
            emulator: Emulator,
            now: Instant,
            frames: u64,
            shots: Option<PathBuf>,
        }

        impl Walk {
            fn frame(&mut self) {
                self.now += FRAME;
                self.emulator.advance(self.now, None);
                self.frames += 1;
            }

            fn frames(&mut self, n: u32) {
                for _ in 0..n {
                    self.frame();
                }
            }

            fn gb(&self, game: Game) -> &GameBoy {
                match game {
                    Game::First => self.emulator.gb(),
                    Game::Second => self
                        .emulator
                        .link()
                        .and_then(|link| link.machine.as_gb())
                        .expect("a linked game"),
                }
            }

            fn wram(&self, game: Game, addr: u16) -> u8 {
                let gb = self.gb(game);
                match addr {
                    0xC000..=0xCFFF => gb.wram_bank(0).expect("bank 0")[usize::from(addr - 0xC000)],
                    _ => gb.wram_bank(1).expect("bank 1")[usize::from(addr - 0xD000)],
                }
            }

            fn pad(game: Game) -> InputSource {
                InputSource::PadButtons(match game {
                    Game::First => 0,
                    Game::Second => 1,
                })
            }

            fn tap(&mut self, game: Game, button: Keys) {
                self.emulator.press(Self::pad(game), button);
                self.frames(6);
                self.emulator.release(Self::pad(game), button);
                self.frames(20);
            }

            fn map(&self, game: Game) -> u8 {
                self.wram(game, CUR_MAP)
            }

            fn at(&self, game: Game) -> (u8, u8) {
                (self.wram(game, X), self.wram(game, Y))
            }

            fn taps(&mut self, game: Game, button: Keys, n: u32) {
                for _ in 0..n {
                    self.tap(game, button);
                }
            }

            fn step(&mut self, game: Game, button: Keys) -> bool {
                let (map, at) = (self.map(game), self.at(game));
                self.emulator.press(Self::pad(game), button);
                for _ in 0..40 {
                    self.frame();
                    if self.map(game) != map || self.at(game) != at {
                        break;
                    }
                }
                self.emulator.release(Self::pad(game), button);
                self.frames(16);
                self.map(game) != map || self.at(game) != at
            }

            fn walk(&mut self, game: Game, button: Keys, steps: u32) {
                for i in 0..steps {
                    if !self.step(game, button) {
                        self.shot(game, &format!("blocked-{game:?}-{i}"));
                        panic!(
                            "{game:?} blocked walking {button:?} at {:?} on {:?}",
                            self.at(game),
                            self.map(game)
                        );
                    }
                }
            }

            fn report(&self, game: Game, name: &str) {
                self.shot(game, name);
                println!(
                    "{name}: frame {} map {:?} at {:?}",
                    self.frames,
                    self.map(game),
                    self.at(game)
                );
            }

            fn live(&self, game: Game, tables: &RomTables) -> Option<GameState> {
                let gb = self.gb(game);
                let wram = [
                    &gb.wram_bank(0).expect("bank 0")[..],
                    &gb.wram_bank(1).expect("bank 1")[..],
                ]
                .concat();
                read_live(tables, &wram, &gb.cartridge.battery_ram()).ok()
            }

            fn shot(&self, game: Game, name: &str) {
                if let Some(dir) = &self.shots {
                    std::fs::create_dir_all(dir).expect("shots dir");
                    std::fs::write(
                        dir.join(format!("{name}.png")),
                        self.gb(game).frame().render_png(),
                    )
                    .expect("shot");
                }
            }
        }

        fn species(mon: &Pokemon) -> u16 {
            match mon {
                Pokemon::Hatched(mon) => mon.species,
                Pokemon::Egg => 0,
            }
        }

        fn nickname(mon: &Pokemon) -> &str {
            match mon {
                Pokemon::Hatched(mon) => &mon.nickname,
                Pokemon::Egg => "egg",
            }
        }

        fn owned(state: &GameState, kind: u16) -> usize {
            state
                .party
                .iter()
                .chain(state.boxes.iter().flat_map(|b| b.pokemon.iter()))
                .filter(|mon| species(mon) == kind)
                .count()
        }

        fn counts(state: &GameState) -> (usize, usize) {
            (
                state.party.len(),
                state.boxes.iter().map(|b| b.pokemon.len()).sum(),
            )
        }

        #[test]
        #[ignore]
        fn blue_trades_with_red_over_local_link() {
            let test = "blue_trades_with_red_over_local_link";
            let Some(save) = save_from(test, "CITRINE_BLUE_SAVE") else {
                return;
            };
            let (blue_rom, blue) = rom_of(test, Version::Blue).expect("a blue rom");
            let (red_rom, red) = rom_of(test, Version::Red).expect("a red rom");
            let tables = [
                RomTables::load(blue, &blue_rom).expect("blue tables"),
                RomTables::load(red, &red_rom).expect("red tables"),
            ];
            let before = read_save(blue, &save).expect("blue save");
            println!("before {:?}", counts(&before));

            let mut emulator = emulator("real-trade");
            let blue_rom = Rom::new(&blue_rom);
            let red_rom = Rom::new(&red_rom);
            let blue_key = blue_rom.header().expect("blue header").sha256_hex_string();
            let red_key = red_rom.header().expect("red header").sha256_hex_string();
            assert_ne!(blue_key, red_key);
            emulator
                .store
                .store_battery(&blue_key, &save)
                .expect("blue battery");
            emulator
                .store
                .store_battery(&red_key, &save)
                .expect("red battery");
            emulator
                .load_rom(&blue_rom, GbModel::for_rom(&blue_rom).expect("model"), None)
                .expect("load blue");
            emulator
                .start_link(
                    &red_rom,
                    GbModel::for_rom(&red_rom).expect("model").into(),
                    Some("Red"),
                )
                .expect("link red");
            link(&mut emulator).set_settings(&LinkSettings {
                controls: Controls::TwoControllers,
                ..LinkSettings::default()
            });
            let mut walk = Walk {
                emulator,
                now: Instant::now(),
                frames: 0,
                shots: std::env::var_os("CITRINE_SHOTS").map(PathBuf::from),
            };
            walk.frames(60);
            for (game, tables) in [(Game::First, &tables[0]), (Game::Second, &tables[1])] {
                for step in 0..120 {
                    let button = if step % 2 == 0 { Keys::START } else { Keys::A };
                    walk.tap(game, button);
                    walk.frames(34);
                    if walk
                        .live(game, tables)
                        .is_some_and(|s| s.party == before.party)
                    {
                        break;
                    }
                }
                for _ in 0..3 {
                    walk.frames(120);
                    walk.tap(game, Keys::A);
                }
                walk.frames(240);
                walk.report(game, &format!("{game:?}-0-start"));

                walk.tap(game, Keys::START);
                walk.tap(game, Keys::DOWN);
                walk.tap(game, Keys::A);
                walk.frames(30);
                walk.taps(game, Keys::DOWN, 4);
                walk.tap(game, Keys::A);
                walk.tap(game, Keys::A);
                walk.frames(400);
                walk.report(game, &format!("{game:?}-1-dug"));
                walk.walk(game, Keys::UP, 1);
                walk.frames(60);
                while walk.step(game, Keys::UP) {}
                walk.walk(game, Keys::RIGHT, 8);
                walk.tap(game, Keys::UP);
                walk.report(game, &format!("{game:?}-2-desk"));
            }
            for _ in 0..160 {
                let maps = [walk.map(Game::First), walk.map(Game::Second)];
                if maps.iter().all(|&m| m == TRADE_CENTER) {
                    break;
                }
                for (game, map) in [Game::First, Game::Second].into_iter().zip(maps) {
                    if map != TRADE_CENTER {
                        walk.tap(game, Keys::A);
                    }
                }
            }
            assert_eq!(
                [walk.map(Game::First), walk.map(Game::Second)],
                [TRADE_CENTER; 2]
            );
            walk.frames(60);
            walk.report(Game::First, "First-4-trade-center");
            walk.report(Game::Second, "Second-4-trade-center");
            walk.frames(200);
            walk.tap(Game::First, Keys::RIGHT);
            walk.tap(Game::First, Keys::A);
            walk.tap(Game::Second, Keys::LEFT);
            walk.tap(Game::Second, Keys::A);
            walk.frames(400);
            walk.report(Game::First, "First-5-menu");
            walk.report(Game::Second, "Second-5-menu");
            walk.tap(Game::First, Keys::A);
            walk.frames(30);
            walk.report(Game::First, "First-6-sub");
            walk.tap(Game::First, Keys::RIGHT);
            walk.tap(Game::First, Keys::A);
            walk.taps(Game::Second, Keys::DOWN, 2);
            walk.tap(Game::Second, Keys::A);
            walk.frames(30);
            walk.report(Game::Second, "Second-6-sub");
            walk.tap(Game::Second, Keys::RIGHT);
            walk.tap(Game::Second, Keys::A);
            walk.frames(300);
            walk.report(Game::First, "First-7-confirm");
            walk.report(Game::Second, "Second-7-confirm");
            for (from, to) in [(Game::First, Game::Second), (Game::Second, Game::First)] {
                let sent: Vec<u8> = (PARTY..PARTY + BLOCK).map(|a| walk.wram(from, a)).collect();
                let got: Vec<u8> = (ENEMY_PARTY..ENEMY_PARTY + BLOCK)
                    .map(|a| walk.wram(to, a))
                    .collect();
                let changed = sent.iter().zip(&got).position(|(a, b)| a != b);
                assert_eq!(changed, None, "party block from {from:?} arrived changed");
            }
            let flareon = species(&before.party[0]);
            let blastoise = species(&before.party[2]);
            for _ in 0..2 {
                walk.tap(Game::First, Keys::A);
                walk.tap(Game::Second, Keys::A);
                walk.frames(60);
            }
            walk.report(Game::First, "First-8-chosen");
            walk.report(Game::Second, "Second-8-chosen");
            let traded = |walk: &Walk, game: Game, gone: u16, got: u16| {
                let version = match game {
                    Game::First => blue,
                    Game::Second => red,
                };
                read_save(version, &walk.gb(game).cartridge.battery_ram()).is_ok_and(|s| {
                    owned(&s, gone) == owned(&before, gone) - 1
                        && owned(&s, got) == owned(&before, got) + 1
                })
            };
            for _ in 0..200 {
                if traded(&walk, Game::First, flareon, blastoise)
                    && traded(&walk, Game::Second, blastoise, flareon)
                {
                    break;
                }
                walk.frames(60);
                for game in [Game::First, Game::Second] {
                    walk.tap(game, Keys::B);
                }
            }
            walk.frames(120);
            walk.report(Game::First, "First-9-done");
            walk.report(Game::Second, "Second-9-done");
            walk.emulator.flush_save().expect("flush");
            walk.emulator.end_link().expect("end link");
            walk.emulator.flush_save().expect("flush");
            let blue_after = read_save(
                blue,
                &walk
                    .emulator
                    .store
                    .load_battery(&blue_key)
                    .expect("blue battery"),
            )
            .expect("blue after");
            let red_after = read_save(
                red,
                &walk
                    .emulator
                    .store
                    .load_battery(&red_key)
                    .expect("red battery"),
            )
            .expect("red after");
            for (name, state) in [("blue", &blue_after), ("red", &red_after)] {
                let party: Vec<String> = state
                    .party
                    .iter()
                    .map(|p| format!("{} {}", species(p), nickname(p)))
                    .collect();
                println!("{name} after {:?} party {party:?}", counts(state));
            }
            assert_eq!(counts(&blue_after), counts(&before));
            assert_eq!(counts(&red_after), counts(&before));
            assert_eq!(owned(&blue_after, flareon), owned(&before, flareon) - 1);
            assert_eq!(owned(&blue_after, blastoise), owned(&before, blastoise) + 1);
            assert_eq!(owned(&red_after, blastoise), owned(&before, blastoise) - 1);
            assert_eq!(owned(&red_after, flareon), owned(&before, flareon) + 1);
            assert_eq!(blue_after.party.last(), Some(&before.party[2]));
            assert_eq!(red_after.party.last(), Some(&before.party[0]));
        }
    }

    #[test]
    fn two_infrared_carts_see_each_others_light() {
        let shining = cart(
            0x00,
            0xFF,
            &[0x3E, 0x0E, 0xEA, 0x00, 0x00, 0x3E, 0x01, 0xEA, 0x00, 0xA0],
        );
        let watching = cart(
            0x00,
            0xFF,
            &[
                0x3E, 0x0E, 0xEA, 0x00, 0x00, 0xFA, 0x00, 0xA0, 0xEA, 0x00, 0xC0, 0x18, 0xF8,
            ],
        );
        let mut first = GameBoy::new_empty(GbModel::Dmg);
        first.load_rom_with_model(&shining, GbModel::Dmg).unwrap();
        let mut second = GameBoy::new_empty(GbModel::Dmg);
        second.load_rom_with_model(&watching, GbModel::Dmg).unwrap();
        for _ in 0..3 {
            infrared_frame([&mut first, &mut second], [Keys::empty(); 2]);
        }
        assert!(first.ir_led());
        assert_eq!(second.memory.read_naive(0xC000), 0xC1);
    }
}
