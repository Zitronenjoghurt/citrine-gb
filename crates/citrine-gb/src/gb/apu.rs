use crate::gb::GbModel;
use crate::gb::apu::channels::channel_1::Channel1;
use crate::gb::apu::channels::channel_2::Channel2;
use crate::gb::apu::channels::channel_3::Channel3;
use crate::gb::apu::channels::channel_4::Channel4;
use crate::{ReadMemory, WriteMemory};
use citrine_gb_blip::BlipBuf;
use registers::audio_master_control::AudioMasterControl;
use registers::master_volume_vin::MasterVolumeVin;
use registers::sound_panning::SoundPanning;

pub(crate) mod channels;
pub(crate) mod components;
pub(crate) mod registers;

#[derive(Debug, Clone, Copy)]
pub struct WriteContext {
    pub frame_clocked_length: bool,
    pub lf_div: u16,
    pub cgb: bool,
    pub double_speed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Idle,
    Same,
    Changed,
}

const SKIP_DIV_EVENT: u8 = 2;
const SKIPPED_DIV_EVENT: u8 = 1;
pub const APU_CLOCK_RATE: u32 = 4_194_304;
pub const MAX_AUDIO_BUFFER_SIZE: u32 = 8192;
pub const CHANNEL_NAMES: [&str; 4] = ["Pulse 1", "Pulse 2", "Wave", "Noise"];
const DEFAULT_SAMPLE_RATE: u32 = 44_100;
const MAX_FRAME_SAMPLES: u64 = 2048;
const BLIP_SCALE: f32 = 16_000.0;
static LEVELS: [f32; 256] = {
    let mut levels = [0.0; 256];
    let mut level = 0;
    while level < levels.len() {
        levels[level] = 1.0 - (level as f32 / 7.5);
        level += 1;
    }
    levels
};

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
    pub blip: BlipBuf<2>,
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
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) muted: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) muted_channels: [bool; 4],
    #[cfg_attr(feature = "serde", serde(skip))]
    owed: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    mixed: bool,
}

#[cfg(feature = "serde")]
fn yes() -> bool {
    true
}

#[cfg(feature = "serde")]
fn default_max_frame_time() -> u32 {
    max_frame_time(DEFAULT_SAMPLE_RATE)
}

#[cfg(feature = "serde")]
fn default_blip_buf() -> BlipBuf<2> {
    BlipBuf::new(MAX_AUDIO_BUFFER_SIZE)
}

impl Default for Apu {
    fn default() -> Self {
        let mut blip = BlipBuf::new(MAX_AUDIO_BUFFER_SIZE);
        blip.set_rates(APU_CLOCK_RATE as f64, DEFAULT_SAMPLE_RATE as f64);

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
            blip,
            time: 0,
            max_frame_time: max_frame_time(DEFAULT_SAMPLE_RATE),
            prev_l: 0,
            prev_r: 0,
            output_sample_rate: DEFAULT_SAMPLE_RATE,
            charge_factor: charge_factor(DEFAULT_SAMPLE_RATE),
            audio_buffer: vec![],
            output_dirty: true,
            muted: false,
            muted_channels: [false; 4],
            owed: 0,
            mixed: false,
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

    pub fn clone_state(&self) -> Self {
        let mut apu = Self::default();
        apu.copy_state_from(self);
        apu
    }

    pub fn copy_state_from(&mut self, source: &Self) {
        let Self {
            cgb,
            div_apu,
            skip_div_event,
            prev_div,
            lagged_div,
            ds_delay,
            div_bit,
            apu_phase,
            pending_envelope_tick,
            nr50,
            nr51,
            nr52,
            ch1,
            ch2,
            ch3,
            ch4,
            hpf_capacitor_l: _,
            hpf_capacitor_r: _,
            blip: _,
            time: _,
            max_frame_time: _,
            prev_l: _,
            prev_r: _,
            output_sample_rate: _,
            charge_factor: _,
            audio_buffer: _,
            output_dirty,
            muted: _,
            muted_channels: _,
            owed,
            mixed,
        } = self;
        *cgb = source.cgb;
        *div_apu = source.div_apu;
        *skip_div_event = source.skip_div_event;
        *prev_div = source.prev_div;
        *lagged_div = source.lagged_div;
        *ds_delay = source.ds_delay;
        *div_bit = source.div_bit;
        *apu_phase = source.apu_phase;
        *pending_envelope_tick = source.pending_envelope_tick;
        *nr50 = source.nr50;
        *nr51 = source.nr51;
        *nr52 = source.nr52;
        ch1.clone_from(&source.ch1);
        ch2.clone_from(&source.ch2);
        ch3.clone_from(&source.ch3);
        ch4.clone_from(&source.ch4);
        *output_dirty = source.output_dirty;
        *owed = source.owed;
        *mixed = false;
    }

    pub fn channel_enabled(&self, index: usize) -> bool {
        !self.muted_channels.get(index).copied().unwrap_or(false)
    }

    pub fn set_channel_enabled(&mut self, index: usize, enabled: bool) {
        if let Some(muted) = self.muted_channels.get_mut(index)
            && *muted == enabled
        {
            *muted = !enabled;
            self.output_dirty = true;
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

    pub(crate) fn owe_cycle(&mut self) {
        self.owed += 1;
    }

    pub(crate) fn owe_cycles(&mut self, cycles: u32) {
        self.owed += cycles;
    }

    pub fn settle(
        &mut self,
        div: u16,
        double_speed: bool,
        #[cfg(feature = "debug")] debugger: &mut impl crate::debug::DebuggerInterface,
    ) {
        let owed = std::mem::take(&mut self.owed);
        if owed == 0 {
            return;
        }

        #[cfg(feature = "debug")]
        let always_sample = debugger.wants_apu_channels();
        #[cfg(not(feature = "debug"))]
        let always_sample = false;

        let mut div = div.wrapping_sub(owed.wrapping_sub(1).wrapping_mul(4) as u16);
        let mut left = owed;
        let mut edge_ticks = 0;
        while left > 0 {
            let span = if always_sample {
                0
            } else {
                self.quiet_cycles(div, double_speed, edge_ticks).min(left)
            };
            if span == 0 {
                if edge_ticks > 0 {
                    self.run_ticks(
                        edge_ticks,
                        #[cfg(feature = "debug")]
                        debugger,
                    );
                }
                edge_ticks = self.cycle(
                    div,
                    double_speed,
                    always_sample,
                    #[cfg(feature = "debug")]
                    debugger,
                );
                div = div.wrapping_add(4);
                left -= 1;
            } else {
                self.run_span(
                    span,
                    edge_ticks,
                    div,
                    double_speed,
                    #[cfg(feature = "debug")]
                    debugger,
                );
                edge_ticks = 0;
                div = div.wrapping_add(span.wrapping_mul(4) as u16);
                left -= span;
            }
        }
        if edge_ticks > 0 {
            self.run_ticks(
                edge_ticks,
                #[cfg(feature = "debug")]
                debugger,
            );
        }
    }

    fn quiet_cycles(&self, div: u16, double_speed: bool, edge_ticks: u32) -> u32 {
        let time = self.time + edge_ticks;
        if self.pending_envelope_tick || self.ch1.sweep_busy() || time >= self.max_frame_time {
            return 0;
        }
        let ticks = if double_speed { 2 } else { 4 };
        let flush = (self.max_frame_time - time).div_ceil(ticks);
        if !self.nr52.audio_enabled {
            return flush;
        }
        let bit = if double_speed { 13 } else { 12 };
        let delayed = self.ds_delay && double_speed;
        let first = if delayed { self.lagged_div } else { div };
        let set = |value: u16| (value >> bit) & 1;
        if set(self.prev_div) != set(first) {
            return 0;
        }
        if delayed && set(first) != set(div) {
            return 1;
        }
        let boundary = ((u32::from(div) >> bit) + 1) << bit;
        let edge = (boundary - u32::from(div)).div_ceil(4) + u32::from(delayed);
        edge.min(flush)
    }

    fn run_span(
        &mut self,
        cycles: u32,
        edge_ticks: u32,
        div: u16,
        double_speed: bool,
        #[cfg(feature = "debug")] debugger: &mut impl crate::debug::DebuggerInterface,
    ) {
        let ticks: u32 = if double_speed { 2 } else { 4 };
        let last = div.wrapping_add(cycles.wrapping_sub(1).wrapping_mul(4) as u16);
        self.div_bit = if double_speed { 13 } else { 12 };
        self.prev_div = match (self.ds_delay && double_speed, cycles) {
            (false, _) => last,
            (true, 1) => self.lagged_div,
            (true, _) => last.wrapping_sub(4),
        };
        self.lagged_div = last;
        self.apu_phase = self.apu_phase.wrapping_add((cycles * ticks / 2) as u8);
        self.run_ticks(
            edge_ticks + cycles * ticks,
            #[cfg(feature = "debug")]
            debugger,
        );
    }

    fn cycle(
        &mut self,
        div: u16,
        double_speed: bool,
        always_sample: bool,
        #[cfg(feature = "debug")] debugger: &mut impl crate::debug::DebuggerInterface,
    ) -> u32 {
        if self.time >= self.max_frame_time {
            self.flush_audio();
        }
        self.update_frame_sequencer(div, double_speed);
        let ticks = if double_speed { 2 } else { 4 };

        if self.ch1.sweep_busy() {
            let enabled = self.ch1.enabled;
            for step in 1..=ticks / 2 {
                let lf_div = 1 ^ (self.apu_phase.wrapping_add(step) & 1) as u16;
                self.ch1.sweep_tick(lf_div);
            }
            self.mixed &= self.ch1.enabled == enabled;
        }
        self.apu_phase = self.apu_phase.wrapping_add(ticks / 2);

        if !always_sample {
            return u32::from(ticks);
        }

        for _ in 0..ticks {
            self.tick();
            self.output_dirty = false;
            if self.muted {
                self.mixed = false;
            } else {
                self.mix(
                    #[cfg(feature = "debug")]
                    debugger,
                );
            }
            self.time += 1;
        }
        0
    }

    fn run_ticks(
        &mut self,
        mut left: u32,
        #[cfg(feature = "debug")] debugger: &mut impl crate::debug::DebuggerInterface,
    ) {
        #[cfg(feature = "debug")]
        let every_step = debugger.wants_apu_channels();
        #[cfg(not(feature = "debug"))]
        let every_step = false;
        while left > 0 {
            let quiet = if self.output_dirty {
                0
            } else {
                self.quiet_ticks(self.ch4_unheard(), every_step)
            };
            if quiet >= left {
                self.advance(left);
                self.time += left;
                return;
            }
            let step = if quiet == 0 {
                self.tick()
            } else if self.muted || every_step {
                self.advance(quiet);
                self.time += quiet;
                left -= quiet;
                continue;
            } else {
                self.advance(quiet + 1);
                self.time += quiet;
                left -= quiet;
                Step::Changed
            };
            if step == Step::Changed
                || self.output_dirty
                || (step == Step::Same && (every_step || !self.mixed))
            {
                self.output_dirty = false;
                if self.muted {
                    self.mixed = false;
                } else {
                    self.mix(
                        #[cfg(feature = "debug")]
                        debugger,
                    );
                }
            }
            self.time += 1;
            left -= 1;
        }
    }

    fn mix(
        &mut self,
        #[cfg(feature = "debug")] debugger: &mut impl crate::debug::DebuggerInterface,
    ) {
        let (out_l_f, out_r_f) = self.sample(
            #[cfg(feature = "debug")]
            debugger,
        );
        let current_l = (out_l_f * BLIP_SCALE) as i32;
        let current_r = (out_r_f * BLIP_SCALE) as i32;

        let delta_l = current_l - self.prev_l;
        let delta_r = current_r - self.prev_r;

        if delta_l != 0 || delta_r != 0 {
            self.blip.add_delta(self.time, [delta_l, delta_r]);
            self.prev_l = current_l;
            self.prev_r = current_r;
        }
        self.mixed = true;
    }

    pub fn flush_audio(&mut self) {
        if self.muted {
            self.time = 0;
            return;
        }
        self.blip.end_frame(self.time);
        self.time = 0;

        let available_samples = self.blip.samples_avail() as usize;
        if available_samples > 0 {
            let start = self.audio_buffer.len();
            self.audio_buffer.resize(start + available_samples * 2, 0.0);
            let mut capacitor = [self.hpf_capacitor_l, self.hpf_capacitor_r];
            let read = self.blip.read_high_passed(
                self.audio_buffer[start..].as_chunks_mut::<2>().0,
                BLIP_SCALE,
                self.charge_factor,
                &mut capacitor,
            );
            self.audio_buffer.truncate(start + read * 2);
            [self.hpf_capacitor_l, self.hpf_capacitor_r] = capacitor;
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

    fn update_frame_sequencer(&mut self, timer_div: u16, double_speed: bool) {
        self.div_bit = if double_speed { 13 } else { 12 };
        let div = if self.ds_delay && double_speed {
            self.lagged_div
        } else {
            timer_div
        };
        self.lagged_div = timer_div;
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

    fn quiet_ticks(&self, ch4_unheard: bool, every_step: bool) -> u32 {
        let steady = self.mixed && !every_step;
        let ch4 = if ch4_unheard {
            self.ch4.unheard_quiet_ticks()
        } else if steady {
            self.ch4.steady_ticks()
        } else {
            self.ch4.quiet_ticks()
        };
        let squares = if steady {
            self.ch1.steady_ticks().min(self.ch2.steady_ticks())
        } else {
            self.ch1.quiet_ticks().min(self.ch2.quiet_ticks())
        };
        let others = squares.min(ch4);
        let wave = self.ch3.quiet_ticks();
        if steady && wave < others {
            return self.ch3.steady_ticks().min(others);
        }
        wave.min(others)
    }

    fn advance(&mut self, ticks: u32) {
        self.ch1.advance(ticks);
        self.ch2.advance(ticks);
        self.ch3.advance(ticks);
        self.ch4.advance_unheard(ticks);
    }

    fn ch4_unheard(&self) -> bool {
        self.muted
            || (self.mixed
                && (self.muted_channels[3]
                    || !(self.nr51.channel_4_left || self.nr51.channel_4_right)
                    || self.ch4.volume_envelope.current_volume == 0))
    }

    fn tick(&mut self) -> Step {
        let ch1 = self.ch1.tick();
        let ch2 = self.ch2.tick();
        let ch3 = self.ch3.tick();
        let ch4 = self.ch4.step();
        ch1.max(ch2).max(ch3).max(ch4)
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

        let mut samples = [
            if self.ch1.dac_enabled() {
                LEVELS[usize::from(self.ch1.sample())]
            } else {
                0.0
            },
            if self.ch2.dac_enabled() {
                LEVELS[usize::from(self.ch2.sample())]
            } else {
                0.0
            },
            if self.ch3.dac_enabled() {
                LEVELS[usize::from(self.ch3.sample())]
            } else {
                0.0
            },
            if self.ch4.dac_enabled() {
                LEVELS[usize::from(self.ch4.sample())]
            } else {
                0.0
            },
        ];
        if self.muted_channels != [false; 4] {
            for (sample, muted) in samples.iter_mut().zip(self.muted_channels) {
                if muted {
                    *sample = 0.0;
                }
            }
        }
        let [ch1_sample, ch2_sample, ch3_sample, ch4_sample] = samples;

        #[cfg(feature = "debug")]
        if debugger.wants_apu_channels() {
            debugger.record_apu_channels(
                self.output_sample_rate,
                &[ch1_sample, ch2_sample, ch3_sample, ch4_sample],
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

        left *= ((self.nr50.volume_left & 0b111) + 1) as f32;
        left /= 64.0;
        right *= ((self.nr50.volume_right & 0b111) + 1) as f32;
        right /= 64.0;

        (left, right)
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.output_sample_rate = sample_rate;
        self.blip
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

#[cfg(test)]
mod tests {
    use crate::gb::{GameBoy, GbModel};
    use crate::rom::Rom;

    const TONE: [u8; 22] = [
        0x3E, 0xF0, 0xE0, 0x17, 0x3E, 0x80, 0xE0, 0x16, 0x3E, 0x00, 0xE0, 0x18, 0x3E, 0x87, 0xE0,
        0x19, 0x3E, 0xFF, 0xE0, 0x25, 0x18, 0xFE,
    ];

    fn tone_rom() -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        data[0x0150..0x0150 + TONE.len()].copy_from_slice(&TONE);
        Rom::new(&data)
    }

    #[test]
    fn undrained_audio_stays_bounded() {
        for sample_rate in [8_000, 44_100, 48_000, 192_000] {
            let mut gb = GameBoy::new_empty(GbModel::Dmg);
            gb.load_rom(&tone_rom()).expect("load");
            gb.apu.set_sample_rate(sample_rate);

            gb.run_cycles(5_000_000);

            let max_len = sample_rate as usize * 2;
            let len = gb.apu.audio_buffer.len();
            assert!(len > max_len / 2, "{sample_rate} Hz: {len} samples");
            assert!(len <= max_len, "{sample_rate} Hz: {len} samples");
            assert!(gb.apu.audio_buffer.iter().any(|s| s.abs() > 0.01));
        }
    }

    fn loud(gb: &mut GameBoy) -> bool {
        for _ in 0..5 {
            gb.run_frame();
        }
        gb.apu.audio_buffer.clear();
        for _ in 0..10 {
            gb.run_frame();
        }
        gb.apu.audio_buffer.iter().any(|s| s.abs() > 0.01)
    }

    #[test]
    fn a_damaged_master_volume_plays_at_its_register_bits() {
        let mut played = Vec::new();
        for volume in [7, 255] {
            let mut gb = GameBoy::new_empty(GbModel::Dmg);
            gb.load_rom(&tone_rom()).expect("load");
            for _ in 0..5 {
                gb.run_frame();
            }
            gb.apu.nr50.volume_left = volume;
            gb.apu.nr50.volume_right = volume;
            gb.apu.audio_buffer.clear();
            gb.run_frame();
            played.push(gb.apu.audio_buffer.clone());
        }
        assert!(played[0].iter().any(|s| s.abs() > 0.01));
        assert_eq!(played[0], played[1]);
    }

    #[test]
    fn a_muted_channel_stays_silent_across_a_reset_and_a_new_game() {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&tone_rom()).expect("load");
        assert!(loud(&mut gb));

        gb.apu.set_channel_enabled(1, false);
        assert!(!loud(&mut gb));
        gb.soft_reset();
        assert!(!loud(&mut gb));
        gb.load_rom(&tone_rom()).expect("load");
        assert!(!loud(&mut gb));

        gb.apu.set_channel_enabled(1, true);
        assert!(loud(&mut gb));
    }

    #[test]
    fn a_channel_mute_stays_out_of_the_saved_state() {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&tone_rom()).expect("load");
        gb.run_frame();
        let audible = gb.dump_full(0).unwrap();

        gb.apu.set_channel_enabled(1, false);

        assert!(gb.dump_full(0).unwrap() == audible);
    }

    fn writes_rom(writes: &[(u8, u8)]) -> Rom {
        let mut code = vec![];
        for &(reg, value) in writes {
            code.extend_from_slice(&[0x3E, value, 0xE0, reg]);
        }
        code.extend_from_slice(&[0x18, 0xFE]);
        let mut data = vec![0u8; 0x8000];
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        data[0x0150..0x0150 + code.len()].copy_from_slice(&code);
        Rom::new(&data)
    }

    fn four_channels_at_full_volume() -> Rom {
        let mut writes = vec![(0x26, 0x80), (0x24, 0x77), (0x25, 0xFF)];
        for i in 0..16 {
            writes.push((0x30 + i, if i < 8 { 0xFF } else { 0x00 }));
        }
        writes.extend_from_slice(&[
            (0x11, 0x80),
            (0x12, 0xF0),
            (0x13, 0x00),
            (0x16, 0x80),
            (0x17, 0xF0),
            (0x18, 0x00),
            (0x1A, 0x80),
            (0x1C, 0x20),
            (0x1D, 0x00),
            (0x21, 0xF0),
            (0x22, 0x00),
            (0x14, 0x87),
            (0x19, 0x87),
            (0x1E, 0x87),
            (0x23, 0x80),
        ]);
        writes_rom(&writes)
    }

    #[test]
    fn four_full_channels_peak_at_half_scale() {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&four_channels_at_full_volume()).expect("load");
        gb.apu.set_sample_rate(48_000);

        let mut peak = 0.0f32;
        for _ in 0..60 {
            gb.run_frame();
            for s in gb.apu.audio_buffer.drain(..) {
                peak = peak.max(s.abs());
            }
        }

        assert!(peak > 0.45, "peak {peak}");
        assert!(peak < 0.55, "peak {peak}");
    }

    #[cfg(feature = "debug")]
    fn wave_fingerprint(gb: &GameBoy) -> Vec<(usize, u64)> {
        gb.debugger
            .waves()
            .iter()
            .map(|ring| {
                let hash = ring.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, sample| {
                    (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x0100_0000_01b3)
                });
                (ring.len(), hash)
            })
            .collect()
    }

    #[cfg(feature = "debug")]
    #[test]
    fn the_wave_rings_keep_their_samples_for_a_fixed_run() {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&four_channels_at_full_volume()).expect("load");
        gb.apu.set_sample_rate(48_000);
        gb.debugger.record_channels = true;

        for _ in 0..20 {
            gb.run_frame();
            gb.apu.audio_buffer.clear();
        }
        gb.apu.set_channel_enabled(1, false);
        for _ in 0..5 {
            gb.run_frame();
            gb.apu.audio_buffer.clear();
        }

        assert_eq!(
            wave_fingerprint(&gb),
            vec![
                (2048, 15466843843163243301),
                (2048, 2937223237197546277),
                (2048, 1756449107157631781),
                (2048, 10978820079561392933),
            ]
        );
    }

    #[test]
    fn the_game_boy_names_its_four_channels() {
        let gb = GameBoy::new_empty(GbModel::Dmg);
        assert_eq!(gb.audio_channels(), ["Pulse 1", "Pulse 2", "Wave", "Noise"]);
    }
}
