use super::{Bus, EEPROM_DMA, read_le, write_le};
use crate::gba::cart::Cart;
use crate::gba::dma::{ACTIVATION_DELAY, IDLE_AFTER, IDLE_BEFORE, Transfer};
use crate::gba::open_bus::lane;
use crate::gba::region::{EWRAM_SIZE, IWRAM_SIZE, Region, WaitState};
use crate::gba::timer::Overflows;
use crate::gba::wait::{Access, EWRAM_CYCLES, Width};

const TIMERS: std::ops::RangeInclusive<u32> = 0x0400_0100..=0x0400_010F;
const VIDEO: std::ops::RangeInclusive<u32> = 0x0400_0000..=0x0400_0057;
const INTERRUPTS: std::ops::RangeInclusive<u32> = 0x0400_0200..=0x0400_020B;
const SERIAL: std::ops::RangeInclusive<u32> = 0x0400_0120..=0x0400_012F;
const SOUND: std::ops::RangeInclusive<u32> = 0x0400_0060..=0x0400_00AF;
const OBJ_VRAM: u32 = 0x1_0000;
const BITMAP_OBJ_VRAM: u32 = 0x4000;
const IO_PAGE: u32 = 0x04;
const EWRAM_PAGE: u32 = 0x02;
pub(super) const IWRAM_PAGE: u32 = 0x03;
pub(super) const PLAIN_ROM_START: u32 = 0x0800_0000;
pub(super) const PLAIN_ROM_SPAN: u32 = 0x0500_0000;
pub(super) const ROM_MIRROR_MASK: u32 = 0x01FF_FFFF;

impl Bus {
    pub(crate) fn fetch_code(&mut self, addr: u32, width: Width) -> u32 {
        self.open_bus.fetched_at(addr);
        self.read_on_bus(addr, width)
    }

    #[inline(always)]
    fn read_on_bus(&mut self, addr: u32, width: Width) -> u32 {
        let region = Region::of(addr);
        if let Region::Ewram | Region::Iwram = region {
            return self.read_ram(region, addr, width);
        }
        if let Region::Bios = region {
            return self.read_bios(addr, width);
        }
        if let Region::Rom(_) = region
            && !self.in_eeprom_window(addr)
            && let Some(value) = self.plain_rom(region.offset(addr) & !(width.bytes() - 1), width)
        {
            self.open_bus.read(region, addr, width, value);
            return value;
        }
        self.read_off_rom(region, addr, width)
    }

    #[inline(always)]
    fn read_ram(&mut self, region: Region, addr: u32, width: Width) -> u32 {
        let aligned = region.offset(addr) & !(width.bytes() - 1);
        let ram: &[u8] = if matches!(region, Region::Ewram) {
            &self.ewram
        } else {
            &self.iwram
        };
        let value = read_le(ram, aligned, width);
        self.open_bus.read(region, addr, width, value);
        value
    }

    #[inline(always)]
    fn read_bios(&mut self, addr: u32, width: Width) -> u32 {
        if self.open_bus.in_bios() {
            let word = read_le(&self.bios, addr as usize & !3, Width::Word);
            self.open_bus.latch_bios(word);
        }
        let word = self.open_bus.bios();
        self.open_bus.put(word);
        lane(word, addr, width)
    }

    fn read_off_rom(&mut self, region: Region, addr: u32, width: Width) -> u32 {
        match region {
            Region::Unmapped => lane(self.open_bus.value(), addr, width),
            Region::Bios => self.read_bios(addr, width),
            Region::Oam => {
                let value = self.read(addr, width);
                self.open_bus.put(self.read(addr & !3, Width::Word));
                value
            }
            Region::Io => {
                self.catch_up_to_step_start();
                let value = self.read_in(region, addr, width);
                self.open_bus.read(region, addr, width, value);
                value
            }
            _ => {
                let value = self.read_in(region, addr, width);
                self.open_bus.read(region, addr, width, value);
                value
            }
        }
    }

    #[inline(always)]
    pub(crate) fn timed_code(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let aligned = addr & !(width.bytes() as u32 - 1);
        if self.clock.now < self.dma_check {
            if let Some(fetched) = self.rom_code(addr, aligned, width, access) {
                return fetched;
            }
            if aligned >> 24 == IWRAM_PAGE {
                return self.iwram_code(aligned, width);
            }
        }
        let cycles = self.code_cycles(addr, width, access);
        (cycles, self.fetch_code(aligned, width))
    }

    #[inline(always)]
    fn rom_code(
        &mut self,
        addr: u32,
        aligned: u32,
        width: Width,
        access: Access,
    ) -> Option<(u32, u32)> {
        if aligned.wrapping_sub(PLAIN_ROM_START) >= PLAIN_ROM_SPAN {
            return None;
        }
        let offset = (aligned & ROM_MIRROR_MASK) as usize;
        if !Cart::in_plain_rom(offset, self.plain_rom_end) {
            return None;
        }
        let halves = if width == Width::Word { 2 } else { 1 };
        let cycles = match self.prefetch.fetch_ahead(aligned, halves) {
            Some(cycles) => cycles,
            None => self
                .prefetch
                .fetch_elsewhere(self.wait, addr, aligned, width, access)?,
        };
        self.clock.sample = self.clock.now + 1;
        self.clock.now += cycles;
        let value = read_le(&self.rom, offset, width);
        self.open_bus.fetched_at(aligned);
        self.open_bus
            .read(Region::Rom(WaitState::Zero), aligned, width, value);
        Some((cycles, value))
    }

    #[inline(always)]
    fn iwram_code(&mut self, addr: u32, width: Width) -> (u32, u32) {
        self.clock.sample = self.clock.now + 1;
        self.prefetch.fetch_in_one(self.wait);
        self.clock.now += 1;
        let value = read_le(&self.iwram, addr as usize % IWRAM_SIZE, width);
        self.open_bus.fetched_at(addr);
        self.open_bus.read(Region::Iwram, addr, width, value);
        (1, value)
    }

    #[cfg(any(jit, test))]
    #[inline(always)]
    pub(crate) fn iwram_run(
        &mut self,
        addr: u32,
        width: Width,
        count: u32,
        cycles: u32,
        room: u32,
    ) -> bool {
        let step = width.bytes() as u32;
        let end = addr.wrapping_add((count - 1) * step);
        if addr >> 24 != IWRAM_PAGE
            || end >> 24 != IWRAM_PAGE
            || cycles < count
            || cycles >= room
            || self.clock.now.saturating_add(cycles) > self.dma_check
        {
            return false;
        }
        if self.wait.prefetch() {
            self.prefetch.run(cycles - 1);
        } else {
            self.prefetch.run(cycles - count);
        }
        self.prefetch.fetch_in_one(self.wait);
        self.clock.sample = self.clock.now + cycles;
        self.clock.now += cycles;
        for index in count.saturating_sub(2)..count {
            let at = addr.wrapping_add(index * step);
            let value = read_le(&self.iwram, at as usize % IWRAM_SIZE, width);
            self.open_bus.read(Region::Iwram, at, width, value);
        }
        self.open_bus.fetched_at(end);
        true
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
        let cycles = match addr >> 24 {
            IWRAM_PAGE => 1,
            EWRAM_PAGE if width == Width::Word => 2 * EWRAM_CYCLES,
            EWRAM_PAGE => EWRAM_CYCLES,
            _ => return self.data_cycles_off_ram(addr, width, access),
        };
        self.clock.sample = self.clock.now + 1;
        self.prefetch.run(cycles);
        self.clock.now += cycles;
        cycles
    }

    #[inline(always)]
    fn data_cycles_off_ram(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        let cycles = self.data_wait(addr, width, access);
        self.clock.now += cycles;
        cycles
    }

    #[inline(always)]
    pub(crate) fn timed_data(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        if self.clock.now < self.dma_check {
            match addr >> 24 {
                IWRAM_PAGE => return self.iwram_data(addr, width),
                EWRAM_PAGE => return self.ewram_data(addr, width),
                _ => {}
            }
        }
        let cycles = self.data_cycles(addr, width, access);
        (cycles, self.cpu_read(addr, width))
    }

    #[inline(always)]
    fn iwram_data(&mut self, addr: u32, width: Width) -> (u32, u32) {
        self.clock.sample = self.clock.now + 1;
        self.prefetch.run(1);
        self.clock.now += 1;
        let offset = (addr as usize % IWRAM_SIZE) & !(width.bytes() - 1);
        let value = read_le(&self.iwram, offset, width);
        self.open_bus.read(Region::Iwram, addr, width, value);
        (1, value)
    }

    #[inline(always)]
    fn ewram_data(&mut self, addr: u32, width: Width) -> (u32, u32) {
        self.clock.sample = self.clock.now + 1;
        let cycles = if width == Width::Word {
            2 * EWRAM_CYCLES
        } else {
            EWRAM_CYCLES
        };
        self.prefetch.run(cycles);
        self.clock.now += cycles;
        let offset = (addr as usize % EWRAM_SIZE) & !(width.bytes() - 1);
        let value = read_le(&self.ewram, offset, width);
        self.open_bus.read(Region::Ewram, addr, width, value);
        (cycles, value)
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
        let region = Region::of(addr);
        let mut cycles = self.wait.region_cycles(region, addr, width, access);
        match region {
            Region::Rom(_) | Region::Sram => cycles += self.prefetch.interrupt(),
            Region::Vram if addr & OBJ_VRAM != 0 => {
                cycles = self.obj_vram_cycles(addr, width, cycles, self.clock.now);
                self.prefetch.run(cycles)
            }
            Region::Oam => {
                cycles = self.oam_cycles(cycles, self.clock.now);
                self.prefetch.run(cycles)
            }
            _ => self.prefetch.run(cycles),
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
        self.clock.touched = true;
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
        self.prefetch.run(1);
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
            self.clock.touched = true;
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
        self.prefetch.run(1);
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
        if addr >> 24 == IO_PAGE {
            self.catch_up_io(addr);
        }
        self.read_on_bus(addr, width)
    }

    pub(crate) fn cpu_write(&mut self, addr: u32, width: Width, value: u32) {
        match addr >> 24 {
            IWRAM_PAGE => return self.write_ram(Region::Iwram, addr, width, value),
            EWRAM_PAGE => return self.write_ram(Region::Ewram, addr, width, value),
            IO_PAGE => self.catch_up_io(addr),
            _ => {}
        }
        self.write(addr, width, value);
        self.open_bus.wrote(Region::of(addr), addr, width, value);
    }

    #[inline(always)]
    fn write_ram(&mut self, region: Region, addr: u32, width: Width, value: u32) {
        let aligned = region.offset(addr) & !(width.bytes() - 1);
        let ram = if matches!(region, Region::Ewram) {
            &mut self.ewram
        } else {
            &mut self.iwram
        };
        write_le(ram.touch(aligned), aligned, width, value);
        #[cfg(jit)]
        self.code.written(region, aligned);
        self.open_bus.wrote(region, addr, width, value);
    }

    fn catch_up_io(&mut self, addr: u32) {
        self.catch_up_to_step_start();
        if TIMERS.contains(&addr) || INTERRUPTS.contains(&addr) || SOUND.contains(&addr) {
            self.catch_up_timers();
        }
        if VIDEO.contains(&addr) || INTERRUPTS.contains(&addr) {
            self.catch_up_video();
        }
        if SERIAL.contains(&addr) {
            self.catch_up_serial();
        }
        if SOUND.contains(&addr) {
            self.sound.step_to(self.clock.now);
        }
    }

    pub fn run_dma(&mut self) -> u32 {
        self.catch_up_to_step_start();
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
        self.catch_up_to_step_start();
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
        self.prefetch.run(IDLE_BEFORE);
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
            let steady = transfer.steady_regions();
            let steady_cycles =
                steady.and_then(|regions| transfer.steady_unit_cycles(self.wait, regions));
            let mut moved = 0;
            for (unit, (source, dest)) in transfer.addresses().enumerate() {
                let (from, to) = steady.unwrap_or_else(|| (Region::of(source), Region::of(dest)));
                let (mut read_cycles, mut write_cycles) = steady_cycles.unwrap_or_else(|| {
                    transfer.unit_cycles_in(self.wait, unit, (from, source), (to, dest))
                });
                let on_game_pak = steady_cycles.is_none() && (on_game_pak(from) || on_game_pak(to));
                if on_game_pak {
                    read_cycles += self.prefetch.interrupt();
                }
                if in_obj_vram(from, source) {
                    read_cycles = self.obj_vram_cycles(source, width, read_cycles, now);
                } else if matches!(from, Region::Oam) {
                    read_cycles = self.oam_cycles(read_cycles, now);
                }
                now += read_cycles;
                self.sync_timers_at(now, source);
                let read = match from {
                    Region::Unmapped if source >> 24 == 0x04 => {
                        Some(lane(self.open_bus.value(), source, width))
                    }
                    Region::Bios | Region::Unmapped => None,
                    _ => Some(self.read_in(from, source, width)),
                };
                let channel = transfer.channel();
                let value = self.dma.latch(channel, read, width, dest);
                let seen =
                    read.unwrap_or_else(|| lane(self.dma.latch_word(channel), source, width));
                self.open_bus.dma_read(from, source, width, seen);
                if in_obj_vram(to, dest) {
                    write_cycles = self.obj_vram_cycles(dest, width, write_cycles, now);
                } else if matches!(to, Region::Oam) {
                    write_cycles = self.oam_cycles(write_cycles, now);
                }
                now += write_cycles;
                if !on_game_pak {
                    self.prefetch.run(read_cycles + write_cycles);
                }
                self.sync_timers_at(now, dest);
                if matches!(to, Region::Oam) {
                    self.clock.now = now;
                }
                self.write(dest, width, value);
                self.open_bus.wrote(to, dest, width, value);
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
        self.prefetch.run(IDLE_AFTER);
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
        if self.clock.touched {
            self.clock = Clock {
                now: self.owed,
                start: self.owed,
                ..Clock::default()
            };
        } else {
            self.clock.now = self.owed;
            self.clock.start = self.owed;
            self.clock.sample = 0;
        }
    }

    #[inline(always)]
    pub(crate) fn finish_cpu_step(&mut self, cycles: u32) -> bool {
        let owed = self.owed + cycles;
        if owed < self.quiet {
            self.owed = owed;
            return false;
        }
        self.finish_quiet_span(owed);
        true
    }

    #[inline(never)]
    fn finish_quiet_span(&mut self, cycles: u32) {
        self.owed = 0;
        self.step(cycles);
        self.step_video(cycles);
        self.quiet = self.quiet_span();
    }

    pub(crate) fn step_devices(&mut self, cycles: u32) {
        let cycles = std::mem::take(&mut self.owed) + cycles;
        self.step(cycles);
        self.step_video(cycles);
        self.quiet = 0;
    }

    pub(crate) fn catch_up_devices(&mut self) {
        if self.owed > 0 {
            self.step_devices(0);
        }
    }

    pub(crate) fn settle_this_step(&mut self) {
        self.quiet = 0;
    }

    #[inline(always)]
    pub(super) fn catch_up_to_step_start(&mut self) {
        self.quiet = 0;
        if self.clock.start > 0 {
            self.settle_step_start();
        }
    }

    #[cold]
    #[inline(never)]
    fn settle_step_start(&mut self) {
        let start = std::mem::take(&mut self.clock.start);
        let now = std::mem::replace(&mut self.clock.now, start);
        self.catch_up_timers();
        self.catch_up_video();
        self.catch_up_serial();
        self.sound.step_to(start);
        self.cart.tick(start);
        self.clock.cart = start;
        self.clock.touched = true;
        self.clock.now = now;
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
            self.clock.touched = true;
        }
    }

    fn catch_up_serial(&mut self) {
        let start = self.clock.serial;
        let (irq, at) = self.serial.step_at(self.clock.now - start);
        self.clock.serial = self.clock.now;
        self.clock.touched = true;
        self.interrupts.raise(irq, start + at);
    }

    pub(super) fn take_serial_start(&mut self) -> u32 {
        std::mem::take(&mut self.clock.serial)
    }

    fn catch_up_timers(&mut self) {
        let start = self.clock.timers;
        let behind = self.clock.now - start;
        self.clock.timers = self.clock.now;
        self.clock.touched = true;
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
            touched: true,
            ..Clock::default()
        };
        last
    }
}

#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(super) struct Clock {
    pub(super) now: u32,
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
    pub(super) sample: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    lead: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    start: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    cart: u32,
    #[cfg_attr(feature = "serde", serde(skip, default = "Clock::loaded"))]
    touched: bool,
}

impl Clock {
    #[cfg(feature = "serde")]
    fn loaded() -> bool {
        true
    }

    pub(super) fn now(&self) -> u32 {
        self.now
    }

    pub(super) fn cart(&self) -> u32 {
        self.cart
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

fn in_obj_vram(region: Region, addr: u32) -> bool {
    matches!(region, Region::Vram) && addr & OBJ_VRAM != 0
}

fn on_game_pak(region: Region) -> bool {
    matches!(region, Region::Rom(_) | Region::Sram)
}
