use crate::media::recording::{FinishedRecording, Outcome, ScreenRecorder, Source, StopReason};
use crate::media::{Frame, NewMedia};
use citrine_gb::console::Console;
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::rom::Rom;
use citrine_gb_replay::{Player, Replay};
use std::fmt::Display;
use std::time::Duration;
use strum_macros::EnumIter;

const RATE: u32 = 48_000;
const READY_PICTURES: usize = 2;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum RenderVideo {
    #[default]
    RightAway,
    WhenIAsk,
}

impl Display for RenderVideo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RightAway => "Right away",
            Self::WhenIAsk => "When I ask",
        })
    }
}

pub struct Rendered {
    pub replay: Box<Replay>,
    pub done: FinishedRecording,
}

pub struct Render {
    player: Player<Box<Replay>>,
    machine: Machine,
    recorder: ScreenRecorder,
    audio: Vec<f32>,
}

impl Render {
    pub fn start(
        spawner: &citrine_gb_threads::Spawner,
        replay: Box<Replay>,
        rom: &Rom,
        firmware: &Firmware,
        mut info: NewMedia,
        scale: usize,
    ) -> Result<Self, String> {
        let mut machine =
            Machine::from_state(&replay.start, rom, replay.header.start_unix, firmware)
                .map_err(|err| err.to_string())?;
        machine.defer_3d(false);
        machine.set_sample_rate(RATE);
        let video = machine.video();
        info.frame = Frame {
            width: video.width as u32,
            height: video.height as u32,
            rgba: machine.frame().to_vec(),
        };
        info.replay = None;
        let mut recorder = ScreenRecorder::default();
        recorder
            .start(spawner, info, Source::of(&machine), RATE, scale)
            .map_err(|err| err.to_string())?;
        Ok(Self {
            player: Player::new(replay),
            machine,
            recorder,
            audio: Vec::new(),
        })
    }

    pub fn played(&self) -> u64 {
        self.player.played()
    }

    pub fn is_done(&self) -> bool {
        self.player.is_done()
    }

    pub fn is_waiting(&self) -> bool {
        self.recorder.pictures_waiting() > READY_PICTURES
    }

    pub fn step(&mut self, budget: Duration) -> Result<(), String> {
        let started = web_time::Instant::now();
        while !self.is_done() && !self.is_waiting() && started.elapsed() < budget {
            self.frame()?;
        }
        Ok(())
    }

    fn frame(&mut self) -> Result<(), String> {
        self.player.step(&mut self.machine).map_err(|diverged| {
            format!(
                "it plays differently in this version from frame {}",
                diverged.frame
            )
        })?;
        self.machine.take_audio(&mut self.audio);
        self.recorder
            .capture(self.machine.frame(), &self.audio, RATE);
        self.audio.clear();
        match self.recorder.take_stopped() {
            None => Ok(()),
            Some(StopReason::TooSlow) => Err("this device couldn't keep up".to_string()),
            Some(StopReason::TooLong) => Err("the video is too long".to_string()),
            Some(StopReason::SoundChanged) => Err("the sound output changed".to_string()),
            Some(StopReason::Asked | StopReason::Failed) => {
                Err("the video encoder stopped".to_string())
            }
        }
    }

    pub fn finish(mut self) -> Finishing {
        self.recorder.stop();
        Finishing {
            replay: self.player.into_replay(),
            recorder: self.recorder,
        }
    }
}

pub struct Finishing {
    replay: Box<Replay>,
    recorder: ScreenRecorder,
}

impl Finishing {
    #[cfg(target_arch = "wasm32")]
    pub fn poll(&mut self) -> Option<Outcome> {
        self.recorder.poll()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(mut self) -> Result<Rendered, String> {
        match self.recorder.wait().pop() {
            Some(outcome) => self.rendered(outcome),
            None => Err("nothing was rendered".to_string()),
        }
    }

    pub fn rendered(self, outcome: Outcome) -> Result<Rendered, String> {
        match outcome {
            Outcome::Saved(done) => Ok(Rendered {
                replay: self.replay,
                done: *done,
            }),
            Outcome::Empty => Err("nothing was rendered".to_string()),
            Outcome::Failed(err) => Err(err.to_string()),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn abandon(mut self) -> Option<Self> {
        match self.recorder.poll() {
            Some(Outcome::Saved(done)) => {
                crate::media::recording::discard(done.output);
                None
            }
            Some(_) => None,
            None => Some(self),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run(
    mut render: Render,
    stop: &std::sync::atomic::AtomicBool,
    played: &std::sync::atomic::AtomicU64,
) -> Result<Option<Rendered>, String> {
    use std::sync::atomic::Ordering;
    let mut failed = None;
    while !render.is_done() && !stop.load(Ordering::Relaxed) {
        if let Err(err) = render.step(Duration::from_millis(50)) {
            failed = Some(err);
            break;
        }
        played.store(render.played(), Ordering::Relaxed);
        if render.is_waiting() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let stopped = !render.is_done();
    let finished = render.finish().wait();
    if let Some(err) = failed {
        if let Ok(rendered) = finished {
            crate::media::recording::discard(rendered.done.output);
        }
        return Err(err);
    }
    if stopped {
        if let Ok(rendered) = finished {
            crate::media::recording::discard(rendered.done.output);
        }
        return Ok(None);
    }
    finished.map(Some)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::emulator::rewind::tests::{RECORDED_ROM, recorded_replay};
    use crate::media::MediaKind;
    use crate::media::recording::DEFAULT_SCALE;
    use crate::media::recording::headless::spawner;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    fn info() -> NewMedia {
        NewMedia {
            kind: MediaKind::Replay,
            game: "Snake".to_string(),
            rom_key: None,
            model: None,
            created_at: 0,
            width: 0,
            height: 0,
            length_ms: None,
            frame_times: false,
            video: true,
            replay: None,
            frame: Frame {
                width: 0,
                height: 0,
                rgba: Vec::new(),
            },
        }
    }

    fn render(rom: &[u8], frames: usize, stop: bool) -> Result<Option<Rendered>, String> {
        let replay = Box::new(recorded_replay(frames));
        let played = AtomicU64::new(0);
        let render = Render::start(
            &spawner(),
            replay,
            &Rom::new(rom),
            &Firmware::default(),
            info(),
            DEFAULT_SCALE,
        )?;
        let rendered = run(render, &AtomicBool::new(stop), &played);
        if !stop && rendered.is_ok() {
            assert_eq!(played.load(Ordering::Relaxed), frames as u64);
        }
        rendered
    }

    #[test]
    fn a_replay_renders_one_picture_per_frame() {
        let rendered = render(RECORDED_ROM, 120, false).unwrap().unwrap();
        assert_eq!(rendered.replay.header.frames, 120);
        let done = rendered.done;
        assert!(done.frames.abs_diff(120) <= 1, "{} frames", done.frames);
        assert_eq!((done.info.frame.width, done.info.frame.height), (160, 144));
        assert_eq!(done.thumbnail.len(), 160 * 144 * 4);
        let size = std::fs::metadata(&done.output.path).unwrap().len();
        assert!(size > 0);
        crate::media::recording::discard(done.output);
    }

    #[test]
    fn a_stopped_render_keeps_nothing() {
        assert!(render(RECORDED_ROM, 60, true).unwrap().is_none());
    }
}
