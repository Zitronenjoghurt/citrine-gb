use super::super::lanes;
use super::Arm7Bus;
use crate::gba::open_bus::lane;
use crate::gba::wait::Width;
use crate::nds::Cpu;
use crate::nds::bus::{Arm7Memory, Shared};
use crate::nds::interrupt::Irq;
use crate::nds::sound::mixer;

const DISPSTAT: u32 = 0x004;
const DMA_FIRST: u32 = 0x0B0;
const DMA_LAST: u32 = 0x0DC;
const TIMER_FIRST: u32 = 0x100;
const TIMER_LAST: u32 = 0x10C;
const KEYINPUT: u32 = 0x130;
const RCNT: u32 = 0x134;
const RTC: u32 = 0x138;
const IPC_FIRST: u32 = 0x180;
const IPC_LAST: u32 = 0x188;
const CART_FIRST: u32 = 0x1A0;
const CART_LAST: u32 = 0x1B8;
const SPI: u32 = 0x1C0;
const EXMEMSTAT: u32 = 0x204;
const IRQ_FIRST: u32 = 0x208;
const IRQ_LAST: u32 = 0x214;
const VRAMSTAT: u32 = 0x240;
const POSTFLG: u32 = 0x300;
const POWCNT2: u32 = 0x304;
const BIOSPROT: u32 = 0x308;
const SOUND_FIRST: u32 = mixer::FIRST as u32;
const SOUND_LAST: u32 = mixer::LAST as u32;
const IPC_RECEIVE: u32 = 0x10_0000;
const CART_DATA: u32 = 0x10_0010;
const POSTFLG_BOOTED: u8 = 1 << 0;
const HALT: u8 = 0x80;
const POWER_MODE: u8 = 0xC0;
const POWCNT2_WRITABLE: u16 = 0x0003;
const POWCNT2_WIFI: u16 = 1 << 1;
const RTC_WRITABLE: u16 = 0xFFFF;

pub(super) struct Arm7Io<'a> {
    pub(super) shared: &'a Shared,
    pub(super) own: &'a Arm7Memory,
}

impl Arm7Bus<'_> {
    fn io(&self) -> Arm7Io<'_> {
        Arm7Io {
            shared: self.shared,
            own: self.own,
        }
    }

    pub(super) fn read_io(&mut self, addr: u32, width: Width) -> u32 {
        self.own.attention = true;
        self.own.pending = true;
        let word = addr & 0x00FF_FFFC;
        let value = match word {
            IPC_RECEIVE => self.shared.receive_ipc(Cpu::Arm7),
            CART_DATA => self.shared.read_cart_data(Cpu::Arm7),
            _ => self.io().read_word(word),
        };
        lane(value, addr, width)
    }
}

impl Arm7Io<'_> {
    pub(super) fn peek(&self, addr: u32) -> u32 {
        match addr & 0x00FF_FFFC {
            IPC_RECEIVE => self.shared.peek_ipc(Cpu::Arm7),
            CART_DATA => self.shared.peek_cart_data(Cpu::Arm7),
            word => self.read_word(word),
        }
    }

    fn read_word(&self, word: u32) -> u32 {
        match word {
            DISPSTAT => {
                u32::from(self.shared.lcd.dispstat(Cpu::Arm7))
                    | u32::from(self.shared.lcd.line()) << 16
            }
            DMA_FIRST..=DMA_LAST => self.own.dma.read((word - DMA_FIRST) as usize),
            TIMER_FIRST..=TIMER_LAST => self.own.timers.read((word - TIMER_FIRST) as usize),
            KEYINPUT => {
                let input = &self.shared.input;
                u32::from(input.keyinput()) | u32::from(input.keycnt(Cpu::Arm7)) << 16
            }
            RCNT => u32::from(self.own.rcnt) | u32::from(self.shared.input.extkeyin()) << 16,
            RTC => u32::from(self.own.rtc.read()),
            IPC_FIRST..=IPC_LAST => self.shared.read_ipc(Cpu::Arm7, (word - IPC_FIRST) as usize),
            CART_FIRST..=CART_LAST => self
                .shared
                .read_cart(Cpu::Arm7, (word - CART_FIRST) as usize),
            SPI => self.own.spi.read(0),
            EXMEMSTAT => {
                u32::from(self.shared.arm7_exmem()) | u32::from(self.own.wifi_waitcnt) << 16
            }
            IRQ_FIRST..=IRQ_LAST => self.shared.irq[1].read((word - IRQ_FIRST) as usize),
            VRAMSTAT => {
                u32::from(self.shared.vram.arm7_status())
                    | u32::from(self.shared.wram.control()) << 8
            }
            POSTFLG => u32::from(self.own.postflg),
            POWCNT2 => u32::from(self.own.power),
            BIOSPROT => u32::from(self.own.biosprot),
            SOUND_FIRST..=SOUND_LAST => self.own.sound.read(word as usize),
            _ => 0,
        }
    }
}

impl Arm7Bus<'_> {
    pub(super) fn write_io(&mut self, addr: u32, width: Width, value: u32) {
        self.own.attention = true;
        self.own.pending = true;
        let word = addr & 0x00FF_FFFC;
        let (value, mask) = lanes(addr, width, value);
        match word {
            DISPSTAT => {
                self.shared
                    .lcd
                    .write_dispstat(Cpu::Arm7, value as u16, mask as u16);
                self.shared
                    .lcd
                    .write_vcount((value >> 16) as u16, (mask >> 16) as u16);
            }
            DMA_FIRST..=DMA_LAST => {
                self.own.dma.write((word - DMA_FIRST) as usize, value, mask);
                self.run_dma();
            }
            TIMER_FIRST..=TIMER_LAST => {
                self.own
                    .timers
                    .write((word - TIMER_FIRST) as usize, value, mask);
            }
            KEYINPUT => {
                let (high, high_mask) = ((value >> 16) as u16, (mask >> 16) as u16);
                if high_mask != 0 && self.shared.input.write_keycnt(Cpu::Arm7, high, high_mask) {
                    self.shared.irq[1].request(Irq::KEYPAD);
                }
            }
            RCNT => {
                let mask = mask as u16;
                self.own.rcnt = self.own.rcnt & !mask | value as u16 & mask;
            }
            RTC if mask & 0xFFFF != 0 => {
                self.own.rtc.write(value as u16, mask as u16 & RTC_WRITABLE);
            }
            IPC_FIRST..=IPC_LAST => {
                self.shared
                    .write_ipc(Cpu::Arm7, (word - IPC_FIRST) as usize, value, mask);
            }
            CART_FIRST..=CART_LAST => {
                self.shared
                    .write_cart(Cpu::Arm7, (word - CART_FIRST) as usize, value, mask);
            }
            SPI => self.own.spi.write(0, value, mask, self.shared.cart.clock()),
            EXMEMSTAT => {
                self.shared.write_arm7_exmem(value as u16, mask as u16);
                let high = (mask >> 16) as u16;
                self.own.wifi_waitcnt = self.own.wifi_waitcnt & !high | (value >> 16) as u16 & high;
            }
            IRQ_FIRST..=IRQ_LAST => {
                self.shared.irq[1].write((word - IRQ_FIRST) as usize, value, mask);
            }
            POSTFLG => self.write_power_bytes(value, mask),
            POWCNT2 => {
                let mask = mask as u16 & POWCNT2_WRITABLE;
                self.own.power = self.own.power & !mask | value as u16 & mask;
                self.own
                    .wifi
                    .set_powered(self.own.power & POWCNT2_WIFI != 0);
            }
            BIOSPROT => {
                let mask = mask as u16;
                self.own.biosprot = self.own.biosprot & !mask | value as u16 & mask;
            }
            SOUND_FIRST..=SOUND_LAST => self.own.sound.write(word as usize, value, mask),
            _ => {}
        }
    }

    fn write_power_bytes(&mut self, value: u32, mask: u32) {
        if mask & 0xFF != 0 {
            self.own.postflg |= value as u8 & POSTFLG_BOOTED;
        }
        if mask >> 8 & 0xFF != 0 && (value >> 8) as u8 & POWER_MODE != 0 {
            let mode = (value >> 8) as u8 & POWER_MODE;
            self.own.halted = mode >= HALT;
            self.own.sleeping = mode == POWER_MODE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::Buses;
    use crate::console::Keys;
    use crate::gba::wait::Width;
    use crate::nds::Cpu;
    use crate::nds::interrupt::Irq;

    const IO: u32 = 0x0400_0000;

    #[test]
    fn extkeyin_shows_x_y_the_pen_and_the_lid() {
        let mut buses = Buses::default();
        buses.set_keys(Keys::X | Keys::LID);
        buses.set_pen(Some((10, 20)));
        assert_eq!(buses.arm7().read(IO + 0x136, Width::Half), 0x00BE);
        assert_eq!(buses.arm9().read(IO + 0x136, Width::Half), 0);
    }

    #[test]
    fn haltcnt_halts_only_the_arm7() {
        let mut buses = Buses::default();
        buses.arm7().write(IO + 0x301, Width::Byte, 0x80);
        assert!(buses.halted(Cpu::Arm7));
        assert!(!buses.halted(Cpu::Arm9));
        assert!(!buses.wake(Cpu::Arm7));
    }

    #[test]
    fn postflg_can_be_set_but_not_cleared() {
        let mut buses = Buses::default();
        buses.arm7().write(IO + 0x300, Width::Byte, 1);
        buses.arm7().write(IO + 0x300, Width::Byte, 0);
        assert_eq!(buses.arm7().read(IO + 0x300, Width::Byte), 1);
    }

    #[test]
    fn sound_registers_hold_what_was_written() {
        let mut buses = Buses::default();
        buses.arm7().write(IO + 0x504, Width::Half, 0x0200);
        buses.arm7().write(IO + 0x414, Width::Word, 0x0200_0000);
        assert_eq!(buses.arm7().read(IO + 0x504, Width::Half), 0x0200);
        assert_eq!(buses.arm7().read(IO + 0x414, Width::Word), 0);
    }

    const RTC: u32 = IO + 0x138;
    const RTC_DATA: u32 = 1 << 0;
    const RTC_CLOCK: u32 = 1 << 1;
    const RTC_SELECT: u32 = 1 << 2;
    const RTC_DATA_WRITE: u32 = 1 << 4;
    const UNIX_2025_03_14_15_09_26: i64 = 1_741_964_966;

    fn rtc_send(buses: &mut Buses, byte: u8) {
        for bit in 0..8 {
            let data = u32::from((byte >> bit) & 1);
            let out = RTC_SELECT | RTC_DATA_WRITE | data;
            buses.arm7().write(RTC, Width::Half, out);
            buses.arm7().write(RTC, Width::Half, out | RTC_CLOCK);
        }
    }

    fn rtc_receive(buses: &mut Buses) -> u8 {
        (0..8).fold(0, |byte, bit| {
            buses.arm7().write(RTC, Width::Half, RTC_SELECT);
            buses.arm7().write(RTC, Width::Half, RTC_SELECT | RTC_CLOCK);
            byte | ((buses.arm7().read(RTC, Width::Half) & RTC_DATA) as u8) << bit
        })
    }

    fn rtc_command(buses: &mut Buses, command: u8, bytes: &[u8], reads: usize) -> Vec<u8> {
        buses
            .arm7()
            .write(RTC, Width::Half, RTC_CLOCK | RTC_DATA_WRITE);
        buses
            .arm7()
            .write(RTC, Width::Half, RTC_SELECT | RTC_CLOCK | RTC_DATA_WRITE);
        rtc_send(buses, command);
        for &byte in bytes {
            rtc_send(buses, byte);
        }
        let read = (0..reads).map(|_| rtc_receive(buses)).collect();
        buses
            .arm7()
            .write(RTC, Width::Half, RTC_CLOCK | RTC_DATA_WRITE);
        read
    }

    #[test]
    fn the_rtc_date_and_time_read_through_the_arm7_bus() {
        let mut buses = Buses::default();
        buses.rtc_mut().set_unix(UNIX_2025_03_14_15_09_26);
        let date = rtc_command(&mut buses, 0xA6, &[], 7);
        assert_eq!(date, [0x25, 0x03, 0x14, 5, 0x15 | 0x40, 0x09, 0x26]);
        assert_eq!(buses.arm9().read(RTC, Width::Half), 0);
    }

    #[test]
    fn the_rtc_interrupt_reaches_the_arm7_as_a_serial_irq_in_gpio_mode() {
        for (rcnt, raised) in [(0x8100, true), (0x8000, false), (0x0100, false)] {
            let mut buses = Buses::default();
            buses.arm7().write(IO + 0x134, Width::Half, rcnt);
            rtc_command(&mut buses, 0x46, &[0x08], 0);
            buses.elapse(2048);
            let flags = buses.interrupts(Cpu::Arm7).flags();
            assert_eq!(flags.contains(Irq::SERIAL), raised, "rcnt {rcnt:#06x}");
        }
    }

    #[test]
    fn the_cart_answers_only_the_cpu_that_owns_the_slot() {
        let mut buses = Buses::default();
        buses.arm9().write(IO + 0x1A0, Width::Half, 0x8000);
        assert_eq!(buses.arm9().read(IO + 0x1A0, Width::Half), 0x8000);
        assert_eq!(buses.arm7().read(IO + 0x1A0, Width::Half), 0);
        buses.arm9().write(IO + 0x204, Width::Half, 1 << 11);
        assert_eq!(buses.arm7().read(IO + 0x1A0, Width::Half), 0x8000);
        assert_eq!(buses.arm9().read(IO + 0x1A0, Width::Half), 0);
    }
}
