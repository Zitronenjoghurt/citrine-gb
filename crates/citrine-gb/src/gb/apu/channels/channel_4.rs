use crate::ReadMemory;
use crate::gb::apu::Step;
use crate::gb::apu::WriteContext;
use crate::gb::apu::components::length_counter::LengthCounter;
use crate::gb::apu::components::volume_envelope::VolumeEnvelope;
use crate::gb::apu::registers::ch4_frequency::Channel4Frequency;
use crate::gb::apu::registers::ch124_volume::Channel124Volume;

const DMG_DELAYED_START: u8 = 6;

#[derive(Debug, Clone, Copy)]
struct LfsrJump {
    columns: [u16; 16],
    constant: u16,
}

impl LfsrJump {
    const fn one_step(high_bit_mask: u16) -> Self {
        let mut columns = [0u16; 16];
        let mut bit = 1;
        while bit < 16 {
            columns[bit] = (1u16 << (bit - 1)) & !high_bit_mask;
            bit += 1;
        }
        columns[0] |= high_bit_mask;
        columns[1] |= high_bit_mask;
        Self {
            columns,
            constant: high_bit_mask,
        }
    }

    const fn apply(&self, lfsr: u16) -> u16 {
        let mut out = self.constant;
        let mut bits = lfsr;
        while bits != 0 {
            out ^= self.columns[bits.trailing_zeros() as usize];
            bits &= bits - 1;
        }
        out
    }

    const fn twice(&self) -> Self {
        let mut columns = [0u16; 16];
        let mut bit = 0;
        while bit < 16 {
            columns[bit] = self.apply(self.columns[bit]) ^ self.constant;
            bit += 1;
        }
        Self {
            columns,
            constant: self.apply(self.constant),
        }
    }

    const fn powers(high_bit_mask: u16) -> [Self; 32] {
        let mut powers = [Self::one_step(high_bit_mask); 32];
        let mut power = 1;
        while power < 32 {
            powers[power] = powers[power - 1].twice();
            power += 1;
        }
        powers
    }
}

#[derive(Debug, Clone, Copy)]
struct LfsrTable {
    low: [u16; 256],
    high: [u16; 128],
}

impl LfsrTable {
    const EMPTY: Self = Self {
        low: [0; 256],
        high: [0; 128],
    };

    const fn new(jump: &LfsrJump) -> Self {
        let mut table = Self {
            low: [jump.constant; 256],
            high: [0; 128],
        };
        let mut index: usize = 1;
        while index < 256 {
            let bit = index.trailing_zeros() as usize;
            table.low[index] = table.low[index & (index - 1)] ^ jump.columns[bit];
            if index < 128 {
                table.high[index] = table.high[index & (index - 1)] ^ jump.columns[bit + 8];
            }
            index += 1;
        }
        table
    }

    const fn all(jumps: &[LfsrJump; 32]) -> [Self; 32] {
        let mut tables = [Self::EMPTY; 32];
        let mut power = 0;
        while power < 32 {
            tables[power] = Self::new(&jumps[power]);
            power += 1;
        }
        tables
    }

    #[inline(always)]
    fn apply(&self, lfsr: u16) -> u16 {
        self.low[usize::from(lfsr & 0xFF)] ^ self.high[usize::from(lfsr >> 8) & 0x7F]
    }
}

static LFSR_TABLES: [[LfsrTable; 32]; 2] = [
    LfsrTable::all(&LfsrJump::powers(0x4000)),
    LfsrTable::all(&LfsrJump::powers(0x4040)),
];

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Channel4 {
    pub enabled: bool,
    pub lfsr: u16,
    pub output: bool,
    pub counter: u16,
    pub countdown: u16,
    pub countdown_reloaded: bool,
    pub did_step_counter: bool,
    pub alignment: u8,
    pub half_tick: bool,
    pub counter_active: bool,
    pub background_counter_active: bool,
    pub started_with_dac_disabled: bool,
    pub dmg_delayed_start: u8,
    pub length_counter: LengthCounter,
    pub volume_envelope: VolumeEnvelope,
    /// NR41 (0xFF20) => Write only (6 lower bits)
    pub initial_length_timer: u8,
    /// NR42 (0xFF21)
    pub volume: Channel124Volume,
    /// NR43 (0xFF22)
    pub frequency: Channel4Frequency,
    /// NR44 (0xFF23) => bit 7
    pub trigger: bool,
}

impl Channel4 {
    pub fn sample(&self) -> u8 {
        if !self.enabled {
            return 0;
        };

        if self.output {
            self.volume_envelope.current_volume
        } else {
            0
        }
    }

    fn nr43(&self) -> u8 {
        self.frequency.into()
    }

    fn divisor(&self) -> u16 {
        match (self.nr43() & 0b111) as u16 {
            0 => 2,
            divider => divider << 2,
        }
    }

    fn counter_bit(&self, counter: u16, nr43: u8) -> bool {
        (counter >> (nr43 >> 4)) & 1 == 1
    }

    fn steps(&self, ticks: u16) -> u16 {
        ((u32::from(ticks) + u32::from(!self.half_tick)) / 2) as u16
    }

    pub fn quiet(&self, ticks: u16) -> bool {
        self.dmg_delayed_start == 0
            && (!(self.counter_active || self.background_counter_active)
                || self.countdown > self.steps(ticks))
    }

    pub fn skip(&mut self, ticks: u16) {
        let steps = self.steps(ticks);
        self.half_tick ^= ticks & 1 == 1;
        if steps == 0 {
            return;
        }
        self.alignment = self.alignment.wrapping_add(steps as u8);
        if self.counter_active || self.background_counter_active {
            self.countdown -= steps;
            self.countdown_reloaded = false;
        }
    }

    pub fn quiet_ticks(&self) -> u32 {
        self.ticks_before_lfsr_step(1)
    }

    pub fn steady_ticks(&self) -> u32 {
        let known: u32 = if self.frequency.lfsr_width { 6 } else { 14 };
        let ahead = (self.lfsr >> 1) ^ if self.output { 0xFFFF } else { 0 };
        match ahead & ((1u16 << known) - 1) {
            0 => self.steady_ticks_past_known_bits(),
            differs => self.ticks_before_lfsr_step(differs.trailing_zeros() + 1),
        }
    }

    #[inline(never)]
    fn steady_ticks_past_known_bits(&self) -> u32 {
        let mut probe = Self {
            lfsr: self.lfsr,
            frequency: self.frequency,
            ..Self::default()
        };
        for step in 1..=16 {
            probe.step_lfsr();
            if probe.output != self.output {
                return self.ticks_before_lfsr_step(step);
            }
        }
        self.unheard_quiet_ticks()
    }

    fn ticks_before_lfsr_step(&self, lfsr_step: u32) -> u32 {
        if self.dmg_delayed_start > 0 {
            return 0;
        }
        if !self.enabled || !(self.counter_active || self.background_counter_active) {
            return u32::MAX;
        }
        let shift = u32::from(self.nr43() >> 4);
        if shift >= 14 {
            return u32::MAX;
        }
        let period = 2u32 << shift;
        let counter = u32::from(self.counter) & (period - 1);
        let increments = match ((1u32 << shift) + period - counter) & (period - 1) {
            0 => period,
            increments => increments,
        } + (lfsr_step - 1) * period;
        let divisor = u32::from(self.divisor());
        let first = if self.countdown == 0 {
            divisor
        } else {
            u32::from(self.countdown)
        };
        let step = first + (increments - 1) * divisor;
        2 * step - u32::from(!self.half_tick) - 1
    }

    pub fn unheard_quiet_ticks(&self) -> u32 {
        if self.dmg_delayed_start > 0 {
            0
        } else {
            u32::MAX
        }
    }

    pub fn advance_unheard(&mut self, ticks: u32) {
        let before = u32::from(self.counter);
        let increments = self.advance_counter(ticks);
        let shift = u32::from(self.nr43() >> 4);
        if !self.enabled || increments == 0 || shift >= 14 {
            return;
        }
        let half = 1u32 << shift;
        let rises = (before + increments + half) >> (shift + 1);
        self.step_lfsr_times(rises - ((before + half) >> (shift + 1)));
    }

    fn advance_counter(&mut self, ticks: u32) -> u32 {
        let steps = (ticks + u32::from(!self.half_tick)) / 2;
        self.half_tick ^= ticks & 1 == 1;
        if steps == 0 {
            return 0;
        }
        self.alignment = self.alignment.wrapping_add(steps as u8);
        if !self.counter_active && !self.background_counter_active {
            return 0;
        }
        let divisor = u32::from(self.divisor());
        let first = if self.countdown == 0 {
            divisor
        } else {
            u32::from(self.countdown)
        };
        if steps < first {
            self.countdown = (first - steps) as u16;
            self.countdown_reloaded = false;
            return 0;
        }
        let rest = steps - first;
        let increments = 1 + rest / divisor;
        self.counter = ((u32::from(self.counter) + increments) & 0x3FFF) as u16;
        self.countdown = (divisor - rest % divisor) as u16;
        self.countdown_reloaded = rest.is_multiple_of(divisor);
        self.did_step_counter = true;
        increments
    }

    pub fn tick(&mut self) -> bool {
        self.step() != Step::Idle
    }

    pub fn step(&mut self) -> Step {
        self.half_tick = !self.half_tick;
        if !self.half_tick {
            return Step::Idle;
        }

        self.alignment = self.alignment.wrapping_add(1);

        if self.dmg_delayed_start > 0 {
            self.dmg_delayed_start -= 1;
            if self.dmg_delayed_start == 0 {
                self.trigger_noise(false, false);
                return Step::Changed;
            }
        }

        if !self.counter_active && !self.background_counter_active {
            return Step::Idle;
        }

        let divisor = self.divisor();
        if self.countdown == 0 {
            self.countdown = divisor;
        }

        if self.countdown > 1 {
            self.countdown -= 1;
            self.countdown_reloaded = false;
            return Step::Idle;
        }

        self.countdown = divisor;
        self.countdown_reloaded = true;
        let nr43 = self.nr43();
        let old_bit = self.counter_bit(self.counter, nr43);
        self.counter = (self.counter + 1) & 0x3FFF;
        self.did_step_counter = true;
        if !old_bit && self.counter_bit(self.counter, nr43) && self.enabled {
            let before = self.output;
            self.step_lfsr();
            return if self.output == before {
                Step::Same
            } else {
                Step::Changed
            };
        }
        Step::Idle
    }

    fn step_lfsr(&mut self) {
        let high_bit_mask = if self.frequency.lfsr_width {
            0x4040
        } else {
            0x4000
        };
        let new_high_bit = (self.lfsr ^ (self.lfsr >> 1) ^ 1) & 1 == 1;
        self.lfsr >>= 1;
        if new_high_bit {
            self.lfsr |= high_bit_mask;
        } else {
            self.lfsr &= !high_bit_mask;
        }
        self.output = self.lfsr & 1 == 1;
    }

    fn step_lfsr_times(&mut self, steps: u32) {
        if steps < 16 {
            self.step_lfsr_by(steps);
            return;
        }
        self.jump_lfsr(steps);
    }

    #[inline(never)]
    fn jump_lfsr(&mut self, steps: u32) {
        let jumps = &LFSR_TABLES[usize::from(self.frequency.lfsr_width)];
        let mut lfsr = self.lfsr;
        let mut rest = steps;
        while rest != 0 {
            lfsr = jumps[rest.trailing_zeros() as usize].apply(lfsr);
            rest &= rest - 1;
        }
        self.lfsr = lfsr;
        self.output = lfsr & 1 == 1;
    }

    fn step_lfsr_by(&mut self, mut steps: u32) {
        if steps == 0 {
            return;
        }
        let narrow = self.frequency.lfsr_width;
        let most = if narrow { 6 } else { 14 };
        while steps > 0 {
            let count = steps.min(most);
            let mask = (1u16 << count) - 1;
            let fed = !(self.lfsr ^ self.lfsr >> 1) & mask;
            let mut lfsr = (self.lfsr & 0x7FFF) >> count | fed << (15 - count);
            if narrow {
                lfsr = lfsr & !(mask << (7 - count)) | fed << (7 - count);
            }
            self.lfsr = lfsr;
            steps -= count;
        }
        self.output = self.lfsr & 1 == 1;
    }

    fn trigger_noise(&mut self, cgb: bool, double_speed: bool) {
        self.volume_envelope.trigger(self.volume.into());
        self.lfsr = 0;
        self.prepare_start(cgb, double_speed);
        self.output = false;
        self.did_step_counter = self.alignment & 3 == 2;
        if self.dac_enabled() {
            self.enabled = true;
        }
    }

    fn prepare_start(&mut self, cgb: bool, double_speed: bool) {
        self.counter_active = self.dac_enabled();
        let was_started_with_dac_disabled = self.started_with_dac_disabled;
        self.started_with_dac_disabled = !self.counter_active;
        let mut divisor = (self.nr43() & 0b111) as u16;
        let was_background_counting = self.background_counter_active;
        self.background_counter_active = true;
        let mut instant_step = false;
        let mut div_1_glitch = false;
        let aligned = self.alignment & 3 == 0;

        let early_step =
            self.countdown == 1 || (self.countdown == 2 && self.enabled && !cgb && double_speed);
        if divisor > 1 && early_step {
            self.counter = (self.counter + 1) & 0x3FFF;
        } else if self.countdown == 2 && aligned && self.enabled {
            if divisor == 0 {
                divisor = 8;
            } else if divisor == 1 {
                if !self.did_step_counter {
                    div_1_glitch = true;
                }
                let nr43 = self.nr43();
                let old_bit = self.counter_bit(self.counter, nr43);
                self.counter = (self.counter + 1) & 0x3FFF;
                instant_step = !old_bit && self.counter_bit(self.counter, nr43);
            }
        }

        let mut countdown = if divisor == 0 { 6 } else { divisor * 4 + 6 };
        if self.alignment & 1 == 1 {
            if divisor == 0 {
                if !cgb || !was_background_counting {
                    countdown += 1;
                } else {
                    countdown -= 1;
                }
            } else if self.alignment & 2 != 0 {
                if divisor == 1 && !self.enabled {
                    countdown += 1;
                } else {
                    countdown -= 3;
                }
            } else {
                countdown -= 1;
                if divisor == 1 && self.enabled {
                    countdown -= 4;
                }
            }
        } else if divisor != 0 {
            if self.alignment & 2 != 0 {
                if double_speed && !cgb && divisor == 1 {
                    countdown += 2;
                } else {
                    countdown -= 2;
                }
            } else if (divisor > 1 && (!double_speed || cgb))
                || (divisor == 1 && self.enabled && self.nr43() & 0xF0 == 0)
            {
                countdown -= 4;
            }
        } else if double_speed && !cgb {
            countdown += 2;
        }

        if divisor > 1 {
            if !self.counter_active && aligned {
                countdown += 4;
            }
        } else if was_background_counting && !self.enabled && aligned {
            if divisor == 0 {
                if was_started_with_dac_disabled {
                    countdown += 28;
                }
            } else {
                countdown -= 4;
            }
        }

        if divisor == 0 && !cgb && was_background_counting && !self.enabled && double_speed {
            countdown -= 1;
        }
        if div_1_glitch {
            countdown -= 4;
        }
        self.countdown = countdown;

        self.lfsr = if divisor == 0 && self.enabled && self.alignment & 3 == 3 {
            0x0055
        } else {
            0
        };
        if instant_step {
            self.step_lfsr();
        }
    }

    pub fn clock_length(&mut self) {
        if self.length_counter.clock() {
            self.enabled = false;
        }
    }

    pub fn envelope_div_event(&mut self, div_apu: u8) {
        self.volume_envelope.div_event(div_apu);
    }

    pub fn envelope_tick(&mut self) {
        self.volume_envelope.tick(self.volume.into());
    }

    pub fn envelope_secondary_div_event(&mut self) {
        if self.enabled {
            self.volume_envelope.secondary_div_event(self.volume.into());
        }
    }

    pub fn dac_enabled(&self) -> bool {
        self.volume.initial_volume > 0 || self.volume.envelope_direction
    }

    pub fn write_length(&mut self, value: u8) {
        self.initial_length_timer = value & 0x3F;
        self.length_counter
            .load(64, self.initial_length_timer as u16);
    }

    fn write_nr43(&mut self, value: u8, cgb: bool) {
        if self.countdown_reloaded {
            let divisor = match (value & 0b111) as u16 {
                0 => 2,
                divider => divider << 2,
            };
            let offsets: [u16; 4] = if cgb { [2, 1, 0, 3] } else { [2, 1, 4, 3] };
            self.countdown = if divisor == 2 {
                divisor
            } else {
                divisor + offsets[(self.alignment & 3) as usize]
            };
        }

        if !cgb && self.countdown_reloaded {
            let old = self.nr43();
            let counter = self.counter;
            if !self.counter_bit(counter, old)
                && self.counter_bit(counter, value)
                && (counter >> 7) & 1 == 1
            {
                let previous = (counter.wrapping_sub(1)) & 0x3FFF;
                if self.counter_bit(previous, old)
                    && !self.counter_bit(previous, value)
                    && (previous >> 7) & 1 == 1
                {
                    self.step_lfsr();
                }
            }
            self.apply_nr43(0xFF, cgb);
        }
        self.apply_nr43(value, cgb);
    }

    fn apply_nr43(&mut self, new: u8, cgb: bool) {
        let old = self.nr43();
        self.frequency = new.into();
        if (old ^ new) & 0xF0 == 0 {
            return;
        }

        let mut counter = self.counter;
        if !cgb && self.countdown_reloaded {
            counter |= counter.wrapping_sub(1) & 0x3FFF;
        }
        let old_bit = self.counter_bit(counter, old);
        let glitch_bit = self.counter_bit(counter, (old & 0x7F) | (new & 0x80));
        let new_bit = self.counter_bit(counter, new);

        if old_bit == new_bit && new_bit != glitch_bit {
            if !new_bit || !cgb || new & 0x80 == 0 {
                self.step_lfsr();
            }
        } else if !old_bit && new_bit {
            self.step_lfsr();
        }
    }

    pub fn write(&mut self, addr: u16, value: u8, ctx: WriteContext) {
        match addr {
            0xFF20 => self.write_length(value),
            0xFF21 => {
                let old_value: u8 = self.volume.into();
                self.volume = value.into();
                if !self.dac_enabled() {
                    if self.enabled && self.nr43() & 0b111 != 0 {
                        if self.countdown <= 2 {
                            self.counter = (self.counter + 1) & 0x3FFF;
                        }
                        self.background_counter_active = false;
                    }
                    self.counter_active = false;
                    self.enabled = false;
                } else if self.enabled {
                    self.volume_envelope.write_nrx2(value, old_value, ctx.cgb);
                }
            }
            0xFF22 => self.write_nr43(value, ctx.cgb),
            0xFF23 => {
                self.trigger = (value & 0b1000_0000) != 0;
                let expired = self.length_counter.write_control(
                    (value & 0b0100_0000) != 0,
                    self.trigger,
                    64,
                    ctx.frame_clocked_length,
                );
                if expired {
                    self.enabled = false;
                }
                if self.trigger {
                    if !ctx.cgb && self.alignment & 3 != 0 {
                        self.volume_envelope.trigger(self.volume.into());
                        self.dmg_delayed_start = DMG_DELAYED_START;
                    } else {
                        self.trigger_noise(ctx.cgb, ctx.double_speed);
                    }
                }
            }
            _ => {}
        }
    }
}

impl ReadMemory for Channel4 {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF21 => self.volume.into(),
            0xFF22 => self.frequency.into(),
            0xFF23 => 0xBF | ((self.length_counter.enabled as u8) << 6),
            _ => 0xFF,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipping_quiet_ticks_matches_ticking_them_one_by_one() {
        for (half_tick, countdown_reloaded) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            for ticks in 1..=9 {
                let start = Channel4 {
                    counter_active: true,
                    countdown: 50,
                    countdown_reloaded,
                    half_tick,
                    ..Channel4::default()
                };
                assert!(start.quiet(ticks));
                let mut skipped = start.clone();
                skipped.skip(ticks);
                let mut ticked = start;
                for _ in 0..ticks {
                    ticked.step();
                }
                assert_eq!(
                    format!("{skipped:?}"),
                    format!("{ticked:?}"),
                    "{half_tick} {countdown_reloaded} {ticks}"
                );
            }
        }
    }

    #[test]
    fn stepping_the_lfsr_in_bulk_matches_stepping_it_one_by_one() {
        for narrow in [false, true] {
            for lfsr in 0..=u16::MAX {
                let mut one = Channel4 {
                    lfsr,
                    ..Channel4::default()
                };
                one.frequency.lfsr_width = narrow;
                let start = one.clone();
                for steps in 1..=29 {
                    one.step_lfsr();
                    let mut bulk = start.clone();
                    bulk.step_lfsr_by(steps);
                    assert_eq!(
                        (bulk.lfsr, bulk.output),
                        (one.lfsr, one.output),
                        "{lfsr:#x} {narrow} {steps}"
                    );
                }
            }
        }
    }

    fn random_channels() -> impl Iterator<Item = Channel4> {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        std::iter::repeat_with(move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        })
        .map(|r| Channel4 {
            enabled: r & 1 == 1,
            lfsr: (r >> 1) as u16 & 0x7FFF,
            counter: (r >> 16) as u16 & 0x3FFF,
            countdown: ((r >> 30) % 40) as u16,
            half_tick: (r >> 37) & 1 == 1,
            counter_active: (r >> 38) & 1 == 1,
            background_counter_active: (r >> 39) & 1 == 1,
            alignment: (r >> 40) as u8,
            frequency: ((r >> 48) as u8).into(),
            ..Channel4::default()
        })
    }

    #[test]
    fn advancing_over_quiet_ticks_matches_ticking_them_one_by_one() {
        for start in random_channels().take(4000) {
            let quiet = start.quiet_ticks().min(3000);
            let mut ticked = start.clone();
            for _ in 0..quiet {
                assert_eq!(ticked.step(), Step::Idle, "{start:?}");
            }
            let mut advanced = start.clone();
            advanced.advance_unheard(quiet);
            assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"), "{start:?}");
            if start.quiet_ticks() < 3000 {
                assert_ne!(ticked.step(), Step::Idle, "{start:?}");
            }
        }
    }

    #[test]
    fn advancing_over_steady_ticks_matches_ticking_them_one_by_one() {
        for start in random_channels().take(4000) {
            let steady = start.steady_ticks().min(5000);
            let mut ticked = start.clone();
            for _ in 0..steady {
                assert_ne!(ticked.step(), Step::Changed, "{start:?}");
            }
            let mut advanced = start.clone();
            advanced.advance_unheard(steady);
            assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"), "{start:?}");
            if start.steady_ticks() < 5000 {
                assert_ne!(ticked.step(), Step::Idle, "{start:?}");
            }
        }
    }

    #[test]
    fn advancing_one_tick_past_the_quiet_ticks_matches_ticking_them() {
        for start in random_channels().take(4000) {
            let quiet = start.quiet_ticks();
            if quiet >= 3000 {
                continue;
            }
            let mut ticked = start.clone();
            for _ in 0..=quiet {
                ticked.step();
            }
            let mut advanced = start.clone();
            advanced.advance_unheard(quiet + 1);
            assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"), "{start:?}");
        }
    }

    #[test]
    fn advancing_one_tick_past_the_steady_ticks_matches_ticking_them() {
        for start in random_channels().take(4000) {
            let steady = start.steady_ticks();
            if steady >= 5000 {
                continue;
            }
            let mut ticked = start.clone();
            for _ in 0..=steady {
                ticked.step();
            }
            let mut advanced = start.clone();
            advanced.advance_unheard(steady + 1);
            assert_eq!(format!("{advanced:?}"), format!("{ticked:?}"), "{start:?}");
        }
    }

    #[test]
    fn the_step_after_the_steady_ticks_changes_the_output() {
        for narrow in [false, true] {
            let locked = if narrow { 0x7F } else { 0x7FFF };
            for lfsr in 0..0x8000u16 {
                let mut start = Channel4 {
                    enabled: true,
                    counter_active: true,
                    lfsr,
                    output: lfsr & 1 == 1,
                    ..Channel4::default()
                };
                start.frequency.lfsr_width = narrow;
                let steady = start.steady_ticks();
                if steady == u32::MAX {
                    assert_eq!(lfsr & locked, locked, "{lfsr:#x} {narrow}");
                    continue;
                }
                let mut ticked = start.clone();
                for _ in 0..steady {
                    assert_ne!(ticked.step(), Step::Changed, "{lfsr:#x} {narrow}");
                }
                assert_eq!(ticked.step(), Step::Changed, "{lfsr:#x} {narrow}");
            }
        }
    }

    #[test]
    fn jumping_the_lfsr_matches_stepping_it() {
        for (index, start) in random_channels().take(3000).enumerate() {
            let steps = match index {
                0..8 => 100_000 + index as u32,
                _ => (index as u32).wrapping_mul(7919) % 3000,
            };
            let mut stepped = start.clone();
            for _ in 0..steps {
                stepped.step_lfsr();
            }
            let mut jumped = start.clone();
            jumped.step_lfsr_times(steps);
            assert_eq!(
                format!("{jumped:?}"),
                format!("{stepped:?}"),
                "{start:?} {steps}"
            );
        }
    }

    #[test]
    fn advancing_matches_ticking_one_by_one() {
        for (index, start) in random_channels().take(4000).enumerate() {
            let ticks = (index as u32).wrapping_mul(7919) % 5000;
            let mut ticked = start.clone();
            for _ in 0..ticks {
                ticked.step();
            }
            let mut advanced = start.clone();
            advanced.advance_unheard(ticks);
            assert_eq!(
                format!("{advanced:?}"),
                format!("{ticked:?}"),
                "{start:?} {ticks}"
            );
        }
    }
}
