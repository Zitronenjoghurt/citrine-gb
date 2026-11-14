use super::interrupt::Irq;
use super::region::Region;
use super::wait::{Access, WaitControl, Width};

pub const CHANNELS: usize = 4;
const CHANNEL_BYTES: usize = 12;
const SOURCE_MASKS: [u32; CHANNELS] = [0x07FF_FFFF, 0x0FFF_FFFF, 0x0FFF_FFFF, 0x0FFF_FFFF];
const DEST_MASKS: [u32; CHANNELS] = [0x07FF_FFFF, 0x07FF_FFFF, 0x07FF_FFFF, 0x0FFF_FFFF];
const COUNT_MASKS: [u16; CHANNELS] = [0x3FFF, 0x3FFF, 0x3FFF, 0xFFFF];
const CONTROL_MASKS: [u16; CHANNELS] = [0xF7E0, 0xF7E0, 0xF7E0, 0xFFE0];
const SOURCE_LOW: usize = 0;
const SOURCE_HIGH: usize = 2;
const DEST_LOW: usize = 4;
const DEST_HIGH: usize = 6;
const COUNT: usize = 8;
const CONTROL: usize = 10;
const REPEAT: u16 = 1 << 9;
const WORD: u16 = 1 << 10;
const IRQ_ENABLE: u16 = 1 << 14;
const ENABLE: u16 = 1 << 15;
const CAPTURE_CHANNEL: usize = 3;
const FIFO_UNITS: u32 = 4;
pub const ACTIVATION_DELAY: u32 = 2;
pub const IDLE_BEFORE: u32 = 1;
pub const IDLE_AFTER: u32 = 1;
const INTERNAL_CYCLES: u32 = IDLE_BEFORE + IDLE_AFTER;
const GAMEPAK_INTERNAL_CYCLES: u32 = 4;
const ROM_PAGE_END: u32 = 0x1_FFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Timing {
    Immediate,
    VBlank,
    HBlank,
    Special,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Step {
    Increment,
    Decrement,
    Fixed,
    Reload,
}

impl Step {
    fn from_bits(bits: u16) -> Self {
        match bits & 0b11 {
            0 => Self::Increment,
            1 => Self::Decrement,
            2 => Self::Fixed,
            _ => Self::Reload,
        }
    }

    fn delta(self, width: Width) -> i32 {
        let bytes = width.bytes() as i32;
        match self {
            Self::Increment | Self::Reload => bytes,
            Self::Decrement => -bytes,
            Self::Fixed => 0,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Channel {
    source: u32,
    dest: u32,
    count: u16,
    control: u16,
    source_now: u32,
    dest_now: u32,
    units_now: u32,
    active: bool,
    latch: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    due: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    paused: bool,
}

impl Channel {
    fn enabled(self) -> bool {
        self.control & ENABLE != 0
    }

    fn timing(self) -> Timing {
        match (self.control >> 12) & 0b11 {
            0 => Timing::Immediate,
            1 => Timing::VBlank,
            2 => Timing::HBlank,
            _ => Timing::Special,
        }
    }

    fn dest_step(self) -> Step {
        Step::from_bits(self.control >> 5)
    }

    fn source_step(self) -> Step {
        match Step::from_bits(self.control >> 7) {
            Step::Reload => Step::Increment,
            step => step,
        }
    }

    fn width(self) -> Width {
        if self.control & WORD != 0 {
            Width::Word
        } else {
            Width::Half
        }
    }

    fn block_units(self, index: usize) -> u32 {
        if is_fifo(index, self) {
            FIFO_UNITS
        } else {
            units(index, self.count)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transfer {
    channel: usize,
    source: u32,
    dest: u32,
    units: u32,
    width: Width,
    source_step: i32,
    dest_step: i32,
    resumed: bool,
}

impl Transfer {
    pub fn channel(self) -> usize {
        self.channel
    }

    pub fn width(self) -> Width {
        self.width
    }

    pub fn units(self) -> u32 {
        self.units
    }

    pub fn resumed(self) -> bool {
        self.resumed
    }

    fn ends_after(self, units: u32) -> (u32, u32) {
        let advance =
            |start: u32, step: i32| start.wrapping_add_signed(step.wrapping_mul(units as i32));
        (
            advance(self.source, self.source_step) & SOURCE_MASKS[self.channel],
            advance(self.dest, self.dest_step) & DEST_MASKS[self.channel],
        )
    }

    pub fn steady_regions(self) -> Option<(Region, Region)> {
        if self.units == 0 {
            return None;
        }
        let ((source, dest), (last_source, last_dest)) =
            (self.ends_after(0), self.ends_after(self.units - 1));
        let same = |a: u32, b: u32| a >> 24 == b >> 24 && Region::of(a) == Region::of(b);
        (same(source, last_source) && same(dest, last_dest))
            .then(|| (Region::of(source), Region::of(dest)))
    }

    pub fn steady_unit_cycles(
        self,
        wait: WaitControl,
        (from, to): (Region, Region),
    ) -> Option<(u32, u32)> {
        let off_game_pak = |region| !matches!(region, Region::Rom(_) | Region::Sram);
        let (source, dest) = self.ends_after(0);
        (off_game_pak(from) && off_game_pak(to))
            .then(|| self.unit_cycles_in(wait, 0, (from, source), (to, dest)))
    }

    pub fn addresses(self) -> impl Iterator<Item = (u32, u32)> {
        let (source_mask, dest_mask) = (SOURCE_MASKS[self.channel], DEST_MASKS[self.channel]);
        (0..self.units).map(move |unit| {
            let source = self
                .source
                .wrapping_add_signed(self.source_step.wrapping_mul(unit as i32));
            let dest = self
                .dest
                .wrapping_add_signed(self.dest_step.wrapping_mul(unit as i32));
            (source & source_mask, dest & dest_mask)
        })
    }

    pub fn cycles(self, wait: WaitControl) -> u32 {
        self.addresses()
            .enumerate()
            .fold(self.internal(), |total, (unit, (source, dest))| {
                let (read, write) = self.unit_cycles(wait, unit, source, dest);
                total + read + write
            })
    }

    pub fn internal(self) -> u32 {
        if is_gamepak(self.source) && is_gamepak(self.dest) {
            GAMEPAK_INTERNAL_CYCLES
        } else {
            INTERNAL_CYCLES
        }
    }

    pub fn gamepak_idle(self) -> u32 {
        self.internal() - INTERNAL_CYCLES
    }

    pub fn unit_cycles(self, wait: WaitControl, unit: usize, source: u32, dest: u32) -> (u32, u32) {
        self.unit_cycles_in(
            wait,
            unit,
            (Region::of(source), source),
            (Region::of(dest), dest),
        )
    }

    #[inline(always)]
    pub fn unit_cycles_in(
        self,
        wait: WaitControl,
        unit: usize,
        (from, source): (Region, u32),
        (to, dest): (Region, u32),
    ) -> (u32, u32) {
        let first = unit == 0;
        (
            wait.region_cycles(
                from,
                source,
                self.width,
                self.access(first, from, source, self.source_step),
            ),
            wait.region_cycles(
                to,
                dest,
                self.width,
                self.access(first, to, dest, self.dest_step),
            ),
        )
    }

    #[inline(always)]
    fn access(self, first: bool, region: Region, addr: u32, step: i32) -> Access {
        let page_end = ROM_PAGE_END & !(self.width.bytes() as u32 - 1);
        let after_page_end = matches!(region, Region::Rom(_))
            && addr.wrapping_sub_signed(step) & page_end == page_end;
        if first || after_page_end {
            Access::NonSequential
        } else {
            Access::Sequential
        }
    }
}

fn is_gamepak(addr: u32) -> bool {
    matches!(Region::of(addr), Region::Rom(_) | Region::Sram)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Dma {
    channels: [Channel; CHANNELS],
    capture: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    scheduled: u8,
}

impl Dma {
    pub fn read(&self, offset: usize) -> u16 {
        let channel = self.channels[offset / CHANNEL_BYTES];
        match offset % CHANNEL_BYTES {
            CONTROL => channel.control,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u16, mask: u16, now: u32) {
        let index = offset / CHANNEL_BYTES;
        let channel = &mut self.channels[index];
        match offset % CHANNEL_BYTES {
            SOURCE_LOW => channel.source = merge_low(channel.source, value, mask),
            SOURCE_HIGH => channel.source = merge_high(channel.source, value, mask),
            DEST_LOW => channel.dest = merge_low(channel.dest, value, mask),
            DEST_HIGH => channel.dest = merge_high(channel.dest, value, mask),
            COUNT => channel.count = (channel.count & !mask) | (value & mask),
            _ => self.write_control(index, value, mask, now),
        }
    }

    fn write_control(&mut self, index: usize, value: u16, mask: u16, now: u32) {
        let channel = &mut self.channels[index];
        let was_enabled = channel.enabled();
        channel.control = ((channel.control & !mask) | (value & mask)) & CONTROL_MASKS[index];
        if !channel.enabled() {
            channel.active = false;
            self.unschedule(index);
        } else if !was_enabled {
            channel.source_now = channel.source & SOURCE_MASKS[index];
            channel.dest_now = channel.dest & DEST_MASKS[index];
            channel.units_now = channel.block_units(index);
            channel.paused = false;
            if channel.timing() == Timing::Immediate {
                self.schedule(index, now);
            }
        }
    }

    pub fn source(&self, channel: usize) -> u32 {
        self.channels[channel].source
    }

    pub fn count(&self, channel: usize) -> u16 {
        self.channels[channel].count
    }

    fn schedule(&mut self, index: usize, at: u32) {
        let due = at + ACTIVATION_DELAY;
        let channel = &mut self.channels[index];
        if self.scheduled & 1 << index == 0 || due < channel.due {
            channel.due = due;
        }
        self.scheduled |= 1 << index;
    }

    fn unschedule(&mut self, index: usize) {
        self.scheduled &= !(1 << index);
        self.channels[index].due = 0;
    }

    pub fn activate(&mut self, now: u32) -> bool {
        for index in 0..CHANNELS {
            if self.scheduled & 1 << index != 0 && self.channels[index].due <= now {
                self.unschedule(index);
                self.channels[index].active = true;
            }
        }
        self.running()
    }

    pub fn due(&self) -> Option<u32> {
        (0..CHANNELS)
            .filter(|&index| self.scheduled & 1 << index != 0)
            .map(|index| self.channels[index].due)
            .min()
    }

    pub fn elapse(&mut self, cycles: u32) {
        if self.scheduled == 0 {
            return;
        }
        for channel in &mut self.channels {
            channel.due = channel.due.saturating_sub(cycles);
        }
    }

    pub fn waits_on_video(&self) -> bool {
        self.capture
            || self.channels.iter().any(|channel| {
                channel.enabled() && matches!(channel.timing(), Timing::VBlank | Timing::HBlank)
            })
    }

    pub fn running(&self) -> bool {
        self.channels.iter().any(|channel| channel.active)
    }

    pub fn dest(&self, channel: usize) -> u32 {
        self.channels[channel].dest
    }

    pub fn control(&self, channel: usize) -> u16 {
        self.channels[channel].control
    }

    pub fn capturing(&self) -> bool {
        self.capture
    }

    pub fn active(&self) -> bool {
        self.scheduled != 0 || self.running()
    }

    pub fn trigger(&mut self, timing: Timing) {
        for channel in &mut self.channels {
            if channel.enabled() && channel.timing() == timing {
                channel.active = true;
            }
        }
    }

    pub fn request(&mut self, timing: Timing, at: u32) -> bool {
        let mut scheduled = false;
        for index in 0..CHANNELS {
            let channel = self.channels[index];
            if channel.enabled() && channel.timing() == timing {
                self.schedule(index, at);
                scheduled = true;
            }
        }
        scheduled
    }

    pub fn trigger_special(&mut self, index: usize) {
        let channel = &mut self.channels[index];
        if index > 0 && channel.enabled() && channel.timing() == Timing::Special {
            channel.active = true;
        }
    }

    pub fn special_armed(&self, index: usize) -> bool {
        let channel = self.channels[index];
        channel.enabled() && channel.timing() == Timing::Special
    }

    pub fn request_special(&mut self, index: usize, at: u32) -> bool {
        let channel = self.channels[index];
        let special = index > 0 && channel.enabled() && channel.timing() == Timing::Special;
        if special {
            self.schedule(index, at);
        }
        special
    }

    pub fn arm_capture(&mut self) -> bool {
        let channel = self.channels[CAPTURE_CHANNEL];
        self.capture = channel.enabled() && channel.timing() == Timing::Special;
        self.capture
    }

    pub fn request_capture(&mut self, at: u32) -> bool {
        self.capture && self.request_special(CAPTURE_CHANNEL, at)
    }

    pub fn stop_capture(&mut self) {
        let channel = &mut self.channels[CAPTURE_CHANNEL];
        if self.capture && channel.timing() == Timing::Special {
            channel.control &= !ENABLE;
            channel.active = false;
            self.unschedule(CAPTURE_CHANNEL);
        }
    }

    pub fn next(&self) -> Option<Transfer> {
        let index = self.channels.iter().position(|channel| channel.active)?;
        let channel = self.channels[index];
        let fifo = is_fifo(index, channel);
        let width = if fifo { Width::Word } else { channel.width() };
        let align = !(width.bytes() as u32 - 1);
        Some(Transfer {
            channel: index,
            source: channel.source_now & align,
            dest: channel.dest_now & align,
            units: if fifo {
                channel.units_now.min(FIFO_UNITS)
            } else {
                channel.units_now
            },
            width,
            source_step: channel.source_step().delta(width),
            dest_step: if fifo {
                0
            } else {
                channel.dest_step().delta(width)
            },
            resumed: channel.paused,
        })
    }

    pub fn pause(&mut self, transfer: Transfer, moved: u32) {
        let (source, dest) = transfer.ends_after(moved);
        let channel = &mut self.channels[transfer.channel];
        channel.source_now = source;
        channel.dest_now = dest;
        channel.units_now = transfer.units - moved;
        channel.paused = true;
    }

    pub fn latch_word(&self, channel: usize) -> u32 {
        self.channels[channel].latch
    }

    pub fn latch(&mut self, channel: usize, read: Option<u32>, width: Width, dest: u32) -> u32 {
        let latch = &mut self.channels[channel].latch;
        match (read, width) {
            (Some(value), Width::Half) => {
                *latch = value & 0xFFFF | value << 16;
                value
            }
            (Some(value), _) => {
                *latch = value;
                value
            }
            (None, Width::Half) => *latch >> (8 * (dest & 2)) & 0xFFFF,
            (None, _) => *latch,
        }
    }

    pub fn finish(&mut self, transfer: Transfer) -> Irq {
        let index = transfer.channel;
        let (source, dest) = transfer.ends_after(transfer.units);
        let channel = &mut self.channels[index];
        channel.source_now = source;
        channel.dest_now = dest;
        channel.active = false;
        channel.paused = false;
        if channel.control & REPEAT != 0 && channel.timing() != Timing::Immediate {
            channel.units_now = channel.block_units(index);
            if channel.dest_step() == Step::Reload {
                channel.dest_now = channel.dest & DEST_MASKS[index];
            }
        } else {
            channel.control &= !ENABLE;
            channel.due = 0;
            self.scheduled &= !(1 << index);
        }
        let channel = self.channels[index];
        if channel.control & IRQ_ENABLE != 0 {
            Irq::dma(index)
        } else {
            Irq::empty()
        }
    }
}

fn is_fifo(index: usize, channel: Channel) -> bool {
    matches!(index, 1 | 2) && channel.timing() == Timing::Special
}

fn units(index: usize, count: u16) -> u32 {
    match count & COUNT_MASKS[index] {
        0 => u32::from(COUNT_MASKS[index]) + 1,
        units => u32::from(units),
    }
}

fn merge_low(register: u32, value: u16, mask: u16) -> u32 {
    let low = (register as u16 & !mask) | (value & mask);
    (register & 0xFFFF_0000) | u32::from(low)
}

fn merge_high(register: u32, value: u16, mask: u16) -> u32 {
    let high = ((register >> 16) as u16 & !mask) | (value & mask);
    (register & 0x0000_FFFF) | u32::from(high) << 16
}

#[cfg(test)]
mod tests {
    use super::*;

    const VBLANK: u16 = 1 << 12;
    const HBLANK: u16 = 2 << 12;
    const SPECIAL: u16 = 3 << 12;
    const DEST_DECREMENT: u16 = 1 << 5;
    const DEST_FIXED: u16 = 2 << 5;
    const DEST_RELOAD: u16 = 3 << 5;
    const SOURCE_DECREMENT: u16 = 1 << 7;
    const SOURCE_FIXED: u16 = 2 << 7;

    fn base(channel: usize) -> usize {
        CHANNEL_BYTES * channel
    }

    fn set_up(dma: &mut Dma, channel: usize, source: u32, dest: u32, count: u16) {
        let base = base(channel);
        dma.write(base + SOURCE_LOW, source as u16, 0xFFFF, 0);
        dma.write(base + SOURCE_HIGH, (source >> 16) as u16, 0xFFFF, 0);
        dma.write(base + DEST_LOW, dest as u16, 0xFFFF, 0);
        dma.write(base + DEST_HIGH, (dest >> 16) as u16, 0xFFFF, 0);
        dma.write(base + COUNT, count, 0xFFFF, 0);
    }

    fn enable(dma: &mut Dma, channel: usize, control: u16) {
        dma.write(base(channel) + CONTROL, control | ENABLE, 0xFFFF, 0);
        dma.activate(ACTIVATION_DELAY);
    }

    fn started(channel: usize, source: u32, dest: u32, count: u16, control: u16) -> Dma {
        let mut dma = Dma::default();
        set_up(&mut dma, channel, source, dest, count);
        enable(&mut dma, channel, control);
        dma
    }

    fn addresses(transfer: Transfer) -> Vec<(u32, u32)> {
        transfer.addresses().collect()
    }

    #[test]
    fn only_control_reads_back() {
        let mut dma = Dma::default();
        set_up(&mut dma, 3, 0x0800_1234, 0x0300_5678, 9);
        dma.write(base(3) + CONTROL, 0x0240, 0xFFFF, 0);
        for offset in [SOURCE_LOW, SOURCE_HIGH, DEST_LOW, DEST_HIGH, COUNT] {
            assert_eq!(dma.read(base(3) + offset), 0, "offset {offset}");
        }
        assert_eq!(dma.read(base(3) + CONTROL), 0x0240);
    }

    #[test]
    fn control_keeps_its_defined_bits_and_drq_only_on_dma3() {
        let mut dma = Dma::default();
        for channel in 0..CHANNELS {
            dma.write(base(channel) + CONTROL, 0x7FFF, 0xFFFF, 0);
        }
        assert_eq!(dma.control(0), 0x77E0);
        assert_eq!(dma.control(2), 0x77E0);
        assert_eq!(dma.control(3), 0x7FE0);
    }

    #[test]
    fn immediate_starts_on_enable_and_clears_enable_when_done() {
        let mut dma = started(3, 0x0200_0000, 0x0300_0000, 4, 0);
        assert!(dma.active());
        let transfer = dma.next().unwrap();
        assert_eq!(transfer.units(), 4);
        assert_eq!(transfer.width(), Width::Half);
        dma.finish(transfer);
        assert!(!dma.active());
        assert_eq!(dma.control(3) & ENABLE, 0);
    }

    #[test]
    fn immediate_ignores_repeat() {
        let mut dma = started(3, 0x0200_0000, 0x0300_0000, 4, REPEAT);
        let transfer = dma.next().unwrap();
        dma.finish(transfer);
        assert_eq!(dma.control(3) & ENABLE, 0);
    }

    #[test]
    fn vblank_and_hblank_wait_for_their_trigger() {
        let mut dma = Dma::default();
        set_up(&mut dma, 1, 0x0200_0000, 0x0300_0000, 1);
        set_up(&mut dma, 2, 0x0200_0000, 0x0300_0000, 1);
        enable(&mut dma, 1, VBLANK);
        enable(&mut dma, 2, HBLANK);
        assert!(!dma.active());
        dma.trigger(Timing::HBlank);
        assert_eq!(dma.next().unwrap().channel(), 2);
        let transfer = dma.next().unwrap();
        dma.finish(transfer);
        assert!(!dma.active());
        dma.trigger(Timing::VBlank);
        assert_eq!(dma.next().unwrap().channel(), 1);
    }

    #[test]
    fn a_trigger_does_not_start_a_disabled_channel() {
        let mut dma = Dma::default();
        dma.write(base(1) + CONTROL, VBLANK, 0xFFFF, 0);
        dma.trigger(Timing::VBlank);
        assert!(!dma.active());
    }

    #[test]
    fn disabling_stops_a_waiting_channel() {
        let mut dma = started(1, 0x0200_0000, 0x0300_0000, 1, HBLANK);
        dma.trigger(Timing::HBlank);
        dma.write(base(1) + CONTROL, 0, 0xFFFF, 0);
        assert!(!dma.active());
    }

    #[test]
    fn special_starts_channels_1_to_3_but_not_0() {
        for channel in 0..CHANNELS {
            let mut dma = started(channel, 0x0200_0000, 0x0300_0000, 1, SPECIAL);
            dma.trigger(Timing::VBlank);
            dma.trigger_special(channel);
            assert_eq!(dma.active(), channel > 0, "channel {channel}");
        }
    }

    #[test]
    fn sound_fifo_moves_four_words_to_a_fixed_address() {
        for channel in [1, 2] {
            let mut dma = started(channel, 0x0200_0000, 0x0400_00A0, 100, SPECIAL | REPEAT);
            dma.trigger_special(channel);
            let transfer = dma.next().unwrap();
            assert_eq!(transfer.width(), Width::Word);
            assert_eq!(
                addresses(transfer),
                [0, 4, 8, 12].map(|step| (0x0200_0000 + step, 0x0400_00A0))
            );
            dma.finish(transfer);
            dma.trigger_special(channel);
            assert_eq!(dma.next().unwrap().source, 0x0200_0010);
        }
    }

    #[test]
    fn video_capture_on_dma3_keeps_its_count() {
        let mut dma = started(3, 0x0200_0000, 0x0600_0000, 120, SPECIAL | REPEAT | WORD);
        dma.trigger_special(3);
        assert_eq!(dma.next().unwrap().units(), 120);
    }

    #[test]
    fn count_zero_means_the_maximum_per_channel() {
        for (channel, max) in [(0, 0x4000), (1, 0x4000), (2, 0x4000), (3, 0x1_0000)] {
            let dma = started(channel, 0x0200_0000, 0x0300_0000, 0, 0);
            assert_eq!(dma.next().unwrap().units(), max, "channel {channel}");
        }
    }

    #[test]
    fn count_keeps_14_bits_below_dma3() {
        let dma = started(0, 0x0200_0000, 0x0300_0000, 0xC005, 0);
        assert_eq!(dma.next().unwrap().units(), 5);
        let dma = started(3, 0x0200_0000, 0x0300_0000, 0xC005, 0);
        assert_eq!(dma.next().unwrap().units(), 0xC005);
    }

    #[test]
    fn address_steps_for_halfwords() {
        let dma = started(
            3,
            0x0200_0010,
            0x0300_0010,
            3,
            DEST_DECREMENT | SOURCE_FIXED,
        );
        let pairs = addresses(dma.next().unwrap());
        assert_eq!(
            pairs,
            [
                (0x0200_0010, 0x0300_0010),
                (0x0200_0010, 0x0300_000E),
                (0x0200_0010, 0x0300_000C)
            ]
        );
    }

    #[test]
    fn address_steps_for_words() {
        let dma = started(
            3,
            0x0200_0010,
            0x0300_0010,
            3,
            DEST_FIXED | SOURCE_DECREMENT | WORD,
        );
        let pairs = addresses(dma.next().unwrap());
        assert_eq!(
            pairs,
            [
                (0x0200_0010, 0x0300_0010),
                (0x0200_000C, 0x0300_0010),
                (0x0200_0008, 0x0300_0010)
            ]
        );
    }

    #[test]
    fn source_control_3_steps_like_increment() {
        let dma = started(3, 0x0200_0000, 0x0300_0000, 2, 3 << 7);
        let pairs = addresses(dma.next().unwrap());
        assert_eq!(
            pairs,
            [(0x0200_0000, 0x0300_0000), (0x0200_0002, 0x0300_0002)]
        );
    }

    #[test]
    fn addresses_align_to_the_unit_size() {
        let dma = started(3, 0x0200_0003, 0x0300_0007, 1, WORD);
        assert_eq!(addresses(dma.next().unwrap()), [(0x0200_0000, 0x0300_0004)]);
        let dma = started(3, 0x0200_0003, 0x0300_0007, 1, 0);
        assert_eq!(addresses(dma.next().unwrap()), [(0x0200_0002, 0x0300_0006)]);
    }

    #[test]
    fn repeat_continues_from_where_the_last_block_ended() {
        let mut dma = started(1, 0x0200_0000, 0x0300_0000, 2, HBLANK | REPEAT);
        for block in 0..3 {
            dma.trigger(Timing::HBlank);
            let transfer = dma.next().unwrap();
            assert_eq!(
                addresses(transfer)[0],
                (0x0200_0000 + 4 * block, 0x0300_0000 + 4 * block)
            );
            dma.finish(transfer);
            assert_ne!(dma.control(1) & ENABLE, 0);
        }
    }

    #[test]
    fn repeat_with_increment_reload_resets_only_the_destination() {
        let mut dma = started(
            1,
            0x0200_0000,
            0x0300_0000,
            2,
            HBLANK | REPEAT | DEST_RELOAD,
        );
        for block in 0..3 {
            dma.trigger(Timing::HBlank);
            let transfer = dma.next().unwrap();
            assert_eq!(
                addresses(transfer),
                [
                    (0x0200_0000 + 4 * block, 0x0300_0000),
                    (0x0200_0002 + 4 * block, 0x0300_0002)
                ]
            );
            dma.finish(transfer);
        }
    }

    #[test]
    fn repeat_reloads_the_count_register_as_written_now() {
        let mut dma = started(1, 0x0200_0000, 0x0300_0000, 2, VBLANK | REPEAT);
        dma.write(base(1) + COUNT, 7, 0xFFFF, 0);
        dma.trigger(Timing::VBlank);
        let transfer = dma.next().unwrap();
        assert_eq!(transfer.units(), 2);
        dma.finish(transfer);
        dma.trigger(Timing::VBlank);
        assert_eq!(dma.next().unwrap().units(), 7);
    }

    #[test]
    fn registers_latch_on_enable() {
        let mut dma = started(3, 0x0200_0000, 0x0300_0000, 2, 0);
        set_up(&mut dma, 3, 0x0200_1000, 0x0300_1000, 9);
        let transfer = dma.next().unwrap();
        assert_eq!(transfer.units(), 2);
        assert_eq!(addresses(transfer)[0], (0x0200_0000, 0x0300_0000));
    }

    #[test]
    fn rewriting_control_while_enabled_does_not_relatch() {
        let mut dma = started(1, 0x0200_0000, 0x0300_0000, 2, VBLANK | REPEAT);
        dma.trigger(Timing::VBlank);
        let transfer = dma.next().unwrap();
        dma.finish(transfer);
        set_up(&mut dma, 1, 0x0200_1000, 0x0300_1000, 2);
        enable(&mut dma, 1, VBLANK | REPEAT);
        dma.trigger(Timing::VBlank);
        assert_eq!(
            addresses(dma.next().unwrap())[0],
            (0x0200_0004, 0x0300_0004)
        );
    }

    #[test]
    fn irq_at_the_end_only_when_enabled() {
        let mut dma = started(2, 0x0200_0000, 0x0300_0000, 1, IRQ_ENABLE);
        let transfer = dma.next().unwrap();
        assert_eq!(dma.finish(transfer), Irq::DMA2);
        let mut dma = started(2, 0x0200_0000, 0x0300_0000, 1, 0);
        let transfer = dma.next().unwrap();
        assert_eq!(dma.finish(transfer), Irq::empty());
    }

    #[test]
    fn address_masks_per_channel() {
        let rom = 0x0800_0010;
        let dma = started(0, rom, rom, 1, 0);
        assert_eq!(addresses(dma.next().unwrap()), [(0x0000_0010, 0x0000_0010)]);
        for channel in [1, 2] {
            let dma = started(channel, rom, rom, 1, 0);
            assert_eq!(addresses(dma.next().unwrap()), [(rom, 0x0000_0010)]);
        }
        let dma = started(3, 0xF800_0010, 0xFD00_0010, 1, 0);
        assert_eq!(addresses(dma.next().unwrap()), [(rom, 0x0D00_0010)]);
    }

    #[test]
    fn addresses_wrap_inside_the_mask() {
        let dma = started(0, 0x07FF_FFFE, 0x0000_0000, 2, DEST_DECREMENT);
        assert_eq!(
            addresses(dma.next().unwrap()),
            [(0x07FF_FFFE, 0x0000_0000), (0x0000_0000, 0x07FF_FFFE)]
        );
    }

    #[test]
    fn lower_channel_goes_first() {
        let mut dma = Dma::default();
        for channel in [3, 1] {
            set_up(&mut dma, channel, 0x0200_0000, 0x0300_0000, 1);
            enable(&mut dma, channel, HBLANK);
        }
        dma.trigger(Timing::HBlank);
        let first = dma.next().unwrap();
        assert_eq!(first.channel(), 1);
        dma.finish(first);
        let second = dma.next().unwrap();
        assert_eq!(second.channel(), 3);
        dma.finish(second);
        assert_eq!(dma.next(), None);
    }

    #[test]
    fn cycles_are_two_n_plus_sequential_plus_two_internal() {
        let wait = WaitControl::default();
        let dma = started(3, 0x0300_0000, 0x0300_0100, 4, WORD);
        assert_eq!(dma.next().unwrap().cycles(wait), 2 + 4 + 4);
        let dma = started(3, 0x0200_0000, 0x0300_0000, 4, 0);
        assert_eq!(dma.next().unwrap().cycles(wait), 2 + 4 * 3 + 4);
        let dma = started(3, 0x0200_0000, 0x0300_0000, 4, WORD);
        assert_eq!(dma.next().unwrap().cycles(wait), 2 + 4 * 6 + 4);
    }

    #[test]
    fn rom_source_pays_first_and_second_access_waits() {
        let wait = WaitControl::default();
        let first = wait.cycles(0x0800_0000, Width::Half, Access::NonSequential);
        let second = wait.cycles(0x0800_0002, Width::Half, Access::Sequential);
        let dma = started(3, 0x0800_0000, 0x0300_0000, 3, 0);
        assert_eq!(dma.next().unwrap().cycles(wait), 2 + first + 2 * second + 3);
    }

    #[test]
    fn rom_to_rom_pays_four_internal_cycles() {
        let wait = WaitControl::default();
        let dma = started(3, 0x0800_0000, 0x0A00_0000, 1, 0);
        let access = wait.cycles(0x0800_0000, Width::Half, Access::NonSequential)
            + wait.cycles(0x0A00_0000, Width::Half, Access::NonSequential);
        assert_eq!(dma.next().unwrap().cycles(wait), 4 + access);
    }

    #[test]
    fn an_immediate_transfer_activates_two_cycles_after_its_enable_write() {
        let mut dma = Dma::default();
        set_up(&mut dma, 3, 0x0200_0000, 0x0300_0000, 1);
        dma.write(base(3) + CONTROL, ENABLE, 0xFFFF, 5);
        assert_eq!(dma.due(), Some(7));
        assert!(!dma.activate(6));
        assert!(dma.activate(7));
        assert_eq!(dma.due(), None);
    }

    #[test]
    fn a_blank_request_activates_two_cycles_later_across_steps() {
        let mut dma = Dma::default();
        set_up(&mut dma, 0, 0x0200_0000, 0x0300_0000, 1);
        dma.write(base(0) + CONTROL, ENABLE | HBLANK, 0xFFFF, 0);
        assert!(dma.waits_on_video());
        dma.request(Timing::HBlank, 10);
        dma.request(Timing::HBlank, 11);
        dma.elapse(11);
        assert_eq!(dma.due(), Some(1));
        assert!(dma.active());
        assert!(!dma.running());
        assert!(dma.activate(1));
    }

    #[test]
    fn disabling_a_scheduled_channel_drops_its_activation() {
        let mut dma = Dma::default();
        set_up(&mut dma, 3, 0x0200_0000, 0x0300_0000, 1);
        dma.write(base(3) + CONTROL, ENABLE, 0xFFFF, 0);
        dma.write(base(3) + CONTROL, 0, 0xFFFF, 1);
        assert!(!dma.active());
        assert!(!dma.activate(10));
    }

    fn read_cycles(transfer: Transfer, wait: WaitControl) -> Vec<u32> {
        transfer
            .addresses()
            .enumerate()
            .map(|(unit, (source, dest))| transfer.unit_cycles(wait, unit, source, dest).0)
            .collect()
    }

    fn rom_waits() -> WaitControl {
        let mut wait = WaitControl::default();
        wait.write(0x0014, 0xFFFF);
        wait
    }

    #[test]
    fn a_fixed_rom_source_at_a_page_end_reads_every_unit_nonsequential() {
        let wait = rom_waits();
        let dma = started(3, 0x0801_FFFC, 0x0300_0000, 3, WORD | SOURCE_FIXED);
        let n = wait.cycles(0x0801_FFFC, Width::Word, Access::NonSequential);
        assert_eq!(read_cycles(dma.next().unwrap(), wait), [n, n, n]);
    }

    #[test]
    fn an_incrementing_rom_source_reads_the_next_page_nonsequential() {
        let wait = rom_waits();
        let dma = started(3, 0x0801_FFF8, 0x0300_0000, 3, WORD);
        let n = wait.cycles(0x0801_FFF8, Width::Word, Access::NonSequential);
        let s = wait.cycles(0x0801_FFF8, Width::Word, Access::Sequential);
        assert_eq!(read_cycles(dma.next().unwrap(), wait), [n, s, n]);
    }

    #[test]
    fn a_decrementing_rom_source_reads_nonsequential_after_a_page_end() {
        let wait = rom_waits();
        let dma = started(3, 0x0802_0000, 0x0300_0000, 3, SOURCE_DECREMENT);
        let n = wait.cycles(0x0802_0000, Width::Half, Access::NonSequential);
        let s = wait.cycles(0x0801_FFFE, Width::Half, Access::Sequential);
        assert_eq!(read_cycles(dma.next().unwrap(), wait), [n, s, n]);
    }

    #[test]
    fn a_paused_channel_resumes_with_the_units_it_has_left() {
        let mut dma = started(3, 0x0200_0000, 0x0300_0000, 4, 0);
        dma.pause(dma.next().unwrap(), 3);
        assert!(dma.running());
        let transfer = dma.next().unwrap();
        assert_eq!(addresses(transfer), [(0x0200_0006, 0x0300_0006)]);
        dma.finish(transfer);
        assert!(!dma.running());
        assert_eq!(dma.control(3) & ENABLE, 0);
    }

    #[test]
    fn a_paused_sound_fifo_dma_moves_only_its_remaining_words() {
        let mut dma = started(1, 0x0200_0000, 0x0400_00A0, 0, SPECIAL | REPEAT | WORD);
        dma.trigger_special(1);
        dma.pause(dma.next().unwrap(), 1);
        let transfer = dma.next().unwrap();
        assert_eq!(transfer.units(), 3);
        dma.finish(transfer);
        dma.trigger_special(1);
        assert_eq!(dma.next().unwrap().units(), 4);
    }

    #[test]
    fn steady_regions_and_cycles_hold_for_every_unit() {
        let mut seed = 0x2545_F491u32;
        let mut next = move |below: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % below
        };
        let edges = [0, 0x400, 0x4000, 0x1_0000, 0x100_0000];
        let pick = |next: &mut dyn FnMut(u32) -> u32| {
            let top = next(0x20) << 24;
            let edge = edges[next(edges.len() as u32) as usize];
            top.wrapping_add(edge)
                .wrapping_add(next(0x200))
                .wrapping_sub(0x100)
        };
        let mut kinds = [0; 2];
        for _ in 0..4000 {
            let channel = next(CHANNELS as u32) as usize;
            let (source, dest) = (pick(&mut next), pick(&mut next));
            let count = 1 + next(0x180) as u16;
            let control =
                (next(4) << 5 | next(4) << 7) as u16 | if next(2) == 1 { WORD } else { 0 };
            let mut wait = WaitControl::default();
            wait.write(next(0x1_0000) as u16, 0xFFFF);
            let dma = started(channel, source, dest, count, control);
            let transfer = dma.next().unwrap();
            let Some(regions) = transfer.steady_regions() else {
                kinds[0] += 1;
                continue;
            };
            kinds[1] += 1;
            let cycles = transfer.steady_unit_cycles(wait, regions);
            for (unit, (source, dest)) in transfer.addresses().enumerate() {
                assert_eq!(
                    (Region::of(source), Region::of(dest)),
                    regions,
                    "{transfer:?}"
                );
                if let Some(cycles) = cycles {
                    assert_eq!(
                        transfer.unit_cycles(wait, unit, source, dest),
                        cycles,
                        "{transfer:?}"
                    );
                }
            }
        }
        assert!(kinds.iter().all(|&kind| kind > 100), "{kinds:?}");
    }
}
