use crate::disassembly::Disassembly;
use crate::gb::apu::APU_CLOCK_RATE;
use crate::gb::cartridge::RomLocation;
use std::collections::{HashSet, VecDeque};

pub mod e2e;

const MAX_PLOT_SAMPLES: usize = 2048;

#[derive(Debug, Default)]
pub struct Debugger {
    pub disassembly: Disassembly,
    pub static_analysis_enabled: bool,
    pub breakpoints: HashSet<RomLocation>,
    pub hit_breakpoint: bool,
    pub total_cycles: u128,
    pub record_channels: bool,
    apu_channel_sample_counter: u32,
    apu_channel_stride: u32,
    apu_channel_stride_for: u32,
    pub ch1_samples: VecDeque<f32>,
    pub ch2_samples: VecDeque<f32>,
    pub ch3_samples: VecDeque<f32>,
    pub ch4_samples: VecDeque<f32>,
    pub ch1_disabled: bool,
    pub ch2_disabled: bool,
    pub ch3_disabled: bool,
    pub ch4_disabled: bool,
}

impl Debugger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn soft_reset(&mut self) {
        self.total_cycles = 0;
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

    fn record_apu_channels(&mut self, sample_rate: u32, ch1: f32, ch2: f32, ch3: f32, ch4: f32) {
        let stride = self.apu_channel_stride(sample_rate);
        if self.apu_channel_sample_counter < stride {
            self.apu_channel_sample_counter += 1;
            return;
        }
        self.apu_channel_sample_counter -= stride;

        if self.ch1_samples.len() >= MAX_PLOT_SAMPLES {
            self.ch1_samples.pop_front();
            self.ch2_samples.pop_front();
            self.ch3_samples.pop_front();
            self.ch4_samples.pop_front();
        }

        let to_visual = |s: f32| {
            if s == 0.0 { 0.0 } else { (1.0 - s) / 2.0 }
        };

        self.ch1_samples.push_back(to_visual(ch1));
        self.ch2_samples.push_back(to_visual(ch2));
        self.ch3_samples.push_back(to_visual(ch3));
        self.ch4_samples.push_back(to_visual(ch4));
    }

    fn channel_1_enabled(&self) -> bool {
        !self.ch1_disabled
    }

    fn channel_2_enabled(&self) -> bool {
        !self.ch2_disabled
    }

    fn channel_3_enabled(&self) -> bool {
        !self.ch3_disabled
    }

    fn channel_4_enabled(&self) -> bool {
        !self.ch4_disabled
    }
}

pub trait DebuggerInterface {
    fn wants_apu_channels(&self) -> bool {
        false
    }

    fn record_apu_channels(
        &mut self,
        _sample_rate: u32,
        _ch1: f32,
        _ch2: f32,
        _ch3: f32,
        _ch4: f32,
    ) {
    }

    fn channel_1_enabled(&self) -> bool {
        true
    }

    fn channel_2_enabled(&self) -> bool {
        true
    }

    fn channel_3_enabled(&self) -> bool {
        true
    }

    fn channel_4_enabled(&self) -> bool {
        true
    }
}

#[cfg(feature = "debug")]
pub trait DebuggerAccess {
    fn debugger(&self) -> &dyn DebuggerInterface;
    fn debugger_mut(&mut self) -> &mut dyn DebuggerInterface;
}

impl<T: DebuggerAccess> DebuggerInterface for T {}
