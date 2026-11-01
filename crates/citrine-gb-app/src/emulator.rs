use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, Captured, GbButton, KeyBindings, PadButton,
};
use crate::app::ui_state::settings::{ScalingMode, Settings};
use crate::audio::AudioLink;
use crate::audio::mix::{Mixer, SoundLevels, level};
use crate::audio::pacer::{AudioFeed, FrameClock};
use crate::autosave::{AutosaveInterval, LastGood, PlayClock, StateCopy};
use crate::controller_debug::PadLog;
use crate::held::{Change, HeldButtons, InputSource};
use crate::local_link::{Game, LocalLink, Source};
use crate::media::recording::ScreenRecorder;
use crate::online::OnlineSettings;
use crate::pad_input::{Ignored, PadAction, PadEvent, PadInput, PadKey, PadSource};
use crate::partner_link::PartnerLink;
use crate::play::{GameFile, LinkedSave, PlaySession, Returned, SaveKey};
use crate::recorder::InputRecorder;
use crate::storage::{SaveStore, now_unix};
use crate::utils::avg_timer::AvgTimer;
use citrine_gb::error::GbResult;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::persistence::full_dump::FullDumpRef;
use citrine_gb::persistence::rtc_footer::RtcFooter;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use strum::IntoEnumIterator;

const SAVE_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);
const PLAYING_AFTER: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_PLAY_STEP: f64 = 0.25;

#[derive(Debug, Default)]
pub struct StatEvents {
    pub launched: bool,
    pub snapshots: u32,
    pub loads: u32,
    pub played: std::time::Duration,
    pub together: Option<TogetherPlay>,
}

#[derive(Debug)]
pub struct TogetherPlay {
    pub rom: String,
    pub name: String,
    pub played: std::time::Duration,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SaveImport {
    Imported,
    NoRom,
    NoClock,
    Nothing,
}

pub struct SaveStatus {
    pub name: String,
    pub battery_backed: bool,
    pub stored: bool,
    pub saved_at: Option<u64>,
    pub location: String,
    pub has_clock: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub legacy_path: Option<std::path::PathBuf>,
}
const GB_WIDTH: usize = 160;
const GB_HEIGHT: usize = 144;

pub struct Emulator {
    pub gb: GameBoy,
    pub running: bool,
    audio_feed: Option<AudioFeed>,
    pub enable_matrix: bool,
    /// 0.0 to 1.0 (e.g., 0.85 = 15% darker)
    pub matrix_edge_brightness: f32,
    /// 0.0 to 1.0 (e.g., 0.75 = 25% darker)
    pub matrix_corner_brightness: f32,
    pub enable_ghosting: bool,
    /// 0.0 (pure smear) to 1.0 (instant, no ghosting)
    pub ghosting_blend: f32,
    pub scaling: ScalingMode,
    held: HeldButtons,
    texture: Option<egui::TextureHandle>,
    upscale_buffer: Vec<u8>,
    last_update: Option<web_time::Instant>,
    clock: FrameClock,
    pub render_scale: usize,
    pub update_avg_timer: AvgTimer,
    pub frame_avg_timer: AvgTimer,
    pub total_frames: u128,
    last_frame: Vec<u8>,
    linked_frame: Option<Vec<u8>>,
    linked_audio: Vec<f32>,
    friend_audio: Vec<f32>,
    mixer: Mixer,
    pub sound: SoundLevels,
    /// SHA-256 of the loaded ROM
    rom_key: Option<String>,
    game_name: String,
    loose_presses: Vec<PadButton>,
    #[cfg(not(target_arch = "wasm32"))]
    rom_path: Option<std::path::PathBuf>,
    pub imported_legacy_save: bool,
    pub store: SaveStore,
    pub last_save: Option<web_time::Instant>,
    pub save_loaded: bool,
    started_at: Option<web_time::Instant>,
    pub recorder: InputRecorder,
    pub replay: Option<crate::replay::ReplayPlayer>,
    pub screen_recorder: ScreenRecorder,
    pub autosave_interval: AutosaveInterval,
    pub autosave_keep: usize,
    play: PlayClock,
    pub last_good: LastGood,
    console_switched: Option<GbModel>,
    pub link: Option<LocalLink>,
    pub partner: Option<PartnerLink>,
    stat_events: StatEvents,
}

impl Default for Emulator {
    fn default() -> Self {
        Self {
            gb: GameBoy::new_empty(GbModel::Dmg),
            running: true,
            audio_feed: None,
            enable_matrix: true,
            matrix_edge_brightness: 0.85,
            matrix_corner_brightness: 0.75,
            enable_ghosting: true,
            ghosting_blend: 0.7,
            scaling: ScalingMode::default(),
            held: HeldButtons::default(),
            texture: None,
            upscale_buffer: Vec::new(),
            last_update: None,
            clock: FrameClock::default(),
            render_scale: 3,
            update_avg_timer: AvgTimer::default(),
            frame_avg_timer: AvgTimer::default(),
            total_frames: 0,
            last_frame: vec![0; GB_WIDTH * GB_HEIGHT * 4],
            linked_frame: None,
            linked_audio: Vec::new(),
            friend_audio: Vec::new(),
            mixer: Mixer::default(),
            sound: SoundLevels {
                mine: level(Settings::DEFAULT_VOLUME, false),
                friend: level(OnlineSettings::DEFAULT_FRIEND_VOLUME, false),
            },
            rom_key: None,
            game_name: String::new(),
            loose_presses: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            rom_path: None,
            imported_legacy_save: false,
            store: SaveStore::default(),
            last_save: None,
            save_loaded: false,
            started_at: None,
            recorder: InputRecorder::default(),
            replay: None,
            screen_recorder: ScreenRecorder::default(),
            autosave_interval: AutosaveInterval::default(),
            autosave_keep: crate::autosave::DEFAULT_KEEP,
            play: PlayClock::default(),
            last_good: LastGood::default(),
            console_switched: None,
            link: None,
            partner: None,
            stat_events: StatEvents::default(),
        }
    }
}

impl Emulator {
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        log: Option<&mut PadLog>,
    ) -> GbResult<()> {
        self.update_linked(ctx, pads, bindings, capture, log, None)
    }

    pub fn update_linked(
        &mut self,
        ctx: &egui::Context,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        log: Option<&mut PadLog>,
        mut session: Option<&mut PlaySession>,
    ) -> GbResult<()> {
        self.update_avg_timer.start();

        if self.texture.is_none() {
            self.update_texture(ctx);
        }

        self.handle_input(ctx, pads, bindings, capture, log);

        if let Some(session) = session.as_deref_mut() {
            if let Some(back) = session.take_returned() {
                self.adopt(back);
            }
            if session.wants_state() {
                session.give_state(citrine_gb::netplay::encode_state(&self.gb)?);
            }
            if session.runs_engine() {
                if self.advance_rollback(web_time::Instant::now(), session) {
                    self.update_texture(ctx);
                    self.total_frames = self.total_frames.wrapping_add(1);
                }
                self.store_linked_saves(session)?;
                self.update_avg_timer.stop();
                return Ok(());
            }
            if session.holds_my_game() || session.watching_only() {
                self.last_update = Some(web_time::Instant::now());
                self.update_avg_timer.stop();
                return Ok(());
            }
        }
        if self.linked_frame.take().is_some() {
            self.update_texture(ctx);
        }
        let cable = session.filter(|s| s.is_live());

        if !self.running {
            self.advance(web_time::Instant::now(), None);
            return Ok(());
        }

        if self.advance(web_time::Instant::now(), cable) {
            self.update_texture(ctx);
            self.drain_audio();
            self.total_frames = self.total_frames.wrapping_add(1);
        }

        self.handle_save()?;

        self.update_avg_timer.stop();
        Ok(())
    }

    fn advance_rollback(&mut self, now: web_time::Instant, session: &mut PlaySession) -> bool {
        let dt = match self.last_update {
            Some(last) => now.duration_since(last).as_secs_f64(),
            None => 0.0,
        };
        self.last_update = Some(now);
        let mut ran = false;
        let step = std::time::Duration::from_secs_f64(dt.min(MAX_PLAY_STEP));
        if let Some(rom) = linked_key(session)
            && let Some(card) = session.my_game()
        {
            let together = self
                .stat_events
                .together
                .get_or_insert_with(|| TogetherPlay {
                    name: together_name(card),
                    rom: rom.clone(),
                    played: std::time::Duration::ZERO,
                });
            if together.rom == rom {
                together.played += step;
            }
        }
        for _ in 0..self.clock.advance(dt) {
            self.frame_avg_timer.start();
            ran |= session.run_frame(self.held_buttons(), now);
            self.frame_avg_timer.stop();
            let owed = self.held.take_owed();
            self.apply(Change {
                pressed: JoypadState::empty(),
                released: owed,
            });
        }
        if let Some(gb) = session.console(true).or_else(|| session.console(false)) {
            let frame = self.linked_frame.get_or_insert_with(Vec::new);
            frame.clear();
            frame.extend_from_slice(gb.frame().as_slice());
        }
        session.drain_audio(&mut self.linked_audio, &mut self.friend_audio);
        self.mixer
            .mix(self.sound, &mut self.linked_audio, &mut self.friend_audio);
        if let Some(feed) = &mut self.audio_feed {
            feed.set_paused(false);
            feed.push(&self.linked_audio);
        }
        self.linked_audio.clear();
        ran
    }

    fn store_linked_saves(&mut self, session: &mut PlaySession) -> GbResult<()> {
        self.store_saves(session.take_saves())
    }

    fn store_saves(&mut self, saves: Vec<LinkedSave>) -> GbResult<()> {
        let mut stored: Vec<&SaveKey> = Vec::new();
        for save in saves.iter().rev() {
            if stored.contains(&&save.key) {
                continue;
            }
            stored.push(&save.key);
            let key = match &save.key {
                SaveKey::Borrowed(hash) => borrowed_key(hash),
                SaveKey::Rom(key) => key.clone(),
            };
            self.store.store_battery(&key, save.dump.as_slice())?;
        }
        Ok(())
    }

    pub fn keep_returned(&mut self, back: Returned) {
        self.adopt(back);
        self.play.play(std::time::Duration::from_secs(1));
        if let Err(err) = self.autosave_if_played() {
            log::error!("could not autosave the game put away: {err:?}");
        }
    }

    pub fn finish_session(&mut self, mut session: PlaySession) -> GbResult<()> {
        let saves = session.finish_saves();
        self.store_saves(saves)?;
        let Some(back) = session.finish() else {
            if self.gb.link_plugged() {
                self.gb.unplug_link();
            }
            return Ok(());
        };
        self.adopt(back);
        Ok(())
    }

    fn adopt(&mut self, back: Returned) {
        let key = Some(match &back.borrowed {
            Some(hash) => borrowed_key(hash),
            None => back.console.cartridge.header.sha256_hex_string(),
        });
        self.screen_recorder.stop();
        let same_game = key == self.rom_key;
        self.gb = back.console;
        self.linked_frame = None;
        self.rom_key = key;
        if !same_game {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.rom_path = None;
            }
            self.name_game(None);
        }
        self.sync_joypad();
        self.running = true;
        self.save_loaded = true;
        self.last_save = None;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.last_good.clear();
    }

    pub fn start_partner(&mut self, partner: PartnerLink) -> GbResult<()> {
        self.end_link()?;
        self.partner = Some(partner);
        Ok(())
    }

    pub fn start_link(&mut self, rom: &Rom, model: GbModel, title: String) -> GbResult<()> {
        self.end_link()?;
        let rate = self.gb.apu.output_sample_rate;
        self.link = Some(LocalLink::start(
            rom,
            model,
            title,
            &self.store,
            rate,
            self.rom_key.as_deref(),
        )?);
        self.release_all();
        Ok(())
    }

    pub fn end_link(&mut self) -> GbResult<()> {
        if let Some(partner) = self.partner.take() {
            partner.finish(&mut self.gb);
        }
        let Some(link) = self.link.take() else {
            return Ok(());
        };
        link.finish(&mut self.gb, &self.store, self.autosave_keep)
    }

    pub fn set_link_control(&mut self, game: Game) {
        let moved = self.link.as_mut().is_some_and(|link| {
            let moved = link.set_control(game);
            if moved {
                link.release_all();
            }
            moved
        });
        if moved {
            self.release_all();
        }
    }

    fn held_of(&mut self, pad: usize, source: InputSource) -> JoypadState {
        if let Some(link) = &mut self.link
            && link.game_for(Source::Pad(pad)) == Game::Second
        {
            return link.held_of(source);
        }
        self.held.of(source)
    }

    pub fn game_file(&mut self) -> Option<GameFile> {
        if !self.gb.cartridge.has_rom_loaded {
            return None;
        }
        Some(GameFile {
            rom: self.gb.cartridge.rom_bytes(),
            title: self.game_name.clone(),
            model: self.console(),
            save: self
                .gb
                .poll_sram_dump(true, now_unix())
                .map(|dump| dump.as_slice().to_vec()),
            running: true,
        })
    }

    pub fn borrowed_save(&self, hash: &citrine_gb_net::GameHash) -> Option<Vec<u8>> {
        self.store.load_battery(&borrowed_key(hash))
    }

    pub fn battery_for(&self, rom: &[u8]) -> Option<Vec<u8>> {
        let header = Rom::new(rom).header().ok()?;
        self.store.load_battery(&header.sha256_hex_string())
    }

    pub fn open_game(&self) -> Option<citrine_gb_net::GameHash> {
        let cartridge = &self.gb.cartridge;
        cartridge.has_rom_loaded.then_some(cartridge.header.sha1)
    }

    pub(crate) fn advance(
        &mut self,
        now: web_time::Instant,
        mut cable: Option<&mut PlaySession>,
    ) -> bool {
        if !self.running {
            self.gb.apu.audio_buffer.clear();
            if let Some(link) = &mut self.link {
                link.clear_audio();
            }
            if let Some(feed) = &self.audio_feed {
                feed.set_paused(true);
            }
            self.last_update = Some(now);
            if self.play.copy_due(true) {
                self.copy_last_good();
            }
            return false;
        }

        let dt = match self.last_update {
            Some(last) => now.duration_since(last).as_secs_f64(),
            None => 0.0,
        };
        self.last_update = Some(now);

        let mut ran_frame = false;
        let mut heard = self.gb.apu.audio_buffer.len();
        for _ in 0..self.clock.advance(dt) {
            if let Some(session) = cable.as_deref_mut() {
                session.pump_link(&mut self.gb);
            }
            self.feed_replay();
            self.frame_avg_timer.start();
            match (self.link.as_mut(), self.partner.as_mut()) {
                (Some(link), _) => link.run_frame(&mut self.gb, self.held.held()),
                (None, Some(partner)) => partner.run_frame(&mut self.gb),
                (None, None) => self.gb.run_frame(),
            }
            self.frame_avg_timer.stop();
            let owed = self.held.take_owed();
            self.apply(Change {
                pressed: JoypadState::empty(),
                released: owed,
            });
            ran_frame = true;
            heard = self.capture_frame(heard);

            if self.gb.debugger.hit_breakpoint {
                self.gb.debugger.hit_breakpoint = false;
                self.running = false;
                break;
            }
        }

        let step = std::time::Duration::from_secs_f64(dt.min(MAX_PLAY_STEP));
        self.play.play(step);
        self.stat_events.played += step;
        if self.play.copy_due(false) {
            self.copy_last_good();
        }
        ran_frame
    }

    fn capture_frame(&mut self, heard: usize) -> usize {
        let samples = &self.gb.apu.audio_buffer;
        if self.screen_recorder.is_recording() {
            let fresh = &samples[heard.min(samples.len())..];
            self.screen_recorder.capture(
                self.gb.frame().as_slice(),
                fresh,
                self.gb.apu.output_sample_rate,
            );
        }
        samples.len()
    }

    pub fn attach_audio(&mut self, link: AudioLink) {
        self.gb.apu.set_sample_rate(link.sample_rate);
        if let Some(second) = &mut self.link {
            second.gb.apu.set_sample_rate(link.sample_rate);
        }
        self.audio_feed = Some(link.feed);
    }

    fn drain_audio(&mut self) {
        let mut samples = std::mem::take(&mut self.gb.apu.audio_buffer);
        if let Some(link) = &mut self.link {
            link.mix(&mut samples);
        }
        self.mixer
            .mix(self.sound, &mut samples, &mut self.friend_audio);
        if let Some(feed) = &mut self.audio_feed {
            feed.set_paused(false);
            feed.push(&samples);
        }
        samples.clear();
        self.gb.apu.audio_buffer = samples;
    }

    pub fn audio_feed(&self) -> Option<&AudioFeed> {
        self.audio_feed.as_ref()
    }

    pub fn play_replay(
        &mut self,
        recording: citrine_gb::recording::Recording,
        game: &str,
    ) -> Result<(), crate::replay::ReplayError> {
        let loaded = self.gb.cartridge.has_rom_loaded
            && self
                .gb
                .cartridge
                .header
                .sha256_hex_string()
                .eq_ignore_ascii_case(&recording.rom_sha256);
        if !loaded {
            return Err(crate::replay::ReplayError::OtherGame(game.to_owned()));
        }
        self.stop_replay();
        self.release_all();
        self.restart_on(recording.model)
            .map_err(|_| crate::replay::ReplayError::Unreadable)?;
        self.running = true;
        self.replay = Some(crate::replay::ReplayPlayer::new(recording));
        Ok(())
    }

    pub fn stop_replay(&mut self) {
        if let Some(player) = self.replay.take() {
            for button in GbButton::iter() {
                let joypad = button.joypad();
                if player.held().contains(joypad) && !self.is_held(joypad) {
                    self.gb.release_button(joypad);
                }
            }
        }
    }

    fn feed_replay(&mut self) {
        let Some(player) = self.replay.as_mut() else {
            return;
        };
        let cycle = self.gb.debugger.total_cycles as u64 * 4;
        for (button, pressed) in player.due(cycle) {
            if pressed {
                self.gb.press_button(button);
            } else if !self.held.held().contains(button) {
                self.gb.release_button(button);
            }
        }
        if player.finished() {
            self.replay = None;
        }
    }

    pub fn start_recording(&mut self) {
        self.recorder.start(&self.gb);
    }

    pub fn press(&mut self, source: InputSource, button: JoypadState) -> bool {
        self.set_source(source, button, button)
    }

    pub fn release(&mut self, source: InputSource, button: JoypadState) -> bool {
        self.set_source(source, button, JoypadState::empty())
    }

    pub fn set_held(&mut self, source: InputSource, button: JoypadState, pressed: bool) -> bool {
        if pressed {
            self.press(source, button)
        } else {
            self.release(source, button)
        }
    }

    pub fn held_buttons(&self) -> JoypadState {
        self.held.held()
    }

    pub fn is_held(&self, button: JoypadState) -> bool {
        self.held.held().contains(button)
    }

    pub fn release_all(&mut self) {
        let change = self.held.clear();
        self.apply(change);
    }

    fn set_source(&mut self, source: InputSource, mask: JoypadState, down: JoypadState) -> bool {
        let routed = match source {
            InputSource::PadButtons(pad) | InputSource::PadAxis(pad, _) => Source::Pad(pad),
            InputSource::Touch => Source::Touch,
            InputSource::Keyboard => return self.set_game_source(Game::First, source, mask, down),
        };
        let game = self
            .link
            .as_mut()
            .map_or(Game::First, |link| link.game_for(routed));
        self.set_game_source(game, source, mask, down)
    }

    fn set_game_source(
        &mut self,
        game: Game,
        source: InputSource,
        mask: JoypadState,
        down: JoypadState,
    ) -> bool {
        if game == Game::Second
            && let Some(link) = &mut self.link
        {
            let before = link.held();
            link.set_held(source, mask, down);
            return before.bits() != link.held().bits();
        }
        let before = self.held.held();
        let change = self.held.set(source, mask, down);
        self.apply(change);
        before.bits() != self.held.held().bits()
    }

    fn apply(&mut self, change: Change) {
        let total_cycles = self.gb.debugger.total_cycles;
        for button in GbButton::iter() {
            let joypad = button.joypad();
            if change.pressed.contains(joypad) {
                self.gb.press_button(joypad);
                self.recorder.record(total_cycles, joypad, true);
            } else if change.released.contains(joypad) {
                self.gb.release_button(joypad);
                self.recorder.record(total_cycles, joypad, false);
            }
        }
    }

    fn sync_joypad(&mut self) {
        let owed = self.held.take_owed();
        self.apply(Change {
            pressed: JoypadState::empty(),
            released: owed,
        });
        for button in GbButton::iter() {
            let joypad = button.joypad();
            if self.is_held(joypad) {
                self.gb.press_button(joypad);
            } else {
                self.gb.release_button(joypad);
            }
        }
    }

    fn apply_pad(
        &mut self,
        source: InputSource,
        action: PadAction,
        capture: &mut BindingCapture,
    ) -> bool {
        match action {
            PadAction::Press(button) => self.press(source, button.joypad()),
            PadAction::Release(button) => self.release(source, button.joypad()),
            PadAction::Steer { on, off } => {
                let mask = off
                    .iter()
                    .chain(on.iter())
                    .fold(JoypadState::empty(), |all, b| all | b.joypad());
                let down = on.map_or(JoypadState::empty(), GbButton::joypad);
                self.set_source(source, mask, down)
            }
            PadAction::Capture(pad) => {
                capture.record(Captured::Pad(pad));
                true
            }
            PadAction::Unplugged => {
                let before = self.held.held();
                let change = match source {
                    InputSource::PadButtons(pad) => {
                        if let Some(link) = &mut self.link {
                            link.forget_pad(pad);
                        }
                        self.held.forget_pad(pad)
                    }
                    _ => Change::default(),
                };
                self.apply(change);
                before.bits() != self.held.held().bits()
            }
            PadAction::Ignore(_) => false,
        }
    }

    pub fn take_loose_presses(&mut self) -> Vec<PadButton> {
        std::mem::take(&mut self.loose_presses)
    }

    pub fn handle_pads(
        &mut self,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        mut log: Option<&mut PadLog>,
    ) {
        while let Some(PadEvent { pad, input, code }) = pads.next_event() {
            let source = match input {
                PadInput::Moved(axis, _) => InputSource::PadAxis(pad, axis),
                _ => InputSource::PadButtons(pad),
            };
            let zone = self.held_of(pad, source);
            let steering = GbButton::iter().find(|b| zone.contains(b.joypad()));
            let action = crate::pad_input::route(input, bindings, capture, steering);
            if let (PadInput::Pressed(PadKey::Known(button)), PadAction::Ignore(Ignored::Unbound)) =
                (input, action)
            {
                self.loose_presses.push(button);
            }
            let changed = self.apply_pad(source, action, capture);
            let Some(log) = log.as_deref_mut() else {
                continue;
            };
            let now = web_time::Instant::now();
            match input {
                PadInput::Moved(axis, value) if self.held.of(source).bits() == zone.bits() => {
                    log.record_still(now, pad, axis, value, code, action);
                }
                _ => log.record(now, pad, input, code, action, changed),
            }
        }
    }

    pub fn handle_input(
        &mut self,
        ctx: &egui::Context,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        log: Option<&mut PadLog>,
    ) {
        self.handle_pads(pads, bindings, capture, log);

        if ctx.wants_keyboard_input() && !capture.wants(BindingKind::Keyboard) {
            return;
        }

        let events: Vec<(egui::Key, bool, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => Some((*key, *pressed, *modifiers)),
                    _ => None,
                })
                .collect()
        });

        for (key, pressed, modifiers) in events {
            if capture.wants(BindingKind::Keyboard) {
                if pressed && key != egui::Key::Escape {
                    capture.record(Captured::Key(key, modifiers));
                }
                continue;
            }
            let routed = match &self.link {
                Some(link) => link.key(key, bindings),
                None => bindings
                    .button_for_key(key)
                    .map(|button| (Game::First, button)),
            };
            if let Some((game, button)) = routed {
                let joypad = button.joypad();
                let down = if pressed {
                    joypad
                } else {
                    JoypadState::empty()
                };
                self.set_game_source(game, InputSource::Keyboard, joypad, down);
            }
        }
    }

    fn screen_layout(&self, available: egui::Vec2) -> (usize, egui::Vec2) {
        let width = GB_WIDTH as f32;
        let height = GB_HEIGHT as f32;
        let fit = (available.x / width).min(available.y / height).max(1.0);

        let mode = if self.enable_matrix {
            ScalingMode::Integer
        } else {
            self.scaling
        };

        match mode {
            ScalingMode::Integer => {
                let scale = fit.floor().max(1.0);
                (scale as usize, egui::vec2(width * scale, height * scale))
            }
            ScalingMode::Fit => (1, egui::vec2(width * fit, height * fit)),
            ScalingMode::Stretch => (
                1,
                egui::vec2(available.x.max(width), available.y.max(height)),
            ),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (ideal_render_scale, display_size) = self.screen_layout(available);

        if self.render_scale != ideal_render_scale {
            self.render_scale = ideal_render_scale;
            self.update_texture(ui.ctx());
        }

        if let Some(tex) = &self.texture {
            let leftover_x = (available.x - display_size.x).max(0.0);
            let leftover_y = (available.y - display_size.y).max(0.0);

            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

            ui.add_space((leftover_y / 2.0).floor());
            ui.horizontal(|ui| {
                ui.add_space((leftover_x / 2.0).floor());
                ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
            });
        }
    }

    pub fn ui_at(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        self.ui_placed(ui, rect, |size| {
            let centre = rect.center();
            let top_left = egui::pos2(
                (centre.x - size.x / 2.0).round(),
                (centre.y - size.y / 2.0).round(),
            );
            egui::Rect::from_min_size(top_left, size)
        });
    }

    pub fn ui_placed(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        place: impl FnOnce(egui::Vec2) -> egui::Rect,
    ) -> egui::Rect {
        let (ideal_render_scale, display_size) = self.screen_layout(area.size());

        if self.render_scale != ideal_render_scale {
            self.render_scale = ideal_render_scale;
            self.update_texture(ui.ctx());
        }

        let target = place(display_size);
        if let Some(tex) = &self.texture {
            egui::Image::new(egui::load::SizedTexture::new(tex.id(), display_size))
                .paint_at(ui, target);
        }
        target
    }

    pub fn paint_fitted(&mut self, ui: &mut egui::Ui, rect: egui::Rect) -> egui::Rect {
        let target = crate::app::tabs::linked::fit(rect, f32::MAX);
        if self.texture.is_none() {
            self.update_texture(ui.ctx());
        }
        if let Some(tex) = &self.texture {
            egui::Image::new(egui::load::SizedTexture::new(tex.id(), target.size()))
                .paint_at(ui, target);
        }
        target
    }

    pub fn screenshot(&self, scale: usize) -> (usize, usize, Vec<u8>) {
        let scale = scale.max(1);
        let source = if self.upscale_buffer.is_empty() {
            self.gb.frame().as_slice()
        } else {
            &self.last_frame
        };
        let mut rgba = Vec::new();
        self.paint(source, scale, &mut rgba);
        (GB_WIDTH * scale, GB_HEIGHT * scale, rgba)
    }

    pub fn load_rom(
        &mut self,
        rom: &Rom,
        model: GbModel,
        #[cfg(not(target_arch = "wasm32"))] path: Option<&std::path::Path>,
    ) -> GbResult<()> {
        self.screen_recorder.stop();
        self.replay = None;
        if let Err(err) = self.autosave_if_played() {
            log::error!("could not autosave before switching games: {err:?}");
        }
        self.running = false;
        self.save_loaded = false;
        self.imported_legacy_save = false;
        self.console_switched = None;
        if let Some(partner) = self.partner.take() {
            partner.finish(&mut self.gb);
        }
        self.gb.load_rom_with_model(rom, model)?;
        self.sync_joypad();

        let key = self.gb.cartridge.header.sha256_hex_string();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.rom_path = path.map(|p| p.to_owned());
            if let Some(path) = path {
                self.imported_legacy_save = self.store.import_legacy_save_if_new(&key, path);
            }
        }

        if let Some(data) = self.store.load_battery(&key) {
            self.gb
                .put_sram_dump(SramDump::from_slice(&data), now_unix());
            self.save_loaded = true;
        }

        self.rom_key = Some(key);
        self.name_game(None);
        self.stat_events.launched = true;
        self.last_save = None;
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.last_good.clear();
        Ok(())
    }

    pub fn console(&self) -> GbModel {
        self.gb.model
    }

    pub fn take_console_switch(&mut self) -> Option<GbModel> {
        self.console_switched.take()
    }

    pub fn restart_on(&mut self, model: GbModel) -> GbResult<()> {
        if self.rom_key.is_none() {
            return Ok(());
        }
        self.flush_save()?;
        let rom = Rom::new(&self.gb.cartridge.rom_bytes());
        let name = self.game_name.clone();
        let launched = self.stat_events.launched;
        #[cfg(not(target_arch = "wasm32"))]
        let path = self.rom_path.clone();
        self.load_rom(
            &rom,
            model,
            #[cfg(not(target_arch = "wasm32"))]
            path.as_deref(),
        )?;
        self.game_name = name;
        self.stat_events.launched = launched;
        Ok(())
    }

    pub fn handle_save(&mut self) -> GbResult<()> {
        if let Some(link) = &mut self.link {
            link.handle_save(&self.store)?;
        }
        if let Some(last_save) = self.last_save
            && last_save.elapsed() < SAVE_COOLDOWN
        {
            return Ok(());
        }
        self.flush_save()
    }

    pub fn flush_save(&mut self) -> GbResult<()> {
        if let Some(link) = &mut self.link {
            link.flush_save(&self.store)?;
        }
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        let Some(sdump) = self.gb.poll_sram_dump(false, now_unix()) else {
            return Ok(());
        };
        self.store.store_battery(&key, sdump.as_slice())?;
        self.last_save = Some(web_time::Instant::now());
        Ok(())
    }

    pub fn save_snapshot(&mut self, slot: usize) -> GbResult<()> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        let state = self.gb.dump_full(now_unix())?;
        let frame = self.gb.frame().as_slice().to_vec();
        self.store
            .store_snapshot(&key, slot, &state, &frame, self.gb.model)?;
        self.started_at = Some(web_time::Instant::now());
        self.stat_events.snapshots += 1;
        Ok(())
    }

    pub fn load_snapshot(&mut self, slot: usize) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        let Some(state) = self.store.load_snapshot(&key, slot) else {
            return Ok(false);
        };
        self.restore(&state)?;
        self.stat_events.loads += 1;
        Ok(true)
    }

    pub fn load_autosave(&mut self, id: usize) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        let Some(state) = self.store.load_autosave(&key, id) else {
            return Ok(false);
        };
        self.restore(&state)?;
        self.stat_events.loads += 1;
        Ok(true)
    }

    fn restore(&mut self, state: &[u8]) -> GbResult<()> {
        let rom = Rom::new(&self.gb.cartridge.rom_bytes());
        let restored = citrine_gb::gb::GameBoy::from_dump(state, &rom, now_unix())?;
        let sample_rate = self.gb.apu.output_sample_rate;
        if restored.model != self.gb.model {
            self.console_switched = Some(restored.model);
        }
        self.gb = restored;
        self.gb.apu.set_sample_rate(sample_rate);
        self.sync_joypad();
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.copy_last_good();
        Ok(())
    }

    pub fn delete_autosave(&mut self, id: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.delete_autosave(&key, id)?;
        }
        Ok(())
    }

    pub fn autosave(&mut self) -> GbResult<Option<usize>> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(None);
        };
        self.play.attempted();
        self.flush_save()?;
        let state = self.gb.dump_full(now_unix())?;
        let frame = self.gb.frame().as_slice().to_vec();
        let id =
            self.store
                .store_autosave(&key, &state, &frame, self.gb.model, self.autosave_keep)?;
        self.play.saved();
        self.last_good.mark_saved();
        self.started_at = Some(web_time::Instant::now());
        Ok(Some(id))
    }

    pub fn autosave_if_played(&mut self) -> GbResult<Option<usize>> {
        if !self.play.has_unsaved_play() {
            return Ok(None);
        }
        self.autosave()
    }

    pub fn autosave_if_due(&mut self) -> GbResult<Option<usize>> {
        if !self.play.autosave_due(self.autosave_interval) {
            return Ok(None);
        }
        self.autosave()
    }

    fn copy_last_good(&mut self) {
        self.play.copied();
        let Some(key) = self.rom_key.clone() else {
            return;
        };
        let sram = self.gb.poll_sram_dump(true, now_unix());
        let battery = sram.as_ref().map(|dump| dump.as_slice().to_vec());
        let state = match (FullDumpRef { gb: &self.gb, sram }).to_rmp() {
            Ok(state) => state,
            Err(err) => {
                log::error!("could not copy the state for autosave: {err:?}");
                return;
            }
        };
        self.last_good.keep(StateCopy {
            store: self.store.clone(),
            rom_key: key,
            state,
            frame: self.gb.frame().as_slice().to_vec(),
            model: self.gb.model,
            battery,
            keep: self.autosave_keep,
            unsaved: self.play.has_unsaved_play(),
        });
    }

    pub fn delete_snapshot(&mut self, slot: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.delete_snapshot(&key, slot)?;
        }
        Ok(())
    }

    pub fn snapshot_name(&self, slot: usize) -> String {
        self.rom_key
            .as_deref()
            .and_then(|key| self.store.snapshot_name(key, slot))
            .unwrap_or_else(|| crate::storage::default_slot_name(slot))
    }

    pub fn rename_snapshot(&mut self, slot: usize, name: &str) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.set_snapshot_name(&key, slot, name)?;
        }
        Ok(())
    }

    pub fn name_game(&mut self, file_name: Option<&str>) {
        let header = &self.gb.cartridge.header;
        self.game_name = crate::game_name::game_name(
            &header.sha1,
            file_name,
            &header.title,
            header.cgb_mode.is_cgb(),
        );
    }

    pub fn game_name(&self) -> &str {
        &self.game_name
    }

    pub fn default_battery_name(&self) -> String {
        if self.game_name.is_empty() {
            "Battery save".to_string()
        } else {
            self.game_name.clone()
        }
    }

    pub fn rename_battery(&mut self, name: &str) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store
                .set_battery_name(&key, name, &self.default_battery_name())?;
        }
        Ok(())
    }

    pub fn unload(&mut self) {
        self.screen_recorder.stop();
        self.last_good.clear();
        *self = Self {
            store: self.store.clone(),
            last_good: self.last_good.clone(),
            audio_feed: self.audio_feed.take(),
            sound: self.sound,
            running: false,
            ..Self::default()
        };
    }

    pub fn rom_key(&self) -> Option<&str> {
        self.rom_key.as_deref()
    }

    pub fn screen_texture(&self) -> Option<egui::TextureId> {
        self.texture.as_ref().map(|t| t.id())
    }

    pub fn take_stat_events(&mut self) -> StatEvents {
        std::mem::take(&mut self.stat_events)
    }

    pub fn reload_battery(&mut self) -> GbResult<()> {
        if self.rom_key.is_none() {
            return Ok(());
        }
        let running = self.running;
        let rom = Rom::new(&self.gb.cartridge.rom_bytes());
        let name = self.game_name.clone();
        let launched = self.stat_events.launched;
        #[cfg(not(target_arch = "wasm32"))]
        let path = self.rom_path.clone();
        self.load_rom(
            &rom,
            self.gb.model,
            #[cfg(not(target_arch = "wasm32"))]
            path.as_deref(),
        )?;
        self.game_name = name;
        self.stat_events.launched = launched;
        self.running = running;
        Ok(())
    }

    pub fn use_battery(&mut self, token: &str) -> GbResult<()> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        self.flush_save()?;
        self.store.swap_battery(&key, token)?;
        self.reload_battery()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn rom_path(&self) -> Option<&std::path::Path> {
        self.rom_path.as_deref()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn reimport_legacy_save(&mut self) -> bool {
        let (Some(key), Some(path)) = (self.rom_key.clone(), self.rom_path.clone()) else {
            return false;
        };
        if !self.store.import_legacy_save(&key, &path) {
            return false;
        }
        if let Some(data) = self.store.load_battery(&key) {
            self.gb
                .put_sram_dump(SramDump::from_slice(&data), now_unix());
            self.save_loaded = true;
            return true;
        }
        false
    }

    pub fn import_save(
        &mut self,
        ram: Option<&[u8]>,
        clock: Option<RtcFooter>,
    ) -> GbResult<SaveImport> {
        if let Some(blocked) = self.import_blocker(clock.is_some()) {
            return Ok(blocked);
        }
        let Some(key) = self.rom_key.clone() else {
            return Ok(SaveImport::NoRom);
        };
        let now = now_unix();
        match (ram, clock) {
            (Some(ram), None) => {
                self.store.store_battery(&key, ram)?;
                self.gb.put_sram_dump(SramDump::from_slice(ram), now);
            }
            (ram, Some(footer)) => {
                if let Some(ram) = ram {
                    self.gb.put_sram_dump(SramDump::from_slice(ram), now);
                }
                self.gb.cartridge.put_rtc_footer(footer, now);
                if let Some(dump) = self.gb.poll_sram_dump(true, now) {
                    self.store.store_battery(&key, dump.as_slice())?;
                }
            }
            (None, None) => return Ok(SaveImport::Nothing),
        }
        self.save_loaded = true;
        self.last_save = Some(web_time::Instant::now());
        Ok(SaveImport::Imported)
    }

    pub fn import_blocker(&self, with_clock: bool) -> Option<SaveImport> {
        if self.rom_key.is_none() {
            Some(SaveImport::NoRom)
        } else if with_clock && self.gb.cartridge.rtc().is_none() {
            Some(SaveImport::NoClock)
        } else {
            None
        }
    }

    pub fn has_been_playing(&self) -> bool {
        self.started_at
            .is_some_and(|started| started.elapsed() >= PLAYING_AFTER)
    }

    pub fn restart(&mut self) {
        self.gb.soft_reset();
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
    }

    pub fn export_rtc_files(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        let footer = self.gb.cartridge.rtc_footer(now_unix())?;
        Some((
            self.gb.cartridge.battery_ram(),
            footer.encode_rtc_file().to_vec(),
        ))
    }

    pub fn export_save_bytes(&mut self) -> Option<Vec<u8>> {
        self.gb
            .poll_sram_dump(true, now_unix())
            .map(|dump| dump.as_slice().to_vec())
    }

    pub fn save_status(&self) -> SaveStatus {
        let key = self.rom_key.as_deref();
        SaveStatus {
            name: key
                .and_then(|k| self.store.battery_name(k))
                .unwrap_or_else(|| self.default_battery_name()),
            battery_backed: self.gb.cartridge.has_battery(),
            stored: key.is_some_and(|k| self.store.has_battery(k)),
            saved_at: key.and_then(|k| self.store.battery_saved_at(k)),
            location: self.store.location(),
            has_clock: self.gb.cartridge.rtc().is_some(),
            #[cfg(not(target_arch = "wasm32"))]
            legacy_path: self
                .rom_path
                .as_deref()
                .map(SaveStore::legacy_save_path)
                .filter(|p| p.is_file()),
        }
    }

    pub fn force_step(&mut self, ctx: &egui::Context, cycle_count: u32) {
        self.gb.step_cycles(cycle_count);
        self.update_texture(ctx);
    }

    pub fn soft_reset(&mut self, ctx: &egui::Context) {
        self.gb.soft_reset();
        self.update_texture(ctx);
    }

    pub fn reset_to(&mut self, ctx: &egui::Context, start_cycles: u32) {
        self.gb.soft_reset();
        self.started_at = Some(web_time::Instant::now());
        self.gb.run_cycles(start_cycles);
        self.update_texture(ctx);
    }

    pub fn clear_frame(&mut self, ctx: &egui::Context) {
        self.gb.ppu.clear_frame();
        self.update_texture(ctx);
    }

    fn blend_frame(&mut self) {
        let current_raw = match &self.linked_frame {
            Some(frame) => frame.as_slice(),
            None => self.gb.frame().as_slice(),
        };
        let blend = if self.enable_ghosting {
            self.ghosting_blend
        } else {
            1.0
        };
        for (last, &current) in self.last_frame.iter_mut().zip(current_raw) {
            *last = (current as f32 * blend + *last as f32 * (1.0 - blend)) as u8;
        }
        for alpha in self.last_frame.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
    }

    fn paint(&self, source: &[u8], scale: usize, out: &mut Vec<u8>) {
        let upscaled_width = GB_WIDTH * scale;
        out.clear();
        out.resize(upscaled_width * GB_HEIGHT * scale * 4, 0);
        let c0 = self.gb.ppu.backdrop_color();
        let toward_backdrop = |pixel: [u8; 3], t: f32| {
            [
                (pixel[0] as f32 * t + c0.r() as f32 * (1.0 - t)) as u8,
                (pixel[1] as f32 * t + c0.g() as f32 * (1.0 - t)) as u8,
                (pixel[2] as f32 * t + c0.b() as f32 * (1.0 - t)) as u8,
            ]
        };

        for y in 0..GB_HEIGHT {
            for x in 0..GB_WIDTH {
                let orig_idx = (y * GB_WIDTH + x) * 4;
                let pixel = [source[orig_idx], source[orig_idx + 1], source[orig_idx + 2]];
                let edge = toward_backdrop(pixel, self.matrix_edge_brightness);
                let corner = toward_backdrop(pixel, self.matrix_corner_brightness);

                for dy in 0..scale {
                    for dx in 0..scale {
                        let up_idx = ((y * scale + dy) * upscaled_width + x * scale + dx) * 4;
                        let right = dx == scale - 1;
                        let bottom = dy == scale - 1;
                        let [r, g, b] = match (self.enable_matrix, right, bottom) {
                            (true, true, true) => corner,
                            (true, true, false) | (true, false, true) => edge,
                            _ => pixel,
                        };
                        out[up_idx..up_idx + 4].copy_from_slice(&[r, g, b, 255]);
                    }
                }
            }
        }
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        let scale = self.render_scale;
        self.blend_frame();
        let mut upscaled_data = std::mem::take(&mut self.upscale_buffer);
        self.paint(&self.last_frame, scale, &mut upscaled_data);

        let image = egui::ColorImage::from_rgba_unmultiplied(
            [GB_WIDTH * scale, GB_HEIGHT * scale],
            &upscaled_data,
        );
        self.upscale_buffer = upscaled_data;

        match &mut self.texture {
            Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.texture =
                    Some(ctx.load_texture("gb_screen", image, egui::TextureOptions::NEAREST))
            }
        }
    }
}

fn borrowed_key(hash: &citrine_gb_net::GameHash) -> String {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("borrowed-{hex}")
}

fn together_name(card: &citrine_gb_net::GameCard) -> String {
    crate::game_name::known_name(&card.hash)
        .map(str::to_string)
        .unwrap_or_else(|| card.title.clone())
}

fn linked_key(session: &PlaySession) -> Option<String> {
    if session.borrowing()
        && let Some(card) = session.my_game()
    {
        return Some(borrowed_key(&card.hash));
    }
    session
        .console(true)
        .map(|gb| gb.cartridge.header.sha256_hex_string())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::app::ui_state::keybinds::{BindingTarget, PadButton};
    use crate::pad_input::{PadAxis, PadInput, PadKey};

    const RAM_LEN: usize = 0x2000;
    const KEYS: InputSource = InputSource::Keyboard;

    fn emulator(name: &str) -> Emulator {
        let root =
            std::env::temp_dir().join(format!("citrine-import-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Emulator {
            store: SaveStore::at(root),
            ..Emulator::default()
        }
    }

    fn rom(cartridge_type: u8) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0147] = cartridge_type;
        data[0x0149] = 0x02;
        Rom::new(&data)
    }

    fn bytes(seed: u8) -> Vec<u8> {
        (0..RAM_LEN)
            .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
            .collect()
    }

    fn running(emulator: &mut Emulator, cartridge_type: u8) {
        emulator
            .load_rom(&rom(cartridge_type), GbModel::Dmg, None)
            .expect("load");
        emulator.gb.run_cycles(100_000);
    }

    fn played(emulator: &mut Emulator, seconds: f64) {
        let start = web_time::Instant::now();
        emulator.advance(start, None);
        let mut at = 0.0;
        while at < seconds {
            at += 0.1;
            emulator.advance(start + std::time::Duration::from_secs_f64(at), None);
        }
    }

    fn write_sram(emulator: &mut Emulator, value: u8) {
        use citrine_gb::WriteMemory;
        emulator.gb.cartridge.write_naive(0x0000, 0x0A);
        emulator.gb.cartridge.write_naive(0xA000, value);
    }

    fn autosaves(emulator: &Emulator) -> Vec<usize> {
        emulator
            .store
            .autosave_ids(emulator.rom_key().expect("key"))
    }

    fn gb_held(emulator: &mut Emulator) -> u8 {
        use citrine_gb::{ReadMemory, WriteMemory};
        let joypad = &mut emulator.gb.joypad;
        joypad.write_naive(0xFF00, 0x10);
        let buttons = !joypad.read_naive(0xFF00) & 0x0F;
        joypad.write_naive(0xFF00, 0x20);
        let dpad = !joypad.read_naive(0xFF00) & 0x0F;
        buttons | (dpad << 4)
    }

    #[test]
    fn the_game_boy_sees_what_is_held_after_a_load() {
        let mut emulator = emulator("held-sync");
        running(&mut emulator, 0x03);
        emulator.press(KEYS, JoypadState::A);
        emulator.save_snapshot(0).expect("save");
        emulator.release(KEYS, JoypadState::A);
        emulator.press(KEYS, JoypadState::RIGHT);

        assert!(emulator.load_snapshot(0).expect("load"));
        assert_eq!(
            gb_held(&mut emulator),
            JoypadState::RIGHT.bits(),
            "A was let go before the load and Right is still down"
        );
        assert!(emulator.release(KEYS, JoypadState::RIGHT));
        assert_eq!(gb_held(&mut emulator), 0);

        emulator.press(KEYS, JoypadState::START);
        running(&mut emulator, 0x03);
        assert_eq!(gb_held(&mut emulator), JoypadState::START.bits());
        assert!(
            emulator.release(KEYS, JoypadState::START),
            "the release reaches the game"
        );
        assert_eq!(gb_held(&mut emulator), 0);
    }

    #[test]
    fn paused_time_does_not_count() {
        let mut emulator = emulator("auto-paused");
        running(&mut emulator, 0x03);
        emulator.running = false;
        played(&mut emulator, 3.0);
        assert_eq!(emulator.autosave_if_played().expect("autosave"), None);

        emulator.running = true;
        played(&mut emulator, 0.5);
        emulator.autosave_interval = AutosaveInterval::OneMinute;
        assert_eq!(emulator.autosave_if_due().expect("autosave"), None);
        emulator.play.play(std::time::Duration::from_secs(60));
        assert_eq!(emulator.autosave_if_due().expect("autosave"), Some(0));
        assert_eq!(emulator.autosave_if_due().expect("autosave"), None);
    }

    #[test]
    fn closing_autosaves_only_after_play() {
        let mut emulator = emulator("auto-close");
        running(&mut emulator, 0x03);
        assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
        played(&mut emulator, 0.3);
        assert_eq!(emulator.autosave_if_played().expect("autosave"), Some(0));
        assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
        assert_eq!(autosaves(&emulator), vec![0]);
    }

    #[test]
    fn switching_games_autosaves_the_old_one() {
        let mut emulator = emulator("auto-switch");
        running(&mut emulator, 0x03);
        let first = emulator.rom_key().expect("key").to_owned();
        played(&mut emulator, 0.3);

        let mut other = vec![0u8; 0x8000];
        other[0x0134] = b'B';
        other[0x0147] = 0x03;
        other[0x0149] = 0x02;
        emulator
            .load_rom(&Rom::new(&other), GbModel::Dmg, None)
            .expect("load");

        assert_ne!(emulator.rom_key(), Some(first.as_str()));
        assert_eq!(emulator.store.autosave_ids(&first), vec![0]);
        assert!(autosaves(&emulator).is_empty());
    }

    #[test]
    fn the_keep_limit_drops_the_oldest() {
        let mut emulator = emulator("auto-keep");
        running(&mut emulator, 0x03);
        emulator.autosave_keep = 2;
        for _ in 0..4 {
            emulator.autosave().expect("autosave");
        }
        assert_eq!(autosaves(&emulator), vec![2, 3]);
    }

    #[test]
    fn an_autosave_loads_back_without_touching_snapshots() {
        let mut emulator = emulator("auto-load");
        running(&mut emulator, 0x03);
        emulator.save_snapshot(0).expect("snapshot");
        let key = emulator.rom_key().expect("key").to_owned();
        let snapshot = emulator.store.load_snapshot(&key, 0);

        played(&mut emulator, 0.3);
        write_sram(&mut emulator, 0x11);
        let id = emulator.autosave().expect("autosave").expect("id");
        played(&mut emulator, 0.3);
        write_sram(&mut emulator, 0x22);
        assert!(emulator.load_autosave(id).expect("load"));

        assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x11);
        assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
        assert_eq!(emulator.store.load_snapshot(&key, 0), snapshot);
        assert_eq!(emulator.store.used_slots(&key), vec![0]);
    }

    fn dual(emulator: &mut Emulator, model: GbModel) {
        let mut data = rom(0x03).data;
        data[0x0143] = 0x80;
        emulator
            .load_rom(&Rom::new(&data), model, None)
            .expect("load");
        emulator.gb.run_cycles(100_000);
    }

    #[test]
    fn switching_consoles_keeps_the_battery_save() {
        let mut emulator = emulator("switch-battery");
        dual(&mut emulator, GbModel::Cgb);
        write_sram(&mut emulator, 0x5A);
        emulator.restart_on(GbModel::Dmg).expect("switch");
        assert_eq!(emulator.console(), GbModel::Dmg);
        assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x5A);

        write_sram(&mut emulator, 0x6B);
        emulator.restart_on(GbModel::Cgb).expect("switch back");
        assert_eq!(emulator.console(), GbModel::Cgb);
        assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x6B);
        assert_eq!(
            emulator.take_console_switch(),
            None,
            "a restart is no surprise"
        );
    }

    #[test]
    fn switching_consoles_keeps_the_game_name() {
        let mut emulator = emulator("switch-name");
        dual(&mut emulator, GbModel::Cgb);
        emulator.name_game(Some("My Game (USA).gbc"));
        let name = emulator.game_name().to_owned();
        emulator.restart_on(GbModel::Dmg).expect("switch");
        assert_eq!(emulator.game_name(), name);
    }

    #[test]
    fn a_known_game_names_its_battery_save_by_its_proper_name() {
        let mut emulator = emulator("proper-name");
        let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/homebrew/libbet.gb"))
            .expect("libbet");
        emulator
            .load_rom(&Rom::new(&rom), GbModel::Dmg, None)
            .expect("load");
        emulator.name_game(Some("libbet.gb"));
        assert_ne!(
            emulator.gb.cartridge.header.title.trim(),
            "Libbet and the Magic Floor"
        );
        assert_eq!(emulator.game_name(), "Libbet and the Magic Floor");
        assert_eq!(emulator.save_status().name, "Libbet and the Magic Floor");
    }

    #[test]
    fn a_snapshot_loads_on_the_console_it_was_made_on() {
        let mut emulator = emulator("snapshot-console");
        dual(&mut emulator, GbModel::Dmg);
        emulator.save_snapshot(0).expect("snapshot");
        let key = emulator.rom_key().expect("key").to_owned();
        let info = emulator.store.snapshot_info(&key, 0).expect("info");
        assert_eq!(info.console, Some(GbModel::Dmg));

        emulator.restart_on(GbModel::Cgb).expect("switch");
        assert!(emulator.load_snapshot(0).expect("load"));
        assert_eq!(emulator.console(), GbModel::Dmg);
        assert_eq!(emulator.take_console_switch(), Some(GbModel::Dmg));
        assert_eq!(emulator.take_console_switch(), None);

        assert!(emulator.load_snapshot(0).expect("load again"));
        assert_eq!(
            emulator.take_console_switch(),
            None,
            "same console, no notice"
        );
    }

    #[test]
    fn an_autosave_records_its_console() {
        let mut emulator = emulator("autosave-console");
        dual(&mut emulator, GbModel::Cgb);
        played(&mut emulator, 0.3);
        let id = emulator.autosave().expect("autosave").expect("id");
        let key = emulator.rom_key().expect("key").to_owned();
        let infos = emulator.store.autosave_infos(&key);
        assert_eq!(infos[0].console, Some(GbModel::Cgb));

        emulator.restart_on(GbModel::Dmg).expect("switch");
        assert!(emulator.load_autosave(id).expect("load"));
        assert_eq!(emulator.console(), GbModel::Cgb);
        assert_eq!(emulator.take_console_switch(), Some(GbModel::Cgb));
    }

    #[test]
    fn a_snapshot_from_before_consoles_were_recorded_still_loads() {
        let name = "old-snapshot";
        let mut emulator = emulator(name);
        dual(&mut emulator, GbModel::Dmg);
        write_sram(&mut emulator, 0x33);
        emulator.save_snapshot(0).expect("snapshot");
        let key = emulator.rom_key().expect("key").to_owned();
        let meta = std::env::temp_dir()
            .join(format!("citrine-import-test-{}-{name}", std::process::id()))
            .join(&key)
            .join("slot0.meta");
        std::fs::write(&meta, 1_700_000_000u64.to_le_bytes()).expect("old meta");

        let info = emulator.store.snapshot_info(&key, 0).expect("info");
        assert_eq!(info.saved_at, 1_700_000_000);
        assert_eq!(info.console, Some(GbModel::Dmg), "read from the snapshot");
        assert_eq!(std::fs::read(&meta).expect("meta").len(), 9, "noted once");

        write_sram(&mut emulator, 0x44);
        assert!(emulator.load_snapshot(0).expect("load"));
        assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x33);
        assert_eq!(emulator.console(), GbModel::Dmg);
    }

    #[test]
    fn a_crash_saves_the_last_frame_boundary() {
        let mut emulator = emulator("auto-crash");
        running(&mut emulator, 0x03);
        write_sram(&mut emulator, 0x42);
        played(&mut emulator, 1.2);
        emulator.running = false;
        played(&mut emulator, 0.1);

        assert_eq!(emulator.last_good.write_out(), Ok(Some(0)));
        let key = emulator.rom_key().expect("key").to_owned();
        let state = emulator.store.load_autosave(&key, 0).expect("autosave");
        let rom = Rom::new(&emulator.gb.cartridge.rom_bytes());
        let restored = GameBoy::from_dump(&state, &rom, now_unix()).expect("restores");
        assert_eq!(restored.cpu.pc, emulator.gb.cpu.pc);
        assert_eq!(restored.cartridge.battery_ram()[0], 0x42);
        let battery = emulator.store.load_battery(&key).expect("battery");
        assert_eq!(battery[0], 0x42);
    }

    #[test]
    fn an_imported_save_survives_the_restart() {
        let mut emulator = emulator("save");
        running(&mut emulator, 0x03);
        emulator.import_save(Some(&bytes(1)), None).expect("first");
        emulator.restart();
        emulator.gb.run_cycles(100_000);

        let result = emulator.import_save(Some(&bytes(2)), None).expect("second");
        assert_eq!(result, SaveImport::Imported);
        emulator.restart();
        emulator.gb.run_cycles(100_000);

        assert_eq!(emulator.gb.cartridge.battery_ram(), bytes(2));
        let key = emulator.rom_key().expect("key").to_owned();
        assert_eq!(emulator.store.load_battery(&key), Some(bytes(2)));
        assert!(emulator.running);
        assert!(!emulator.has_been_playing());
    }

    #[test]
    fn an_imported_clock_survives_the_restart() {
        let mut emulator = emulator("clock");
        running(&mut emulator, 0x10);
        let mut footer = emulator.gb.cartridge.rtc_footer(now_unix()).expect("clock");
        footer.real.seconds = 12;
        footer.real.minutes = 34;
        footer.real.hours = 5;
        footer.real.days_low = 6;
        footer.real.days_high = 0x40;
        footer.latched = footer.real;

        let result = emulator
            .import_save(Some(&bytes(3)), Some(footer))
            .expect("import");
        assert_eq!(result, SaveImport::Imported);
        emulator.restart();
        emulator.gb.run_cycles(100_000);

        assert_eq!(emulator.gb.cartridge.battery_ram(), bytes(3));
        let clock = emulator.gb.cartridge.rtc_footer(now_unix()).expect("clock");
        assert_eq!(clock.real, footer.real);
    }

    #[test]
    fn a_clock_needs_a_cart_with_one() {
        let mut emulator = emulator("no-clock");
        assert_eq!(emulator.import_blocker(false), Some(SaveImport::NoRom));
        running(&mut emulator, 0x03);
        assert_eq!(emulator.import_blocker(false), None);
        assert_eq!(emulator.import_blocker(true), Some(SaveImport::NoClock));
    }

    #[test]
    fn a_screenshot_lands_in_the_media_store() {
        use crate::media::{MediaKind, MediaStore, save_screenshot};
        let root = std::env::temp_dir().join(format!("citrine-shot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut media = MediaStore::at(root.clone());
        let mut emulator = emulator("screenshot");
        assert!(save_screenshot(&emulator, &mut media, 3).is_err());

        running(&mut emulator, 0x00);
        let item = save_screenshot(&emulator, &mut media, 3).expect("saved");
        assert_eq!(item.info.kind, MediaKind::Screenshot);
        assert_eq!((item.info.width, item.info.height), (480, 432));
        assert!(media.path(&item.id).starts_with(root.join("screenshots")));
        assert!(media.path(&item.id).is_file());
        assert!(item.thumbnail.is_some());
        assert_eq!(MediaStore::at(root.clone()).items().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    struct FakePads(std::collections::VecDeque<PadEvent>);

    impl PadSource for FakePads {
        fn next_event(&mut self) -> Option<PadEvent> {
            self.0.pop_front()
        }

        fn pads(&self, _seen: &[usize]) -> Vec<crate::controller_debug::PadInfo> {
            Vec::new()
        }

        fn api(&self) -> String {
            "fake".to_string()
        }
    }

    fn fake(events: &[(usize, PadInput)]) -> FakePads {
        FakePads(
            events
                .iter()
                .map(|&(pad, input)| PadEvent {
                    pad,
                    input,
                    code: None,
                })
                .collect(),
        )
    }

    #[test]
    fn any_pad_source_drives_the_bindings() {
        let mut emulator = emulator("pad-source");
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let mut log = PadLog::new(web_time::Instant::now());
        let mut pads = fake(&[
            (3, PadInput::Connected),
            (3, PadInput::Pressed(PadKey::Known(PadButton::East))),
            (3, PadInput::Moved(PadAxis::LeftStickX, -0.9)),
        ]);
        emulator.handle_pads(&mut pads, &bindings, &mut capture, Some(&mut log));
        assert!(emulator.is_held(GbButton::A.joypad()));
        assert!(emulator.is_held(GbButton::Left.joypad()));
        assert_eq!(log.len(), 3);
        assert_eq!(log.seen(), &[3]);

        let mut pads = fake(&[
            (3, PadInput::Released(PadKey::Known(PadButton::East))),
            (3, PadInput::Moved(PadAxis::LeftStickX, 0.0)),
        ]);
        emulator.handle_pads(&mut pads, &bindings, &mut capture, None);
        assert!(!emulator.is_held(GbButton::A.joypad()));
        assert!(!emulator.is_held(GbButton::Left.joypad()));
    }

    #[test]
    fn any_pad_source_feeds_a_waiting_binding() {
        let mut emulator = emulator("pad-capture");
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::B), BindingKind::Gamepad);
        let mut pads = fake(&[(
            1,
            PadInput::Pressed(PadKey::Known(PadButton::RightTrigger2)),
        )]);
        emulator.handle_pads(&mut pads, &bindings, &mut capture, None);
        assert_eq!(
            capture.take(),
            Some((
                BindingTarget::Button(GbButton::B),
                Captured::Pad(PadButton::RightTrigger2)
            ))
        );
        assert!(!emulator.is_held(GbButton::B.joypad()));
    }

    fn d_pad(button: PadButton, pressed: bool) -> PadInput {
        if pressed {
            PadInput::Pressed(PadKey::Known(button))
        } else {
            PadInput::Released(PadKey::Known(button))
        }
    }

    fn stick_x(value: f32) -> PadInput {
        PadInput::Moved(PadAxis::LeftStickX, value)
    }

    fn stick_y(value: f32) -> PadInput {
        PadInput::Moved(PadAxis::LeftStickY, value)
    }

    fn noise(count: usize) -> Vec<(usize, PadInput)> {
        (0..count)
            .map(|i| {
                let value = if i % 2 == 0 { 0.02 } else { -0.02 };
                let input = if i % 3 == 0 {
                    stick_y(value)
                } else {
                    stick_x(value)
                };
                (0, input)
            })
            .collect()
    }

    fn feed(emulator: &mut Emulator, bindings: &KeyBindings, events: &[(usize, PadInput)]) {
        let mut capture = BindingCapture::default();
        emulator.handle_pads(&mut fake(events), bindings, &mut capture, None);
    }

    fn pads_in(emulator: &mut Emulator, events: &[(usize, PadInput)]) {
        feed(emulator, &KeyBindings::default(), events);
    }

    fn play_a_frame(emulator: &mut Emulator) {
        emulator.running = true;
        let start = web_time::Instant::now();
        emulator.advance(start, None);
        emulator.advance(start + std::time::Duration::from_millis(50), None);
    }

    fn holds(emulator: &Emulator, button: GbButton) -> bool {
        emulator.is_held(button.joypad())
    }

    #[test]
    fn stick_noise_never_lets_go_of_a_held_d_pad() {
        let mut emulator = emulator("pad-noise");
        running(&mut emulator, 0x03);
        pads_in(&mut emulator, &[(0, d_pad(PadButton::DPadLeft, true))]);
        pads_in(&mut emulator, &noise(50));
        assert!(holds(&emulator, GbButton::Left));
        play_a_frame(&mut emulator);
        assert_eq!(gb_held(&mut emulator), JoypadState::LEFT.bits());
    }

    #[test]
    fn a_d_pad_tap_reaches_the_game_for_a_frame() {
        let mut emulator = emulator("pad-tap");
        running(&mut emulator, 0x03);
        let mut tap = vec![(0, d_pad(PadButton::DPadLeft, true))];
        tap.extend(noise(6));
        tap.push((0, d_pad(PadButton::DPadLeft, false)));
        pads_in(&mut emulator, &tap);
        assert!(!holds(&emulator, GbButton::Left), "the player let go");
        assert_eq!(
            gb_held(&mut emulator),
            JoypadState::LEFT.bits(),
            "the game still sees the tap until a frame runs"
        );
        play_a_frame(&mut emulator);
        assert_eq!(gb_held(&mut emulator), 0);
    }

    #[test]
    fn a_stick_flick_reaches_the_game_for_a_frame() {
        let mut emulator = emulator("pad-flick");
        running(&mut emulator, 0x03);
        pads_in(
            &mut emulator,
            &[(0, stick_x(0.95)), (0, stick_x(0.3)), (0, stick_x(0.01))],
        );
        assert!(!holds(&emulator, GbButton::Right));
        assert_eq!(gb_held(&mut emulator), JoypadState::RIGHT.bits());
        play_a_frame(&mut emulator);
        assert_eq!(gb_held(&mut emulator), 0);
    }

    #[test]
    fn a_centred_stick_lets_go_of_only_what_it_held() {
        let mut emulator = emulator("pad-centre");
        running(&mut emulator, 0x03);
        pads_in(
            &mut emulator,
            &[(0, d_pad(PadButton::DPadUp, true)), (0, stick_x(-0.9))],
        );
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, stick_x(0.0))]);
        assert!(!holds(&emulator, GbButton::Left));
        assert!(holds(&emulator, GbButton::Up));
        assert_eq!(gb_held(&mut emulator), JoypadState::UP.bits());
    }

    #[test]
    fn the_d_pad_and_the_stick_hold_the_same_button_together() {
        let mut emulator = emulator("pad-both");
        running(&mut emulator, 0x03);
        pads_in(
            &mut emulator,
            &[(0, d_pad(PadButton::DPadLeft, true)), (0, stick_x(-0.9))],
        );
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, stick_x(0.0))]);
        assert!(holds(&emulator, GbButton::Left), "the d-pad still holds it");
        pads_in(&mut emulator, &[(0, stick_x(-0.9))]);
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, d_pad(PadButton::DPadLeft, false))]);
        assert!(holds(&emulator, GbButton::Left), "the stick still holds it");
        assert_eq!(gb_held(&mut emulator), JoypadState::LEFT.bits());
        pads_in(&mut emulator, &[(0, stick_x(0.0))]);
        assert!(!holds(&emulator, GbButton::Left));
        assert_eq!(gb_held(&mut emulator), 0);
    }

    #[test]
    fn the_keyboard_and_a_pad_do_not_let_go_of_each_other() {
        let mut emulator = emulator("pad-keys");
        running(&mut emulator, 0x03);
        emulator.press(KEYS, JoypadState::A);
        pads_in(&mut emulator, &[(0, d_pad(PadButton::East, true))]);
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, d_pad(PadButton::East, false))]);
        assert!(holds(&emulator, GbButton::A));
        pads_in(&mut emulator, &[(0, d_pad(PadButton::East, true))]);
        emulator.release(KEYS, JoypadState::A);
        assert!(holds(&emulator, GbButton::A));
    }

    #[test]
    fn stick_diagonals_act_on_each_axis_alone() {
        let mut emulator = emulator("pad-diagonal");
        running(&mut emulator, 0x03);
        pads_in(&mut emulator, &[(0, stick_x(-0.8)), (0, stick_y(-0.8))]);
        assert!(holds(&emulator, GbButton::Left));
        assert!(holds(&emulator, GbButton::Down));
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, stick_y(0.02))]);
        assert!(holds(&emulator, GbButton::Left));
        assert!(!holds(&emulator, GbButton::Down));
        pads_in(&mut emulator, &[(0, stick_y(0.8)), (0, stick_x(0.01))]);
        assert!(holds(&emulator, GbButton::Up));
        assert!(!holds(&emulator, GbButton::Left));
    }

    #[test]
    fn a_stick_resting_near_the_dead_zone_does_not_flicker() {
        let mut emulator = emulator("pad-edge");
        running(&mut emulator, 0x03);
        let mut log = PadLog::new(web_time::Instant::now());
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let mut send = |emulator: &mut Emulator, log: &mut PadLog, values: &[f32]| {
            let events: Vec<(usize, PadInput)> = values.iter().map(|&v| (0, stick_x(v))).collect();
            emulator.handle_pads(&mut fake(&events), &bindings, &mut capture, Some(log));
        };
        send(&mut emulator, &mut log, &[0.46, 0.49, 0.47, 0.5, 0.48]);
        assert!(!holds(&emulator, GbButton::Right), "not past the dead zone");
        send(&mut emulator, &mut log, &[0.53]);
        assert!(holds(&emulator, GbButton::Right));
        let before = log.len();
        let jitter: Vec<f32> = (0..40)
            .map(|i| if i % 2 == 0 { 0.52 } else { 0.42 })
            .collect();
        send(&mut emulator, &mut log, &jitter);
        assert!(holds(&emulator, GbButton::Right), "held through the jitter");
        assert_eq!(log.len(), before + 1, "the jitter is one log line");
        send(&mut emulator, &mut log, &[0.38]);
        assert!(!holds(&emulator, GbButton::Right));
    }

    #[test]
    fn the_dead_zone_setting_moves_where_the_stick_presses() {
        let mut emulator = emulator("pad-deadzone");
        running(&mut emulator, 0x03);
        let mut bindings = KeyBindings::default();
        bindings.stick_deadzone = 30;
        feed(&mut emulator, &bindings, &[(0, stick_x(-0.35))]);
        assert!(holds(&emulator, GbButton::Left));
        feed(&mut emulator, &bindings, &[(0, stick_x(-0.22))]);
        assert!(holds(&emulator, GbButton::Left), "lets go only below 0.20");
        feed(&mut emulator, &bindings, &[(0, stick_x(-0.15))]);
        assert!(!holds(&emulator, GbButton::Left));
    }

    #[test]
    fn an_unplugged_pad_lets_go_of_what_it_held() {
        let mut emulator = emulator("pad-unplug");
        running(&mut emulator, 0x03);
        emulator.press(KEYS, JoypadState::A);
        pads_in(
            &mut emulator,
            &[
                (0, d_pad(PadButton::East, true)),
                (0, d_pad(PadButton::Start, true)),
                (0, stick_x(-0.9)),
                (1, d_pad(PadButton::South, true)),
            ],
        );
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, PadInput::Disconnected)]);
        assert!(holds(&emulator, GbButton::A), "the keyboard still holds it");
        assert!(holds(&emulator, GbButton::B), "pad 1 still holds it");
        assert!(!holds(&emulator, GbButton::Start));
        assert!(!holds(&emulator, GbButton::Left));
        assert_eq!(
            gb_held(&mut emulator),
            (JoypadState::A | JoypadState::B).bits()
        );
    }

    #[test]
    fn letting_go_of_everything_clears_every_source() {
        let mut emulator = emulator("pad-release-all");
        running(&mut emulator, 0x03);
        emulator.press(KEYS, JoypadState::A);
        emulator.press(InputSource::Touch, JoypadState::B);
        pads_in(
            &mut emulator,
            &[(0, d_pad(PadButton::DPadUp, true)), (0, stick_x(0.9))],
        );
        emulator.release_all();
        assert!(GbButton::iter().all(|b| !holds(&emulator, b)));
        assert_eq!(gb_held(&mut emulator), 0, "taps owed a frame go too");
        pads_in(&mut emulator, &[(0, stick_x(0.95))]);
        assert!(
            holds(&emulator, GbButton::Right),
            "a stick still pushed holds again on its next move"
        );
    }

    #[test]
    fn stick_noise_folds_into_one_log_line_per_axis() {
        let mut emulator = emulator("pad-log");
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let mut log = PadLog::new(web_time::Instant::now());
        let mut events = vec![(0, d_pad(PadButton::DPadUp, true))];
        events.extend(noise(90));
        events.push((0, PadInput::Moved(PadAxis::RightStickX, 0.01)));
        events.push((0, d_pad(PadButton::DPadUp, false)));
        events.extend(noise(9));
        emulator.handle_pads(&mut fake(&events), &bindings, &mut capture, Some(&mut log));
        let lines: Vec<String> = log.entries().map(|e| e.event()).collect();
        assert_eq!(lines.len(), 7, "{lines:#?}");
        assert!(lines[0].contains("DPadUp pressed"));
        assert!(
            lines[1].contains("LeftStickY moved 30 times between -0.02 and +0.02"),
            "{}",
            lines[1]
        );
        assert!(
            lines[2].contains("LeftStickX moved 60 times"),
            "{}",
            lines[2]
        );
        assert!(lines[3].contains("RightStickX at +0.01"), "{}", lines[3]);
        assert!(lines[4].contains("DPadUp released"));
    }

    #[test]
    fn a_finished_session_hands_its_console_to_the_emulator() {
        use crate::play::MyGame;
        use crate::play::tests::{Pair, SNAKE, homebrew, snake};
        let mut emu = emulator("finish-session");
        let model = GbModel::for_rom(&Rom::new(SNAKE)).unwrap();
        emu.load_rom(&Rom::new(SNAKE), model, None).unwrap();
        let name = emu.game_name().to_owned();
        let mut pair = Pair::new(false, 0, 0, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(90);

        let now = web_time::Instant::now();
        emu.last_update = Some(now - std::time::Duration::from_millis(50));
        let session = pair.host.play.session_mut().unwrap();
        emu.advance_rollback(now, session);
        let shown = session.console(true).unwrap().frame().as_slice().to_vec();
        assert!(emu.linked_frame.as_deref() == Some(shown.as_slice()));

        pair.host.play.leave(&mut pair.host.online);
        let screen = pair
            .host
            .session()
            .console(true)
            .unwrap()
            .frame()
            .as_slice()
            .to_vec();
        let session = pair.host.play.close().unwrap();
        emu.finish_session(session).unwrap();
        assert!(emu.gb.frame().as_slice() == screen.as_slice());
        assert!(emu.linked_frame.is_none());
        assert!(emu.running);
        assert!(!emu.gb.link_plugged());
        assert_eq!(emu.game_name(), name);
    }
}
