use super::psr::Bank;
use super::{Cpu, CpuBus};
use crate::gba::wait::{Access, Width};

#[derive(Debug, Clone, Copy)]
pub struct Block {
    pub base: usize,
    pub list: u32,
    pub load: bool,
    pub up: bool,
    pub pre: bool,
    pub writeback: bool,
    pub user: bool,
}

pub fn offset_base(base: u32, offset: u32, up: bool) -> u32 {
    if up {
        base.wrapping_add(offset)
    } else {
        base.wrapping_sub(offset)
    }
}

pub fn truncate(value: u32, width: Width) -> u32 {
    match width {
        Width::Byte => value & 0xFF,
        Width::Half => value & 0xFFFF,
        Width::Word => value,
    }
}

pub fn rotate_word(value: u32, addr: u32) -> u32 {
    value.rotate_right((addr & 3) * 8)
}

impl Cpu {
    pub(super) fn load_value(
        &mut self,
        bus: &mut impl CpuBus,
        addr: u32,
        width: Width,
        signed: bool,
    ) -> u32 {
        match (width, signed) {
            (Width::Word, _) => {
                let value = self.load(bus, addr, Width::Word, Access::NonSequential);
                rotate_word(value, addr)
            }
            (Width::Byte, false) => self.load(bus, addr, Width::Byte, Access::NonSequential),
            (Width::Byte, true) => {
                let value = self.load(bus, addr, Width::Byte, Access::NonSequential);
                value as u8 as i8 as u32
            }
            (Width::Half, false) => {
                let value = self.load(bus, addr, Width::Half, Access::NonSequential);
                value.rotate_right((addr & 1) * 8)
            }
            (Width::Half, true) => {
                let value = self.load(bus, addr, Width::Half, Access::NonSequential);
                if addr & 1 != 0 {
                    (value >> 8) as u8 as i8 as u32
                } else {
                    value as u16 as i16 as u32
                }
            }
        }
    }

    pub(super) fn block_transfer(&mut self, bus: &mut impl CpuBus, block: Block) {
        let Block {
            base: rn,
            list,
            load,
            up,
            pre,
            writeback,
            user,
        } = block;
        let (list, bytes) = if list == 0 {
            (1 << 15, 0x40)
        } else {
            (list, list.count_ones() * 4)
        };

        let base = self.operand(rn);
        let new_base = offset_base(base, bytes, up);
        let mut addr = if up { base } else { new_base };
        if pre == up {
            addr = addr.wrapping_add(4);
        }

        let user_bank = user && !(load && list & (1 << 15) != 0);
        self.prefetch(bus);
        let bank = self.regs.bank();
        if user_bank {
            self.regs.switch_bank(Bank::User);
        }

        let mut access = Access::NonSequential;
        let mut first = true;
        for reg in (0..16).filter(|r| list & (1 << r) != 0) {
            if load {
                let value = self.load(bus, addr, Width::Word, access);
                if first && writeback {
                    self.regs.r[rn] = new_base;
                }
                self.regs.r[reg] = value;
            } else {
                let value = self.regs.r[reg];
                self.store(bus, addr, Width::Word, value, access);
                if first && writeback {
                    self.regs.r[rn] = new_base;
                }
            }
            first = false;
            access = Access::Sequential;
            addr = addr.wrapping_add(4);
        }

        if user_bank {
            self.regs.switch_bank(bank);
        }
        let loads_pc = load && list & (1 << 15) != 0;
        if load {
            self.idle(bus);
            self.ldm_glitch = user_bank && bank != Bank::User;
        }
        if writeback && rn == 15 && !loads_pc {
            self.reload(bus);
        } else if loads_pc {
            if user {
                self.restore_cpsr();
            }
            self.reload(bus);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gba::bus::Bus;
    use crate::gba::cpu::{Cpu, CpuState};

    const LDM_R8_USER: u32 = 0xE8D1_0100;
    const LDM_R8_TO_R10_USER: u32 = 0xE8D1_0700;
    const LDM_R8_FROM_R8_USER: u32 = 0xE8D8_0100;
    const MOV_R0_R8: u32 = 0xE1A0_0008;
    const MOV_R2_R8: u32 = 0xE1A0_2008;
    const MOV_R8_0: u32 = 0xE3A0_8000;
    const MLA_R2_R8_R9_R10: u32 = 0xE022_A998;
    const SWP_R10_R9_R8: u32 = 0xE108_A099;

    fn fiq_cpu(words: &[u32]) -> (Cpu, Bus) {
        let rom: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        let mut bus = Bus::new(&rom);
        bus.write32(0x0300_0000, 0x00F0);
        let mut state = CpuState {
            cpsr: 0xD1,
            ..CpuState::default()
        };
        state.r[1] = 0x0300_0000;
        state.r[15] = 0x0800_0000;
        state.fiq[0] = 0x0F00;
        let mut cpu = Cpu::from_state(&state);
        cpu.reload(&mut bus);
        (cpu, bus)
    }

    fn run(cpu: &mut Cpu, bus: &mut Bus, steps: usize) {
        for _ in 0..steps {
            cpu.step(bus);
        }
    }

    #[test]
    fn only_the_instruction_right_after_a_user_bank_ldm_reads_both_banks_ored() {
        let (mut cpu, mut bus) = fiq_cpu(&[LDM_R8_USER, MOV_R0_R8, MOV_R2_R8]);
        run(&mut cpu, &mut bus, 3);
        assert_eq!(cpu.reg(0), 0x0FF0);
        assert_eq!(cpu.reg(2), 0x0F00);
        assert_eq!(cpu.regs.user_reg(8), 0x00F0);
    }

    #[test]
    fn a_write_right_after_a_user_bank_ldm_goes_to_the_mode_bank_only() {
        let (mut cpu, mut bus) = fiq_cpu(&[LDM_R8_USER, MOV_R8_0]);
        run(&mut cpu, &mut bus, 2);
        assert_eq!(cpu.reg(8), 0);
        assert_eq!(cpu.regs.user_reg(8), 0x00F0);
    }

    #[test]
    fn a_user_bank_ldm_right_after_another_reads_the_ored_base_and_glitches_again() {
        let (mut cpu, mut bus) = fiq_cpu(&[LDM_R8_USER, LDM_R8_FROM_R8_USER, MOV_R0_R8]);
        bus.write32(0x0300_0000, 0x0004);
        bus.write32(0x0300_0004, 0x0010);
        cpu.regs.r[8] = 0x0300_0000;
        run(&mut cpu, &mut bus, 3);
        assert_eq!(cpu.regs.user_reg(8), 0x0010);
        assert_eq!(cpu.reg(0), 0x0300_0010);
    }

    #[test]
    fn a_multiply_accumulate_ors_only_the_accumulator() {
        let (mut cpu, mut bus) = fiq_cpu(&[LDM_R8_TO_R10_USER, MLA_R2_R8_R9_R10]);
        bus.write32(0x0300_0000, 0x0002);
        bus.write32(0x0300_0004, 0x0003);
        bus.write32(0x0300_0008, 0x00F0);
        cpu.regs.r[8] = 5;
        cpu.regs.r[9] = 7;
        cpu.regs.r[10] = 0x0F;
        run(&mut cpu, &mut bus, 2);
        assert_eq!(cpu.reg(2), 5 * 7 + 0xFF);
    }

    #[test]
    fn a_swap_ors_its_address_but_stores_the_mode_bank_value() {
        let (mut cpu, mut bus) = fiq_cpu(&[LDM_R8_TO_R10_USER, SWP_R10_R9_R8]);
        bus.write32(0x0300_0000, 0x0004);
        bus.write32(0x0300_0004, 0x0008);
        bus.write32(0x0300_0008, 0x000C);
        cpu.regs.r[8] = 0x0300_0000;
        cpu.regs.r[9] = 3;
        run(&mut cpu, &mut bus, 2);
        assert_eq!(cpu.reg(10), 8);
        assert_eq!(bus.read32(0x0300_0004), 3);
        assert_eq!(bus.read32(0x0300_0000), 4);
    }
}
