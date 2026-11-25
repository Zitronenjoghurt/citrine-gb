use super::fast_forward::Spending;
use super::gimmicks::Catch;
use super::rewind::{Rewind, RewindPlan};
use super::tape_play::{SEEK_BUDGET, TapePlay};
use crate::app::ui_state::keybinds::{DsButton, GbButton, Shoulder};
use crate::audio::mix::{Mixer, SoundLevels};
use crate::audio::pacer::{AudioFeed, FastForwardSound};
use crate::audio::preview::Playing;
use crate::game_report::flight::Flight;
use crate::local_link::LocalLink;
use crate::partner_link::PartnerLink;
use crate::utils::avg_timer::AvgTimer;
use citrine_gb::cheats::Cheat;
use citrine_gb::console::{Console, Keys, VideoSpec};
use citrine_gb::debug::Debug;
use citrine_gb::error::GbResult;
use citrine_gb::machine::Machine;
use citrine_gb::persistence::battery_save::BatterySave;
use citrine_gb::persistence::state::StateBody;
use citrine_gb_replay::{Input, Replay};
use citrine_gb_threads::Split;
use strum::IntoEnumIterator;

const PREVIEW_RATE: u32 = 48_000;

pub(crate) struct Core {
    pub(crate) machine: Machine,
    pub(crate) link: Option<LocalLink>,
    pub(crate) partner: Option<PartnerLink>,
    #[cfg(feature = "nds")]
    pub(crate) walker: Option<crate::walker::WalkerLink>,
    #[cfg(ds_wifi)]
    pub(crate) wifi: Option<crate::ds_wifi::Network>,
    pub(crate) audio: Vec<f32>,
    pub(crate) audio_feed: Option<AudioFeed>,
    #[cfg(feature = "nds")]
    pub(crate) mic: Option<crate::audio::mic::MicLine>,
    #[cfg(feature = "nds")]
    mic_heard: Vec<i16>,
    pub(crate) preview: Option<Playing>,
    pub(crate) mixer: Mixer,
    pub(crate) linked_audio: Vec<f32>,
    pub(crate) friend_audio: Vec<f32>,
    pub(crate) catch: Catch,
    pub(crate) frame_timer: AvgTimer,
    pub(crate) screen: Screen,
    pub(crate) ran: Option<Ran>,
    pub(crate) taken: Option<Taken>,
    pub(crate) flight: Flight,
    cheats: Vec<Cheat>,
    pub(crate) rewind: Rewind,
    pub(crate) cut_recording: Option<Replay>,
    pub(crate) clock_hold: ClockHold,
    pub(crate) tape_play: Option<TapePlay>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) achievements: Option<citrine_gb_achievements::Client>,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ClockHold {
    ahead: f64,
}

impl ClockHold {
    pub(crate) fn ran(
        &mut self,
        frames: usize,
        frame_secs: f64,
        pace: f64,
        hold: impl FnOnce(f64) -> f64,
    ) {
        if pace > 1.0 {
            self.ahead += frames as f64 * frame_secs * (1.0 - 1.0 / pace);
        }
        if self.ahead > 0.0 {
            self.ahead -= hold(self.ahead);
        }
    }
}

pub(crate) struct Taken {
    pub(crate) state: StateBody,
    pub(crate) frame: Vec<u8>,
    pub(crate) battery: Option<BatterySave>,
}

impl Taken {
    pub(crate) fn of(machine: &mut Machine, now: u64) -> GbResult<Self> {
        let battery = machine.battery(true, now);
        Ok(Self {
            state: machine.state_body(now)?,
            frame: machine.frame().to_vec(),
            battery,
        })
    }
}

#[derive(Default)]
pub(crate) struct Screen {
    pub(crate) last_frame: Vec<u8>,
    pub(crate) painted: Vec<u8>,
    pub(crate) size: [usize; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Effects {
    pub(crate) matrix: bool,
    pub(crate) edge: f32,
    pub(crate) corner: f32,
    pub(crate) ghosting: bool,
    pub(crate) blend: f32,
    pub(crate) scale: usize,
}

pub(crate) struct Plan {
    pub(crate) frames: usize,
    pub(crate) held: Keys,
    pub(crate) owed: Keys,
    pub(crate) effects: Effects,
    pub(crate) sound: SoundLevels,
    pub(crate) fast_forward: bool,
    pub(crate) take: bool,
    pub(crate) rewind: RewindPlan,
    pub(crate) budget: Option<std::time::Duration>,
    pub(crate) defer_3d: bool,
    pub(crate) pace: f64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ran {
    pub(crate) frames: usize,
    pub(crate) hit: bool,
    pub(crate) finished: Option<web_time::Instant>,
}

pub(crate) trait Hooks {
    fn before_frame(&mut self, _machine: &mut Machine) {}

    fn record(&mut self, _cycles: u128, _key: Keys, _pressed: bool) {}

    fn captured(&mut self, _machine: &Machine, _fresh: &[f32]) {}
}

pub(crate) struct NoHooks;

impl Hooks for NoHooks {}

impl Core {
    pub(crate) fn new(machine: Machine) -> Self {
        let video = machine.video();
        Self {
            machine,
            link: None,
            partner: None,
            #[cfg(feature = "nds")]
            walker: None,
            #[cfg(ds_wifi)]
            wifi: None,
            audio: Vec::new(),
            audio_feed: None,
            #[cfg(feature = "nds")]
            mic: None,
            #[cfg(feature = "nds")]
            mic_heard: Vec::new(),
            preview: None,
            mixer: Mixer::default(),
            linked_audio: Vec::new(),
            friend_audio: Vec::new(),
            catch: Catch::default(),
            frame_timer: AvgTimer::default(),
            screen: Screen {
                last_frame: vec![0; video.width * video.height * 4],
                ..Screen::default()
            },
            ran: None,
            taken: None,
            flight: Flight::default(),
            cheats: Vec::new(),
            rewind: Rewind::default(),
            cut_recording: None,
            #[cfg(not(target_arch = "wasm32"))]
            achievements: None,
            clock_hold: ClockHold::default(),
            tape_play: None,
        }
    }

    pub(crate) fn step(&mut self, plan: &Plan, split: &mut Split<'_>) -> Ran {
        let ran = self.run(plan, &mut NoHooks, split);
        self.taken = if plan.take {
            Taken::of(&mut self.machine, crate::storage::now_unix())
                .inspect_err(|err| {
                    log::error!("could not copy the state on the console thread: {err:?}")
                })
                .ok()
        } else {
            None
        };
        if ran.frames > 0 {
            self.paint(plan.effects, None);
            self.drain_audio(plan.sound, plan.pace, plan.fast_forward, plan.rewind.back);
        }
        ran
    }

    fn run_tape(&mut self, plan: &Plan, hooks: &mut impl Hooks, split: &mut Split<'_>) -> Ran {
        let Some(mut play) = self.tape_play.take() else {
            return Ran::default();
        };
        let wanted = play.wanted();
        let seeking = wanted > 0;
        self.machine.set_muted(
            seeking || (plan.fast_forward && plan.sound.fast == FastForwardSound::Silent),
        );
        let (frames, budget) = if seeking {
            (wanted, Some(SEEK_BUDGET))
        } else {
            (plan.frames, plan.budget)
        };
        let mut ran = Ran::default();
        let mut heard = self.audio.len();
        let mut spending = Spending::start(frames, budget);
        while !play.is_done() && spending.next() {
            let frame_started = web_time::Instant::now();
            self.machine
                .defer_3d(plan.defer_3d && !play.keeps_next_frame());
            self.frame_timer.start();
            play.step(&mut self.machine, |machine| tape_frame(machine, split));
            self.frame_timer.stop();
            ran.frames += 1;
            self.machine.take_audio(&mut self.audio);
            hooks.captured(&self.machine, &self.audio[heard.min(self.audio.len())..]);
            heard = self.audio.len();
            spending.frame_took(frame_started.elapsed());
        }
        self.tape_play = Some(play);
        ran
    }

    pub(crate) fn run_plain_frame(&mut self, split: &mut Split<'_>) {
        #[cfg(feature = "nds")]
        if let Machine::Nds(nds) = &mut self.machine {
            let mut no_link = citrine_gb::nds::ir::NoLink;
            let port: &mut dyn citrine_gb::nds::ir::IrPort = match self.walker.as_mut() {
                Some(walker) => walker,
                None => &mut no_link,
            };
            #[cfg(ds_wifi)]
            if let Some(network) = self.wifi.as_mut() {
                nds.run_frame_on(port, network, split);
                return;
            }
            nds.run_frame_on(port, &mut citrine_gb::nds::wifi::air::NoPeer, split);
            return;
        }
        #[cfg(not(feature = "nds"))]
        let _ = split;
        self.machine.run_frame();
    }

    pub(crate) fn run(
        &mut self,
        plan: &Plan,
        hooks: &mut impl Hooks,
        split: &mut Split<'_>,
    ) -> Ran {
        if self.tape_play.is_some() {
            return self.run_tape(plan, hooks, split);
        }
        if self.follow_rewind(plan.rewind) {
            return self.run_back(plan.frames);
        }
        self.land_rewind();
        self.machine
            .set_muted(plan.fast_forward && plan.sound.fast == FastForwardSound::Silent);
        let mut ran = Ran::default();
        let mut heard = self.audio.len();
        let mut spending = Spending::start(plan.frames, plan.budget);
        while spending.next() {
            let frame_started = web_time::Instant::now();
            crate::profile::scope!("console frame");
            hooks.before_frame(&mut self.machine);
            self.flight
                .before_frame(&mut self.machine, crate::storage::now_unix());
            self.rewind.record(Input::Keys(self.machine.keys().bits()));
            if !plan.fast_forward {
                self.hold_clock(plan.pace);
            }
            self.machine.apply_cheats(&self.cheats);
            #[cfg(feature = "nds")]
            self.feed_mic();
            let rewinds = plan.rewind.keep.is_some() && self.rewind.captures_next();
            self.machine
                .defer_3d(plan.defer_3d && !rewinds && !self.flight.shoots_next());
            self.frame_timer.start();
            match (self.link.as_mut(), self.partner.as_mut()) {
                (Some(link), _) => link.run_frame(&mut self.machine, plan.held),
                (None, Some(partner)) => match self.machine.as_gb_mut() {
                    Some(gb) => partner.run_frame(gb),
                    None => self.machine.run_frame(),
                },
                (None, None) => self.run_plain_frame(split),
            }
            self.frame_timer.stop();
            self.flight.after_frame(&mut self.machine);
            self.catch.collect(&mut self.machine);
            #[cfg(not(target_arch = "wasm32"))]
            super::achievements::after_frame(&mut self.achievements, &self.machine);
            let owed = if ran.frames == 0 {
                plan.owed
            } else {
                Keys::empty()
            };
            ran.frames += 1;
            if plan.rewind.keep.is_some() {
                #[cfg(not(target_arch = "wasm32"))]
                let progress = || self.achievements.as_mut()?.progress();
                #[cfg(target_arch = "wasm32")]
                let progress = || None;
                self.rewind.after_frame(&mut self.machine, progress);
            }
            if !owed.is_empty() {
                apply(&mut self.machine, Keys::empty(), owed, hooks);
                self.rewind.record(Input::Keys(self.machine.keys().bits()));
            }
            self.machine.take_audio(&mut self.audio);
            hooks.captured(&self.machine, &self.audio[heard.min(self.audio.len())..]);
            heard = self.audio.len();
            spending.frame_took(frame_started.elapsed());
            if self.machine.take_breakpoint_hit() {
                ran.hit = true;
                break;
            }
        }
        ran
    }

    fn hold_clock(&mut self, pace: f64) {
        let frame_secs = self.machine.timing().frame_secs();
        let machine = &mut self.machine;
        let rewind = &mut self.rewind;
        self.clock_hold.ran(1, frame_secs, pace, |secs| {
            let held = machine.hold_clock(secs);
            rewind.record(Input::HoldClock(secs));
            held
        });
    }

    #[cfg(feature = "nds")]
    fn feed_mic(&mut self) {
        if !matches!(self.machine, Machine::Nds(_)) {
            return;
        }
        match &self.mic {
            Some(line) => line.frame(self.machine.timing().frame_secs(), &mut self.mic_heard),
            None => self.mic_heard.clear(),
        }
        self.rewind
            .feed(&mut self.machine, Input::Mic(self.mic_heard.clone()));
    }

    #[cfg(threaded)]
    pub(crate) fn run_rewind_gaps_on(&mut self, spawner: citrine_gb_threads::Spawner) {
        self.rewind.run_gaps_on(spawner);
    }

    pub(crate) fn feed(&mut self, input: Input) {
        if self.tape_play.is_some() {
            return;
        }
        if matches!(
            input,
            Input::Reset | Input::SetClock(_) | Input::ShiftClock(..)
        ) {
            self.land_rewind();
        }
        self.rewind.feed(&mut self.machine, input);
    }

    pub(crate) fn set_cheats(&mut self, codes: Vec<String>) {
        let kind = self.machine.kind();
        self.cheats = codes
            .iter()
            .filter_map(|code| Cheat::parse(kind, code).ok())
            .collect();
        self.rewind.record(Input::Cheats(codes));
    }

    fn follow_rewind(&mut self, plan: RewindPlan) -> bool {
        let Some(seconds) = plan.keep else {
            self.rewind.clear();
            return false;
        };
        let fps = 1.0 / self.machine.timing().frame_secs();
        self.rewind
            .follow(self.machine.kind(), seconds, plan.reach, fps);
        plan.back
    }

    fn run_back(&mut self, frames: usize) -> Ran {
        let mut ran = Ran::default();
        for _ in 0..frames {
            if !self.rewind.step_back(&self.machine) {
                break;
            }
            ran.frames += 1;
        }
        self.drop_audio();
        ran
    }

    pub(crate) fn land_rewind(&mut self) {
        if !self.rewind.is_rewound() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(progress) = self.rewind.landing_progress()
                && let Some(client) = self.achievements.as_mut()
            {
                client.restore_progress(progress);
            }
            let achievements = &mut self.achievements;
            self.rewind.land(&mut self.machine, |_, machine| {
                super::achievements::after_frame(achievements, machine)
            });
        }
        #[cfg(target_arch = "wasm32")]
        self.rewind.land(&mut self.machine, |_, _| {});
    }

    pub(crate) fn start_recording(&mut self, now: u64, plan: RewindPlan) -> bool {
        self.follow_rewind(plan);
        self.land_rewind();
        #[cfg(not(target_arch = "wasm32"))]
        let progress = self
            .achievements
            .as_mut()
            .and_then(|client| client.progress());
        #[cfg(target_arch = "wasm32")]
        let progress = None;
        match self
            .rewind
            .start_recording(&mut self.machine, now, progress)
        {
            Ok(started) => started,
            Err(err) => {
                log::error!("could not start a recording: {err:?}");
                false
            }
        }
    }

    pub(crate) fn restart_recording(&mut self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        let achievements = &mut self.achievements;
        #[cfg(not(target_arch = "wasm32"))]
        let restore = |progress: Option<&[u8]>| {
            if let Some(client) = achievements.as_mut() {
                client.restore_progress(progress);
            }
        };
        #[cfg(target_arch = "wasm32")]
        let restore = |_: Option<&[u8]>| {};
        let restarted = match self.rewind.restart_recording(&mut self.machine, restore) {
            Ok(restarted) => restarted,
            Err(err) => {
                log::error!("could not start the recording over: {err:?}");
                self.rewind.clear();
                false
            }
        };
        self.drop_audio();
        restarted
    }

    pub(crate) fn stop_recording(&mut self) -> Option<Replay> {
        self.land_rewind();
        self.rewind.stop_recording(&self.machine)
    }

    pub(crate) fn clear_rewind(&mut self) {
        if let Some(replay) = self.stop_recording() {
            self.cut_recording = Some(replay);
        }
        self.rewind.clear();
    }

    pub(crate) fn put_machine(&mut self, machine: Machine) {
        self.clear_rewind();
        self.machine = machine;
        self.flight.restart();
        self.follow_feed_rate();
    }

    pub(crate) fn follow_feed_rate(&mut self) {
        if let Some(feed) = &self.audio_feed {
            self.machine.set_sample_rate(feed.sample_rate());
        }
    }

    pub(crate) fn drop_audio(&mut self) {
        self.machine.take_audio(&mut self.audio);
        self.audio.clear();
    }

    pub(crate) fn drain_audio(
        &mut self,
        sound: SoundLevels,
        pace: f64,
        fast_forward: bool,
        rewinding: bool,
    ) {
        let sped_up = fast_forward && !rewinding && sound.fast == FastForwardSound::SpedUp;
        if (fast_forward || rewinding) && !sped_up {
            self.drop_audio();
            if let Some(link) = &mut self.link {
                link.clear_audio();
            }
            if let Some(feed) = &self.audio_feed {
                feed.set_paused(true);
            }
            return;
        }
        self.machine.take_audio(&mut self.audio);
        let mut samples = std::mem::take(&mut self.audio);
        if self.render_preview(&mut samples) {
            if let Some(link) = &mut self.link {
                link.clear_audio();
            }
            self.friend_audio.clear();
        } else if let Some(link) = &mut self.link {
            link.mix(&mut samples);
        }
        if sped_up && let Some(feed) = &self.audio_feed {
            squeeze(
                &mut samples,
                feed.target_frames().saturating_sub(feed.queued_frames()),
            );
        }
        self.mixer.mix(sound, &mut samples, &mut self.friend_audio);
        if let Some(feed) = &mut self.audio_feed {
            feed.set_paused(false);
            feed.set_session(false);
            feed.set_slow_sound(sound.slow);
            feed.set_pace(pace);
            feed.push(&samples);
        }
        samples.clear();
        self.audio = samples;
    }

    pub(crate) fn play_preview(&mut self, sound: SoundLevels) -> bool {
        let Some(feed) = &self.audio_feed else {
            return false;
        };
        if self.preview.is_none() {
            return false;
        }
        let frames = feed.target_frames().saturating_sub(feed.queued_frames());
        let mut samples = std::mem::take(&mut self.audio);
        samples.clear();
        samples.resize(frames * 2, 0.0);
        self.render_preview(&mut samples);
        self.friend_audio.clear();
        self.mixer.mix(sound, &mut samples, &mut self.friend_audio);
        if let Some(feed) = &mut self.audio_feed {
            feed.set_paused(false);
            feed.set_slow_sound(sound.slow);
            feed.push(&samples);
        }
        samples.clear();
        self.audio = samples;
        true
    }

    fn render_preview(&mut self, samples: &mut Vec<f32>) -> bool {
        let rate = self
            .audio_feed
            .as_ref()
            .map_or(PREVIEW_RATE, AudioFeed::sample_rate);
        let Some(playing) = &mut self.preview else {
            return false;
        };
        let frames = samples.len() / 2;
        samples.clear();
        if !playing.render(frames, rate, samples) {
            self.preview = None;
        }
        true
    }

    pub(crate) fn paint(&mut self, effects: Effects, linked: Option<(&[u8], VideoSpec)>) {
        let Self {
            machine,
            screen,
            rewind,
            ..
        } = self;
        let (source, video) = linked
            .or_else(|| rewind.picture())
            .unwrap_or_else(|| (machine.frame(), machine.video()));
        screen.blend(source, effects);
        let mut painted = std::mem::take(&mut screen.painted);
        paint(
            &screen.last_frame,
            video,
            backdrop(machine),
            effects,
            effects.scale,
            &mut painted,
        );
        screen.painted = painted;
        screen.size = [video.width * effects.scale, video.height * effects.scale];
    }
}

impl Screen {
    fn blend(&mut self, source: &[u8], effects: Effects) {
        self.last_frame.resize(source.len(), 0);
        if effects.ghosting {
            let blend = effects.blend;
            for (last, &current) in self.last_frame.iter_mut().zip(source) {
                *last = (current as f32 * blend + *last as f32 * (1.0 - blend)) as u8;
            }
        } else {
            self.last_frame.copy_from_slice(source);
        }
        for alpha in self.last_frame.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
    }
}

pub(crate) fn apply(machine: &mut Machine, pressed: Keys, released: Keys, hooks: &mut impl Hooks) {
    let total_cycles = Debug::cycles(machine);
    let buttons = GbButton::iter().map(GbButton::key);
    let extra = Shoulder::iter()
        .map(Shoulder::key)
        .chain(DsButton::iter().map(DsButton::key))
        .chain([Keys::LID]);
    for joypad in buttons.chain(extra) {
        if pressed.contains(joypad) {
            machine.set_keys(machine.keys() | joypad);
            hooks.record(total_cycles, joypad, true);
        } else if released.contains(joypad) {
            machine.set_keys(machine.keys() - joypad);
            hooks.record(total_cycles, joypad, false);
        }
    }
}

fn squeeze(samples: &mut Vec<f32>, room: usize) {
    let frames = samples.len() / 2;
    if frames <= room {
        return;
    }
    for out in 0..room {
        let start = out * frames / room;
        let end = ((out + 1) * frames / room).max(start + 1);
        let (mut left, mut right) = (0.0, 0.0);
        for [l, r] in samples[start * 2..end * 2].as_chunks::<2>().0 {
            left += l;
            right += r;
        }
        let count = (end - start) as f32;
        samples[out * 2] = left / count;
        samples[out * 2 + 1] = right / count;
    }
    samples.truncate(room * 2);
}

pub(crate) fn backdrop(machine: &Machine) -> [u8; 3] {
    machine.as_gb().map_or([0; 3], |gb| {
        let color = gb.ppu.backdrop_color();
        [color.r(), color.g(), color.b()]
    })
}

pub(crate) fn paint(
    source: &[u8],
    video: VideoSpec,
    backdrop: [u8; 3],
    effects: Effects,
    scale: usize,
    out: &mut Vec<u8>,
) {
    let VideoSpec { width, height, .. } = video;
    let upscaled_width = width * scale;
    out.resize(upscaled_width * height * scale * 4, 0);
    if out.is_empty() {
        return;
    }
    if !super::screen::draws_grid(effects.matrix, scale) {
        replicate(source, width, scale, out);
        return;
    }
    let toward_backdrop = |pixel: [u8; 3], t: f32| {
        [
            (pixel[0] as f32 * t + backdrop[0] as f32 * (1.0 - t)) as u8,
            (pixel[1] as f32 * t + backdrop[1] as f32 * (1.0 - t)) as u8,
            (pixel[2] as f32 * t + backdrop[2] as f32 * (1.0 - t)) as u8,
        ]
    };

    for y in 0..height {
        for x in 0..width {
            let orig_idx = (y * width + x) * 4;
            let pixel = [source[orig_idx], source[orig_idx + 1], source[orig_idx + 2]];
            let edge = toward_backdrop(pixel, effects.edge);
            let corner = toward_backdrop(pixel, effects.corner);

            for dy in 0..scale {
                for dx in 0..scale {
                    let up_idx = ((y * scale + dy) * upscaled_width + x * scale + dx) * 4;
                    let right = dx == scale - 1;
                    let bottom = dy == scale - 1;
                    let [r, g, b] = match (right, bottom) {
                        (true, true) => corner,
                        (true, false) | (false, true) => edge,
                        _ => pixel,
                    };
                    out[up_idx..up_idx + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
    }
}

fn replicate(source: &[u8], width: usize, scale: usize, out: &mut [u8]) {
    let row = width * scale * 4;
    for (rows, line) in out
        .chunks_exact_mut(row * scale)
        .zip(source.chunks_exact(width * 4))
    {
        let (first, rest) = rows.split_at_mut(row);
        for (dots, &[r, g, b, _]) in first.chunks_exact_mut(scale * 4).zip(line.as_chunks().0) {
            dots.as_chunks_mut().0.fill([r, g, b, 255]);
        }
        for copy in rest.chunks_exact_mut(row) {
            copy.copy_from_slice(first);
        }
    }
}

fn tape_frame(machine: &mut Machine, split: &mut Split<'_>) {
    #[cfg(feature = "nds")]
    if let Machine::Nds(nds) = machine {
        nds.run_frame_on(
            &mut citrine_gb::nds::ir::NoLink,
            &mut citrine_gb::nds::wifi::air::NoPeer,
            split,
        );
        return;
    }
    #[cfg(not(feature = "nds"))]
    let _ = split;
    machine.run_frame();
}

#[cfg(test)]
mod tests {
    use super::{Core, Effects, NoHooks, Plan, Screen, apply};
    use crate::audio::mix::SoundLevels;
    use crate::audio::pacer::{FastForwardSound, SlowSound};
    use crate::emulator::rewind::RewindPlan;
    use citrine_gb::console::{Console, ConsoleKind, Keys};
    use citrine_gb::machine::{Firmware, Machine};
    use citrine_gb::rom::Rom;
    use citrine_gb_replay::{Diverged, Input, Player};
    use citrine_gb_threads::Split;
    use std::hash::{DefaultHasher, Hash, Hasher};

    fn ghosting(on: bool, blend: f32) -> Effects {
        Effects {
            matrix: false,
            edge: 1.0,
            corner: 1.0,
            ghosting: on,
            blend,
            scale: 1,
        }
    }

    fn screen(last: &[u8]) -> Screen {
        Screen {
            last_frame: last.to_vec(),
            painted: Vec::new(),
            size: [0; 2],
        }
    }

    #[test]
    fn without_ghosting_the_last_frame_is_the_source_with_full_alpha() {
        let source = [10, 20, 30, 0, 250, 1, 128, 7];
        let mut screen = screen(&[99; 8]);
        screen.blend(&source, ghosting(false, 0.3));
        assert_eq!(screen.last_frame, [10, 20, 30, 255, 250, 1, 128, 255]);
    }

    #[test]
    fn ghosting_mixes_the_source_into_the_last_frame() {
        let mut screen = screen(&[100, 0, 200, 255]);
        screen.blend(&[200, 100, 0, 255], ghosting(true, 0.5));
        assert_eq!(screen.last_frame, [150, 50, 100, 255]);
    }

    #[cfg(feature = "gba")]
    fn unpacked(stored: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut brotli::Decompressor::new(stored, 4096), &mut data)
            .unwrap();
        data
    }

    fn state_after(rom: &[u8], kind: ConsoleKind, frames: usize, fast_forward: bool) -> Played {
        played(rom, kind, frames, fast_forward, FastForwardSound::Silent)
    }

    fn played(
        rom: &[u8],
        kind: ConsoleKind,
        frames: usize,
        fast_forward: bool,
        fast: FastForwardSound,
    ) -> Played {
        let machine = Machine::load(&Rom::new(rom), kind, &Firmware::default()).unwrap();
        let mut core = Core::new(machine);
        let plan = Plan {
            frames: 1,
            held: Keys::empty(),
            owed: Keys::empty(),
            effects: ghosting(false, 0.0),
            sound: SoundLevels {
                mine: 1.0,
                friend: 1.0,
                slow: SlowSound::default(),
                fast,
            },
            fast_forward,
            take: false,
            rewind: RewindPlan::OFF,
            budget: None,
            defer_3d: false,
            pace: 1.0,
        };
        let mut screens = Vec::with_capacity(frames);
        for frame in 0..frames {
            let keys = match frame % 90 {
                0..10 => Keys::START,
                10..20 => Keys::A,
                20..40 => Keys::RIGHT,
                _ => Keys::empty(),
            };
            core.machine.set_keys(keys);
            core.step(&plan, &mut Split::inline());
            let mut hasher = DefaultHasher::new();
            core.machine.frame().hash(&mut hasher);
            screens.push(hasher.finish());
        }
        let game = match core.machine.as_gb() {
            Some(gb) => (0..8)
                .filter_map(|bank| gb.wram_bank(bank))
                .flatten()
                .copied()
                .collect(),
            None => core.machine.rewind_state().unwrap(),
        };
        let mut samples = Vec::new();
        core.machine.run_frame();
        core.machine.take_audio(&mut samples);
        Played {
            screens,
            game,
            samples: samples.len(),
        }
    }

    struct Played {
        screens: Vec<u64>,
        game: Vec<u8>,
        samples: usize,
    }

    fn same_game_muted(rom: &[u8], kind: ConsoleKind, frames: usize) {
        let heard = state_after(rom, kind, frames, false);
        let muted = state_after(rom, kind, frames, true);
        assert!(
            heard.screens == muted.screens,
            "{kind:?}: fast forward changed the picture"
        );
        assert!(
            heard.game == muted.game,
            "{kind:?}: fast forward changed the game"
        );
        assert!(heard.samples > 0, "{kind:?}: no sound at normal speed");
        assert_eq!(muted.samples, 0, "{kind:?}: sound while fast forwarding");
    }

    fn same_game_sped_up(rom: &[u8], kind: ConsoleKind, frames: usize) {
        let silent = played(rom, kind, frames, true, FastForwardSound::Silent);
        let sped_up = played(rom, kind, frames, true, FastForwardSound::SpedUp);
        assert!(
            silent.screens == sped_up.screens,
            "{kind:?}: sound while fast forwarding changed the picture"
        );
        assert!(
            silent.game == sped_up.game,
            "{kind:?}: sound while fast forwarding changed the game"
        );
        assert!(
            sped_up.samples > 0,
            "{kind:?}: no sound while fast forwarding sped up"
        );
    }

    #[test]
    fn sped_up_fast_forward_sound_plays_the_game_boy_game_the_same() {
        same_game_sped_up(
            include_bytes!("../../homebrew/tobu.gb"),
            ConsoleKind::Gb,
            600,
        );
    }

    #[cfg(feature = "gba")]
    #[test]
    fn sped_up_fast_forward_sound_plays_the_game_boy_advance_game_the_same() {
        let rom = unpacked(include_bytes!("../../homebrew/gba/pentris.gba.br"));
        same_game_sped_up(&rom, ConsoleKind::Gba, 600);
    }

    #[test]
    fn sped_up_sound_squeezes_the_frames_into_the_room_left_in_order() {
        let mut samples = vec![0.0, 0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0];
        super::squeeze(&mut samples, 2);
        assert_eq!(samples, [0.5, -0.5, 2.5, -2.5]);
        let mut samples = vec![0.25, -0.25];
        super::squeeze(&mut samples, 4);
        assert_eq!(samples, [0.25, -0.25]);
        super::squeeze(&mut samples, 0);
        assert!(samples.is_empty());
    }

    #[test]
    fn muted_fast_forward_plays_the_game_boy_game_the_same() {
        same_game_muted(
            include_bytes!("../../homebrew/tobu.gb"),
            ConsoleKind::Gb,
            600,
        );
    }

    #[test]
    fn muted_fast_forward_plays_the_game_boy_color_game_the_same() {
        same_game_muted(
            include_bytes!("../../homebrew/ucity.gbc"),
            ConsoleKind::Gbc,
            600,
        );
    }

    #[cfg(feature = "gba")]
    #[test]
    fn muted_fast_forward_plays_the_game_boy_advance_game_the_same() {
        let rom = unpacked(include_bytes!("../../homebrew/gba/pentris.gba.br"));
        same_game_muted(&rom, ConsoleKind::Gba, 600);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn muted_fast_forward_plays_the_ds_game_the_same() {
        let rom = unpacked(include_bytes!("../../homebrew/nds/pong_nds.nds.br"));
        same_game_muted(&rom, ConsoleKind::Nds, 240);
    }

    #[test]
    fn an_hour_matched_to_60_hz_keeps_the_game_clock_on_true_time() {
        let frame_secs = ConsoleKind::Gb.timing().frame_secs();
        let pace = 60.0 * frame_secs;
        let rtc_cycles_per_frame = 70_224 / 4;
        let mut rtc = citrine_gb::gb::cartridge::Rtc::default();
        let mut hold = super::ClockHold::default();
        for _ in 0..60 * 3600 {
            rtc.advance_cycles(rtc_cycles_per_frame);
            hold.ran(1, frame_secs, pace, |secs| rtc.hold(secs));
        }
        let real = rtc.real;
        let secs =
            i64::from(real.hours) * 3600 + i64::from(real.minutes) * 60 + i64::from(real.seconds);
        assert!((3599..=3600).contains(&secs), "{secs} s after an hour");
    }

    #[test]
    fn the_console_speed_holds_the_game_clock_back_by_nothing() {
        let frame_secs = ConsoleKind::Gb.timing().frame_secs();
        let mut hold = super::ClockHold::default();
        let mut held = 0.0;
        for _ in 0..1000 {
            hold.ran(1, frame_secs, 1.0, |secs| {
                held += secs;
                secs
            });
        }
        assert_eq!(held, 0.0);
    }

    fn rewinding(back: bool) -> Plan {
        Plan {
            frames: 1,
            held: Keys::empty(),
            owed: Keys::empty(),
            effects: ghosting(false, 0.0),
            sound: SoundLevels {
                mine: 1.0,
                friend: 1.0,
                slow: SlowSound::default(),
                fast: FastForwardSound::Silent,
            },
            fast_forward: false,
            take: false,
            rewind: RewindPlan {
                keep: Some(10),
                reach: 10,
                back,
            },
            budget: None,
            defer_3d: false,
            pace: 1.0,
        }
    }

    fn tobu() -> Core {
        let rom = include_bytes!("../../homebrew/tobu.gb");
        Core::new(Machine::load(&Rom::new(rom), ConsoleKind::Gb, &Firmware::default()).unwrap())
    }

    fn steps(core: &mut Core, back: bool, frames: usize) {
        for _ in 0..frames {
            core.step(&rewinding(back), &mut Split::inline());
        }
    }

    #[test]
    fn a_reset_while_rewound_resets_the_game_from_the_frame_shown() {
        let mut rewound = tobu();
        steps(&mut rewound, false, 60);
        steps(&mut rewound, true, 30);
        rewound.feed(Input::Reset);
        assert!(!rewound.rewind.is_rewound());
        steps(&mut rewound, false, 30);

        let mut straight = tobu();
        steps(&mut straight, false, 30);
        straight.feed(Input::Reset);
        steps(&mut straight, false, 30);
        assert_eq!(rewound.machine.state_hash(), straight.machine.state_hash());
    }

    const NOW: u64 = 1_780_000_000;

    #[test]
    fn putting_in_another_game_keeps_the_running_recording() {
        let mut core = tobu();
        steps(&mut core, false, 30);
        assert!(core.start_recording(NOW, rewinding(false).rewind));
        steps(&mut core, false, 40);
        core.put_machine(tobu().machine);
        let replay = core.cut_recording.take().unwrap();
        assert_eq!(replay.header.frames, 40);
        assert_eq!(core.rewind.recording_frames(), None);
    }

    #[test]
    fn starting_over_while_rewound_goes_back_to_the_mark() {
        let mut core = tobu();
        steps(&mut core, false, 30);
        core.start_recording(NOW, rewinding(false).rewind);
        let then = core.machine.state_hash();
        steps(&mut core, false, 60);
        steps(&mut core, true, 10);
        assert!(core.rewind.is_rewound());
        assert!(core.restart_recording());
        assert!(!core.rewind.is_rewound());
        assert_eq!(core.machine.state_hash(), then);
        assert_eq!(core.rewind.recording_frames(), Some(0));
    }

    struct Recorded {
        live: Vec<u64>,
        played: Vec<u64>,
        live_starts: Vec<u64>,
        played_starts: Vec<u64>,
        diverged: Option<Diverged>,
    }

    fn recorded_and_played(
        rom: &[u8],
        kind: ConsoleKind,
        plan: &Plan,
        before: impl Fn(&mut Core),
    ) -> Recorded {
        cut_and_played(rom, kind, plan, before, |core| {
            let replay = core.stop_recording().expect("a replay");
            assert_eq!(replay.header.frames, 150);
            replay
        })
    }

    fn cut_and_played(
        rom: &[u8],
        kind: ConsoleKind,
        plan: &Plan,
        before: impl Fn(&mut Core),
        cut: impl FnOnce(&mut Core) -> citrine_gb_replay::Replay,
    ) -> Recorded {
        let rom = Rom::new(rom);
        let mut core = Core::new(Machine::load(&rom, kind, &Firmware::default()).unwrap());
        let mut live = Vec::new();
        let mut live_starts = Vec::new();
        for _ in 0..25 {
            before(&mut core);
            live_starts.push(core.machine.state_hash());
            core.step(plan, &mut Split::inline());
            live.push(core.machine.state_hash());
        }
        assert!(core.start_recording(NOW, plan.rewind));
        for _ in 0..150 {
            before(&mut core);
            live_starts.push(core.machine.state_hash());
            core.step(plan, &mut Split::inline());
            live.push(core.machine.state_hash());
        }
        let replay = cut(&mut core);
        assert!(!replay.checks.is_empty());
        let mut machine = Machine::from_state(
            &replay.start,
            &rom,
            replay.header.start_unix,
            &Firmware::default(),
        )
        .unwrap();
        let mut player = Player::new(&replay);
        let mut played = Vec::new();
        let mut played_starts = Vec::new();
        let mut diverged = None;
        while !player.is_done() {
            let stepped = player.step_with(&mut machine, |machine| {
                played_starts.push(machine.state_hash());
                machine.run_frame();
            });
            if let Err(err) = stepped {
                diverged.get_or_insert(err);
            }
            machine.take_audio(&mut Vec::new());
            played.push(machine.state_hash());
        }
        assert!(played.len() <= live.len(), "{} frames", played.len());
        live.drain(..live.len() - played.len());
        live_starts.drain(..live_starts.len() - played_starts.len());
        Recorded {
            live,
            played,
            live_starts,
            played_starts,
            diverged,
        }
    }

    fn plays_back_every_frame_at_the_screen_pace(rom: &[u8], kind: ConsoleKind) {
        let plan = Plan {
            pace: 60.0 * kind.timing().frame_secs(),
            ..rewinding(false)
        };
        assert!(plan.pace > 1.0);
        let recorded = recorded_and_played(rom, kind, &plan, |_| {});
        let first = recorded
            .live
            .iter()
            .zip(&recorded.played)
            .position(|(live, played)| live != played);
        assert_eq!(first, None, "the first frame that played back another way");
        assert_eq!(recorded.diverged, None);
    }

    #[test]
    fn a_game_boy_clock_held_to_the_screen_plays_back_every_frame() {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x10;
        rom[0x0149] = 0x02;
        plays_back_every_frame_at_the_screen_pace(&rom, ConsoleKind::Gb);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_gba_game_held_to_the_screen_plays_back_every_frame() {
        let rom = unpacked(include_bytes!("../../homebrew/gba/pentris.gba.br"));
        plays_back_every_frame_at_the_screen_pace(&rom, ConsoleKind::Gba);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_clock_held_to_the_screen_plays_back_every_frame() {
        let rom = unpacked(include_bytes!("../../homebrew/nds/pong_nds.nds.br"));
        plays_back_every_frame_at_the_screen_pace(&rom, ConsoleKind::Nds);
    }

    fn a_clip_cut_while_a_key_is_held_plays_back_every_frame(rom: &[u8], kind: ConsoleKind) {
        let recorded = cut_and_played(
            rom,
            kind,
            &rewinding(false),
            |_| {},
            |core| {
                core.machine.set_keys(Keys::A);
                core.land_rewind();
                let clip = core.rewind.clip(&core.machine, 2).expect("a clip");
                let mut twin = crate::emulator::rewind::twin_of(&core.machine);
                clip.cut(&mut twin, NOW).expect("a replay")
            },
        );
        assert!(
            recorded.played.len() >= 120,
            "{} frames",
            recorded.played.len()
        );
        assert_eq!(recorded.diverged, None);
    }

    #[test]
    fn a_game_boy_clip_cut_while_a_key_is_held_plays_back_every_frame() {
        a_clip_cut_while_a_key_is_held_plays_back_every_frame(
            include_bytes!("../../homebrew/tobu.gb"),
            ConsoleKind::Gb,
        );
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_clip_cut_while_a_key_is_held_plays_back_every_frame() {
        let rom = unpacked(include_bytes!("../../homebrew/nds/pong_nds.nds.br"));
        a_clip_cut_while_a_key_is_held_plays_back_every_frame(&rom, ConsoleKind::Nds);
    }

    fn a_clip_with_taps_on_back_to_back_frames_plays_back_every_frame(
        rom: &[u8],
        kind: ConsoleKind,
    ) {
        let tapping = Plan {
            owed: Keys::A,
            ..rewinding(false)
        };
        let recorded = cut_and_played(
            rom,
            kind,
            &tapping,
            |core| apply(&mut core.machine, Keys::A, Keys::empty(), &mut NoHooks),
            |core| {
                let clip = core.rewind.clip(&core.machine, 2).expect("a clip");
                let mut twin = crate::emulator::rewind::twin_of(&core.machine);
                clip.cut(&mut twin, NOW).expect("a replay")
            },
        );
        assert!(
            recorded.played.len() >= 120,
            "{} frames",
            recorded.played.len()
        );
        let first = recorded
            .live_starts
            .iter()
            .zip(&recorded.played_starts)
            .position(|(live, played)| live != played);
        assert_eq!(first, None, "the first frame that started another way");
        assert_eq!(recorded.diverged, None);
    }

    #[test]
    fn a_game_boy_clip_with_taps_on_back_to_back_frames_plays_back_every_frame() {
        a_clip_with_taps_on_back_to_back_frames_plays_back_every_frame(
            include_bytes!("../../homebrew/tobu.gb"),
            ConsoleKind::Gb,
        );
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_clip_with_taps_on_back_to_back_frames_plays_back_every_frame() {
        let rom = unpacked(include_bytes!("../../homebrew/nds/pong_nds.nds.br"));
        a_clip_with_taps_on_back_to_back_frames_plays_back_every_frame(&rom, ConsoleKind::Nds);
    }

    #[test]
    fn a_game_boy_recording_with_taps_on_back_to_back_frames_plays_back_every_frame() {
        let tapping = Plan {
            owed: Keys::A,
            ..rewinding(false)
        };
        let recorded = recorded_and_played(
            include_bytes!("../../homebrew/tobu.gb"),
            ConsoleKind::Gb,
            &tapping,
            |core| apply(&mut core.machine, Keys::A, Keys::empty(), &mut NoHooks),
        );
        let first = recorded
            .live_starts
            .iter()
            .zip(&recorded.played_starts)
            .position(|(live, played)| live != played);
        assert_eq!(first, None, "the first frame that started another way");
        assert_eq!(recorded.diverged, None);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_tape_leaves_3d_to_the_gpu_except_on_frames_a_seek_key_keeps() {
        use crate::emulator::tape_play::TapePlay;
        use citrine_gb_replay::SeekPoints;
        use std::sync::Arc;
        let rom = Rom::new(include_bytes!(
            "../../fixtures/nds/blocksds-lighting_cube.nds"
        ));
        let plan = rewinding(false);
        let machine = Machine::load(&rom, ConsoleKind::Nds, &Firmware::default()).unwrap();
        let mut core = Core::new(machine);
        for _ in 0..25 {
            core.step(&plan, &mut Split::inline());
        }
        assert!(core.start_recording(NOW, plan.rewind));
        for _ in 0..150 {
            core.step(&plan, &mut Split::inline());
        }
        let replay = core.stop_recording().expect("a replay");
        let start = replay.header.start_unix;
        let machine = Machine::from_state(&replay.start, &rom, start, &Firmware::default());
        let mut core = Core::new(machine.unwrap());
        if let Machine::Nds(nds) = &mut core.machine {
            nds.record_composite(true);
        }
        let play = TapePlay::new(Arc::new(replay), SeekPoints::default());
        core.tape_play = Some(play);
        let gpu = Plan {
            defer_3d: true,
            ..rewinding(false)
        };
        let mut drawn = Vec::new();
        for frame in 1..=130 {
            core.step(&gpu, &mut Split::inline());
            if !core.machine.finish_picture() {
                drawn.push(frame);
            }
        }
        assert_eq!(drawn, [60, 120]);
    }

    #[test]
    fn taps_let_go_after_their_frame_play_back_to_the_last_check() {
        let plan = Plan {
            owed: Keys::A,
            ..rewinding(false)
        };
        let recorded = recorded_and_played(
            include_bytes!("../../homebrew/tobu.gb"),
            ConsoleKind::Gb,
            &plan,
            |core| core.machine.set_keys(Keys::A),
        );
        assert_eq!(recorded.diverged, None);
    }
}
