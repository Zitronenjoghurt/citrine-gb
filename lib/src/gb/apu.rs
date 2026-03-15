use crate::gb::apu::channels::channel_1::Channel1;
use crate::gb::apu::channels::channel_2::Channel2;
use crate::gb::apu::channels::channel_3::Channel3;
use crate::gb::apu::channels::channel_4::Channel4;
use crate::gb::timer::Timer;
use crate::{ReadMemory, WriteMemory};
use blip_buf::BlipBuf;
use registers::audio_master_control::AudioMasterControl;
use registers::master_volume_vin::MasterVolumeVin;
use registers::sound_panning::SoundPanning;

mod channels;
mod components;
mod registers;

pub const APU_CLOCK_RATE: u32 = 4_194_304;
pub const MAX_AUDIO_BUFFER_SIZE: u32 = 8192;
const DEFAULT_SAMPLE_RATE: u32 = 44_100;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Apu {
    /// Increments at a frequency of 512 Hz
    div_apu: u8,
    prev_div: u16,
    pub nr50: MasterVolumeVin,
    pub nr51: SoundPanning,
    pub nr52: AudioMasterControl,
    pub ch1: Channel1,
    pub ch2: Channel2,
    pub ch3: Channel3,
    pub ch4: Channel4,
    hpf_capacitor_l: f32,
    hpf_capacitor_r: f32,
    #[cfg_attr(feature = "serde", serde(skip, default = "default_blip_buf"))]
    pub blip_l: BlipBuf,
    #[cfg_attr(feature = "serde", serde(skip, default = "default_blip_buf"))]
    pub blip_r: BlipBuf,
    time: u32,
    prev_l: i32,
    prev_r: i32,
    pub output_sample_rate: u32,
    pub charge_factor: f32,
    pub audio_buffer: Vec<f32>,
}

#[cfg(feature = "serde")]
fn default_blip_buf() -> BlipBuf {
    BlipBuf::new(MAX_AUDIO_BUFFER_SIZE)
}

impl Default for Apu {
    fn default() -> Self {
        let mut blip_l = BlipBuf::new(MAX_AUDIO_BUFFER_SIZE);
        let mut blip_r = BlipBuf::new(MAX_AUDIO_BUFFER_SIZE);
        blip_l.set_rates(APU_CLOCK_RATE as f64, DEFAULT_SAMPLE_RATE as f64);
        blip_r.set_rates(APU_CLOCK_RATE as f64, DEFAULT_SAMPLE_RATE as f64);

        Self {
            div_apu: 0,
            prev_div: 0,
            nr50: Default::default(),
            nr51: Default::default(),
            nr52: Default::default(),
            ch1: Default::default(),
            ch2: Default::default(),
            ch3: Default::default(),
            ch4: Default::default(),
            hpf_capacitor_l: 0.0,
            hpf_capacitor_r: 0.0,
            blip_l,
            blip_r,
            time: 0,
            prev_l: 0,
            prev_r: 0,
            output_sample_rate: DEFAULT_SAMPLE_RATE,
            charge_factor: charge_factor(DEFAULT_SAMPLE_RATE),
            audio_buffer: vec![],
        }
    }
}

impl Apu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cycle(
        &mut self,
        timer: &Timer,
        double_speed: bool,
        #[cfg(feature = "debug")] debugger: &mut impl crate::debug::DebuggerInterface,
    ) {
        self.update_frame_sequencer(timer, double_speed);
        let ticks = if double_speed { 2 } else { 4 };

        for _ in 0..ticks {
            self.tick();

            let (out_l_f, out_r_f) = self.sample(
                #[cfg(feature = "debug")]
                debugger,
            );
            let current_l = (out_l_f * 1000.0) as i32;
            let current_r = (out_r_f * 1000.0) as i32;

            let delta_l = current_l - self.prev_l;
            let delta_r = current_r - self.prev_r;

            if delta_l != 0 {
                self.blip_l.add_delta(self.time, delta_l);
                self.prev_l = current_l;
            }

            if delta_r != 0 {
                self.blip_r.add_delta(self.time, delta_r);
                self.prev_r = current_r;
            }

            self.time += 1;
        }
    }

    pub fn flush_audio(&mut self) {
        self.blip_l.end_frame(self.time);
        self.blip_r.end_frame(self.time);
        self.time = 0;

        let available_samples = self.blip_l.samples_avail() as usize;
        let mut out_l = vec![0i16; available_samples];
        let mut out_r = vec![0i16; available_samples];

        self.blip_l.read_samples(&mut out_l, false);
        self.blip_r.read_samples(&mut out_r, false);

        for i in 0..available_samples {
            let raw_l = out_l[i] as f32 / 1000.0;
            let raw_r = out_r[i] as f32 / 1000.0;

            let filtered_l = raw_l - self.hpf_capacitor_l;
            let filtered_r = raw_r - self.hpf_capacitor_r;

            self.hpf_capacitor_l = raw_l - filtered_l * self.charge_factor;
            self.hpf_capacitor_r = raw_r - filtered_r * self.charge_factor;

            self.audio_buffer.push(filtered_l);
            self.audio_buffer.push(filtered_r);
        }
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

    fn tick(&mut self) {
        self.ch1.tick();
        self.ch2.tick();
        self.ch3.tick();
        self.ch4.tick();
    }

    fn clock_length_counters(&mut self) {
        self.ch1.clock_length();
        self.ch2.clock_length();
        self.ch3.clock_length();
        self.ch4.clock_length();
    }

    fn clock_volume_envelopes(&mut self) {
        self.ch1.clock_volume_envelope();
        self.ch2.clock_volume_envelope();
        self.ch4.clock_volume_envelope();
    }

    fn clock_sweep(&mut self) {
        self.ch1.clock_sweep();
    }
}

// Sampling
impl Apu {
    pub fn sample(
        &mut self,
        #[cfg(feature = "debug")] debugger: &mut impl crate::debug::DebuggerInterface,
    ) -> (f32, f32) {
        if !self.nr52.audio_enabled {
            return (0.0, 0.0);
        };

        let ch1_sample = 'ch1: {
            if !self.ch1.dac_enabled() {
                break 'ch1 0.0;
            }

            #[cfg(feature = "debug")]
            if !debugger.channel_1_enabled() {
                break 'ch1 0.0;
            }

            1.0 - (self.ch1.sample() as f32 / 7.5)
        };

        let ch2_sample = 'ch2: {
            if !self.ch2.dac_enabled() {
                break 'ch2 0.0;
            }

            #[cfg(feature = "debug")]
            if !debugger.channel_2_enabled() {
                break 'ch2 0.0;
            }

            1.0 - (self.ch2.sample() as f32 / 7.5)
        };

        let ch3_sample = 'ch3: {
            if !self.ch3.dac_enabled() {
                break 'ch3 0.0;
            }

            #[cfg(feature = "debug")]
            if !debugger.channel_3_enabled() {
                break 'ch3 0.0;
            }

            1.0 - (self.ch3.sample() as f32 / 7.5)
        };

        let ch4_sample = 'ch4: {
            if !self.ch4.dac_enabled() {
                break 'ch4 0.0;
            }

            #[cfg(feature = "debug")]
            if !debugger.channel_4_enabled() {
                break 'ch4 0.0;
            }

            1.0 - (self.ch4.sample() as f32 / 7.5)
        };

        #[cfg(feature = "debug")]
        {
            debugger.record_apu_channels(
                self.output_sample_rate,
                ch1_sample,
                ch2_sample,
                ch3_sample,
                ch4_sample,
            );
        }

        let mut left = 0.0;
        let mut right = 0.0;

        if self.nr51.channel_1_left {
            left += ch1_sample;
        };

        if self.nr51.channel_1_right {
            right += ch1_sample;
        };

        if self.nr51.channel_2_left {
            left += ch2_sample;
        };

        if self.nr51.channel_2_right {
            right += ch2_sample;
        };

        if self.nr51.channel_3_left {
            left += ch3_sample;
        };

        if self.nr51.channel_3_right {
            right += ch3_sample;
        };

        if self.nr51.channel_4_left {
            left += ch4_sample;
        };

        if self.nr51.channel_4_right {
            right += ch4_sample;
        };

        left *= (self.nr50.volume_left + 1) as f32;
        left /= 32.0;
        right *= (self.nr50.volume_right + 1) as f32;
        right /= 32.0;

        let any_dac_enabled = self.ch1.dac_enabled()
            || self.ch2.dac_enabled()
            || self.ch3.dac_enabled()
            || self.ch4.dac_enabled();

        if any_dac_enabled {
            (left, right)
        } else {
            (0.0, 0.0)
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.output_sample_rate = sample_rate;
        self.blip_l
            .set_rates(APU_CLOCK_RATE as f64, sample_rate as f64);
        self.blip_r
            .set_rates(APU_CLOCK_RATE as f64, sample_rate as f64);
        self.charge_factor = charge_factor(sample_rate);
    }
}

impl ReadMemory for Apu {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF10..=0xFF14 => self.ch1.read_naive(addr),
            0xFF16..=0xFF19 => self.ch2.read_naive(addr),
            0xFF1A..=0xFF1E | 0xFF30..=0xFF3F => self.ch3.read_naive(addr),
            0xFF20..=0xFF23 => self.ch4.read_naive(addr),
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
            0xFF16..=0xFF19 => self.ch2.write_naive(addr, value),
            0xFF1A..=0xFF1E | 0xFF30..=0xFF3F => self.ch3.write_naive(addr, value),
            0xFF20..=0xFF23 => self.ch4.write_naive(addr, value),
            0xFF24 => self.nr50 = value.into(),
            0xFF25 => self.nr51 = value.into(),
            0xFF26 => self.nr52 = value.into(),
            _ => {}
        }
    }
}

pub fn charge_factor(sample_rate: u32) -> f32 {
    0.999958_f64.powf(APU_CLOCK_RATE as f64 / sample_rate as f64) as f32
}
