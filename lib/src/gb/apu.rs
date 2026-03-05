use crate::gb::apu::channels::channel_1::Channel1;
use crate::gb::timer::Timer;
use crate::{ReadMemory, WriteMemory};
use global::audio_master_control::AudioMasterControl;
use global::master_volume_vin::MasterVolumeVin;
use global::sound_panning::SoundPanning;

mod channels;
mod components;
mod global;

const CHARGE_FACTOR: f32 = 0.996;
const APU_CLOCK_RATE: u32 = 4_194_304;
const MAX_AUDIO_BUFFER_SIZE: usize = 4096;

#[derive(Debug)]
pub struct Apu {
    /// Increments at a frequency of 512 Hz
    div_apu: u8,
    prev_div: u16,
    pub nr50: MasterVolumeVin,
    pub nr51: SoundPanning,
    pub nr52: AudioMasterControl,
    pub ch1: Channel1,
    capacitor_l: f32,
    capacitor_r: f32,
    downsample_counter: u32,
    acc_l: f32,
    acc_r: f32,
    acc_samples: u32,
    pub output_sample_rate: u32,
    pub audio_buffer: Vec<f32>,
}

impl Default for Apu {
    fn default() -> Self {
        Self {
            div_apu: 0,
            prev_div: 0,
            nr50: Default::default(),
            nr51: Default::default(),
            nr52: Default::default(),
            ch1: Default::default(),
            capacitor_l: 0.0,
            capacitor_r: 0.0,
            downsample_counter: 0,
            acc_l: 0.0,
            acc_r: 0.0,
            acc_samples: 0,
            output_sample_rate: 44_100,
            audio_buffer: vec![],
        }
    }
}

impl Apu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cycle(&mut self, timer: &Timer, double_speed: bool) {
        self.update_frame_sequencer(timer, double_speed);
        let ticks = if double_speed { 2 } else { 4 };

        for _ in 0..ticks {
            self.tick();

            let (l, r) = self.sample();
            self.acc_l += l;
            self.acc_r += r;
            self.acc_samples += 1;

            self.downsample_counter += self.output_sample_rate;

            if self.downsample_counter >= APU_CLOCK_RATE {
                self.downsample_counter -= APU_CLOCK_RATE;

                let avg_l = self.acc_l / self.acc_samples as f32;
                let avg_r = self.acc_r / self.acc_samples as f32;

                if self.audio_buffer.len() < MAX_AUDIO_BUFFER_SIZE {
                    self.audio_buffer.push(avg_l);
                    self.audio_buffer.push(avg_r);
                }

                self.acc_l = 0.0;
                self.acc_r = 0.0;
                self.acc_samples = 0;
            }
        }
    }

    pub fn tick(&mut self) {
        self.ch1.tick();
    }

    fn update_frame_sequencer(&mut self, timer: &Timer, double_speed: bool) {
        let bit = if double_speed { 13 } else { 12 };
        let set_prev = ((self.prev_div >> bit) & 1) == 1;
        let set_now = ((timer.div >> bit) & 1) == 1;

        if set_prev && !set_now {
            self.div_apu = (self.div_apu + 1) & 0b111;

            match self.div_apu {
                0 => self.clock_length_counters(),
                1 => {}
                2 => {
                    self.clock_length_counters();
                    self.clock_sweep();
                }
                3 => {}
                4 => self.clock_length_counters(),
                5 => {}
                6 => {
                    self.clock_length_counters();
                    self.clock_sweep();
                }
                7 => self.clock_volume_envelopes(),
                _ => unreachable!(),
            }
        }

        self.prev_div = timer.div;
    }

    fn clock_length_counters(&mut self) {
        self.ch1.clock_length();
    }

    fn clock_volume_envelopes(&mut self) {
        self.ch1.clock_volume_envelope();
    }

    fn clock_sweep(&mut self) {
        self.ch1.clock_sweep();
    }
}

// Sampling
impl Apu {
    pub fn sample(&mut self) -> (f32, f32) {
        if !self.nr52.audio_enabled {
            return (0.0, 0.0);
        };

        let ch1_sample = if self.ch1.dac_enabled() {
            1.0 - (self.ch1.sample() as f32 / 7.5)
        } else {
            0.0
        };

        let mut left = 0.0;
        let mut right = 0.0;

        if self.nr51.channel_1_left {
            left = ch1_sample;
        };

        if self.nr51.channel_1_right {
            right = ch1_sample;
        };

        left *= (self.nr50.volume_left + 1) as f32;
        left /= 32.0;
        right *= (self.nr50.volume_right + 1) as f32;
        right /= 32.0;

        let any_dac_enabled = self.ch1.dac_enabled();

        let mut out_l = 0.0;
        let mut out_r = 0.0;

        if any_dac_enabled {
            out_l = left - self.capacitor_l;
            out_r = right - self.capacitor_r;

            self.capacitor_l = left - out_l * CHARGE_FACTOR;
            self.capacitor_r = right - out_r * CHARGE_FACTOR;
        } else {
            self.capacitor_l *= CHARGE_FACTOR;
            self.capacitor_r *= CHARGE_FACTOR;
        }

        (out_l, out_r)
    }
}

impl ReadMemory for Apu {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF10..=0xFF14 => self.ch1.read_naive(addr),
            0xFF24 => self.nr50.into(),
            0xFF25 => self.nr51.into(),
            0xFF26 => self.nr52.into(),
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Apu {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF10..=0xFF14 => self.ch1.write_naive(addr, value),
            0xFF24 => self.nr50 = value.into(),
            0xFF25 => self.nr51 = value.into(),
            0xFF26 => self.nr52 = value.into(),
            _ => {}
        }
    }
}
