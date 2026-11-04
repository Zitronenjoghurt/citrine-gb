mod alu;
pub(super) mod multiply;
mod transfer;

use super::exception::Exception;
use super::psr::FLAG_T;
use super::{Cpu, CpuBus};
use crate::gba::wait::Access;

const PSR_FLAGS: u32 = 0xFF00_0000;
const PSR_FLAGS_NZCV: u32 = 0xF000_0000;
const MODE_BIT_4: u32 = 0x10;
const DEBUG_COPROCESSOR: u32 = 14;

impl Cpu {
    pub(super) fn execute_arm(&mut self, bus: &mut impl CpuBus, op: u32) {
        match (op >> 25) & 7 {
            0b000 => {
                if op & 0x0FFF_FFF0 == 0x012F_FF10 {
                    self.arm_bx(bus, op);
                } else if op & 0x0FB0_0FF0 == 0x0100_0090 {
                    self.arm_swap(bus, op);
                } else if op & 0x0FC0_00F0 == 0x0000_0090 {
                    self.arm_multiply(bus, op);
                } else if op & 0x0F80_00F0 == 0x0080_0090 {
                    self.arm_multiply_long(bus, op);
                } else if op & 0x0000_0090 == 0x0000_0090 {
                    self.arm_halfword_transfer(bus, op);
                } else if op & 0x0190_0000 == 0x0100_0000 {
                    self.arm_psr_transfer(bus, op);
                } else {
                    self.arm_data_processing(bus, op);
                }
            }
            0b001 => {
                if op & 0x0190_0000 == 0x0100_0000 {
                    self.arm_psr_transfer(bus, op);
                } else {
                    self.arm_data_processing(bus, op);
                }
            }
            0b010 => self.arm_single_transfer(bus, op),
            0b011 if op & 0x10 == 0 => self.arm_single_transfer(bus, op),
            0b100 => self.arm_block_transfer(bus, op),
            0b101 => self.arm_branch(bus, op),
            0b111 if op & (1 << 24) != 0 => self.arm_software_interrupt(bus),
            0b111 if op & 0x10 != 0 && (op >> 8) & 0xF == DEBUG_COPROCESSOR => {
                self.arm_debug_transfer(bus, op)
            }
            _ => self.arm_undefined(bus),
        }
    }

    fn arm_branch(&mut self, bus: &mut impl CpuBus, op: u32) {
        let offset = (((op << 8) as i32) >> 6) as u32;
        let pc = self.regs.r[15];
        self.prefetch(bus);
        if op & (1 << 24) != 0 {
            self.regs.r[14] = pc.wrapping_sub(4);
        }
        self.regs.r[15] = pc.wrapping_add(offset);
        self.reload(bus);
    }

    fn arm_bx(&mut self, bus: &mut impl CpuBus, op: u32) {
        let target = self.operand((op & 0xF) as usize);
        self.branch_exchange(bus, target);
    }

    pub(super) fn branch_exchange(&mut self, bus: &mut impl CpuBus, target: u32) {
        self.prefetch(bus);
        let cpsr = self.regs.cpsr();
        if target & 1 != 0 {
            self.regs.set_cpsr(cpsr | FLAG_T);
        } else {
            self.regs.set_cpsr(cpsr & !FLAG_T);
        }
        self.regs.r[15] = target;
        self.reload(bus);
    }

    fn arm_software_interrupt(&mut self, bus: &mut impl CpuBus) {
        let link = self.regs.r[15].wrapping_sub(4);
        self.prefetch(bus);
        self.enter(bus, Exception::SoftwareInterrupt, link);
    }

    fn arm_debug_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        self.prefetch(bus);
        self.cycles += 1;
        self.next_fetch = Access::NonSequential;
        bus.idle();
        let rd = ((op >> 12) & 0xF) as usize;
        if op & (1 << 20) == 0 {
            bus.drive(self.regs.r[rd]);
            return;
        }
        let value = bus.bus_value();
        if rd == 15 {
            let cpsr = self.regs.cpsr();
            self.regs
                .set_cpsr((cpsr & !PSR_FLAGS_NZCV) | (value & PSR_FLAGS_NZCV));
        } else {
            self.regs.r[rd] = value;
        }
        self.idle(bus);
    }

    fn arm_undefined(&mut self, bus: &mut impl CpuBus) {
        let link = self.regs.r[15].wrapping_sub(4);
        self.prefetch(bus);
        self.enter(bus, Exception::Undefined, link);
    }

    fn arm_psr_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        let spsr = op & (1 << 22) != 0;
        if op & (1 << 21) == 0 {
            let value = if spsr {
                self.regs.spsr().unwrap_or(self.regs.cpsr())
            } else {
                self.regs.cpsr()
            };
            self.prefetch(bus);
            let rd = ((op >> 12) & 0xF) as usize;
            self.regs.r[rd] = if rd == 15 {
                value.wrapping_add(4)
            } else {
                value
            };
            return;
        }
        let value = if op & (1 << 25) != 0 {
            (op & 0xFF).rotate_right(((op >> 8) & 0xF) * 2)
        } else {
            self.operand((op & 0xF) as usize)
        };
        let mut mask = (0..4)
            .filter(|byte| op & (1 << (16 + byte)) != 0)
            .fold(0, |mask, byte| mask | (0xFF << (8 * byte)));
        if !spsr && self.regs.user_mode() {
            mask &= PSR_FLAGS;
        }
        self.prefetch(bus);
        if spsr {
            if let Some(old) = self.regs.spsr() {
                self.regs.set_spsr((old & !mask) | (value & mask));
            }
        } else {
            let old = self.regs.cpsr();
            let value = if mask & MODE_BIT_4 != 0 {
                value | MODE_BIT_4
            } else {
                value
            };
            self.regs.set_cpsr((old & !mask) | (value & mask));
        }
    }

    pub(super) fn write_pc_or_reg(&mut self, bus: &mut impl CpuBus, rd: usize, value: u32) {
        self.regs.r[rd] = value;
        if rd == 15 {
            self.reload(bus);
        }
    }

    pub(super) fn restore_cpsr(&mut self) {
        if let Some(spsr) = self.regs.spsr() {
            self.regs.set_cpsr(spsr);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gba::bus::Bus;
    use crate::gba::cpu::psr::Mode;
    use crate::gba::cpu::{Cpu, CpuState};

    const MRC_P14_R0: u32 = 0xEE10_0E10;
    const MRC_P14_R15: u32 = 0xEE10_FE10;
    const MCR_P14_R1: u32 = 0xEE00_1E10;
    const MRC_P15_R0: u32 = 0xEE10_0F10;
    const MOV_R0_0: u32 = 0xE3A0_0000;

    fn cpu(words: &[u32]) -> (Cpu, Bus) {
        let rom: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        let mut bus = Bus::new(&rom);
        let mut state = CpuState {
            cpsr: 0x1F,
            ..CpuState::default()
        };
        state.r[15] = 0x0800_0000;
        let mut cpu = Cpu::from_state(&state);
        cpu.reload(&mut bus);
        (cpu, bus)
    }

    #[test]
    fn mrc_on_cp14_reads_the_word_the_bus_fetched_last() {
        let (mut cpu, mut bus) = cpu(&[MRC_P14_R0, MOV_R0_0, 0x1234_5678]);
        cpu.step(&mut bus);
        assert_eq!(cpu.reg(0), 0x1234_5678);
        assert_eq!(cpu.mode(), Some(Mode::System));
        assert_eq!(cpu.reg(15), 0x0800_000C);
    }

    #[test]
    fn mrc_on_cp14_into_r15_sets_only_the_flags() {
        let (mut cpu, mut bus) = cpu(&[MRC_P14_R15, MOV_R0_0, 0xA000_00FF]);
        cpu.step(&mut bus);
        assert_eq!(cpu.cpsr(), 0xA000_001F);
        assert_eq!(cpu.reg(15), 0x0800_000C);
    }

    #[test]
    fn mcr_on_cp14_runs_without_a_trap() {
        let (mut cpu, mut bus) = cpu(&[MCR_P14_R1, MOV_R0_0, MOV_R0_0]);
        cpu.step(&mut bus);
        assert_eq!(cpu.mode(), Some(Mode::System));
        assert_eq!(cpu.reg(15), 0x0800_000C);
    }

    #[test]
    fn mrc_on_another_coprocessor_is_undefined() {
        let (mut cpu, mut bus) = cpu(&[MRC_P15_R0, MOV_R0_0, MOV_R0_0]);
        cpu.step(&mut bus);
        assert_eq!(cpu.mode(), Some(Mode::Undefined));
    }
}
