use super::{Bus, EEPROM_DMA, read_le};
use crate::gba::dma::{ACTIVATION_DELAY, IDLE_AFTER, IDLE_BEFORE, Transfer};
use crate::gba::open_bus::lane;
use crate::gba::region::Region;
use crate::gba::timer::Overflows;
use crate::gba::wait::{Access, Width};

const TIMERS: std::ops::RangeInclusive<u32> = 0x0400_0100..=0x0400_010F;
const VIDEO: std::ops::RangeInclusive<u32> = 0x0400_0000..=0x0400_0057;
const INTERRUPTS: std::ops::RangeInclusive<u32> = 0x0400_0200..=0x0400_020B;
const SERIAL: std::ops::RangeInclusive<u32> = 0x0400_0120..=0x0400_012F;
const OBJ_VRAM: u32 = 0x1_0000;
const BITMAP_OBJ_VRAM: u32 = 0x4000;

impl Bus {
    pub(crate) fn fetch_code(&mut self, addr: u32, width: Width) -> u32 {
        self.open_bus.fetched_at(addr);
        self.read_on_bus(addr, width)
    }

    fn read_on_bus(&mut self, addr: u32, width: Width) -> u32 {
        match Region::of(addr) {
            Region::Unmapped => lane(self.open_bus.value(), addr, width),
            Region::Bios => {
                if self.open_bus.in_bios() {
                    let word = read_le(&self.bios, addr as usize & !3, Width::Word);
                    self.open_bus.latch_bios(word);
                }
                let word = self.open_bus.bios();
                self.open_bus.put(word);
                lane(word, addr, width)
            }
            Region::Oam => {
                let value = self.read(addr, width);
                self.open_bus.put(self.read(addr & !3, Width::Word));
                value
            }
            _ => {
                let value = self.read(addr, width);
                self.open_bus.read(addr, width, value);
                value
            }
        }
    }

    #[inline(always)]
    pub(crate) fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        if self.clock.now >= self.dma_check {
            return self.code_cycles_beside_dma(addr, width, access);
        }
        self.clock.sample = self.clock.now + 1;
        let cycles = self.prefetch.fetch(self.wait, addr, width, access);
        self.clock.now += cycles;
        cycles
    }

    #[cold]
    #[inline(never)]
    fn code_cycles_beside_dma(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let access = self.dma_before_access(access);
        self.clock.sample = self.clock.now + 1;
        let cycles = self.prefetch.fetch(self.wait, addr, width, access);
        self.pass_unpaid(cycles)
    }

    #[inline(always)]
    pub(crate) fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        if self.clock.now >= self.dma_check {
            return self.data_cycles_beside_dma(addr, width, access);
        }
        let cycles = self.data_wait(addr, width, access);
        self.clock.now += cycles;
        cycles
    }

    #[cold]
    #[inline(never)]
    fn data_cycles_beside_dma(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let access = self.dma_before_access(access);
        let cycles = self.data_wait(addr, width, access);
        self.pass_unpaid(cycles)
    }

    #[inline(always)]
    fn data_wait(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.clock.sample = self.clock.now + 1;
        let mut cycles = self.wait.cycles(addr, width, access);
        match Region::of(addr) {
            Region::Rom(_) | Region::Sram => cycles += self.prefetch.interrupt(self.wait),
            Region::Vram if addr & OBJ_VRAM != 0 => {
                cycles = self.obj_vram_cycles(addr, width, cycles, self.clock.now);
                self.prefetch.run(self.wait, cycles)
            }
            Region::Oam => {
                cycles = self.oam_cycles(cycles, self.clock.now);
                self.prefetch.run(self.wait, cycles)
            }
            _ => self.prefetch.run(self.wait, cycles),
        }
        cycles
    }

    fn pass_unpaid(&mut self, cycles: u32) -> u32 {
        let paid = cycles.min(self.prepaid);
        self.prepaid -= paid;
        if self.prepaid > 0 {
            self.dma_check = 0;
        }
        self.clock.now += cycles - paid;
        cycles - paid
    }

    pub(crate) fn woke_before(&mut self, lead: u32) -> bool {
        self.catch_up_video();
        self.catch_up_timers();
        self.interrupts
            .wakes_at(self.clock.now.saturating_sub(lead))
    }

    pub(crate) fn resume_held(&mut self, idle: u32, prepaid: u32) {
        self.clock.now += idle;
        self.prepaid = prepaid;
        self.dma_check = 0;
    }

    pub(crate) fn end_held(&mut self) -> u32 {
        std::mem::take(&mut self.prepaid)
    }

    pub(crate) fn step_cycles(&self) -> u32 {
        self.clock.now
    }

    #[cold]
    #[inline(never)]
    fn obj_vram_cycles(&mut self, addr: u32, width: Width, cycles: u32, start: u32) -> u32 {
        if addr & BITMAP_OBJ_VRAM == 0 && self.ppu.bitmap_mode() {
            return cycles;
        }
        let ahead = start.saturating_sub(self.clock.video);
        self.ppu
            .obj_vram_stall(&self.oam, ahead, cycles, width == Width::Word)
    }

    #[cold]
    #[inline(never)]
    fn oam_cycles(&mut self, cycles: u32, start: u32) -> u32 {
        let ahead = start.saturating_sub(self.clock.video);
        self.ppu.oam_stall(&self.oam, ahead, cycles)
    }

    #[cold]
    #[inline(never)]
    fn dma_before_access(&mut self, access: Access) -> Access {
        self.stall_rest_of_dma();
        let start = self.clock.now;
        let mut dma = self.run_due_dma();
        while dma > 0 && self.next_dma_check() <= self.clock.now {
            let more = self.run_due_dma();
            if more == 0 {
                break;
            }
            dma += more;
        }
        self.interrupts.stall(start, start + dma);
        self.clock.dma += dma;
        if std::mem::take(&mut self.after_dma) {
            Access::NonSequential
        } else {
            access
        }
    }

    #[inline(always)]
    pub(super) fn stall_rest_of_dma(&mut self) {
        if self.clock.parallel > 0 {
            let rest = std::mem::take(&mut self.clock.parallel);
            self.interrupts.stall(self.clock.now - rest, self.clock.now);
        }
    }

    pub(crate) fn branch(&mut self) {
        self.prefetch.branch();
    }

    #[inline(always)]
    pub(crate) fn cpu_idle(&mut self) {
        if self.clock.now >= self.dma_check {
            self.idle_beside_dma();
            return;
        }
        self.prefetch.run(self.wait, 1);
        self.clock.now += 1;
        self.clock.sample = self.clock.now;
    }

    #[cold]
    #[inline(never)]
    fn idle_beside_dma(&mut self) {
        let dma = self.run_due_dma();
        if dma > 0 {
            self.clock.parallel = dma - 1;
            self.clock.dma += dma - 1;
            self.dma_check = 0;
            self.clock.sample = self.clock.now;
            return;
        }
        if self.clock.parallel > 0 {
            self.clock.parallel -= 1;
            self.clock.dma -= 1;
            self.dma_check = 0;
            self.clock.sample = self.clock.now;
            return;
        }
        if self.prepaid > 0 {
            self.pass_unpaid(1);
            return;
        }
        self.prefetch.run(self.wait, 1);
        self.clock.now += 1;
        self.clock.sample = self.clock.now;
    }

    pub(crate) fn open_bus_value(&self) -> u32 {
        self.open_bus.value()
    }

    pub(crate) fn drive_open_bus(&mut self, value: u32) {
        self.open_bus.put(value);
    }

    pub(crate) fn dma_cycles(&self) -> u32 {
        self.clock.dma
    }

    pub(crate) fn cpu_read(&mut self, addr: u32, width: Width) -> u32 {
        #[cfg(feature = "debug")]
        if let Some(word) = self.debug_print.read(addr) {
            return lane(word, addr, width);
        }
        if TIMERS.contains(&addr) || INTERRUPTS.contains(&addr) {
            self.catch_up_timers();
        }
        if VIDEO.contains(&addr) || INTERRUPTS.contains(&addr) {
            self.catch_up_video();
        }
        if SERIAL.contains(&addr) {
            self.catch_up_serial();
        }
        self.read_on_bus(addr, width)
    }

    pub(crate) fn cpu_write(&mut self, addr: u32, width: Width, value: u32) {
        if TIMERS.contains(&addr) || INTERRUPTS.contains(&addr) {
            self.catch_up_timers();
        }
        if VIDEO.contains(&addr) || INTERRUPTS.contains(&addr) {
            self.catch_up_video();
        }
        if SERIAL.contains(&addr) {
            self.catch_up_serial();
        }
        self.write(addr, width, value);
        self.open_bus.wrote(addr, width, value);
    }

    pub fn run_dma(&mut self) -> u32 {
        let start = self.clock.now;
        if !self.dma.running()
            && let Some(due) = self.dma.due()
        {
            self.clock.now = self.clock.now.max(due);
        }
        self.catch_up_video();
        if self.dma.activate(self.clock.now) {
            self.transfer_dma();
        }
        self.dma_check = 0;
        self.clock.now - start
    }

    fn run_due_dma(&mut self) -> u32 {
        if self.fifo_dma_armed() {
            self.catch_up_timers();
        }
        self.catch_up_video();
        let cycles = if self.dma.activate(self.clock.now) {
            self.transfer_dma()
        } else {
            0
        };
        self.dma_check = if self.after_dma {
            0
        } else {
            self.next_dma_check()
        };
        cycles
    }

    fn next_dma_check(&self) -> u32 {
        let due = self
            .dma
            .due()
            .unwrap_or(u32::MAX)
            .min(self.next_fifo_check());
        if self.dma.waits_on_video() {
            let event = self.clock.video + self.ppu.until_dma_event() + ACTIVATION_DELAY;
            due.min(event)
        } else {
            due
        }
    }

    fn next_fifo_check(&self) -> u32 {
        if !self.fifo_dma_armed() {
            return u32::MAX;
        }
        self.timers
            .until_overflow()
            .map_or(u32::MAX, |until| self.clock.timers + until)
    }

    fn transfer_dma(&mut self) -> u32 {
        let start = self.clock.now;
        let mut now = start + IDLE_BEFORE;
        self.prefetch.run(self.wait, IDLE_BEFORE);
        let mut check = self.next_dma_check();
        while let Some(transfer) = self.dma.next() {
            if !transfer.resumed()
                && transfer.channel() == EEPROM_DMA
                && transfer
                    .addresses()
                    .next()
                    .is_some_and(|(_, dest)| self.in_eeprom_window(dest))
            {
                self.backup.eeprom_dma(transfer.units());
            }
            let width = transfer.width();
            let mut moved = 0;
            for (unit, (source, dest)) in transfer.addresses().enumerate() {
                let (mut read_cycles, mut write_cycles) =
                    transfer.unit_cycles(self.wait, unit, source, dest);
                let on_game_pak = on_game_pak(source) || on_game_pak(dest);
                if on_game_pak {
                    read_cycles += self.prefetch.interrupt(self.wait);
                }
                if in_obj_vram(source) {
                    read_cycles = self.obj_vram_cycles(source, width, read_cycles, now);
                } else if Region::of(source) == Region::Oam {
                    read_cycles = self.oam_cycles(read_cycles, now);
                }
                now += read_cycles;
                self.sync_timers_at(now, source);
                let read = match Region::of(source) {
                    Region::Unmapped if source >> 24 == 0x04 => {
                        Some(lane(self.open_bus.value(), source, width))
                    }
                    Region::Bios | Region::Unmapped => None,
                    _ => Some(self.read(source, width)),
                };
                let channel = transfer.channel();
                let value = self.dma.latch(channel, read, width, dest);
                let seen =
                    read.unwrap_or_else(|| lane(self.dma.latch_word(channel), source, width));
                self.open_bus.dma_read(source, width, seen);
                if in_obj_vram(dest) {
                    write_cycles = self.obj_vram_cycles(dest, width, write_cycles, now);
                } else if Region::of(dest) == Region::Oam {
                    write_cycles = self.oam_cycles(write_cycles, now);
                }
                now += write_cycles;
                if !on_game_pak {
                    self.prefetch.run(self.wait, read_cycles + write_cycles);
                }
                self.sync_timers_at(now, dest);
                if Region::of(dest) == Region::Oam {
                    self.clock.now = now;
                }
                self.write(dest, width, value);
                self.open_bus.wrote(dest, width, value);
                moved += 1;
                if now >= check {
                    self.clock.now = now;
                    self.catch_up_video();
                    self.dma.activate(now);
                    check = self.next_dma_check();
                    if self.taken_over(transfer) {
                        break;
                    }
                }
            }
            if moved < transfer.units() {
                self.dma.pause(transfer, moved);
                continue;
            }
            now += transfer.gamepak_idle();
            let irq = self.dma.finish(transfer);
            self.interrupts.raise(irq, now + IDLE_AFTER);
            self.dma.activate(now);
        }
        now += IDLE_AFTER;
        self.prefetch.run(self.wait, IDLE_AFTER);
        self.clock.now = now;
        self.after_dma = true;
        now - start
    }

    fn taken_over(&self, transfer: Transfer) -> bool {
        self.dma
            .next()
            .is_some_and(|next| next.channel() < transfer.channel())
    }

    pub(crate) fn start_cpu_step(&mut self) {
        self.clock = Clock::default();
    }

    pub(super) fn sync_timers_at(&mut self, now: u32, addr: u32) {
        if TIMERS.contains(&addr) {
            self.clock.now = now;
            self.catch_up_timers();
        }
    }

    pub(super) fn take_irq_lead(&mut self) -> u32 {
        std::mem::take(&mut self.clock.lead)
    }

    pub(super) fn take_video_start(&mut self) -> u32 {
        std::mem::take(&mut self.clock.video)
    }

    fn catch_up_video(&mut self) {
        let start = self.clock.video;
        if self.clock.now > start {
            self.run_video(start, self.clock.now - start);
            self.clock.video = self.clock.now;
        }
    }

    fn catch_up_serial(&mut self) {
        let start = self.clock.serial;
        let (irq, at) = self.serial.step_at(self.clock.now - start);
        self.clock.serial = self.clock.now;
        self.interrupts.raise(irq, start + at);
    }

    pub(super) fn take_serial_start(&mut self) -> u32 {
        std::mem::take(&mut self.clock.serial)
    }

    fn catch_up_timers(&mut self) {
        let start = self.clock.timers;
        let behind = self.clock.now - start;
        self.clock.timers = self.clock.now;
        let overflows = self.timers.step(behind);
        self.interrupts
            .raise(overflows.irq(), start + overflows.irq_at());
        let overflows = overflows.starting_at(start);
        self.tick_fifos(overflows);
        self.clock.overflows.add(overflows);
    }

    pub(crate) fn step_timers(&mut self, cycles: u32) {
        self.timers_step(cycles);
    }

    pub(super) fn timers_step(&mut self, cycles: u32) -> Overflows {
        let mut overflows = self.clock.overflows;
        overflows.add(self.timers_step_last(cycles));
        overflows
    }

    #[inline(always)]
    pub(super) fn timers_step_last(&mut self, cycles: u32) -> Overflows {
        let start = self.clock.timers;
        let last = self
            .timers
            .step(cycles.saturating_sub(start))
            .starting_at(start);
        self.interrupts.raise(last.irq(), start + last.irq_at());
        self.clock = Clock {
            video: self.clock.video,
            lead: self.clock.lead(),
            ..Clock::default()
        };
        last
    }
}

#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(super) struct Clock {
    now: u32,
    timers: u32,
    video: u32,
    overflows: Overflows,
    #[cfg_attr(feature = "serde", serde(default))]
    parallel: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    dma: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    serial: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    sample: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    lead: u32,
}

impl Clock {
    pub(super) fn now(&self) -> u32 {
        self.now
    }

    fn lead(&self) -> u32 {
        match self.sample {
            0 => self.lead,
            sample => self.now.saturating_sub(sample),
        }
    }

    pub(super) fn before_now_since_video(&self) -> u32 {
        self.now.saturating_sub(self.video + 1)
    }

    pub(super) fn since_serial(&self) -> u32 {
        self.now.saturating_sub(self.serial)
    }
}

fn in_obj_vram(addr: u32) -> bool {
    Region::of(addr) == Region::Vram && addr & OBJ_VRAM != 0
}

fn on_game_pak(addr: u32) -> bool {
    matches!(Region::of(addr), Region::Rom(_) | Region::Sram)
}
