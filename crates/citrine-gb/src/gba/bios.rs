use super::bus::Bus;
use super::cpu::{Cpu, CpuState};
use super::machine::CYCLES_PER_FRAME;
use super::region::BIOS_SIZE;
use super::wait::Access;

pub const CULT_OF_GBA: &[u8; BIOS_SIZE] = include_bytes!("../../assets/gba-bios/cult-of-gba.bin");

const ROM_ENTRY: u32 = 0x0800_0000;
const SP_SYSTEM: u32 = 0x0300_7F00;
const SP_IRQ: u32 = 0x0300_7FA0;
const SP_SUPERVISOR: u32 = 0x0300_7FE0;
const CPSR_SYSTEM: u32 = 0x1F;
const POSTFLG: u32 = 0x0400_0300;
const TM0CNT_L: u32 = 0x0400_0100;
const TM0CNT_H: u32 = 0x0400_0102;
const TIMER_START: u16 = 0x0080;
const BOOT_TIMER_RELOAD: u16 = 0xFB1A;
const BOOT_TIMER_START: u32 = 1_884_975;
const BOOT_TIMER_STOP: u32 = 75_487_133;
const TIMER_STOP_DELAY: u32 = 1;
const HEADER_BOOT_BYTE: u32 = 0x0800_00B4;
pub(crate) const SKIP_ENTRY_CYCLES: u64 = 76_001_689;
const IO_AFTER_BOOT: [(u32, u16); 8] = [
    (0x0400_0000, 0x0080),
    (0x0400_0020, 0x0100),
    (0x0400_0026, 0x0100),
    (0x0400_0030, 0x0100),
    (0x0400_0036, 0x0100),
    (0x0400_0082, 0x880E),
    (0x0400_0088, 0x0200),
    (0x0400_0134, 0x8000),
];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BiosKind {
    #[default]
    BuiltIn,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum BiosError {
    #[error("a GBA BIOS is 16 KB, this file is {0} bytes")]
    Size(usize),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Boot {
    #[default]
    Skip,
    Intro,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bios {
    kind: BiosKind,
    image: Box<[u8]>,
}

impl Default for Bios {
    fn default() -> Self {
        Self::built_in()
    }
}

impl Bios {
    pub fn built_in() -> Self {
        Self {
            kind: BiosKind::BuiltIn,
            image: CULT_OF_GBA.as_slice().into(),
        }
    }

    pub fn user(image: &[u8]) -> Result<Self, BiosError> {
        if image.len() != BIOS_SIZE {
            return Err(BiosError::Size(image.len()));
        }
        Ok(Self {
            kind: BiosKind::User,
            image: image.into(),
        })
    }

    pub fn kind(&self) -> BiosKind {
        self.kind
    }

    pub fn image(&self) -> &[u8] {
        &self.image
    }

    pub(crate) fn boot(&self, bus: &mut Bus, boot: Boot) -> Cpu {
        bus.load_bios(&self.image);
        bus.ppu_mut().power_on();
        let mut cpu = Cpu::default();
        match boot {
            Boot::Intro => {
                cpu.reset(bus);
            }
            Boot::Skip => {
                for (addr, value) in IO_AFTER_BOOT {
                    bus.write16(addr, value);
                }
                bus.write8(POSTFLG, 1);
                run_boot_timer(bus);
                bus.step_video((SKIP_ENTRY_CYCLES % CYCLES_PER_FRAME) as u32);
                cpu = Cpu::from_state(&state_after_boot(bus));
            }
        }
        cpu
    }
}

impl Boot {
    pub(crate) fn entry_cycles(self) -> u64 {
        match self {
            Boot::Skip => SKIP_ENTRY_CYCLES,
            Boot::Intro => 0,
        }
    }
}

fn run_boot_timer(bus: &mut Bus) {
    let start = BOOT_TIMER_START - u32::from(bus.read8(HEADER_BOOT_BYTE) >> 7);
    bus.step_timers(start);
    bus.write16(TM0CNT_L, BOOT_TIMER_RELOAD);
    bus.write16(TM0CNT_H, TIMER_START);
    bus.step_timers(BOOT_TIMER_STOP + TIMER_STOP_DELAY - start);
    bus.write32(TM0CNT_L, 0);
    bus.step_timers((SKIP_ENTRY_CYCLES - u64::from(BOOT_TIMER_STOP + TIMER_STOP_DELAY)) as u32);
}

fn state_after_boot(bus: &Bus) -> CpuState {
    let mut state = CpuState {
        cpsr: CPSR_SYSTEM,
        ..CpuState::default()
    };
    state.r[13] = SP_SYSTEM;
    state.r[14] = ROM_ENTRY;
    state.irq[0] = SP_IRQ;
    state.svc[0] = SP_SUPERVISOR;
    state.r[15] = ROM_ENTRY + 8;
    state.pipeline = [bus.read32(ROM_ENTRY), bus.read32(ROM_ENTRY + 4)];
    state.next_fetch = Access::Sequential;
    state
}

#[cfg(test)]
mod tests;
