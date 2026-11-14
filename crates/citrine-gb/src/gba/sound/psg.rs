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
        if !self.ch1.sweep_busy() && self.ch1.quiet(ticks) {
            self.ch1.skip(ticks);
            self.phase_ticks = self.phase_ticks.wrapping_add(ticks as u8);
        } else {
            for _ in 0..ticks {
                self.phase_ticks = self.phase_ticks.wrapping_add(1);
                if self.phase_ticks.is_multiple_of(2) && self.ch1.sweep_busy() {
                    self.ch1.sweep_tick(self.write_context().lf_div);
                }
                self.ch1.tick();
            }
        }
        if self.ch2.quiet(ticks) {
            self.ch2.skip(ticks);
        } else {
            for _ in 0..ticks {
                self.ch2.tick();
            }
        }
        if self.ch3.quiet(ticks) {
            self.ch3.skip(ticks);
        } else {
            for _ in 0..ticks {
                self.ch3.tick();
            }
        }
        run_channel(
            &mut self.ch4,
            ticks,
            Channel4::unheard_quiet_ticks,
            Channel4::advance_unheard,
            Channel4::tick,
        );
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

    pub fn levels(&self, muted: [bool; 4]) -> Levels {
        let [ch1, ch2, ch3, ch4] = self.channel_levels();
        let heard = |level: i32, muted: bool| if muted { 0 } else { level };
        let [ch1, ch2, ch3, ch4] = [
            heard(ch1, muted[0]),
            heard(ch2, muted[1]),
            heard(ch3, muted[2]),
            heard(ch4, muted[3]),
        ];
        let routed = |level: i32, on: bool| if on { level } else { 0 };
        let pan = &self.panning;
        let left = routed(ch1, pan.channel_1_left)
            + routed(ch2, pan.channel_2_left)
            + routed(ch3, pan.channel_3_left)
            + routed(ch4, pan.channel_4_left);
        let right = routed(ch1, pan.channel_1_right)
            + routed(ch2, pan.channel_2_right)
            + routed(ch3, pan.channel_3_right)
            + routed(ch4, pan.channel_4_right);
        Levels {
            left: left * (i32::from(self.master_volume.volume_left) + 1),
            right: right * (i32::from(self.master_volume.volume_right) + 1),
        }
    }
}

#[inline(always)]
fn run_channel<C>(
    channel: &mut C,
    ticks: u16,
    quiet_ticks: fn(&C) -> u32,
    advance: fn(&mut C, u32),
    tick: fn(&mut C) -> bool,
) {
    let mut left = u32::from(ticks);
    while left > 0 {
        let quiet = quiet_ticks(channel).min(left);
        if quiet > 0 {
            advance(channel, quiet);
            left -= quiet;
        } else {
            tick(channel);
            left -= 1;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTERS: [usize; 20] = [
        0x00, 0x02, 0x03, 0x04, 0x05, 0x08, 0x09, 0x0C, 0x0D, 0x10, 0x12, 0x13, 0x14, 0x15, 0x18,
        0x19, 0x1C, 0x1D, 0x20, 0x21,
    ];
    const ENVELOPES: [usize; 3] = [0x03, 0x09, 0x19];
    const TRIGGERS: [usize; 4] = [0x05, 0x0D, 0x15, 0x1D];

    fn random() -> impl Iterator<Item = u64> {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        std::iter::repeat_with(move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        })
    }

    fn random_psg(random: &mut impl Iterator<Item = u64>) -> Psg {
        let mut psg = Psg::default();
        psg.power_on();
        for offset in REGISTERS {
            let r = random.next().unwrap();
            let mut value = r as u8;
            if ENVELOPES.contains(&offset) {
                value |= 0x08;
            }
            if TRIGGERS.contains(&offset) && r >> 8 & 3 != 0 {
                value |= 0x80;
            }
            psg.write(offset, value);
        }
        psg.run((random.next().unwrap() % 20_000) as u32);
        psg
    }

    fn ticked(mut psg: Psg, ticks: u16) -> Psg {
        for _ in 0..ticks {
            psg.phase_ticks = psg.phase_ticks.wrapping_add(1);
            if psg.phase_ticks.is_multiple_of(2) && psg.ch1.sweep_busy() {
                psg.ch1.sweep_tick(psg.write_context().lf_div);
            }
            psg.ch1.tick();
            psg.ch2.tick();
            psg.ch3.tick();
            psg.ch4.tick();
        }
        psg
    }

    #[test]
    fn running_channels_over_quiet_spans_matches_ticking_them_one_by_one() {
        let mut random = random();
        for _ in 0..2000 {
            let start = random_psg(&mut random);
            let ticks = (random.next().unwrap() % 4096) as u16 + 1;
            let mut ran = start.clone();
            ran.run_channels(ticks);
            assert_eq!(
                format!("{ran:?}"),
                format!("{:?}", ticked(start.clone(), ticks)),
                "{start:?} {ticks}"
            );
        }
    }
}
