use super::Citrine;
use crate::app::tabs::Tab;
use crate::app::viewer::{ItemKey, Source};
use crate::app::widgets::replay_bar::ReplayBarAction;
use crate::emulator::Emulator;
use crate::media::MediaId;
use citrine_gb::rom::Rom;
use citrine_gb_replay::{Replay, SeekPoints};
use std::sync::mpsc;

pub(super) const NO_GAME: &str = "Open the game once to play the replay";
pub(super) const OFF_IN_HARDCORE: &str = "Replays are off in hardcore";
pub(super) const LINKED: &str = "The game is linked to a friend, so here is the replay's video";

enum Got {
    Bytes(MediaId, Option<Vec<u8>>),
    Rom(MediaId, Box<Replay>, Option<Rom>),
}

pub(super) struct ReplayShow {
    parked: Option<Box<Emulator>>,
    tx: mpsc::Sender<Got>,
    rx: mpsc::Receiver<Got>,
}

impl Default for ReplayShow {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            parked: None,
            tx,
            rx,
        }
    }
}

impl ReplayShow {
    pub(super) fn is_open(&self) -> bool {
        self.parked.is_some()
    }

    pub(super) fn live(&self) -> Option<&Emulator> {
        self.parked.as_deref()
    }

    pub(super) fn park(&mut self, live: Box<Emulator>) {
        self.parked = Some(live);
    }

    pub(super) fn live_mut(&mut self) -> Option<&mut Emulator> {
        self.parked.as_deref_mut()
    }
}

impl Citrine {
    pub(super) fn tick_replay_show(&mut self) {
        if let Some(id) = self.ui.media.play.take() {
            self.ask_play_replay(id);
        }
        while let Ok(got) = self.replay_show.rx.try_recv() {
            match got {
                Got::Bytes(id, bytes) => self.read_replay(id, bytes),
                Got::Rom(id, replay, Some(rom)) => self.open_replay(id, *replay, &rom),
                Got::Rom(id, _, None) => self.show_replay_video(id, Some(NO_GAME)),
            }
        }
    }

    fn ask_play_replay(&mut self, id: MediaId) {
        let live = self.replay_show.live().unwrap_or(&self.emulator);
        if live.hardcore() {
            self.toasts.info(OFF_IN_HARDCORE);
            return;
        }
        if live.together || live.linked() || live.partnered() {
            self.show_replay_video(id, Some(LINKED));
            return;
        }
        let tx = self.replay_show.tx.clone();
        let key = id.clone();
        self.media.read_bytes(&id, move |bytes| {
            let _ = tx.send(Got::Bytes(key, bytes));
        });
    }

    fn read_replay(&mut self, id: MediaId, bytes: Option<Vec<u8>>) {
        let replay = match bytes.as_deref().map(Replay::read) {
            Some(Ok(replay)) => replay,
            Some(Err(err)) => {
                self.toasts
                    .error(format!("Could not open the replay: {err}"));
                return;
            }
            None => {
                self.toasts.error("Could not read the replay");
                return;
            }
        };
        if replay.header.no_inputs.is_some() {
            self.show_replay_video(id, None);
            return;
        }
        if let Some(rom) = self.replay_rom(&replay.header.sha1) {
            self.open_replay(id, replay, &rom);
            return;
        }
        let shelf = self.ui.playing.shelf(&self.ui.recent);
        let tx = self.replay_show.tx.clone();
        self.files.spawner.run("find the replay's game", move || {
            let rom = super::video_render::sha1_bytes(&replay.header.sha1)
                .and_then(|hash| shelf.find(&hash))
                .map(Rom::from);
            let _ = tx.send(Got::Rom(id, Box::new(replay), rom));
        });
    }

    fn show_replay_video(&mut self, id: MediaId, note: Option<&str>) {
        if let Some(note) = note {
            self.toasts.info(note);
        }
        self.ui.viewer.open(Source::Media, ItemKey::Media(id));
    }

    fn open_replay(&mut self, id: MediaId, replay: Replay, rom: &Rom) {
        if self.emulator.is_recording() {
            self.toggle_recording();
        }
        let base = self.replay_show.live().unwrap_or(&self.emulator);
        let mut show = match base.for_replay(replay, rom, SeekPoints::default()) {
            Ok(show) => Box::new(show),
            Err(err) => {
                log::error!("could not open replay {}: {err:?}", id.file_name());
                self.show_replay_video(
                    id,
                    Some("Could not start the replay, so here is its video"),
                );
                return;
            }
        };
        self.ui.settings.apply_emulator(&mut show);
        if self.replay_show.parked.is_some() {
            self.emulator.hand_audio_to(&mut show);
            self.emulator = show;
        } else {
            #[cfg(threaded)]
            self.emulator.follow_visibility(false, &self.threads);
            self.emulator.hand_audio_to(&mut show);
            let live = std::mem::replace(&mut self.emulator, show);
            self.replay_show.parked = Some(live);
        }
        #[cfg(threaded)]
        self.emulator.run_on(&self.threads);
        self.ui.viewer.close();
        self.events.open_tab(Tab::GameBoy);
    }

    pub(super) fn replay_bar_action(&mut self, action: ReplayBarAction) {
        match action {
            ReplayBarAction::Toggle => self.emulator.toggle_tape(),
            ReplayBarAction::Seek(to) => self.emulator.seek_tape(to),
            ReplayBarAction::Clip => self.save_a_clip(),
            ReplayBarAction::Close => self.close_replay_show(),
        }
    }

    pub(super) fn close_replay_show(&mut self) {
        let Some(mut live) = self.replay_show.parked.take() else {
            return;
        };
        self.emulator.hand_audio_to(&mut live);
        self.emulator = live;
        #[cfg(threaded)]
        self.emulator.follow_visibility(true, &self.threads);
    }

    pub(super) fn flush_parked_save(&mut self) {
        if let Some(live) = self.replay_show.live_mut()
            && let Err(err) = live.flush_save()
        {
            log::error!("failed to flush the parked game's save: {err:?}");
        }
    }

    fn replay_rom(&mut self, sha1: &str) -> Option<Rom> {
        if let Some(rom) = self.video_renders.tape_rom_for(sha1) {
            return Some(rom);
        }
        let live = self.replay_show.live().unwrap_or(&self.emulator);
        live.rom_with_sha1(sha1)
    }
}
