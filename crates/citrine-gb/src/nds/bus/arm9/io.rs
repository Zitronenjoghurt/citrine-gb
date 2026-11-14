use super::super::{lanes, read_engine, write_engine};
use super::{Arm9Bus, Region};
use crate::gba::open_bus::lane;
use crate::gba::wait::Width;
use crate::nds::Cpu;
use crate::nds::bus::{Arm9Memory, Shared};
use crate::nds::dma::{Run, Start};
use crate::nds::interrupt::Irq;
use crate::nds::three_d::{REGISTERS_FIRST, REGISTERS_LAST};
use crate::nds::vram::Bank;
use crate::nds::{geometry, render};

const ENGINE_A: u32 = 0x0000;
const ENGINE_B: u32 = 0x1000;
const ENGINE_END: u32 = 0x70;
const DISPSTAT: u32 = 0x004;
const DMA_FIRST: u32 = 0x0B0;
const DMA_LAST: u32 = 0x0EC;
const TIMER_FIRST: u32 = 0x100;
const TIMER_LAST: u32 = 0x10C;
const KEYINPUT: u32 = 0x130;
const IPC_FIRST: u32 = 0x180;
const IPC_LAST: u32 = 0x188;
const CART_FIRST: u32 = 0x1A0;
const CART_LAST: u32 = 0x1B8;
const EXMEMCNT: u32 = 0x204;
const IRQ_FIRST: u32 = 0x208;
const IRQ_LAST: u32 = 0x214;
const VRAMCNT_A: u32 = 0x240;
const VRAMCNT_E: u32 = 0x244;
const VRAMCNT_H: u32 = 0x248;
const WRAMCNT_SHIFT: u32 = 24;
const MATH_FIRST: u32 = 0x280;
const MATH_LAST: u32 = 0x2BC;
const POSTFLG: u32 = 0x300;
const POWCNT1: u32 = 0x304;
const IPC_RECEIVE: u32 = 0x10_0000;
const CART_DATA: u32 = 0x10_0010;
const POSTFLG_BOOTED: u8 = 1 << 0;
const POSTFLG_RW: u8 = 1 << 1;
const POWCNT1_WRITABLE: u16 = 0x820F;
const DISP3DCNT: u32 = render::DISP3DCNT as u32;
const THREE_D_FIRST: u32 = REGISTERS_FIRST as u32;
const THREE_D_LAST: u32 = REGISTERS_LAST as u32;
const GEOMETRY_FIRST: u32 = geometry::FIRST as u32;
const GEOMETRY_LAST: u32 = geometry::LAST as u32;
const GXFIFO_LAST: u32 = GEOMETRY_FIRST + geometry::GXFIFO_END as u32 - 1;

pub(super) struct Arm9Io<'a> {
    pub(super) shared: &'a Shared,
    pub(super) own: &'a Arm9Memory,
}

impl Arm9Bus<'_> {
    fn io(&self) -> Arm9Io<'_> {
        Arm9Io {
            shared: self.shared,
            own: self.own,
        }
    }

    #[inline(always)]
    pub(super) fn read_io(&mut self, addr: u32, width: Width) -> u32 {
        self.own.attention = true;
        self.own.pending = true;
        let word = addr & 0x00FF_FFFC;
        let value = match word {
            IPC_RECEIVE => self.shared.receive_ipc(Cpu::Arm9),
            CART_DATA => self.shared.read_cart_data(Cpu::Arm9),
            DISP3DCNT | THREE_D_FIRST..=THREE_D_LAST | GEOMETRY_FIRST..=GEOMETRY_LAST => {
                self.own.three_d.read(word as usize)
            }
            _ => self.io().read_word(word),
        };
        lane(value, addr, width)
    }
}

impl Arm9Io<'_> {
    pub(super) fn peek(&self, addr: u32) -> u32 {
        match addr & 0x00FF_FFFC {
            IPC_RECEIVE => self.shared.peek_ipc(Cpu::Arm9),
            CART_DATA => self.shared.peek_cart_data(Cpu::Arm9),
            word => self.read_word(word),
        }
    }

    fn read_word(&self, word: u32) -> u32 {
        match word {
            DISPSTAT => {
                u32::from(self.shared.lcd.dispstat(Cpu::Arm9))
                    | u32::from(self.shared.lcd.line()) << 16
            }
            DISP3DCNT | THREE_D_FIRST..=THREE_D_LAST | GEOMETRY_FIRST..=GEOMETRY_LAST => {
                self.own.three_d.peek(word as usize)
            }
            ENGINE_A..ENGINE_END => read_engine(&self.own.engines[0], (word - ENGINE_A) as usize),
            DMA_FIRST..=DMA_LAST => self.own.dma.read((word - DMA_FIRST) as usize),
            TIMER_FIRST..=TIMER_LAST => self.own.timers.read((word - TIMER_FIRST) as usize),
            KEYINPUT => {
                let input = &self.shared.input;
                u32::from(input.keyinput()) | u32::from(input.keycnt(Cpu::Arm9)) << 16
            }
            IPC_FIRST..=IPC_LAST => self.shared.read_ipc(Cpu::Arm9, (word - IPC_FIRST) as usize),
            CART_FIRST..=CART_LAST => self
                .shared
                .read_cart(Cpu::Arm9, (word - CART_FIRST) as usize),
            EXMEMCNT => u32::from(self.shared.arm9_exmem()),
            IRQ_FIRST..=IRQ_LAST => self.shared.irq[0].read((word - IRQ_FIRST) as usize),
            VRAMCNT_A => self.memory_control(&[Bank::A, Bank::B, Bank::C, Bank::D]),
            VRAMCNT_E => {
                self.memory_control(&[Bank::E, Bank::F, Bank::G])
                    | u32::from(self.shared.wram.control()) << WRAMCNT_SHIFT
            }
            VRAMCNT_H => self.memory_control(&[Bank::H, Bank::I]),
            MATH_FIRST..=MATH_LAST => self.own.math.read((word - MATH_FIRST) as usize),
            POSTFLG => u32::from(self.own.postflg),
            POWCNT1 => u32::from(self.own.power),
            _ if (ENGINE_B..ENGINE_B + ENGINE_END).contains(&word) => {
                read_engine(&self.own.engines[1], (word - ENGINE_B) as usize)
            }
            _ => 0,
        }
    }

    fn memory_control(&self, banks: &[Bank]) -> u32 {
        banks.iter().enumerate().fold(0, |word, (byte, &bank)| {
            word | u32::from(self.shared.vram.control(bank)) << (8 * byte)
        })
    }
}

impl Arm9Bus<'_> {
    pub(super) fn copy_to_gxfifo(&mut self, run: &Run) -> bool {
        let into_fifo = run.wide
            && run.dest_step == 0
            && Region::of(run.dest) == Region::Io
            && (GEOMETRY_FIRST..=GXFIFO_LAST).contains(&(run.dest & 0x00FF_FFFF));
        let from_main_ram = Region::of(run.source) == Region::MainRam
            && Region::of(run.last_source()) == Region::MainRam;
        if !into_fifo || !from_main_ram {
            return false;
        }
        let mut source = run.source;
        let mut stall = 0;
        let mut raised = false;
        let mut level = false;
        for _ in 0..run.units {
            let value = self
                .shared
                .main_ram
                .read((source & !3) as usize, Width::Word);
            stall += self.own.three_d.write_packed(value);
            level = self.own.three_d.irq();
            raised |= level;
            source = source.wrapping_add(run.source_step);
        }
        self.own.attention = true;
        self.own.stall += stall * 2;
        self.own.pending = true;
        let irq = &mut self.shared.irq[0];
        if raised {
            irq.hold(Irq::GEOMETRY_FIFO, true);
        }
        irq.hold(Irq::GEOMETRY_FIFO, level);
        true
    }

    #[inline(always)]
    pub(super) fn write_io(&mut self, addr: u32, width: Width, value: u32) {
        self.own.attention = true;
        self.own.pending = true;
        let word = addr & 0x00FF_FFFC;
        let (value, mask) = lanes(addr, width, value);
        match word {
            DISPSTAT => {
                self.shared
                    .lcd
                    .write_dispstat(Cpu::Arm9, value as u16, mask as u16);
                self.shared
                    .lcd
                    .write_vcount((value >> 16) as u16, (mask >> 16) as u16);
            }
            DISP3DCNT | THREE_D_FIRST..=THREE_D_LAST | GEOMETRY_FIRST..=GEOMETRY_LAST => {
                let stall = self.own.three_d.write(word as usize, value, mask);
                self.own.stall += stall * 2;
                self.own.pending = true;
                self.shared.irq[0].hold(Irq::GEOMETRY_FIFO, self.own.three_d.irq());
            }
            ENGINE_A..ENGINE_END => {
                write_engine(
                    &mut self.own.engines[0],
                    (word - ENGINE_A) as usize,
                    value,
                    mask,
                );
            }
            DMA_FIRST..=DMA_LAST => {
                let armed = self.own.dma.write((word - DMA_FIRST) as usize, value, mask);
                if armed && self.own.three_d.dma_request() {
                    self.own.dma.trigger(Start::GeometryFifo);
                }
                self.run_dma();
            }
            TIMER_FIRST..=TIMER_LAST => {
                self.own
                    .timers
                    .write((word - TIMER_FIRST) as usize, value, mask);
            }
            KEYINPUT => {
                let (high, high_mask) = ((value >> 16) as u16, (mask >> 16) as u16);
                if high_mask != 0 && self.shared.input.write_keycnt(Cpu::Arm9, high, high_mask) {
                    self.shared.irq[0].request(Irq::KEYPAD);
                }
            }
            IPC_FIRST..=IPC_LAST => {
                self.shared
                    .write_ipc(Cpu::Arm9, (word - IPC_FIRST) as usize, value, mask);
            }
            CART_FIRST..=CART_LAST => {
                self.shared
                    .write_cart(Cpu::Arm9, (word - CART_FIRST) as usize, value, mask);
            }
            EXMEMCNT => self.shared.write_arm9_exmem(value as u16, mask as u16),
            IRQ_FIRST..=IRQ_LAST => {
                self.shared.irq[0].write((word - IRQ_FIRST) as usize, value, mask);
            }
            VRAMCNT_A | VRAMCNT_E | VRAMCNT_H => self.write_memory_control(word, value, mask),
            MATH_FIRST..=MATH_LAST => {
                self.own
                    .math
                    .write((word - MATH_FIRST) as usize, value, mask);
            }
            POSTFLG if mask & 0xFF != 0 => {
                let value = value as u8;
                self.own.postflg =
                    self.own.postflg & !POSTFLG_RW | value & (POSTFLG_BOOTED | POSTFLG_RW);
            }
            POWCNT1 => {
                let mask = mask as u16 & POWCNT1_WRITABLE;
                self.own.power = self.own.power & !mask | value as u16 & mask;
            }
            _ if (ENGINE_B..ENGINE_B + ENGINE_END).contains(&word) => {
                write_engine(
                    &mut self.own.engines[1],
                    (word - ENGINE_B) as usize,
                    value,
                    mask,
                );
            }
            _ => {}
        }
    }

    fn write_memory_control(&mut self, word: u32, value: u32, mask: u32) {
        for byte in 0..4 {
            if mask >> (8 * byte) & 0xFF == 0 {
                continue;
            }
            let control = (value >> (8 * byte)) as u8;
            match (word - VRAMCNT_A + byte) as usize {
                index @ 0..=6 => self.shared.vram.write_control(Bank::ALL[index], control),
                7 => self.shared.wram.write_control(control),
                8 => self.shared.vram.write_control(Bank::H, control),
                9 => self.shared.vram.write_control(Bank::I, control),
                _ => {}
            }
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
    const IPCSYNC: u32 = 0x0400_0180;
    const IPCFIFOCNT: u32 = 0x0400_0184;
    const IPCFIFOSEND: u32 = 0x0400_0188;
    const IPCFIFORECV: u32 = 0x0410_0000;
    const IE: u32 = 0x0400_0210;

    #[test]
    fn dispstat_and_vcount_read_as_one_word() {
        let mut buses = Buses::default();
        buses.elapse(crate::nds::lcd::LINE_CYCLES * 3 + crate::nds::lcd::HBLANK_START);
        let word = buses.arm9().read(IO + 4, Width::Word);
        assert_eq!(word >> 16, 3);
        assert_eq!(word & 0b10, 0b10);
        assert_eq!(buses.arm9().read(IO + 6, Width::Half), 3);
    }

    #[test]
    fn vramcnt_reads_back_what_libnds_wrote_for_its_texture_check() {
        let mut buses = Buses::default();
        buses.arm9().write(IO + 0x240, Width::Byte, 0x83);
        buses.arm9().write(IO + 0x241, Width::Byte, 0x8B);
        buses.arm9().write(IO + 0x246, Width::Byte, 0x84);
        buses.arm9().write(IO + 0x249, Width::Byte, 0x81);
        assert_eq!(buses.arm9().read(IO + 0x240, Width::Byte), 0x83);
        assert_eq!(buses.arm9().read(IO + 0x240, Width::Half), 0x8B83);
        assert_eq!(buses.arm9().read(IO + 0x246, Width::Byte), 0x84);
        assert_eq!(buses.arm9().read(IO + 0x249, Width::Byte), 0x81);
    }

    #[test]
    fn keyinput_shows_the_held_buttons_without_x_and_y() {
        let mut buses = Buses::default();
        buses.set_keys(Keys::A | Keys::START | Keys::X);
        assert_eq!(buses.arm9().read(IO + 0x130, Width::Half), 0x03F6);
    }

    #[test]
    fn ipcsync_reaches_the_other_cpu_with_its_irq() {
        let mut buses = Buses::default();
        buses.arm7().write(IE, Width::Word, Irq::IPC_SYNC.bits());
        buses.arm7().write(IPCSYNC, Width::Half, 1 << 14);
        buses.arm9().write(IPCSYNC, Width::Half, 0x0500 | 1 << 13);
        assert_eq!(buses.arm7().read(IPCSYNC, Width::Half) & 0xF, 5);
        assert!(buses.interrupts(Cpu::Arm7).flags().contains(Irq::IPC_SYNC));
        assert!(buses.interrupts(Cpu::Arm7).wake());
    }

    #[test]
    fn a_word_sent_through_the_fifo_comes_out_on_the_other_side_once() {
        let mut buses = Buses::default();
        buses.arm9().write(IPCFIFOCNT, Width::Half, 0x8000);
        buses.arm7().write(IPCFIFOCNT, Width::Half, 0x8000);
        buses.arm9().write(IPCFIFOSEND, Width::Word, 0xCAFE_F00D);
        buses.arm9().write(IPCFIFOSEND, Width::Word, 0x1234_5678);
        assert_eq!(buses.arm7().read(IPCFIFORECV, Width::Word), 0xCAFE_F00D);
        assert_eq!(buses.arm7().read(IPCFIFORECV, Width::Word), 0x1234_5678);
        assert_eq!(buses.arm7().read(IPCFIFOCNT, Width::Half) & 0x100, 0x100);
    }

    #[test]
    fn powcnt1_keeps_only_its_writable_bits() {
        let mut buses = Buses::default();
        buses.arm9().write(IO + 0x304, Width::Half, 0xFFFF);
        assert_eq!(buses.arm9().read(IO + 0x304, Width::Half), 0x820F);
    }

    #[test]
    fn the_divider_answers_through_its_registers() {
        let mut buses = Buses::default();
        buses.arm9().write(IO + 0x290, Width::Word, 100);
        buses.arm9().write(IO + 0x298, Width::Word, 7);
        buses.elapse(40);
        assert_eq!(buses.arm9().read(IO + 0x2A0, Width::Word), 14);
        assert_eq!(buses.arm9().read(IO + 0x2A8, Width::Word), 2);
    }

    #[test]
    fn arm9_timers_count_at_the_bus_clock() {
        let mut buses = Buses::default();
        buses.arm9().write(IO + 0x100, Width::Word, 0x0080_0000);
        buses.elapse(100);
        let first = buses.arm9().read(IO + 0x100, Width::Half);
        buses.elapse(50);
        assert_eq!(buses.arm9().read(IO + 0x100, Width::Half) - first, 50);
    }

    #[test]
    fn engine_registers_take_halves_of_a_word() {
        let mut buses = Buses::default();
        buses.arm9().write(IO + 0x1008, Width::Word, 0x0123_0456);
        assert_eq!(buses.arm9().read(IO + 0x100A, Width::Half), 0x0123);
        assert_eq!(buses.arm9().read(IO + 0x1008, Width::Half), 0x0456);
    }
}
