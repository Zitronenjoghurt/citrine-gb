use super::{Emulator, MAX_PLAY_STEP, TogetherPlay};
use crate::app::ui_state::keybinds::{BindingCapture, KeyBindings};
use crate::autosave::PlayClock;
use crate::controller_debug::PadLog;
use crate::held::{Change, InputSource};
use crate::local_link::{Game, LocalLink, Source};
use crate::pad_input::PadSource;
use crate::partner_link::PartnerLink;
use crate::play::{PlaySession, Returned};
use citrine_gb::console::{Console, ConsoleKind, Keys};
use citrine_gb::error::GbResult;
use citrine_gb::machine::Machine;
use citrine_gb::rom::Rom;
use citrine_gb_net::OnlineId;

impl Emulator {
    pub fn update_linked(
        &mut self,
        ctx: &egui::Context,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        log: Option<&mut PadLog>,
        mut session: Option<&mut PlaySession>,
    ) -> GbResult<()> {
        let solo = session.is_none() && !self.linked() && !self.partnered();
        self.clock.set_matching(self.match_screen && solo);
        self.clock
            .set_screen_hz(self.display.monitor.filter(|_| solo).map(|rate| rate.hz));
        if self.lent.is_some() {
            return self.finish_lent(ctx);
        }
        self.settle(ctx);
        let lent_frames = self.session_lent.take();
        if lent_frames.is_none() {
            self.update_avg_timer.start();
        }

        if self.texture.is_none() {
            self.update_texture(ctx);
        }

        if lent_frames.is_none() {
            self.handle_input(ctx, pads, bindings, capture, log);
        }

        if let Some(session) = session.as_deref_mut() {
            if let Some(back) = session.take_returned() {
                if back.paused {
                    self.handed_back = Some(back.friend);
                }
                self.adopt(back);
            }
            if session.wants_state() {
                if self.divert.is_some() {
                    session.mark_from_peer();
                }
                session.give_state(self.encoded_state()?, self.drop_history.take());
            }
            if session.runs_engine() {
                let ran = match lent_frames {
                    Some(true) => {
                        #[cfg(not(target_arch = "wasm32"))]
                        if !session.wait_frames(self.frame_wait) {
                            self.late_frames += 1;
                        }
                        self.finish_rollback(session)
                    }
                    Some(false) => false,
                    None => self.advance_rollback(web_time::Instant::now(), session),
                };
                if ran {
                    self.update_texture(ctx);
                    self.total_frames = self.total_frames.wrapping_add(1);
                }
                self.store_linked_saves(session)?;
                self.update_avg_timer.stop();
                return Ok(());
            }
            if session.holds_my_game() || session.watching_only() || session.shelves_mine() {
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
            self.continue_step(ctx);
            return Ok(());
        }
        self.drop_history = None;

        if self.advance(web_time::Instant::now(), cable) {
            self.update_texture(ctx);
            self.drain_audio();
            self.total_frames = self.total_frames.wrapping_add(1);
        }

        self.handle_save()?;

        self.update_avg_timer.stop();
        Ok(())
    }

    pub(super) fn begin_rollback(
        &mut self,
        ctx: &egui::Context,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        log: Option<&mut PadLog>,
        session: &mut PlaySession,
    ) {
        if !session.runs_engine() || !session.engine_on_worker() || self.texture.is_none() {
            return;
        }
        self.update_avg_timer.start();
        self.handle_input(ctx, pads, bindings, capture, log);
        let now = web_time::Instant::now();
        let frames = self.rollback_pads(now, session);
        self.session_lent = Some(frames.is_some());
        if let Some(frames) = frames {
            session.set_touch(self.touch);
            session.run_frames(frames, now);
        }
    }

    pub(super) fn advance_rollback(
        &mut self,
        now: web_time::Instant,
        session: &mut PlaySession,
    ) -> bool {
        let Some(frames) = self.rollback_pads(now, session) else {
            return false;
        };
        session.set_touch(self.touch);
        session.run_frames(frames, now);
        self.finish_rollback(session)
    }

    fn rollback_pads(
        &mut self,
        now: web_time::Instant,
        session: &mut PlaySession,
    ) -> Option<Vec<Keys>> {
        let dt = match self.last_update {
            Some(last) => now.duration_since(last).as_secs_f64(),
            None => 0.0,
        };
        self.last_update = Some(now);
        if !self.running && session.in_group() && !session.watching_only() {
            if let Some(feed) = &mut self.seat.core_mut().audio_feed {
                feed.set_paused(true);
            }
            return None;
        }
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
        let timing = session
            .console(true)
            .or_else(|| session.console(false))
            .map_or_else(|| self.timing(), Console::timing);
        self.clock.set_matching(false);
        self.clock.set_screen_hz(None);
        let due = self.clock.advance(dt, timing);
        let mut frames = Vec::with_capacity(due);
        for _ in 0..due {
            frames.push(self.held_buttons());
            let owed = self.held.take_owed();
            self.apply(Change {
                pressed: Keys::empty(),
                released: owed,
            });
        }
        Some(frames)
    }

    fn finish_rollback(&mut self, session: &mut PlaySession) -> bool {
        let ran = session.settle_frames();
        if let Some(timer) = session.frame_timer() {
            self.frame_avg_timer = timer.clone();
        }
        if let Some(console) = session.console(true).or_else(|| session.console(false)) {
            self.linked_video = console.video();
            self.linked_kind = console.kind();
            let frame = self.linked_frame.get_or_insert_with(Vec::new);
            frame.clear();
            frame.extend_from_slice(Console::frame(console));
        }
        #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
        if self.linked_frame.is_some() {
            self.draw_3d(session.shown_nds_mut());
        }
        let core = self.seat.core_mut();
        session.drain_audio(&mut core.linked_audio, &mut core.friend_audio);
        core.mixer
            .mix(self.sound, &mut core.linked_audio, &mut core.friend_audio);
        if let Some(feed) = &mut core.audio_feed {
            feed.set_paused(false);
            feed.set_session(true);
            feed.set_pace(1.0);
            feed.push(&core.linked_audio);
        }
        core.linked_audio.clear();
        ran
    }

    fn encoded_state(&self) -> GbResult<Vec<u8>> {
        match &self.seat.core().machine {
            Machine::Gb(gb) => citrine_gb::netplay::encode_state(gb.as_ref()),
            #[cfg(feature = "gba")]
            Machine::Gba(gba) => citrine_gb::netplay::encode_state(gba.as_ref()),
            #[cfg(feature = "nds")]
            Machine::Nds(nds) => citrine_gb::netplay::encode_state(nds.as_ref()),
            #[cfg(not(feature = "nds"))]
            #[allow(unreachable_patterns)]
            _ => Err(citrine_gb::error::GbError::NdsGame),
        }
    }

    pub(super) fn store_linked_saves(&mut self, session: &mut PlaySession) -> GbResult<()> {
        self.store_saves(session.take_saves())
    }

    pub fn keep_returned(&mut self, back: Returned) {
        self.adopt(back);
        self.play.play(std::time::Duration::from_secs(1));
        if let Err(err) = self.autosave_if_played() {
            log::error!("could not autosave the game put away: {err:?}");
        }
    }

    pub fn finish_session(&mut self, mut session: PlaySession) -> GbResult<bool> {
        let saves = session.finish_saves();
        self.store_saves(saves)?;
        let Some(back) = session.finish() else {
            if let Some(gb) = self.seat.core_mut().machine.as_gb_mut()
                && gb.link_plugged()
            {
                gb.unplug_link();
            }
            return Ok(false);
        };
        let paused = back.paused;
        self.adopt(back);
        Ok(paused)
    }

    pub fn holds_download(&self) -> bool {
        self.download_key.is_some() && self.download_key == self.rom_key
    }

    pub fn take_handed_back(&mut self) -> Option<OnlineId> {
        self.handed_back.take()
    }

    pub(super) fn adopt(&mut self, back: Returned) {
        let key = Some(match &back.borrowed {
            Some(hash) => borrowed_key(hash),
            None => back.console.rom_id().sha256_hex_string(),
        });
        let divert =
            (back.peer_touched && back.borrowed.is_none()).then(|| super::peer_saves::Divert {
                own: back.console.rom_id().sha256_hex_string(),
                borrowed: borrowed_key(&back.console.game_sha1()),
            });
        self.screen_recorder.stop();
        let same_game = key == self.rom_key;
        self.replace_machine(back.console);
        self.drop_history = back.history;
        self.linked_frame = None;
        let core = self.seat.core_mut();
        core.machine.finish_picture();
        core.screen.last_frame.clear();
        core.screen
            .last_frame
            .extend_from_slice(core.machine.frame());
        self.texture = None;
        self.download_key = key.clone().filter(|_| back.downloaded);
        self.rom_key = key;
        if let Some(divert) = divert {
            self.start_divert(back.friend, divert);
        }
        #[cfg(ds_wifi)]
        {
            self.ds_wifi.peer_console = back.peer_touched;
        }
        if !same_game {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.rom_path = None;
            }
            self.name_game(None);
        }
        self.sync_joypad();
        self.running = !back.paused;
        self.save_loaded = true;
        self.last_save = None;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.last_good.clear();
        self.achievements_restore(None);
    }

    pub fn start_partner(&mut self, partner: PartnerLink) -> GbResult<()> {
        self.refuse_in_hardcore()?;
        self.end_link()?;
        self.seat.core_mut().partner = Some(partner);
        self.attach_printer();
        Ok(())
    }

    pub fn start_link(
        &mut self,
        rom: &Rom,
        kind: ConsoleKind,
        file_name: Option<&str>,
    ) -> GbResult<()> {
        self.refuse_in_hardcore()?;
        self.end_link()?;
        let rate = self.sample_rate();
        let link = Some(LocalLink::start(
            rom,
            kind,
            &self.firmware,
            file_name,
            &self.store,
            rate,
            self.rom_key.as_deref(),
        )?);
        self.seat.core_mut().link = link;
        self.release_all();
        self.attach_printer();
        Ok(())
    }

    pub fn end_link(&mut self) -> GbResult<()> {
        let core = self.seat.core_mut();
        if let Some(gb) = core.machine.as_gb_mut()
            && let Some(partner) = core.partner.take()
        {
            partner.finish(gb);
        }
        let result = match core.link.take() {
            Some(link) => link.finish(
                &mut core.machine,
                &self.store,
                &mut self.writes,
                self.autosave_keep,
            ),
            None => Ok(()),
        };
        self.attach_printer();
        result
    }

    pub fn set_link_control(&mut self, game: Game) {
        let moved = self.seat.core_mut().link.as_mut().is_some_and(|link| {
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

    pub(super) fn held_of(&mut self, pad: usize, source: InputSource) -> Keys {
        if let Some(link) = &mut self.seat.core_mut().link
            && link.game_for(Source::Pad(pad)) == Game::Second
        {
            return link.held_of(source);
        }
        self.held.of(source)
    }
}

pub(super) fn borrowed_key(hash: &citrine_gb_net::GameHash) -> String {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}{hex}", crate::storage::BORROWED)
}

pub(super) fn together_name(card: &citrine_gb_net::GameCard) -> String {
    crate::game_name::known_name(&card.hash)
        .map(str::to_string)
        .unwrap_or_else(|| card.title.clone())
}

pub(super) fn linked_key(session: &PlaySession) -> Option<String> {
    if session.borrowing()
        && let Some(card) = session.my_game()
    {
        return Some(borrowed_key(&card.hash));
    }
    session
        .console(true)
        .map(|console| console.rom_id().sha256_hex_string())
}
