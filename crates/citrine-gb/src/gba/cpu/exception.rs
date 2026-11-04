use super::psr::{FLAG_F, FLAG_I, FLAG_T, MODE_MASK, Mode};
use super::{Cpu, CpuBus};
use crate::gba::wait::Width;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exception {
    Reset,
    Undefined,
    SoftwareInterrupt,
    PrefetchAbort,
    DataAbort,
    Irq,
    Fiq,
}

impl Exception {
    pub fn vector(self) -> u32 {
        match self {
            Self::Reset => 0x00,
            Self::Undefined => 0x04,
            Self::SoftwareInterrupt => 0x08,
            Self::PrefetchAbort => 0x0C,
            Self::DataAbort => 0x10,
            Self::Irq => 0x18,
            Self::Fiq => 0x1C,
        }
    }

    pub fn mode(self) -> Mode {
        match self {
            Self::Reset | Self::SoftwareInterrupt => Mode::Supervisor,
            Self::Undefined => Mode::Undefined,
            Self::PrefetchAbort | Self::DataAbort => Mode::Abort,
            Self::Irq => Mode::Irq,
            Self::Fiq => Mode::Fiq,
        }
    }

    fn masks_fiq(self) -> bool {
        matches!(self, Self::Reset | Self::Fiq)
    }

    fn masked_by(self) -> u32 {
        match self {
            Self::Irq => FLAG_I,
            Self::Fiq => FLAG_F,
            _ => 0,
        }
    }
}

impl Cpu {
    pub fn interrupt(&mut self, bus: &mut impl CpuBus, exception: Exception) -> Option<u32> {
        let masked = match exception {
            Exception::Irq => self.irq_masked || self.irq_held,
            _ => self.regs.cpsr() & exception.masked_by() != 0,
        };
        if masked {
            return None;
        }
        self.ldm_glitch = false;
        self.cycles = 0;
        let pc = self.regs.r[15];
        let width = if self.thumb() {
            Width::Half
        } else {
            Width::Word
        };
        self.code(bus, pc, width, self.next_fetch);
        let link = match (self.thumb(), exception) {
            (false, _) => pc.wrapping_sub(4),
            (true, Exception::DataAbort) => pc.wrapping_add(2),
            (true, _) => pc,
        };
        Some(self.enter(bus, exception, link))
    }

    pub(super) fn enter(&mut self, bus: &mut impl CpuBus, exception: Exception, link: u32) -> u32 {
        let old = self.regs.cpsr();
        let mut cpsr = (old & !(MODE_MASK | FLAG_T)) | exception.mode().bits() | FLAG_I;
        if exception.masks_fiq() {
            cpsr |= FLAG_F;
        }
        self.regs.set_cpsr(cpsr);
        self.regs.set_spsr(old);
        self.regs.r[14] = link;
        self.regs.r[15] = exception.vector();
        self.reload(bus);
        self.cycles
    }
}
