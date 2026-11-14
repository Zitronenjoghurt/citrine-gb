mod timing;

use super::Cpu;
use super::interrupt::Irq;
pub use timing::{Area, Timing};
use timing::{Meter, Unit};

pub const FIRST: usize = 0xB0;
pub const LAST: usize = 0xEF;
pub const CHANNELS: usize = 4;

const CHANNEL_BYTES: usize = 12;
const SOURCE: usize = 0;
const DEST: usize = 4;
const CONTROL: usize = 8;
const FILL: usize = CHANNEL_BYTES * CHANNELS;

const ARM9_ADDRESS: u32 = 0x0FFF_FFFF;
const ARM7_SOURCE: [u32; CHANNELS] = [0x07FF_FFFF, 0x0FFF_FFFF, 0x0FFF_FFFF, 0x0FFF_FFFF];
const ARM7_DEST: [u32; CHANNELS] = [0x07FF_FFFF, 0x07FF_FFFF, 0x07FF_FFFF, 0x0FFF_FFFF];
const ARM9_COUNT: u32 = 0x001F_FFFF;
const ARM7_COUNT: [u32; CHANNELS] = [0x3FFF, 0x3FFF, 0x3FFF, 0xFFFF];
const ARM7_CONTROL: u32 = 0xF7E0_0000;

const DEST_SHIFT: u32 = 21;
const SOURCE_SHIFT: u32 = 23;
const REPEAT: u32 = 1 << 25;
const WORD: u32 = 1 << 26;
const IRQ_ENABLE: u32 = 1 << 30;
const ENABLE: u32 = 1 << 31;
const RELOAD: u32 = 3;
const GEOMETRY_BLOCK: u32 = 112;
const TIMING_SPAN: u32 = 0x4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Start {
    Immediate,
    VBlank,
    HBlank,
    DisplayStart,
    MainDisplay,
    Card,
    GbaSlot,
    GeometryFifo,
    Wifi,
}

pub trait DmaBus {
    fn read_half(&mut self, address: u32) -> u16;
    fn read_word(&mut self, address: u32) -> u32;
    fn write_half(&mut self, address: u32, value: u16);
    fn write_word(&mut self, address: u32, value: u32);

    fn timing(&self, _address: u32) -> Timing {
        Timing::UNMAPPED
    }

    fn copy(&mut self, run: &Run)
    where
        Self: Sized,
    {
        run.copy_each(self);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub source: u32,
    pub source_step: u32,
    pub dest: u32,
    pub dest_step: u32,
    pub wide: bool,
    pub units: u32,
}

impl Run {
    pub fn last_source(&self) -> u32 {
        self.source
            .wrapping_add(self.source_step.wrapping_mul(self.units.saturating_sub(1)))
    }

    pub fn copy_each(&self, bus: &mut impl DmaBus) {
        let (mut source, mut dest) = (self.source, self.dest);
        for _ in 0..self.units {
            if self.wide {
                let value = bus.read_word(source & !3);
                bus.write_word(dest & !3, value);
            } else {
                let value = bus.read_half(source & !1);
                bus.write_half(dest & !1, value);
            }
            source = source.wrapping_add(self.source_step);
            dest = dest.wrapping_add(self.dest_step);
        }
    }
}

fn units_in_span(address: u32, step: u32, width: u32) -> u32 {
    let offset = address & (TIMING_SPAN - 1);
    match step.cast_signed() {
        0 => u32::MAX,
        1.. => (TIMING_SPAN - offset).div_ceil(width),
        _ => offset / width + 1,
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Channel {
    source: u32,
    dest: u32,
    control: u32,
    current_source: u32,
    current_dest: u32,
    remaining: u32,
    pending: bool,
}

impl Channel {
    fn enabled(self) -> bool {
        self.control & ENABLE != 0
    }

    fn width(self) -> u32 {
        if self.control & WORD != 0 { 4 } else { 2 }
    }

    fn dest_mode(self) -> u32 {
        (self.control >> DEST_SHIFT) & 3
    }

    fn source_mode(self) -> u32 {
        (self.control >> SOURCE_SHIFT) & 3
    }
}

fn step(mode: u32, width: u32) -> u32 {
    match mode {
        1 => width.wrapping_neg(),
        2 => 0,
        _ => width,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Dma {
    cpu: Cpu,
    channels: [Channel; CHANNELS],
    fill: [u32; CHANNELS],
}

impl Dma {
    pub fn new(cpu: Cpu) -> Self {
        Self {
            cpu,
            channels: [Channel::default(); CHANNELS],
            fill: [0; CHANNELS],
        }
    }

    pub fn idle_with_fill(&self) -> Self {
        Self {
            fill: self.fill,
            ..Self::new(self.cpu)
        }
    }

    pub fn read(&self, offset: usize) -> u32 {
        let offset = offset & !3;
        if offset >= FILL {
            return match self.cpu {
                Cpu::Arm9 => self.fill[(offset - FILL) / 4],
                Cpu::Arm7 => 0,
            };
        }
        let channel = &self.channels[offset / CHANNEL_BYTES];
        match offset % CHANNEL_BYTES {
            SOURCE => channel.source,
            DEST => channel.dest,
            CONTROL => channel.control,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) -> bool {
        let offset = offset & !3;
        if offset >= FILL {
            if self.cpu == Cpu::Arm9 {
                let fill = &mut self.fill[(offset - FILL) / 4];
                *fill = (*fill & !mask) | (value & mask);
            }
            return false;
        }
        let index = offset / CHANNEL_BYTES;
        let (source_mask, dest_mask, control_mask) = self.masks(index);
        let channel = &mut self.channels[index];
        match offset % CHANNEL_BYTES {
            SOURCE => channel.source = merge(channel.source, value, mask & source_mask),
            DEST => channel.dest = merge(channel.dest, value, mask & dest_mask),
            CONTROL => {
                let was_enabled = channel.enabled();
                channel.control = merge(channel.control, value, mask & control_mask);
                return self.control_written(index, was_enabled);
            }
            _ => {}
        }
        false
    }

    pub fn fill(&self, channel: usize) -> u32 {
        self.fill[channel]
    }

    pub fn start(&self, channel: usize) -> Start {
        let control = self.channels[channel].control;
        match self.cpu {
            Cpu::Arm9 => match (control >> 27) & 7 {
                0 => Start::Immediate,
                1 => Start::VBlank,
                2 => Start::HBlank,
                3 => Start::DisplayStart,
                4 => Start::MainDisplay,
                5 => Start::Card,
                6 => Start::GbaSlot,
                _ => Start::GeometryFifo,
            },
            Cpu::Arm7 => match (control >> 28) & 3 {
                0 => Start::Immediate,
                1 => Start::VBlank,
                2 => Start::Card,
                _ if channel.is_multiple_of(2) => Start::Wifi,
                _ => Start::GbaSlot,
            },
        }
    }

    #[inline]
    pub fn trigger(&mut self, start: Start) -> bool {
        let controls = self
            .channels
            .iter()
            .fold(0, |controls, channel| controls | channel.control);
        controls & ENABLE != 0 && self.trigger_enabled(start)
    }

    #[inline(never)]
    fn trigger_enabled(&mut self, start: Start) -> bool {
        let mut pending = false;
        for index in 0..CHANNELS {
            if self.channels[index].enabled() && self.start(index) == start {
                self.channels[index].pending = true;
            }
            pending |= self.channels[index].pending;
        }
        pending
    }

    pub fn pending(&self) -> bool {
        self.channels.iter().any(|channel| channel.pending)
    }

    pub fn pending_from(&self, start: Start) -> bool {
        (0..CHANNELS).any(|index| self.channels[index].pending && self.start(index) == start)
    }

    pub fn waiting(&self, start: Start) -> bool {
        (0..CHANNELS).any(|index| self.channels[index].enabled() && self.start(index) == start)
    }

    pub fn run(&mut self, bus: &mut impl DmaBus) -> Irq {
        self.run_timed(bus).0
    }

    pub fn run_timed(&mut self, bus: &mut impl DmaBus) -> (Irq, u32) {
        let mut irq = Irq::empty();
        let mut cycles = 0;
        for index in 0..CHANNELS {
            if self.channels[index].pending {
                let (raised, spent) = self.transfer(index, bus);
                irq |= raised;
                cycles += spent;
            }
        }
        (irq, cycles)
    }

    fn masks(&self, index: usize) -> (u32, u32, u32) {
        match self.cpu {
            Cpu::Arm9 => (ARM9_ADDRESS, ARM9_ADDRESS, 0xFFFF_FFFF),
            Cpu::Arm7 => (
                ARM7_SOURCE[index],
                ARM7_DEST[index],
                ARM7_CONTROL | ARM7_COUNT[index],
            ),
        }
    }

    fn count(&self, index: usize) -> u32 {
        let count_mask = match self.cpu {
            Cpu::Arm9 => ARM9_COUNT,
            Cpu::Arm7 => ARM7_COUNT[index],
        };
        match self.channels[index].control & count_mask {
            0 => count_mask + 1,
            count => count,
        }
    }

    fn control_written(&mut self, index: usize, was_enabled: bool) -> bool {
        let count = self.count(index);
        let start = self.start(index);
        let channel = &mut self.channels[index];
        if !channel.enabled() {
            channel.pending = false;
            return false;
        }
        if was_enabled {
            return false;
        }
        channel.current_source = channel.source;
        channel.current_dest = channel.dest;
        channel.remaining = count;
        channel.pending = start == Start::Immediate;
        start == Start::GeometryFifo
    }

    fn transfer(&mut self, index: usize, bus: &mut impl DmaBus) -> (Irq, u32) {
        let start = self.start(index);
        let count = self.count(index);
        let (source_mask, dest_mask, _) = self.masks(index);
        let channel = &mut self.channels[index];
        channel.pending = false;
        let units = match start {
            Start::GeometryFifo => channel.remaining.min(GEOMETRY_BLOCK),
            Start::Card => 1,
            _ => channel.remaining,
        };
        let width = channel.width();
        let source_step = step(channel.source_mode(), width);
        let dest_step = step(channel.dest_mode(), width);
        let mut meter = Meter::default();
        let mut cycles = 0;
        let mut left = units;
        while left > 0 {
            let source = channel.current_source & source_mask;
            let dest = channel.current_dest & dest_mask;
            let unit = Unit {
                source: bus.timing(source),
                dest: bus.timing(dest),
                source_address: source,
                source_increments: source_step.cast_signed() > 0,
                dest_increments: dest_step.cast_signed() > 0,
                wide: width == 4,
            };
            let steady = unit.source.steady() && unit.dest.steady();
            let run = if steady {
                left.min(units_in_span(source, source_step, width))
                    .min(units_in_span(dest, dest_step, width))
            } else {
                1
            };
            cycles += meter.run(&unit, run, source_step);
            bus.copy(&Run {
                source,
                source_step,
                dest,
                dest_step,
                wide: width == 4,
                units: run,
            });
            channel.current_source = channel
                .current_source
                .wrapping_add(source_step.wrapping_mul(run));
            channel.current_dest = channel
                .current_dest
                .wrapping_add(dest_step.wrapping_mul(run));
            left -= run;
        }
        channel.remaining -= units;
        if channel.remaining > 0 {
            return (Irq::empty(), cycles);
        }
        if channel.control & REPEAT != 0 && start != Start::Immediate {
            channel.remaining = count;
            if channel.dest_mode() == RELOAD {
                channel.current_dest = channel.dest;
            }
        } else {
            channel.control &= !ENABLE;
        }
        let irq = if channel.control & IRQ_ENABLE != 0 {
            Irq::dma(index)
        } else {
            Irq::empty()
        };
        (irq, cycles)
    }
}

fn merge(old: u32, value: u32, mask: u32) -> u32 {
    (old & !mask) | (value & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = 0xFFFF_FFFF;

    #[derive(Default)]
    struct Log {
        writes: Vec<(u32, u32)>,
        reads: Vec<u32>,
    }

    impl DmaBus for Log {
        fn read_half(&mut self, address: u32) -> u16 {
            self.reads.push(address);
            address as u16
        }

        fn read_word(&mut self, address: u32) -> u32 {
            self.reads.push(address);
            address
        }

        fn write_half(&mut self, address: u32, value: u16) {
            self.writes.push((address, u32::from(value)));
        }

        fn write_word(&mut self, address: u32, value: u32) {
            self.writes.push((address, value));
        }
    }

    const EXMEMCNT: u32 = 0x0400_0204;

    #[derive(Default)]
    struct Mapped {
        log: Log,
        slow_slot: bool,
    }

    impl DmaBus for Mapped {
        fn read_half(&mut self, address: u32) -> u16 {
            self.log.read_half(address)
        }

        fn read_word(&mut self, address: u32) -> u32 {
            self.log.read_word(address)
        }

        fn write_half(&mut self, address: u32, value: u16) {
            self.log.write_half(address, value);
        }

        fn write_word(&mut self, address: u32, value: u32) {
            if address == EXMEMCNT {
                self.slow_slot = true;
            }
            self.log.write_word(address, value);
        }

        fn timing(&self, address: u32) -> Timing {
            match address >> 24 {
                0x02 => Timing::new(Area::MainRam, 16, 8, 1),
                0x08 if self.slow_slot => Timing::new(Area::Slow(9), 16, 10, 6).set_by_registers(),
                0x08 => Timing::new(Area::Slow(9), 16, 6, 4).set_by_registers(),
                _ if address & TIMING_SPAN == 0 => Timing::new(Area::Plain(1), 32, 1, 1),
                _ => Timing::new(Area::Plain(2), 16, 3, 2),
            }
        }
    }

    fn channel(cpu: Cpu, index: usize, source: u32, dest: u32, control: u32) -> Dma {
        let mut dma = Dma::new(cpu);
        let base = index * CHANNEL_BYTES;
        dma.write(base + SOURCE, source, ALL);
        dma.write(base + DEST, dest, ALL);
        dma.write(base + CONTROL, control, ALL);
        dma
    }

    fn arm9_start(mode: u32) -> u32 {
        mode << 27
    }

    fn arm7_start(mode: u32) -> u32 {
        mode << 28
    }

    #[test]
    fn immediate_word_copy_runs_once_and_disables() {
        let mut dma = channel(
            Cpu::Arm9,
            0,
            0x0200_0000,
            0x0300_0000,
            ENABLE | WORD | IRQ_ENABLE | 3,
        );
        assert!(dma.pending());
        let mut bus = Log::default();
        assert_eq!(dma.run(&mut bus), Irq::DMA0);
        assert_eq!(
            bus.writes,
            [
                (0x0300_0000, 0x0200_0000),
                (0x0300_0004, 0x0200_0004),
                (0x0300_0008, 0x0200_0008),
            ]
        );
        assert_eq!(dma.read(CONTROL) & ENABLE, 0);
        assert!(!dma.pending());
    }

    #[test]
    fn halfwords_with_decrement_and_fixed() {
        let control = ENABLE | (2 << DEST_SHIFT) | (1 << SOURCE_SHIFT) | 3;
        let mut dma = channel(Cpu::Arm7, 1, 0x0200_0010, 0x0400_0400, control);
        let mut bus = Log::default();
        assert!(dma.run(&mut bus).is_empty());
        assert_eq!(bus.reads, [0x0200_0010, 0x0200_000E, 0x0200_000C]);
        assert!(
            bus.writes
                .iter()
                .all(|&(address, _)| address == 0x0400_0400)
        );
    }

    #[test]
    fn unaligned_addresses_round_down_to_the_width() {
        let mut dma = channel(Cpu::Arm9, 0, 0x0200_0003, 0x0300_0006, ENABLE | WORD | 1);
        let mut bus = Log::default();
        dma.run(&mut bus);
        assert_eq!(bus.writes, [(0x0300_0004, 0x0200_0000)]);
    }

    #[test]
    fn address_widths_follow_gbatek() {
        let dma = channel(Cpu::Arm9, 0, ALL, ALL, 0);
        assert_eq!(dma.read(SOURCE), 0x0FFF_FFFF);
        assert_eq!(dma.read(DEST), 0x0FFF_FFFF);
        let dma = channel(Cpu::Arm7, 0, ALL, ALL, 0);
        assert_eq!(dma.read(SOURCE), 0x07FF_FFFF);
        assert_eq!(dma.read(DEST), 0x07FF_FFFF);
        let dma = channel(Cpu::Arm7, 3, ALL, ALL, 0);
        assert_eq!(dma.read(3 * CHANNEL_BYTES + SOURCE), 0x0FFF_FFFF);
        assert_eq!(dma.read(3 * CHANNEL_BYTES + DEST), 0x0FFF_FFFF);
    }

    #[test]
    fn arm7_control_drops_unused_bits() {
        let dma = channel(Cpu::Arm7, 0, 0, 0, 0x7FFF_FFFF);
        assert_eq!(dma.read(CONTROL), 0x77E0_3FFF);
        let dma = channel(Cpu::Arm7, 3, 0, 0, 0x7FFF_FFFF);
        assert_eq!(dma.read(3 * CHANNEL_BYTES + CONTROL), 0x77E0_FFFF);
        let dma = channel(Cpu::Arm9, 0, 0, 0, 0x7FFF_FFFF);
        assert_eq!(dma.read(CONTROL), 0x7FFF_FFFF);
    }

    fn units_for_zero_count(cpu: Cpu, index: usize) -> usize {
        let mut dma = channel(cpu, index, 0, 0, ENABLE);
        let mut bus = Log::default();
        dma.run(&mut bus);
        bus.writes.len()
    }

    #[test]
    fn zero_count_means_the_maximum() {
        assert_eq!(units_for_zero_count(Cpu::Arm9, 0), 0x20_0000);
        assert_eq!(units_for_zero_count(Cpu::Arm7, 2), 0x4000);
        assert_eq!(units_for_zero_count(Cpu::Arm7, 3), 0x1_0000);
    }

    #[test]
    fn arm9_start_modes() {
        let starts = [
            Start::Immediate,
            Start::VBlank,
            Start::HBlank,
            Start::DisplayStart,
            Start::MainDisplay,
            Start::Card,
            Start::GbaSlot,
            Start::GeometryFifo,
        ];
        for (mode, start) in starts.into_iter().enumerate() {
            let dma = channel(Cpu::Arm9, 2, 0, 0, arm9_start(mode as u32));
            assert_eq!(dma.start(2), start);
        }
    }

    #[test]
    fn arm7_start_modes() {
        let dma = channel(Cpu::Arm7, 0, 0, 0, arm7_start(1));
        assert_eq!(dma.start(0), Start::VBlank);
        let dma = channel(Cpu::Arm7, 1, 0, 0, arm7_start(2));
        assert_eq!(dma.start(1), Start::Card);
        let dma = channel(Cpu::Arm7, 2, 0, 0, arm7_start(3));
        assert_eq!(dma.start(2), Start::Wifi);
        let dma = channel(Cpu::Arm7, 3, 0, 0, arm7_start(3));
        assert_eq!(dma.start(3), Start::GbaSlot);
        let dma = channel(Cpu::Arm7, 0, 0, 0, arm9_start(1));
        assert_eq!(dma.start(0), Start::Immediate);
    }

    #[test]
    fn timed_channels_wait_for_their_trigger() {
        let mut dma = channel(Cpu::Arm9, 1, 0, 0x100, ENABLE | arm9_start(1) | 1);
        assert!(!dma.pending());
        assert!(dma.waiting(Start::VBlank));
        dma.trigger(Start::HBlank);
        assert!(!dma.pending());
        dma.trigger(Start::VBlank);
        assert!(dma.pending());
        let mut bus = Log::default();
        dma.run(&mut bus);
        assert_eq!(bus.writes.len(), 1);
        assert!(!dma.waiting(Start::VBlank));
    }

    #[test]
    fn repeat_reloads_count_and_reload_dest() {
        let control = ENABLE | REPEAT | WORD | (RELOAD << DEST_SHIFT) | arm9_start(2) | 2;
        let mut dma = channel(Cpu::Arm9, 0, 0x0200_0000, 0x0400_0010, control);
        let mut bus = Log::default();
        dma.trigger(Start::HBlank);
        dma.run(&mut bus);
        dma.trigger(Start::HBlank);
        dma.run(&mut bus);
        assert_eq!(
            bus.writes,
            [
                (0x0400_0010, 0x0200_0000),
                (0x0400_0014, 0x0200_0004),
                (0x0400_0010, 0x0200_0008),
                (0x0400_0014, 0x0200_000C),
            ]
        );
        assert_ne!(dma.read(CONTROL) & ENABLE, 0);
    }

    #[test]
    fn repeat_keeps_incrementing_dest_without_reload() {
        let control = ENABLE | REPEAT | WORD | arm9_start(5) | 1;
        let mut dma = channel(
            Cpu::Arm9,
            0,
            0x0410_0010,
            0x0200_0000,
            control | (2 << SOURCE_SHIFT),
        );
        let mut bus = Log::default();
        for _ in 0..3 {
            dma.trigger(Start::Card);
            dma.run(&mut bus);
        }
        let dests: Vec<u32> = bus.writes.iter().map(|&(address, _)| address).collect();
        assert_eq!(dests, [0x0200_0000, 0x0200_0004, 0x0200_0008]);
        assert!(bus.reads.iter().all(|&address| address == 0x0410_0010));
    }

    #[test]
    fn immediate_ignores_repeat() {
        let mut dma = channel(Cpu::Arm7, 0, 0, 0, ENABLE | REPEAT | 1);
        dma.run(&mut Log::default());
        assert_eq!(dma.read(CONTROL) & ENABLE, 0);
    }

    #[test]
    fn geometry_fifo_moves_112_words_per_trigger() {
        let control = ENABLE | WORD | IRQ_ENABLE | (2 << DEST_SHIFT) | arm9_start(7) | 300;
        let mut dma = channel(Cpu::Arm9, 3, 0x0200_0000, 0x0400_0400, control);
        let mut bus = Log::default();
        dma.trigger(Start::GeometryFifo);
        assert!(dma.run(&mut bus).is_empty());
        assert_eq!(bus.writes.len(), 112);
        dma.trigger(Start::GeometryFifo);
        assert!(dma.run(&mut bus).is_empty());
        assert_eq!(bus.writes.len(), 224);
        dma.trigger(Start::GeometryFifo);
        assert_eq!(dma.run(&mut bus), Irq::DMA3);
        assert_eq!(bus.writes.len(), 300);
        assert_eq!(bus.reads.last(), Some(&(0x0200_0000 + 299 * 4)));
        assert_eq!(dma.read(3 * CHANNEL_BYTES + CONTROL) & ENABLE, 0);
    }

    #[test]
    fn card_channels_move_one_word_per_request() {
        let control = ENABLE | WORD | arm9_start(5) | 3;
        let mut dma = channel(Cpu::Arm9, 0, 0x0410_0010, 0x0200_0000, control);
        let mut bus = Log::default();
        for words in 1..=3 {
            dma.trigger(Start::Card);
            dma.run(&mut bus);
            assert_eq!(bus.writes.len(), words);
        }
        assert_eq!(dma.read(CONTROL) & ENABLE, 0);
    }

    #[test]
    fn disabling_drops_a_pending_trigger() {
        let mut dma = channel(Cpu::Arm9, 0, 0, 0, ENABLE | arm9_start(1) | 1);
        dma.trigger(Start::VBlank);
        dma.write(CONTROL, 0, ENABLE);
        assert!(!dma.pending());
    }

    #[test]
    fn a_trigger_reports_whether_any_channel_waits_to_run() {
        let mut dma = channel(Cpu::Arm9, 0, 0, 0, ENABLE | arm9_start(1) | 1);
        assert!(!dma.trigger(Start::HBlank));
        assert!(dma.trigger(Start::VBlank));
        assert!(dma.trigger(Start::HBlank));
        dma.run(&mut Log::default());
        assert!(!dma.trigger(Start::HBlank));
    }

    #[test]
    fn rewriting_an_enabled_channel_keeps_its_progress() {
        let control = ENABLE | WORD | arm9_start(7) | 200;
        let mut dma = channel(Cpu::Arm9, 0, 0x0200_0000, 0x0400_0400, control);
        let mut bus = Log::default();
        dma.trigger(Start::GeometryFifo);
        dma.run(&mut bus);
        dma.write(CONTROL, control, ALL);
        dma.trigger(Start::GeometryFifo);
        dma.run(&mut bus);
        assert_eq!(bus.writes.len(), 200);
        assert_eq!(bus.reads[112], 0x0200_0000 + 112 * 4);
    }

    #[test]
    fn channels_run_in_priority_order() {
        let mut dma = Dma::new(Cpu::Arm9);
        for index in [2, 0] {
            let base = index * CHANNEL_BYTES;
            dma.write(base + SOURCE, index as u32 * 0x100, ALL);
            dma.write(base + CONTROL, ENABLE | IRQ_ENABLE | arm9_start(1) | 1, ALL);
        }
        dma.trigger(Start::VBlank);
        let mut bus = Log::default();
        assert_eq!(dma.run(&mut bus), Irq::DMA0 | Irq::DMA2);
        assert_eq!(bus.reads, [0x000, 0x200]);
    }

    #[test]
    fn a_copy_across_timing_spans_costs_what_timing_each_unit_does() {
        let units = 3000;
        for (source, dest) in [(0x0300_3F00, 0x0500_3FF0), (0x0200_3F00, 0x0500_3FF0)] {
            let mut dma = channel(Cpu::Arm9, 0, source, dest, ENABLE | WORD | units);
            let mut bus = Mapped::default();
            let (_, cycles) = dma.run_timed(&mut bus);
            let mut meter = Meter::default();
            let expected: u32 = (0..units)
                .map(|index| {
                    let (source, dest) = (source + 4 * index, dest + 4 * index);
                    let unit = Unit {
                        source: bus.timing(source),
                        dest: bus.timing(dest),
                        source_address: source,
                        source_increments: true,
                        dest_increments: true,
                        wide: true,
                    };
                    meter.run(&unit, 1, 0)
                })
                .sum();
            assert_eq!(cycles, expected);
            assert_eq!(bus.log.writes.len(), units as usize);
            assert_eq!(bus.log.writes[2999], (dest + 4 * 2999, source + 4 * 2999));
        }
    }

    #[test]
    fn a_timing_set_by_registers_is_read_again_for_every_unit() {
        let mut dma = channel(Cpu::Arm9, 0, 0x0800_0000, EXMEMCNT - 4, ENABLE | WORD | 3);
        let mut bus = Mapped::default();
        assert_eq!(dma.run_timed(&mut bus).1, 11 + 9 + 13);
    }

    #[test]
    fn bus_timings_hold_across_each_timing_span() {
        let mut buses = super::super::bus::Buses::default();
        for address in (0..0x1000_0000u32).step_by(0x400) {
            let span = address & !(TIMING_SPAN - 1);
            assert_eq!(
                buses.arm9().timing(address),
                buses.arm9().timing(span),
                "{address:08X}"
            );
            assert_eq!(
                buses.arm7().timing(address),
                buses.arm7().timing(span),
                "{address:08X}"
            );
        }
    }

    #[test]
    fn triggers_match_the_channel_walk_over_random_writes() {
        let starts = [
            Start::Immediate,
            Start::VBlank,
            Start::HBlank,
            Start::DisplayStart,
            Start::MainDisplay,
            Start::Card,
            Start::GbaSlot,
            Start::GeometryFifo,
            Start::Wifi,
        ];
        let mut seed = 0x2545_F491u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let mut dma = Dma::new(cpu);
            let (mut skipped, mut walked) = (0, 0);
            for _ in 0..4000 {
                let index = next() as usize % CHANNELS;
                match next() % 4 {
                    0 => {
                        let enable = if next() % 4 == 0 { ENABLE } else { 0 };
                        let control = (next() & 0x7FE0_0000) | enable | (next() & 7) | 1;
                        dma.write(index * CHANNEL_BYTES + CONTROL, control, ALL);
                    }
                    1 => {
                        dma.run(&mut Log::default());
                    }
                    _ => {
                        let start = starts[next() as usize % starts.len()];
                        let mut walk = dma.clone();
                        if dma.channels.iter().any(|channel| channel.enabled()) {
                            walked += 1;
                        } else {
                            skipped += 1;
                        }
                        assert_eq!(dma.trigger(start), walk.trigger_enabled(start));
                        assert_eq!(dma, walk);
                    }
                }
            }
            assert!(skipped > 100 && walked > 100, "{skipped} {walked}");
        }
    }

    #[test]
    fn fill_registers_are_arm9_only() {
        let mut dma = Dma::new(Cpu::Arm9);
        dma.write(0xE8 - FIRST, 0xDEAD_BEEF, ALL);
        dma.write(0xE8 - FIRST, 0x0000_0000, 0x0000_FFFF);
        assert_eq!(dma.read(0xE8 - FIRST), 0xDEAD_0000);
        assert_eq!(dma.fill(2), 0xDEAD_0000);
        let mut dma = Dma::new(Cpu::Arm7);
        dma.write(0xE0 - FIRST, 0x1234_5678, ALL);
        assert_eq!(dma.read(0xE0 - FIRST), 0);
    }
}
