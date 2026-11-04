use super::wave::Wave;
use crate::gb::apu::WriteContext;
use crate::gb::apu::channels::channel_1::Channel1;
use crate::gb::apu::channels::channel_2::Channel2;
use crate::gb::apu::channels::channel_4::Channel4;
use crate::gb::apu::registers::master_volume_vin::MasterVolumeVin;
use crate::gb::apu::registers::sound_panning::SoundPanning;

const SEQUENCER_TICKS: u16 = 8192;
const SEQUENCER_RISE: u16 = SEQUENCER_TICKS / 2;

const SOUND1CNT_L: usize = 0x00;
const SOUND1CNT_H: usize = 0x02;
const SOUND1CNT_X: usize = 0x04;
const SOUND2CNT_L: usize = 0x08;
const SOUND2CNT_H: usize = 0x0C;
const SOUND3CNT_L: usize = 0x10;
const SOUND3CNT_H: usize = 0x12;
const SOUND3CNT_X: usize = 0x14;
const SOUND4CNT_L: usize = 0x18;
const SOUND4CNT_H: usize = 0x1C;
const SOUNDCNT_L: usize = 0x20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Levels {
    pub left: i32,
    pub right: i32,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Psg {
    pub ch1: Channel1,
    pub ch2: Channel2,
    pub ch3: Wave,
    pub ch4: Channel4,
    pub master_volume: MasterVolumeVin,
    pub panning: SoundPanning,
    div_apu: u8,
    sequencer: u16,
    phase_ticks: u8,
}

impl Psg {
    pub fn enabled_flags(&self) -> u8 {
        u8::from(self.ch1.enabled)
            | u8::from(self.ch2.enabled) << 1
            | u8::from(self.ch3.enabled) << 2
            | u8::from(self.ch4.enabled) << 3
    }

    pub fn power_on(&mut self) {
        self.div_apu = 0;
        self.phase_ticks = 0;
    }

    pub fn power_off(&mut self) {
        let ram = self.ch3.ram;
        let sequencer = self.sequencer;
        *self = Self::default();
        self.ch3.ram = ram;
        self.sequencer = sequencer;
    }

    pub fn write(&mut self, offset: usize, value: u8) {
        let ctx = self.write_context();
        match offset {
            SOUND1CNT_L => self.ch1.write(0xFF10, value, ctx),
            SOUND1CNT_H => self.ch1.write(0xFF11, value, ctx),
            0x03 => self.ch1.write(0xFF12, value, ctx),
            SOUND1CNT_X => self.ch1.write(0xFF13, value, ctx),
            0x05 => self.ch1.write(0xFF14, value, ctx),
            SOUND2CNT_L => self.ch2.write(0xFF16, value, ctx),
            0x09 => self.ch2.write(0xFF17, value, ctx),
            SOUND2CNT_H => self.ch2.write(0xFF18, value, ctx),
            0x0D => self.ch2.write(0xFF19, value, ctx),
            SOUND3CNT_L => self.ch3.write_select(value),
            SOUND3CNT_H => self.ch3.write_length(value),
            0x13 => self.ch3.write_volume(value),
            SOUND3CNT_X => self.ch3.write_period_low(value),
            0x15 => self.ch3.write_control(value, ctx),
            SOUND4CNT_L => self.ch4.write(0xFF20, value, ctx),
            0x19 => self.ch4.write(0xFF21, value, ctx),
            SOUND4CNT_H => self.ch4.write(0xFF22, value, ctx),
            0x1D => self.ch4.write(0xFF23, value, ctx),
            SOUNDCNT_L => self.master_volume = value.into(),
            0x21 => self.panning = value.into(),
            _ => {}
        }
    }

    fn write_context(&self) -> WriteContext {
        WriteContext {
            frame_clocked_length: self.div_apu & 1 == 1,
            lf_div: 1 ^ u16::from((self.phase_ticks >> 1) & 1),
            cgb: true,
            double_speed: false,
        }
    }

    pub fn run(&mut self, mut ticks: u32) {
        while ticks > 0 {
            let edge = if self.sequencer < SEQUENCER_RISE {
                SEQUENCER_RISE
            } else {
                SEQUENCER_TICKS
            };
            let chunk = ticks.min(u32::from(edge - self.sequencer)) as u16;
            self.run_channels(chunk);
            ticks -= u32::from(chunk);
            self.sequencer += chunk;
            if self.sequencer == SEQUENCER_RISE {
                self.ch1.envelope_secondary_div_event();
                self.ch2.envelope_secondary_div_event();
                self.ch4.envelope_secondary_div_event();
            } else if self.sequencer == SEQUENCER_TICKS {
                self.sequencer = 0;
                self.sequencer_step();
            }
        }
    }

    fn run_channels(&mut self, ticks: u16) {
        if !self.ch1.sweep_busy() && self.quiet(ticks) {
            self.ch1.skip(ticks);
            self.ch2.skip(ticks);
            self.ch3.skip(ticks);
            self.ch4.skip(ticks);
            self.phase_ticks = self.phase_ticks.wrapping_add(ticks as u8);
            return;
        }
        for _ in 0..ticks {
            self.phase_ticks = self.phase_ticks.wrapping_add(1);
            if self.phase_ticks.is_multiple_of(2) && self.ch1.sweep_busy() {
                self.ch1.sweep_tick(self.write_context().lf_div);
            }
            self.ch1.tick();
            self.ch2.tick();
            self.ch3.tick();
            self.ch4.tick();
        }
    }

    fn quiet(&self, ticks: u16) -> bool {
        self.ch1.quiet(ticks)
            && self.ch2.quiet(ticks)
            && self.ch3.quiet(ticks)
            && self.ch4.quiet(ticks)
    }

    fn sequencer_step(&mut self) {
        self.div_apu = (self.div_apu + 1) & 0b111;
        if self.div_apu & 1 == 1 {
            self.ch1.clock_length();
            self.ch2.clock_length();
            self.ch3.clock_length();
            self.ch4.clock_length();
        }
        if self.div_apu & 3 == 3 {
            let lf_div = self.write_context().lf_div;
            self.ch1.clock_sweep(lf_div);
        }
        self.ch1.envelope_div_event(self.div_apu);
        self.ch2.envelope_div_event(self.div_apu);
        self.ch4.envelope_div_event(self.div_apu);
        self.ch1.envelope_tick();
        self.ch2.envelope_tick();
        self.ch4.envelope_tick();
    }

    pub fn channel_levels(&self) -> [i32; 4] {
        [
            square_level(
                self.ch1.dac_enabled(),
                self.ch1.sample(),
                self.ch1.volume_envelope.current_volume,
                self.ch1.enabled,
            ),
            square_level(
                self.ch2.dac_enabled(),
                self.ch2.sample(),
                self.ch2.volume_envelope.current_volume,
                self.ch2.enabled,
            ),
            self.ch3.level(),
            square_level(
                self.ch4.dac_enabled(),
                self.ch4.sample(),
                self.ch4.volume_envelope.current_volume,
                self.ch4.enabled,
            ),
        ]
    }

    pub fn levels(&self, muted: &[bool]) -> Levels {
        let channels = self.channel_levels();
        let routes = [
            (self.panning.channel_1_left, self.panning.channel_1_right),
            (self.panning.channel_2_left, self.panning.channel_2_right),
            (self.panning.channel_3_left, self.panning.channel_3_right),
            (self.panning.channel_4_left, self.panning.channel_4_right),
        ];
        let mut levels = Levels::default();
        for ((level, (left, right)), muted) in channels.into_iter().zip(routes).zip(muted) {
            if *muted {
                continue;
            }
            if left {
                levels.left += level;
            }
            if right {
                levels.right += level;
            }
        }
        levels.left *= i32::from(self.master_volume.volume_left) + 1;
        levels.right *= i32::from(self.master_volume.volume_right) + 1;
        levels
    }
}

fn square_level(dac_enabled: bool, sample: u8, volume: u8, enabled: bool) -> i32 {
    if !enabled || !dac_enabled {
        return 0;
    }
    if sample > 0 {
        i32::from(volume)
    } else {
        -i32::from(volume)
    }
}
