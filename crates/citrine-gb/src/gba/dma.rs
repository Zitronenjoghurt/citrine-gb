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
        let first = unit == 0;
        (
            wait.cycles(
                source,
                self.width,
                self.access(first, source, self.source_step),
            ),
            wait.cycles(dest, self.width, self.access(first, dest, self.dest_step)),
        )
    }

    fn access(self, first: bool, addr: u32, step: i32) -> Access {
        let page_end = ROM_PAGE_END & !(self.width.bytes() as u32 - 1);
        let after_page_end = matches!(Region::of(addr), Region::Rom(_))
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
mod tests;
