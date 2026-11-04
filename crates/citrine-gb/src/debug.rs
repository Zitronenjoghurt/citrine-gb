use crate::console::Console;
use crate::debug::breakpoints::Breakpoints;
use crate::debug::code::{Address, Line};
use crate::debug::disassembly::Disassembly;
use crate::debug::memory::MemoryBlock;
use crate::debug::print::PrintLog;
use crate::debug::registers::RegisterGroup;
use crate::debug::step::{Step, Stop};
use crate::debug::trace::TraceLog;
use crate::debug::video::{Palette, SheetColors, Sprite, TileMap, TileSheet};
use crate::gb::apu::APU_CLOCK_RATE;
use std::collections::VecDeque;
use std::ops::Range;

pub mod breakpoints;
pub mod code;
pub mod disassembly;
pub mod e2e;
pub mod memory;
pub mod print;
pub mod registers;
pub mod step;
pub mod tiles;
pub mod trace;
pub mod video;

pub trait Debug: Console {
    fn features(&self) -> Features;
    fn registers(&self, out: &mut Vec<RegisterGroup>);
    fn memory_blocks(&self, out: &mut Vec<MemoryBlock>);
    fn locate(&self, addr: u32) -> Option<Address>;
    fn peek(&self, at: Address, out: &mut [u8]);
    fn poke(&mut self, at: Address, bytes: &[u8]);
    fn pc(&self) -> Option<Address>;
    fn code_analysis(&self) -> bool;
    fn set_code_analysis(&mut self, on: bool);
    fn code_rows(&self) -> usize;
    fn code_row_of(&self, at: Address) -> Option<usize>;
    fn disassemble(&self, rows: Range<usize>, out: &mut Vec<Line>);
    fn breakpoints(&mut self) -> &mut Breakpoints;
    fn cycles(&self) -> u128;
    fn record_waves(&mut self, on: bool);
    fn waves(&self) -> &[VecDeque<f32>];
    fn step(&mut self, step: Step) -> Stop;
    fn step_pending(&self) -> bool;
    fn resume_step(&mut self) -> Stop;
    fn cancel_step(&mut self);
    fn take_breakpoint_hit(&mut self) -> bool;
    fn frame_cycles(&self) -> u32;
    fn scanline(&self) -> u16;
    fn trace(&mut self) -> &mut TraceLog;
    fn tile_sheets(&self, colors: SheetColors, out: &mut Vec<TileSheet>);
    fn tile_maps(&self, out: &mut Vec<TileMap>);
    fn sprites(&self, out: &mut Vec<Sprite>);
    fn palettes(&self, out: &mut Vec<Palette>);
    fn print_log(&mut self) -> Option<&mut PrintLog>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    pub pc_digits: u8,
    pub thumb: bool,
    pub vectors: &'static [(&'static str, u32)],
    pub cpu_groups: &'static [&'static str],
    pub watchpoints: bool,
    pub cycle: &'static str,
}

const MAX_PLOT_SAMPLES: usize = 2048;

#[derive(Debug, Default)]
pub struct Debugger {
    pub disassembly: Disassembly,
    pub static_analysis_enabled: bool,
    pub breakpoints: Breakpoints,
    pub hit_breakpoint: bool,
    pub total_cycles: u128,
    pub(crate) step_goal: Option<step::Goal>,
    pub record_channels: bool,
    pub trace: TraceLog,
    apu_channel_sample_counter: u32,
    apu_channel_stride: u32,
    apu_channel_stride_for: u32,
    waves: Vec<VecDeque<f32>>,
}

impl Debugger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn soft_reset(&mut self) {
        self.total_cycles = 0;
        self.step_goal = None;
    }

    pub fn waves(&self) -> &[VecDeque<f32>] {
        &self.waves
    }

    fn apu_channel_stride(&mut self, sample_rate: u32) -> u32 {
        if self.apu_channel_stride_for != sample_rate {
            self.apu_channel_stride_for = sample_rate;
            self.apu_channel_stride = APU_CLOCK_RATE / sample_rate.max(1);
        }
        self.apu_channel_stride
    }
}

impl DebuggerInterface for Debugger {
    fn wants_apu_channels(&self) -> bool {
        self.record_channels
    }

    fn record_apu_channels(&mut self, sample_rate: u32, samples: &[f32]) {
        let stride = self.apu_channel_stride(sample_rate);
        if self.apu_channel_sample_counter < stride {
            self.apu_channel_sample_counter += 1;
            return;
        }
        self.apu_channel_sample_counter -= stride;
        self.push_waves(samples);
    }
}

impl Debugger {
    pub fn push_waves(&mut self, samples: &[f32]) {
        if self.waves.len() != samples.len() {
            self.waves = vec![VecDeque::new(); samples.len()];
        }

        for (wave, &sample) in self.waves.iter_mut().zip(samples) {
            if wave.len() >= MAX_PLOT_SAMPLES {
                wave.pop_front();
            }
            wave.push_back(if sample == 0.0 {
                0.0
            } else {
                (1.0 - sample) / 2.0
            });
        }
    }
}

pub trait DebuggerInterface {
    fn wants_apu_channels(&self) -> bool {
        false
    }

    fn record_apu_channels(&mut self, _sample_rate: u32, _samples: &[f32]) {}
}

#[cfg(feature = "debug")]
pub trait DebuggerAccess {
    fn debugger(&self) -> &dyn DebuggerInterface;
    fn debugger_mut(&mut self) -> &mut dyn DebuggerInterface;
}

impl<T: DebuggerAccess> DebuggerInterface for T {}
