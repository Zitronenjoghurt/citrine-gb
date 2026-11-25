use super::Citrine;
use crate::app::widgets::clip_panel::{ClipAction, ClipFacts, StripPicture};
use crate::emulator::Emulator;
use crate::media::{MediaKind, NewMedia};
use citrine_gb::console::{Console as _, VideoSpec};
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::rom::Rom;
use citrine_gb_replay::{Player, Replay, SeekPoints};
use egui::{Context, TextureHandle, TextureOptions};
use std::sync::{Arc, mpsc};

pub const CLIPS_OFF: &str = "Clips are off. Turn them on in Settings, Capture, Clip length";
const PICTURES: usize = 24;
const CROP_SEEK_CAP: usize = 64 << 20;

pub(super) struct Strip {
    video: VideoSpec,
    pictures: Vec<(u64, Vec<u8>)>,
}

enum Got {
    Clip(Box<Replay>, SeekPoints, Strip),
    Strip(Strip),
    Saved(Result<Box<Replay>, String>),
}

pub struct ClipView {
    pub from: u64,
    pub to: u64,
    pub live: bool,
    pub saving: bool,
    pub resume: bool,
    pub pictures: Vec<StripPicture>,
    textures: Vec<TextureHandle>,
    raw: Option<Strip>,
    thumb: Option<NewMedia>,
}

impl ClipView {
    fn new(frames: u64, live: bool, resume: bool) -> Self {
        Self {
            from: 0,
            to: frames,
            live,
            saving: false,
            resume,
            pictures: Vec::new(),
            textures: Vec::new(),
            raw: None,
            thumb: None,
        }
    }

    pub fn facts(&self, emulator: &Emulator, phone: bool) -> Option<ClipFacts> {
        let tape = emulator.tape_facts()?;
        Some(ClipFacts {
            frames: tape.frames,
            from: self.from,
            to: self.to.min(tape.frames),
            at: tape.seeking.unwrap_or(tape.played),
            running: tape.running,
            frame_secs: tape.frame_secs,
            saving: self.saving,
            phone,
        })
    }

    pub fn load_pictures(&mut self, ctx: &Context) {
        let Some(strip) = self.raw.take() else {
            return;
        };
        let (width, height) = (strip.video.width, strip.video.height);
        let top = strip
            .video
            .screens
            .first()
            .map_or(height, |screen| screen.height);
        let uv = egui::Rect::from_min_max(
            egui::pos2(0.0, 0.0),
            egui::pos2(1.0, top as f32 / height.max(1) as f32),
        );
        let aspect = width as f32 / top.max(1) as f32;
        self.textures.clear();
        self.pictures.clear();
        for (index, (frame, rgba)) in strip.pictures.into_iter().enumerate() {
            if rgba.len() != width * height * 4 {
                continue;
            }
            let image = egui::ColorImage::from_rgba_premultiplied([width, height], &rgba);
            let texture = ctx.load_texture(
                format!("clip-strip-{index}"),
                image,
                TextureOptions::NEAREST,
            );
            self.pictures.push(StripPicture {
                frame,
                texture: texture.id(),
                uv,
                aspect,
            });
            self.textures.push(texture);
        }
    }
}

pub(super) struct ClipWork {
    tx: mpsc::Sender<Got>,
    rx: mpsc::Receiver<Got>,
    waiting: bool,
    resume: bool,
}

impl Default for ClipWork {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            waiting: false,
            resume: false,
        }
    }
}

fn strip_of(points: &SeekPoints, machine: &mut Machine) -> Strip {
    Strip {
        video: machine.video(),
        pictures: points.pictures(machine, PICTURES).unwrap_or_default(),
    }
}

fn cropped(
    replay: &Replay,
    points: SeekPoints,
    rom: &Rom,
    firmware: &Firmware,
    (from, to): (u64, u64),
) -> Result<Replay, String> {
    if from == 0 && to >= replay.header.frames {
        return Ok(Replay {
            video: None,
            thumb: None,
            ..replay.clone()
        });
    }
    let mut machine = Machine::from_state(&replay.start, rom, replay.header.start_unix, firmware)
        .map_err(|err| err.to_string())?;
    let mut player = Player::new(replay).seekable_from(CROP_SEEK_CAP, points);
    if from > 0 {
        player
            .seek(&mut machine, from)
            .map_err(|err| err.to_string())?;
    }
    let mut crop = player
        .crop(&mut machine, from, to, crate::storage::now_unix())
        .map_err(|err| err.to_string())?;
    crop.perf = replay
        .perf
        .as_ref()
        .and_then(|perf| super::recording::cropped_perf(perf, from, to));
    Ok(crop)
}

impl Citrine {
    pub(super) fn clip_blocked(&self) -> Option<String> {
        if self.replay_show.is_open() || !self.emulator.has_game() {
            return None;
        }
        if let Some(why) = self.emulator.tape_blocker() {
            return Some(format!("Clips are off while {why} is on"));
        }
        if self.emulator.hardcore() {
            return Some("Clips are off in hardcore".to_owned());
        }
        (!self.emulator.can_clip()).then(|| CLIPS_OFF.to_owned())
    }

    pub(super) fn save_a_clip(&mut self) {
        if self.ui.clip.is_some() || self.clip_work.waiting {
            return;
        }
        if self.replay_show.is_open() {
            self.crop_open_replay();
            return;
        }
        if !self.emulator.has_game() {
            return;
        }
        if let Some(why) = self.clip_blocked() {
            self.toasts.info(why);
            return;
        }
        let Some((clip, mut twin)) = self.emulator.take_clip() else {
            self.toasts.error("Could not save a clip");
            return;
        };
        let resume = self.emulator.running;
        self.emulator.running = false;
        self.clip_work.waiting = true;
        self.clip_work.resume = resume;
        let tx = self.clip_work.tx.clone();
        self.files.spawner.run("save a clip", move || {
            let got = match clip.cut(&mut twin, crate::storage::now_unix()) {
                Ok(replay) => {
                    let points = clip.seek_points(&replay);
                    let strip = strip_of(&points, &mut twin);
                    Got::Clip(Box::new(replay), points, strip)
                }
                Err(err) => Got::Saved(Err(err.to_string())),
            };
            let _ = tx.send(got);
        });
    }

    fn crop_open_replay(&mut self) {
        let Some((replay, points)) = self.emulator.tape_source() else {
            return;
        };
        let running = self.emulator.tape_facts().is_some_and(|tape| tape.running);
        if running {
            self.emulator.toggle_tape();
        }
        self.ui.clip = Some(ClipView::new(replay.header.frames, false, false));
        if points.is_empty() {
            return;
        }
        let rom = self.replay_rom_now(&replay.header.sha1);
        let firmware = self.emulator.firmware.clone();
        let tx = self.clip_work.tx.clone();
        self.files.spawner.run("clip pictures", move || {
            let Some(rom) = rom else {
                return;
            };
            if let Ok(mut machine) =
                Machine::from_state(&replay.start, &rom, replay.header.start_unix, &firmware)
            {
                let _ = tx.send(Got::Strip(strip_of(&points, &mut machine)));
            }
        });
    }

    fn replay_rom_now(&self, sha1: &str) -> Option<Rom> {
        self.emulator.rom_with_sha1(sha1).or_else(|| {
            self.replay_show
                .live()
                .and_then(|live| live.rom_with_sha1(sha1))
        })
    }

    pub(super) fn tick_clip(&mut self, ctx: &Context) {
        while let Ok(got) = self.clip_work.rx.try_recv() {
            match got {
                Got::Clip(replay, points, strip) => {
                    self.clip_work.waiting = false;
                    self.open_clip(*replay, points, strip);
                }
                Got::Strip(strip) => {
                    if let Some(view) = &mut self.ui.clip {
                        view.raw = Some(strip);
                    }
                }
                Got::Saved(Ok(replay)) => self.clip_saved(*replay),
                Got::Saved(Err(err)) => {
                    self.clip_work.waiting = false;
                    log::error!("could not save a clip: {err}");
                    self.toasts.error("Could not save the clip");
                    if let Some(view) = &mut self.ui.clip {
                        view.saving = false;
                    } else {
                        self.emulator.running = self.clip_work.resume;
                    }
                }
            }
        }
        let Some(view) = &mut self.ui.clip else {
            return;
        };
        view.load_pictures(ctx);
        if !self.emulator.is_playing_tape() {
            self.ui.clip = None;
            return;
        }
        if let Some(tape) = self.emulator.tape_facts()
            && tape.running
            && tape.seeking.is_none()
            && tape.played >= view.to
        {
            self.emulator.toggle_tape();
        }
    }

    fn open_clip(&mut self, replay: Replay, points: SeekPoints, strip: Strip) {
        let frames = replay.header.frames;
        let Some(rom) = self.emulator.rom_with_sha1(&replay.header.sha1) else {
            self.emulator.running = self.clip_work.resume;
            return;
        };
        let mut show = match self.emulator.for_replay(replay, &rom, points) {
            Ok(show) => Box::new(show),
            Err(err) => {
                log::error!("could not open the clip: {err:?}");
                self.toasts.error("Could not save a clip");
                self.emulator.running = self.clip_work.resume;
                return;
            }
        };
        self.ui.settings.apply_emulator(&mut show);
        show.running = false;
        #[cfg(threaded)]
        self.emulator.follow_visibility(false, &self.threads);
        self.emulator.hand_audio_to(&mut show);
        let live = std::mem::replace(&mut self.emulator, show);
        self.replay_show.park(live);
        #[cfg(threaded)]
        self.emulator.run_on(&self.threads);
        self.emulator.seek_tape(frames);
        let mut view = ClipView::new(frames, true, self.clip_work.resume);
        view.raw = Some(strip);
        self.ui.clip = Some(view);
        self.ui.viewer.close();
        self.events.open_tab(crate::app::tabs::Tab::GameBoy);
    }

    pub(super) fn clip_action(&mut self, action: ClipAction) {
        let Some(view) = &mut self.ui.clip else {
            return;
        };
        if view.saving {
            return;
        }
        match action {
            ClipAction::Toggle => {
                let at = self.emulator.tape_facts().map_or(0, |tape| tape.played);
                let stopped = self.emulator.tape_facts().is_some_and(|tape| !tape.running);
                if stopped && (at >= view.to || at < view.from) {
                    self.emulator.seek_tape(view.from.max(1));
                }
                self.emulator.toggle_tape();
            }
            ClipAction::Seek(to) => self.emulator.seek_tape(to),
            ClipAction::Range(from, to) => {
                let moved_start = from != view.from;
                view.from = from;
                view.to = to;
                self.emulator
                    .seek_tape(if moved_start { from.max(1) } else { to });
            }
            ClipAction::Cancel => self.leave_clip(),
            ClipAction::Save => self.save_clip(),
        }
    }

    fn save_clip(&mut self) {
        let Some((replay, points)) = self.emulator.tape_source() else {
            return;
        };
        let Some(rom) = self.replay_rom_now(&replay.header.sha1) else {
            self.toasts.error("Could not save the clip");
            return;
        };
        let Some(view) = &mut self.ui.clip else {
            return;
        };
        view.saving = true;
        view.thumb = Some(NewMedia::current(MediaKind::Replay, &mut self.emulator));
        let range = (view.from, view.to);
        let firmware = self.emulator.firmware.clone();
        let tx = self.clip_work.tx.clone();
        self.clip_work.waiting = true;
        self.files.spawner.run("crop a clip", move || {
            let replay: Arc<Replay> = replay;
            let got = cropped(&replay, points, &rom, &firmware, range).map(Box::new);
            let _ = tx.send(Got::Saved(got));
        });
    }

    fn clip_saved(&mut self, replay: Replay) {
        self.clip_work.waiting = false;
        let thumb = self.ui.clip.as_mut().and_then(|view| view.thumb.take());
        self.save_replay(replay, thumb);
        self.leave_clip();
    }

    pub(super) fn leave_clip(&mut self) {
        let Some(view) = self.ui.clip.take() else {
            return;
        };
        if view.live {
            self.close_replay_show();
            self.emulator.running = view.resume;
        }
    }
}
