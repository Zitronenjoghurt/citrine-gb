use crate::gb::GbModel;
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

#[derive(Debug, Clone, Copy)]
pub struct WriteContext {
    pub frame_clocked_length: bool,
    pub lf_div: u16,
    pub cgb: bool,
    pub double_speed: bool,
}

const SKIP_DIV_EVENT: u8 = 2;
const SKIPPED_DIV_EVENT: u8 = 1;
pub const APU_CLOCK_RATE: u32 = 4_194_304;
pub const MAX_AUDIO_BUFFER_SIZE: u32 = 8192;
const DEFAULT_SAMPLE_RATE: u32 = 44_100;
const MAX_FRAME_SAMPLES: u64 = 2048;
const BLIP_SCALE: f32 = 16_000.0;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Apu {
    cgb: bool,
    /// Increments at a frequency of 512 Hz
    div_apu: u8,
    skip_div_event: u8,
    prev_div: u16,
    lagged_div: u16,
    ds_delay: bool,
    div_bit: u8,
    apu_phase: u8,
    pending_envelope_tick: bool,
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
    #[cfg_attr(feature = "serde", serde(skip, default = "default_max_frame_time"))]
    max_frame_time: u32,
    prev_l: i32,
    prev_r: i32,
    pub output_sample_rate: u32,
    pub charge_factor: f32,
    pub audio_buffer: Vec<f32>,
    #[cfg_attr(feature = "serde", serde(skip, default = "yes"))]
    output_dirty: bool,
}

#[cfg(feature = "serde")]
fn yes() -> bool {
    true
}

#[cfg(feature = "serde")]
fn default_max_frame_time() -> u32 {
    max_frame_time(DEFAULT_SAMPLE_RATE)
}

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
            cgb: false,
            div_apu: 0,
            skip_div_event: 0,
            prev_div: 0,
            lagged_div: 0,
            ds_delay: false,
            div_bit: 12,
            apu_phase: 0,
            pending_envelope_tick: false,
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
            max_frame_time: max_frame_time(DEFAULT_SAMPLE_RATE),
            prev_l: 0,
            prev_r: 0,
            output_sample_rate: DEFAULT_SAMPLE_RATE,
            charge_factor: charge_factor(DEFAULT_SAMPLE_RATE),
            audio_buffer: vec![],
            output_dirty: true,
        }
    }
}

impl Apu {
    pub fn new(model: GbModel) -> Self {
        Self {
            cgb: model == GbModel::Cgb,
            ..Self::default()
        }
    }

    pub fn restore(&mut self, model: GbModel) {
        self.cgb = model == GbModel::Cgb;
        if self.ch4.enabled {
            self.ch4.counter_active = true;
        }
    }

    pub fn apply_post_boot(&mut self) {
        self.write_naive(0xFF26, 0x80);
        self.write_naive(0xFF11, 0x80);
        self.write_naive(0xFF12, 0xF3);
        self.write_naive(0xFF24, 0x77);
        self.write_naive(0xFF25, 0xF3);
        self.ch1.enabled = true;
    }

    pub fn cycle(
        &mut self,
        timer: &Timer,
        double_speed: bool,
        #[cfg(feature = "debug")] debugger: &mut impl crate::debug::DebuggerInterface,
    ) {
        if self.time >= self.max_frame_time {
            self.flush_audio();
        }
        self.update_frame_sequencer(timer, double_speed);
        let ticks = if double_speed { 2 } else { 4 };

        #[cfg(feature = "debug")]
        let always_sample = debugger.wants_apu_channels();
        #[cfg(not(feature = "debug"))]
        let always_sample = false;

        if self.ch1.sweep_busy() {
            for step in 1..=ticks / 2 {
                let lf_div = 1 ^ (self.apu_phase.wrapping_add(step) & 1) as u16;
                self.ch1.sweep_tick(lf_div);
            }
        }
        self.apu_phase = self.apu_phase.wrapping_add(ticks / 2);

        if !self.output_dirty && !always_sample && self.quiet(ticks as u16) {
            self.skip(ticks as u16);
            self.time += ticks as u32;
            return;
        }

        for _ in 0..ticks {
            let moved = self.tick();

            if moved || self.output_dirty || always_sample {
                self.output_dirty = false;

                let (out_l_f, out_r_f) = self.sample(
                    #[cfg(feature = "debug")]
                    debugger,
                );
                let current_l = (out_l_f * BLIP_SCALE) as i32;
                let current_r = (out_r_f * BLIP_SCALE) as i32;

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
            let raw_l = out_l[i] as f32 / BLIP_SCALE;
            let raw_r = out_r[i] as f32 / BLIP_SCALE;

            let filtered_l = raw_l - self.hpf_capacitor_l;
            let filtered_r = raw_r - self.hpf_capacitor_r;

            self.hpf_capacitor_l = raw_l - filtered_l * self.charge_factor;
            self.hpf_capacitor_r = raw_r - filtered_r * self.charge_factor;

            self.audio_buffer.push(filtered_l);
            self.audio_buffer.push(filtered_r);
        }

        let max_len = self.output_sample_rate as usize * 2;
        if self.audio_buffer.len() > max_len {
            let excess = self.audio_buffer.len() - max_len;
            self.audio_buffer.drain(..excess);
        }
    }

    pub fn speed_switched(&mut self, div: u16, double_speed: bool) {
        let bit = |value: u16| (value >> self.div_bit) & 1 == 1;
        let step = bit(self.prev_div) || bit(div);
        self.prev_div = 0;
        self.lagged_div = 0;
        self.div_bit = if double_speed { 13 } else { 12 };
        if !self.nr52.audio_enabled {
            return;
        }
        if step {
            self.div_event(!double_speed);
        }
        if double_speed {
            self.ds_delay = !self.ds_delay;
        }
    }

    fn update_frame_sequencer(&mut self, timer: &Timer, double_speed: bool) {
        self.div_bit = if double_speed { 13 } else { 12 };
        let div = if self.ds_delay && double_speed {
            self.lagged_div
        } else {
            timer.div
        };
        self.lagged_div = timer.div;
        let set_prev = ((self.prev_div >> self.div_bit) & 1) == 1;
        let set_now = ((div >> self.div_bit) & 1) == 1;
        self.prev_div = div;

        if !self.nr52.audio_enabled {
            self.pending_envelope_tick = false;
            return;
        }

        if self.pending_envelope_tick {
            self.pending_envelope_tick = false;
            self.output_dirty = true;
            self.tick_envelopes();
        }

        if !set_prev && set_now {
            self.ch1.envelope_secondary_div_event();
            self.ch2.envelope_secondary_div_event();
            self.ch4.envelope_secondary_div_event();
            return;
        }

        if set_prev && !set_now {
            self.div_event(double_speed);
        }
    }

    fn div_event(&mut self, double_speed: bool) {
        match self.skip_div_event {
            SKIP_DIV_EVENT => {
                self.skip_div_event = SKIPPED_DIV_EVENT;
                return;
            }
            SKIPPED_DIV_EVENT => self.skip_div_event = 0,
            _ => self.div_apu = (self.div_apu + 1) & 0b111,
        }

        self.output_dirty = true;

        if self.div_apu & 1 == 1 {
            self.clock_length_counters();
        }
        if self.div_apu & 3 == 3 {
            self.clock_sweep();
        }
        self.ch1.envelope_div_event(self.div_apu);
        self.ch2.envelope_div_event(self.div_apu);
        self.ch4.envelope_div_event(self.div_apu);
        if self.cgb && double_speed {
            self.pending_envelope_tick = true;
        } else {
            self.tick_envelopes();
        }
    }

    fn write_context(&self) -> WriteContext {
        WriteContext {
            frame_clocked_length: self.div_apu & 1 == 1,
            lf_div: 1 ^ (self.apu_phase & 1) as u16,
            cgb: self.cgb,
            double_speed: self.div_bit == 13,
        }
    }

    fn write_nr52(&mut self, value: u8) {
        let on = (value & 0x80) != 0;
        if on && !self.nr52.audio_enabled {
            self.nr52.audio_enabled = true;
            self.apu_phase = 0;
            self.div_apu = 0;
            self.skip_div_event = 0;
            if (self.prev_div >> self.div_bit) & 1 == 1 {
                self.div_apu = 1;
                self.skip_div_event = SKIP_DIV_EVENT;
            }
        } else if !on && self.nr52.audio_enabled {
            self.power_off();
        }
    }

    fn power_off(&mut self) {
        let lengths = [
            self.ch1.length_counter.counter,
            self.ch2.length_counter.counter,
            self.ch3.length_counter.counter,
            self.ch4.length_counter.counter,
        ];
        let wave_ram = self.ch3.wave_ram;

        self.ds_delay = false;
        self.nr52.audio_enabled = false;
        self.nr50 = Default::default();
        self.nr51 = Default::default();
        self.ch1 = Default::default();
        self.ch2 = Default::default();
        self.ch3 = Default::default();
        self.ch4 = Default::default();
        self.ch3.wave_ram = wave_ram;

        if !self.cgb {
            self.ch1.length_counter.counter = lengths[0];
            self.ch2.length_counter.counter = lengths[1];
            self.ch3.length_counter.counter = lengths[2];
            self.ch4.length_counter.counter = lengths[3];
        }
    }

    fn read_nr52(&self) -> u8 {
        AudioMasterControl {
            channel_1_enabled: self.ch1.enabled,
            channel_2_enabled: self.ch2.enabled,
            channel_3_enabled: self.ch3.enabled,
            channel_4_enabled: self.ch4.enabled,
            audio_enabled: self.nr52.audio_enabled,
        }
        .into()
    }

    fn quiet(&self, ticks: u16) -> bool {
        self.ch1.quiet(ticks)
            && self.ch2.quiet(ticks)
            && self.ch3.quiet(ticks)
            && self.ch4.quiet(ticks)
    }

    fn skip(&mut self, ticks: u16) {
        self.ch1.skip(ticks);
        self.ch2.skip(ticks);
        self.ch3.skip(ticks);
        self.ch4.skip(ticks);
    }

    fn tick(&mut self) -> bool {
        let ch1 = self.ch1.tick();
        let ch2 = self.ch2.tick();
        let ch3 = self.ch3.tick();
        let ch4 = self.ch4.tick();
        ch1 || ch2 || ch3 || ch4
    }

    fn clock_length_counters(&mut self) {
        self.ch1.clock_length();
        self.ch2.clock_length();
        self.ch3.clock_length();
        self.ch4.clock_length();
    }

    fn tick_envelopes(&mut self) {
        self.ch1.envelope_tick();
        self.ch2.envelope_tick();
        self.ch4.envelope_tick();
    }

    fn clock_sweep(&mut self) {
        let lf_div = self.write_context().lf_div;
        self.ch1.clock_sweep(lf_div);
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
        if debugger.wants_apu_channels() {
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
        left /= 64.0;
        right *= (self.nr50.volume_right + 1) as f32;
        right /= 64.0;

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
        self.max_frame_time = max_frame_time(sample_rate);
    }
}

impl ReadMemory for Apu {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF10..=0xFF14 => self.ch1.read_naive(addr),
            0xFF16..=0xFF19 => self.ch2.read_naive(addr),
            0xFF1A..=0xFF1E => self.ch3.read_naive(addr),
            0xFF30..=0xFF3F => self.ch3.read_wave(addr, self.cgb),
            0xFF20..=0xFF23 => self.ch4.read_naive(addr),
            0xFF24 => self.nr50.into(),
            0xFF25 => self.nr51.into(),
            0xFF26 => self.read_nr52(),
            0xFF76 => self.ch1.sample() | (self.ch2.sample() << 4),
            0xFF77 => self.ch3.sample() | (self.ch4.sample() << 4),
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Apu {
    fn write_naive(&mut self, addr: u16, value: u8) {
        self.output_dirty = true;

        let ctx = self.write_context();
        match addr {
            0xFF26 => self.write_nr52(value),
            0xFF30..=0xFF3F => self.ch3.write_wave(addr, value, self.cgb),
            _ if !self.nr52.audio_enabled => {
                if !self.cgb {
                    match addr {
                        0xFF11 => self.ch1.write_length(value),
                        0xFF16 => self.ch2.write_length(value),
                        0xFF1B => self.ch3.write_length(value),
                        0xFF20 => self.ch4.write_length(value),
                        _ => {}
                    }
                }
            }
            0xFF10..=0xFF14 => self.ch1.write(addr, value, ctx),
            0xFF16..=0xFF19 => self.ch2.write(addr, value, ctx),
            0xFF1A..=0xFF1E => self.ch3.write(addr, value, ctx),
            0xFF20..=0xFF23 => self.ch4.write(addr, value, ctx),
            0xFF24 => self.nr50 = value.into(),
            0xFF25 => self.nr51 = value.into(),
            _ => {}
        }
    }
}

pub fn charge_factor(sample_rate: u32) -> f32 {
    0.999958_f64.powf(APU_CLOCK_RATE as f64 / sample_rate as f64) as f32
}

fn max_frame_time(sample_rate: u32) -> u32 {
    (APU_CLOCK_RATE as u64 * MAX_FRAME_SAMPLES / sample_rate.max(1) as u64).min(u32::MAX as u64)
        as u32
}
