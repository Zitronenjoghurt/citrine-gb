use citrine_gb::error::GbResult;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::netplay::{
    Advance, CableEvent, Check, ConsoleSetup, Input, Player, Session, SessionConfig,
};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb_net::{Origin, Pad, PlayMsg, Start};
use std::time::Duration;
use web_time::Instant;

const FRAME_MS: f64 = 70_224.0 * 1000.0 / 4_194_304.0;
const SAVE_EVERY: u32 = 60;
const SKIP_EVERY: u32 = 6;
const AHEAD_LIMIT: f64 = 1.5;
const WATCH_BEHIND: u32 = 6;

pub type Setup = (Vec<u8>, GbModel, Origin);

pub struct Rollback {
    session: Session,
    round: u32,
    host: bool,
    authority: bool,
    watching_only: bool,
    watched_only: bool,
    hidden: bool,
    delay: u32,
    filled: u32,
    unsent_from: u32,
    unsent: Vec<Pad>,
    cable: Option<CableEvent>,
    cable_at: Option<u32>,
    plug_at_start: bool,
    ahead: f64,
    skips: u32,
    stalled_since: Option<Instant>,
    answered: Option<u32>,
    resyncs: u32,
    outgoing: Vec<PlayMsg>,
    save: Option<SramDump>,
    #[cfg(test)]
    pub log: Log,
}

#[cfg(test)]
#[derive(Default, Clone)]
pub struct Log {
    pub local: Vec<Option<Input>>,
    pub remote: Vec<Option<Input>>,
    pub checks: Vec<Check>,
    pub resynced: Vec<u32>,
    pub started: [Option<Vec<u8>>; 2],
}

#[cfg(test)]
impl Log {
    fn put(list: &mut Vec<Option<Input>>, frame: u32, input: Input) {
        let at = frame as usize;
        if list.len() <= at {
            list.resize(at + 1, None);
        }
        list[at] = Some(input);
    }
}

impl Rollback {
    pub fn new(
        local: Player,
        start: Start,
        games: [Option<Setup>; 2],
        sample_rate: u32,
        now: Instant,
    ) -> GbResult<Self> {
        let roms = games
            .each_ref()
            .map(|game| game.as_ref().map(|(rom, _, _)| Rom::new(rom)));
        let setups = [0, 1].map(|i| {
            let (Some(rom), Some((_, model, origin))) = (&roms[i], &games[i]) else {
                return None;
            };
            let (save, state) = match origin {
                Origin::Boot(save) => (save.as_deref().map(SramDump::from_slice), None),
                Origin::Resume(state) => (None, Some(state.as_slice())),
            };
            Some(ConsoleSetup {
                rom,
                model: *model,
                save,
                state,
            })
        });
        let present = [setups[0].is_some(), setups[1].is_some()];
        let config = SessionConfig {
            power_on: start.power_on,
            sample_rate,
            ..SessionConfig::new(local, start.seed, start.start_time)
        };
        let session = Session::new(config, setups)?;
        let _ = now;
        #[cfg(test)]
        let started = [Player::One, Player::Two].map(|player| {
            session
                .present(player)
                .then(|| citrine_gb::netplay::encode_state(session.console(player)).ok())
                .flatten()
        });
        let host = local == Player::One;
        let me = present[local.index()];
        let friend = present[local.other().index()];
        Ok(Self {
            session,
            round: start.round,
            host,
            authority: if me && friend { host } else { me },
            watching_only: !me,
            watched_only: me && !friend,
            hidden: false,
            delay: start.delay as u32,
            filled: 0,
            unsent_from: 0,
            unsent: Vec::new(),
            cable: None,
            cable_at: start.plugged.then_some(0),
            plug_at_start: start.plugged,
            ahead: 0.0,
            skips: 0,
            stalled_since: None,
            answered: None,
            resyncs: 0,
            outgoing: Vec::new(),
            save: None,
            #[cfg(test)]
            log: Log {
                started,
                ..Log::default()
            },
        })
    }

    pub fn frame(&self) -> u32 {
        self.session.frame()
    }

    pub fn resyncs(&self) -> u32 {
        self.resyncs
    }

    pub fn cable_plugged(&self) -> bool {
        match self.cable {
            Some(CableEvent::Plug) => true,
            Some(CableEvent::Pull) => false,
            None => self.session.cable_plugged(),
        }
    }

    pub fn console(&self, local: bool) -> Option<&GameBoy> {
        let me = self.session.local();
        let player = if local { me } else { me.other() };
        self.session
            .present(player)
            .then(|| self.session.console(player))
    }

    pub fn stalled_for(&self, now: Instant) -> Duration {
        self.stalled_since
            .map_or(Duration::ZERO, |at| now.duration_since(at))
    }

    pub fn watch(&mut self, on: bool) {
        self.session.watch_remote(on);
    }

    pub fn show(&mut self, on: bool) {
        self.hidden = self.watched_only && !on;
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn cable_settling(&self) -> bool {
        self.cable.is_some() || self.cable_at.is_some_and(|at| at >= self.session.frame())
    }

    pub fn request_cable(&mut self, plugged: bool) {
        if plugged == self.session.cable_plugged() {
            self.cable = None;
        } else {
            self.cable = Some(if plugged {
                CableEvent::Plug
            } else {
                CableEvent::Pull
            });
        }
    }

    pub fn drain_audio(&mut self, mine: &mut Vec<f32>, friend: &mut Vec<f32>) {
        self.session.drain_audio(mine);
        self.session.drain_remote_audio(friend);
    }

    pub fn take_save(&mut self) -> Option<SramDump> {
        self.save.take()
    }

    pub fn final_save(&mut self) -> Option<SramDump> {
        self.save.take().or_else(|| self.session.local_save(false))
    }

    #[cfg(test)]
    pub fn cable_transfers(&self) -> [u64; 2] {
        self.session.cable_transfers()
    }

    #[cfg(test)]
    pub fn confirmed_frame(&self) -> u32 {
        self.session.confirmed_frame()
    }

    pub fn into_console(self, confirmed: bool) -> Option<GameBoy> {
        let local = self.session.local();
        let present = self.session.present(local);
        match confirmed {
            _ if !present => None,
            true => Some(self.session.into_confirmed_console(local)),
            false => Some(self.session.into_console(local)),
        }
    }

    pub fn tick(
        &mut self,
        buttons: JoypadState,
        round_trip_ms: u32,
        now: Instant,
        outbox: &mut Vec<PlayMsg>,
    ) -> bool {
        if self.watching_only {
            return self.follow(now);
        }
        let frame = self.session.frame();
        let heard = self.session.remote_frame();
        if heard > 0 {
            let remote_now =
                heard as f64 - self.delay as f64 - 1.0 + round_trip_ms as f64 / FRAME_MS / 2.0;
            self.ahead = self.ahead * 0.9 + (frame as f64 - remote_now) * 0.1;
            if self.ahead > AHEAD_LIMIT {
                self.skips += 1;
                if self.skips.is_multiple_of(SKIP_EVERY) {
                    return false;
                }
            }
        }
        while self.filled <= frame + self.delay {
            let at = self.filled;
            let mut input = if at < self.delay {
                Input::default()
            } else {
                Input::new(buttons)
            };
            if at == 0 && self.host && self.plug_at_start {
                input = input.with_cable(CableEvent::Plug);
            } else if at >= self.delay
                && let Some(event) = self.cable.take()
            {
                input = input.with_cable(event);
                self.cable_at = Some(at);
            }
            if self.session.add_local_input(at, input).is_err() {
                break;
            }
            #[cfg(test)]
            Log::put(&mut self.log.local, at, input);
            if self.unsent.is_empty() {
                self.unsent_from = at;
            }
            self.unsent.push(pad_of(input));
            self.filled += 1;
        }
        let ran = self.session.advance() == Advance::Ran;
        if ran {
            self.stalled_since = None;
            if self.session.frame().is_multiple_of(SAVE_EVERY)
                && let Some(save) = self.session.local_save(false)
            {
                self.save = Some(save);
            }
        } else {
            self.stalled_since.get_or_insert(now);
        }
        self.collect();
        self.flush(outbox);
        ran
    }

    fn follow(&mut self, now: Instant) -> bool {
        let behind = self
            .session
            .remote_frame()
            .saturating_sub(self.session.frame());
        let runs = if behind > WATCH_BEHIND { 2 } else { 1 };
        let mut ran = false;
        for _ in 0..runs {
            if self.session.advance() != Advance::Ran {
                break;
            }
            ran = true;
        }
        if ran {
            self.stalled_since = None;
        } else {
            self.stalled_since.get_or_insert(now);
        }
        self.collect();
        ran
    }

    fn collect(&mut self) {
        while let Some(check) = self.session.take_check() {
            #[cfg(test)]
            self.log.checks.push(check);
            self.outgoing.push(PlayMsg::Check {
                round: self.round,
                frame: check.frame,
                hashes: check.hashes,
            });
        }
        if !self.authority {
            return;
        }
        let Some(desync) = self.session.desync() else {
            return;
        };
        if self.answered == Some(desync.frame) {
            return;
        }
        self.answered = Some(desync.frame);
        for player in [Player::One, Player::Two] {
            if desync.differs(player)
                && let Some(data) = self.session.checkpoint(desync.frame, player)
            {
                self.outgoing.push(PlayMsg::Resync {
                    round: self.round,
                    frame: desync.frame,
                    player: player.index() as u8,
                    data,
                });
            }
        }
    }

    pub fn flush(&mut self, outbox: &mut Vec<PlayMsg>) {
        if self.hidden {
            self.unsent.clear();
            self.outgoing.clear();
            return;
        }
        if !self.unsent.is_empty() {
            outbox.push(PlayMsg::Pads {
                round: self.round,
                first: self.unsent_from,
                pads: std::mem::take(&mut self.unsent),
            });
        }
        outbox.append(&mut self.outgoing);
    }

    pub fn receive(&mut self, msg: PlayMsg) {
        match msg {
            PlayMsg::Pads { round, first, pads } if round == self.round => {
                for (i, pad) in pads.into_iter().enumerate() {
                    let frame = first.wrapping_add(i as u32);
                    let input = input_of(pad);
                    #[cfg(test)]
                    Log::put(&mut self.log.remote, frame, input);
                    self.session.add_remote_input(frame, input);
                }
            }
            PlayMsg::Check {
                round,
                frame,
                hashes,
            } if round == self.round => {
                self.session.add_remote_check(Check { frame, hashes });
                self.collect();
            }
            PlayMsg::Resync {
                round,
                frame,
                player,
                data,
            } if round == self.round && !self.authority => {
                let player = if player == 0 {
                    Player::One
                } else {
                    Player::Two
                };
                if self.session.resync(frame, player, &data).is_ok() {
                    self.resyncs += 1;
                    #[cfg(test)]
                    self.log.resynced.push(frame);
                }
            }
            _ => {}
        }
    }
}

fn pad_of(input: Input) -> Pad {
    Pad {
        buttons: input.buttons.bits(),
        cable: input.cable.map(|event| event == CableEvent::Plug),
    }
}

fn input_of(pad: Pad) -> Input {
    let input = Input::new(JoypadState::from_bits_retain(pad.buttons));
    match pad.cable {
        Some(true) => input.with_cable(CableEvent::Plug),
        Some(false) => input.with_cable(CableEvent::Pull),
        None => input,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_carry_buttons_and_cable_events_both_ways() {
        for input in [
            Input::default(),
            Input::new(JoypadState::A | JoypadState::LEFT),
            Input::new(JoypadState::all()).with_cable(CableEvent::Plug),
            Input::new(JoypadState::DOWN).with_cable(CableEvent::Pull),
        ] {
            assert_eq!(input_of(pad_of(input)), input);
        }
    }
}
