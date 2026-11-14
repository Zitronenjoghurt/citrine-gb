use super::Value;
use crate::frame_log::readout::{FPS_WINDOW, StripFrame};
use crate::frame_log::{FrameSample, Readout};
use std::collections::VecDeque;
use std::time::Duration;

pub const MISSING: &str = "-";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gpu {
    Unmeasured,
    Timed(Duration),
}

pub struct Facts {
    pub readout: Option<Readout>,
    pub speed: Option<f64>,
    pub strip: Vec<StripFrame>,
    pub gpu: Gpu,
    pub ds: bool,
    pub jit: bool,
    pub resolution: String,
    pub played: Option<u64>,
    pub clock: String,
}

pub struct Cell {
    pub label: &'static str,
    pub value: String,
    pub unit: &'static str,
}

#[derive(Default)]
pub struct Cache {
    clock: Option<(u64, String)>,
    speed: Option<(Duration, Option<f64>)>,
}

impl Cache {
    pub fn clock(&mut self, unix: u64) -> String {
        let minute = unix / 60;
        match &self.clock {
            Some((at, text)) if *at == minute => text.clone(),
            _ => {
                let text = crate::media::local_stamp(unix, "%H:%M");
                self.clock = Some((minute, text.clone()));
                text
            }
        }
    }

    pub fn speed(&mut self, samples: &VecDeque<FrameSample>) -> Option<f64> {
        let newest = samples.back()?.at;
        match self.speed {
            Some((at, speed)) if newest.saturating_sub(at) < crate::frame_log::readout::EVERY => {
                speed
            }
            _ => {
                let speed = speed(samples);
                self.speed = Some((newest, speed));
                speed
            }
        }
    }
}

pub fn speed(samples: &VecDeque<FrameSample>) -> Option<f64> {
    let newest = samples.back()?;
    let (oldest, frames) = samples
        .iter()
        .filter(|sample| newest.at.saturating_sub(sample.at) < FPS_WINDOW)
        .fold((newest.at, 0u64), |(oldest, frames), sample| {
            (oldest.min(sample.at), frames + sample.game_frames)
        });
    let span = newest.at.saturating_sub(oldest).as_secs_f64();
    (span > 0.0).then(|| frames as f64 / span * newest.budget.game.as_secs_f64() * 100.0)
}

fn ms(duration: Duration) -> String {
    format!("{:.1}", duration.as_secs_f64() * 1000.0)
}

fn or_missing(value: Option<String>) -> String {
    value.unwrap_or_else(|| MISSING.into())
}

pub fn cell(value: Value, facts: &Facts) -> Cell {
    let r = facts.readout.as_ref();
    let (text, unit) = match value {
        Value::Fps => (or_missing(r.map(|r| format!("{:.1}", r.fps))), ""),
        Value::Low => (
            or_missing(r.and_then(|r| r.low).map(|low| format!("{low:.1}"))),
            "fps",
        ),
        Value::Speed => (
            or_missing(facts.speed.map(|speed| format!("{speed:.0}"))),
            "%",
        ),
        Value::Graph => (String::new(), ""),
        Value::FrameTime => (
            or_missing(
                r.filter(|r| r.fps > 0.0)
                    .map(|r| format!("{:.1}", 1000.0 / r.fps)),
            ),
            "ms",
        ),
        Value::Late => (or_missing(r.map(|r| r.late.to_string())), "in 5 s"),
        Value::Game => (or_missing(r.and_then(|r| r.game).map(ms)), "ms"),
        Value::App => (or_missing(r.map(|r| ms(r.app))), "ms"),
        Value::Gpu => match facts.gpu {
            Gpu::Timed(time) => (ms(time), "ms"),
            Gpu::Unmeasured => (MISSING.into(), ""),
        },
        Value::SoundBuffer => (
            or_missing(
                r.and_then(|r| r.audio_queued)
                    .map(|queued| format!("{:.0}", queued.as_secs_f64() * 1000.0)),
            ),
            "ms",
        ),
        Value::SoundGaps => (
            or_missing(r.map(|r| format!("{:.0}", r.audio_underrun.as_secs_f64() * 1000.0))),
            "ms",
        ),
        Value::Jit => ((if facts.jit { "on" } else { "off" }).into(), ""),
        Value::Resolution => (facts.resolution.clone(), ""),
        Value::Played => (
            or_missing(facts.played.map(crate::app::tabs::stats::play_time)),
            "",
        ),
        Value::Refresh => (
            or_missing(r.and_then(|r| r.refresh).map(|hz| format!("{hz:.0}"))),
            "Hz",
        ),
        Value::Clock => (facts.clock.clone(), ""),
    };
    let unit = if text == MISSING { "" } else { unit };
    Cell {
        label: value.name(),
        value: text,
        unit,
    }
}

pub struct Sources<'a> {
    pub frame_log: &'a mut crate::frame_log::FrameLog,
    pub load: crate::app::widgets::performance::LoadView,
    pub emulator: &'a crate::emulator::Emulator,
    pub stats: &'a crate::stats::Stats,
}

pub fn gather(sources: Sources, cache: &mut Cache, huds: &super::HudSettings) -> Facts {
    let ds = sources.emulator.kind() == citrine_gb::console::ConsoleKind::Nds;
    let wants = |value: Value| {
        huds.huds
            .iter()
            .any(|hud| super::shown_values(&hud.values, ds).any(|shown| shown == value))
    };
    let samples = sources.frame_log.samples();
    let strip = if wants(Value::Graph) {
        crate::frame_log::readout::strip(samples)
    } else {
        Vec::new()
    };
    let speed = if wants(Value::Speed) {
        cache.speed(samples)
    } else {
        None
    };
    let clock = if wants(Value::Clock) {
        cache.clock(crate::storage::now_unix())
    } else {
        String::new()
    };
    let gpu = match sources.load.gpu {
        crate::app::widgets::performance::GpuView::Timed(_, time) => Gpu::Timed(time),
        _ => Gpu::Unmeasured,
    };
    let played = if wants(Value::Played) {
        sources
            .emulator
            .rom_key()
            .and_then(|rom| sources.stats.session_ms(rom))
    } else {
        None
    };
    Facts {
        readout: sources.frame_log.readout(),
        speed,
        strip,
        gpu,
        ds,
        jit: sources.emulator.ds_jit,
        resolution: sources.emulator.ds_3d.resolution.to_string(),
        played,
        clock,
    }
}
