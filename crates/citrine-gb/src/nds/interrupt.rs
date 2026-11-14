use super::Cpu;
use bitflags::bitflags;

pub const FIRST: usize = 0x208;
pub const LAST: usize = 0x217;

const IME: usize = 0x00;
const IE: usize = 0x08;
const IF: usize = 0x0C;

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct Irq: u32 {
        const VBLANK = 1 << 0;
        const HBLANK = 1 << 1;
        const VCOUNT = 1 << 2;
        const TIMER0 = 1 << 3;
        const TIMER1 = 1 << 4;
        const TIMER2 = 1 << 5;
        const TIMER3 = 1 << 6;
        const SERIAL = 1 << 7;
        const DMA0 = 1 << 8;
        const DMA1 = 1 << 9;
        const DMA2 = 1 << 10;
        const DMA3 = 1 << 11;
        const KEYPAD = 1 << 12;
        const GBA_SLOT = 1 << 13;
        const IPC_SYNC = 1 << 16;
        const IPC_SEND_EMPTY = 1 << 17;
        const IPC_RECEIVE = 1 << 18;
        const CARD_DONE = 1 << 19;
        const CARD_IREQ = 1 << 20;
        const GEOMETRY_FIFO = 1 << 21;
        const LID = 1 << 22;
        const SPI = 1 << 23;
        const WIFI = 1 << 24;
    }
}

impl Irq {
    pub fn timer(index: usize) -> Self {
        Self::from_bits_truncate(Self::TIMER0.bits() << index)
    }

    pub fn dma(channel: usize) -> Self {
        Self::from_bits_truncate(Self::DMA0.bits() << channel)
    }

    pub fn sources(cpu: Cpu) -> Self {
        let shared =
            Self::all() - Self::SERIAL - Self::GEOMETRY_FIFO - Self::LID - Self::SPI - Self::WIFI;
        match cpu {
            Cpu::Arm9 => shared | Self::GEOMETRY_FIFO,
            Cpu::Arm7 => shared | Self::SERIAL | Self::LID | Self::SPI | Self::WIFI,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Interrupts {
    cpu: Cpu,
    master: bool,
    enabled: u32,
    flags: Irq,
    held: Irq,
}

impl Interrupts {
    pub fn new(cpu: Cpu) -> Self {
        Self {
            cpu,
            master: false,
            enabled: 0,
            flags: Irq::empty(),
            held: Irq::empty(),
        }
    }

    pub fn cpu(&self) -> Cpu {
        self.cpu
    }

    pub fn flags(&self) -> Irq {
        self.flags
    }

    pub fn read(&self, offset: usize) -> u32 {
        match offset & !3 {
            IME => u32::from(self.master),
            IE => self.enabled,
            IF => self.flags.bits(),
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) {
        match offset & !3 {
            IME => {
                if mask & 1 != 0 {
                    self.master = value & 1 != 0;
                }
            }
            IE => self.enabled = (self.enabled & !mask) | (value & mask),
            IF => {
                self.flags -= Irq::from_bits_truncate(value & mask);
                self.flags |= self.held;
            }
            _ => {}
        }
    }

    pub fn enables(&self, irq: Irq) -> bool {
        self.enabled & irq.bits() != 0
    }

    #[inline]
    pub fn request(&mut self, irq: Irq) {
        if !irq.is_empty() {
            self.flags |= irq & Irq::sources(self.cpu);
        }
    }

    pub fn hold(&mut self, irq: Irq, level: bool) {
        let irq = irq & Irq::sources(self.cpu);
        self.held.set(irq, level);
        if level {
            self.flags |= irq;
        }
    }

    pub fn wake(&self) -> bool {
        self.enabled & self.flags.bits() != 0
    }

    pub fn line(&self) -> bool {
        self.master && self.wake()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = 0xFFFF_FFFF;

    #[test]
    fn bits_follow_gbatek() {
        assert_eq!(Irq::timer(2).bits(), 1 << 5);
        assert_eq!(Irq::dma(3).bits(), 1 << 11);
        assert_eq!(Irq::IPC_SYNC.bits(), 1 << 16);
        assert_eq!(Irq::IPC_RECEIVE.bits(), 1 << 18);
        assert_eq!(Irq::GEOMETRY_FIFO.bits(), 1 << 21);
        assert_eq!(Irq::SPI.bits(), 1 << 23);
    }

    #[test]
    fn each_cpu_has_its_own_sources() {
        assert_eq!(Irq::sources(Cpu::Arm9).bits(), 0x003F_3F7F);
        assert_eq!(Irq::sources(Cpu::Arm7).bits(), 0x01DF_3FFF);
    }

    #[test]
    fn requests_outside_the_cpu_are_dropped() {
        let mut arm9 = Interrupts::new(Cpu::Arm9);
        arm9.request(Irq::SPI | Irq::VBLANK);
        assert_eq!(arm9.read(IF), Irq::VBLANK.bits());
        let mut arm7 = Interrupts::new(Cpu::Arm7);
        arm7.request(Irq::GEOMETRY_FIFO | Irq::LID);
        assert_eq!(arm7.read(IF), Irq::LID.bits());
    }

    #[test]
    fn line_needs_ime_ie_and_if() {
        let mut irq = Interrupts::new(Cpu::Arm7);
        irq.request(Irq::IPC_SYNC);
        assert!(!irq.wake());
        irq.write(IE, Irq::IPC_SYNC.bits(), ALL);
        assert!(irq.wake());
        assert!(!irq.line());
        irq.write(IME, 1, ALL);
        assert!(irq.line());
    }

    #[test]
    fn ime_keeps_only_bit_zero() {
        let mut irq = Interrupts::new(Cpu::Arm9);
        irq.write(IME, 0xFFFF_FFFF, ALL);
        assert_eq!(irq.read(IME), 1);
    }

    #[test]
    fn ie_takes_any_bits_by_mask() {
        let mut irq = Interrupts::new(Cpu::Arm9);
        irq.write(IE, 0x1234_5678, 0xFFFF_0000);
        assert_eq!(irq.read(IE), 0x1234_0000);
        irq.write(IE + 2, 0x0000_5678, 0x0000_FFFF);
        assert_eq!(irq.read(IE), 0x1234_5678);
    }

    #[test]
    fn writing_one_acknowledges() {
        let mut irq = Interrupts::new(Cpu::Arm9);
        irq.request(Irq::VBLANK | Irq::TIMER0 | Irq::IPC_SYNC);
        irq.write(IF, (Irq::VBLANK | Irq::IPC_SYNC).bits(), ALL);
        assert_eq!(irq.flags(), Irq::TIMER0);
        irq.write(IF, Irq::TIMER0.bits(), 0xFFFF_0000);
        assert_eq!(irq.flags(), Irq::TIMER0);
    }

    #[test]
    fn held_sources_come_back_after_acknowledge() {
        let mut irq = Interrupts::new(Cpu::Arm9);
        irq.hold(Irq::GEOMETRY_FIFO, true);
        irq.write(IF, Irq::GEOMETRY_FIFO.bits(), ALL);
        assert_eq!(irq.flags(), Irq::GEOMETRY_FIFO);
        irq.hold(Irq::GEOMETRY_FIFO, false);
        assert_eq!(irq.flags(), Irq::GEOMETRY_FIFO);
        irq.write(IF, Irq::GEOMETRY_FIFO.bits(), ALL);
        assert!(irq.flags().is_empty());
    }
}
