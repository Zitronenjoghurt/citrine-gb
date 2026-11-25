use crate::frame_log::{Budget, RunContext, Totals};
use citrine_gb::console::ConsoleKind;
#[cfg(feature = "nds")]
use citrine_gb::nds::machine::JitCounts;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuClockKind {
    Stamps,
    SubmitToDone,
}

pub const GPU_PASSES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScreenMatch {
    Off,
    Shared,
    Measuring,
    Matched,
    Unmatched(f64),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuTotals {
    pub kind: Option<GpuClockKind>,
    pub time: Duration,
    pub done: Duration,
    pub frames: u64,
    pub work: Option<Duration>,
    pub passes: [Duration; GPU_PASSES],
}

impl GpuTotals {
    pub fn full_clock_ms(&self) -> Option<f64> {
        self.work.map(|work| work.as_secs_f64() * 1000.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadReading {
    pub ds: bool,
    pub upscaling: bool,
    pub gpu: GpuTotals,
    #[cfg(feature = "nds")]
    pub jit: Option<JitCounts>,
}

#[derive(Debug, Default)]
pub(crate) struct Load {
    pub(crate) reading: LoadReading,
}

impl super::Emulator {
    pub fn load(&self) -> LoadReading {
        self.load.reading
    }

    pub fn frame_totals(&self) -> Totals {
        let pace = self.clock.count();
        let reading = self.load.reading;
        #[cfg(feature = "nds")]
        let jit = reading.jit.unwrap_or_default();
        #[cfg(feature = "nds")]
        let (jit_compile, jit_blocks) = (jit.compile_time, jit.blocks);
        #[cfg(not(feature = "nds"))]
        let (jit_compile, jit_blocks) = (Duration::ZERO, 0);
        Totals {
            game: self.frame_avg_timer.total(),
            game_frames: self.frame_avg_timer.stops(),
            repeated: pace.repeated_total(),
            dropped: pace.skipped_total(),
            gpu: reading.gpu.time,
            gpu_frames: reading.gpu.frames,
            gpu_full: reading.gpu.work,
            gpu_passes: reading.gpu.passes,
            jit_compile,
            jit_blocks,
            #[cfg(threaded)]
            console_wait: self.console_waited(),
            #[cfg(not(threaded))]
            console_wait: Duration::ZERO,
            audio: Default::default(),
            session: None,
        }
    }

    pub fn frame_budget(&self) -> Budget {
        let frame = self
            .clock
            .matched()
            .unwrap_or_else(|| self.timing().frame_secs());
        Budget {
            game: Duration::from_secs_f64(frame),
            refresh: self.clock.refresh(),
        }
    }

    pub fn screen_match(&self) -> Option<ScreenMatch> {
        if !self.has_game() {
            return None;
        }
        Some(if !self.match_screen {
            ScreenMatch::Off
        } else if !self.clock.matching() {
            ScreenMatch::Shared
        } else if self.clock.matched().is_some() {
            ScreenMatch::Matched
        } else {
            match self.clock.refresh_period() {
                Some(period) if !self.clock.screen_matches(self.timing()) => {
                    ScreenMatch::Unmatched(
                        self.display
                            .monitor
                            .map_or(1.0 / period, |monitor| monitor.hz),
                    )
                }
                _ => ScreenMatch::Measuring,
            }
        })
    }

    pub fn run_context(&self) -> RunContext {
        let kind = self.kind();
        let ds = kind == ConsoleKind::Nds;
        let ds_only = |text: String| if ds { text } else { String::new() };
        RunContext {
            console: match kind {
                ConsoleKind::Gb => "GB",
                ConsoleKind::Gbc => "GBC",
                ConsoleKind::Gba => "GBA",
                ConsoleKind::Nds => "DS",
            },
            game: self.game_name().to_string(),
            renderer: match (ds, self.load.reading.upscaling) {
                (false, _) => "",
                (true, false) => "software",
                (true, true) => "gpu",
            },
            resolution: ds_only(self.ds_3d.resolution.to_string().to_lowercase()),
            smooth_edges: ds_only(self.ds_3d.smooth_edges.log_name().to_owned()),
            #[cfg(feature = "nds")]
            jit: self.load.reading.jit.is_some(),
            #[cfg(not(feature = "nds"))]
            jit: false,
            gpu_clock: match self.load.reading.gpu.kind {
                Some(GpuClockKind::Stamps) => "stamps",
                Some(GpuClockKind::SubmitToDone) => "submit-to-done",
                None => "",
            },
        }
    }

    #[cfg(feature = "nds")]
    pub(super) fn sample_load(&mut self) {
        use citrine_gb::machine::Machine;
        let machine = &self.seat.core().machine;
        self.load.reading.ds = matches!(machine, Machine::Nds(_));
        self.load.reading.jit = match machine {
            Machine::Nds(nds) => nds.jit_counts(),
            Machine::Gba(gba) => gba.jit_counts(),
            _ => None,
        };
    }
}
