use super::psr::Bank;
use super::{Core, CpuBus, Jump};
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

#[inline(always)]
pub fn loaded_value(value: u32, addr: u32, width: Width, signed: bool) -> u32 {
    match (width, signed) {
        (Width::Word, _) => rotate_word(value, addr),
        (Width::Byte, true) => value as u8 as i8 as u32,
        (Width::Half, true) => value as u16 as i16 as u32,
        _ => value,
    }
}

const BYTE_ONES: [u8; 256] = {
    let mut table = [0; 256];
    let mut index = 0;
    while index < 256 {
        table[index] = (index as u32).count_ones() as u8;
        index += 1;
    }
    table
};

#[inline(always)]
fn list_len(list: u32) -> u32 {
    if cfg!(all(target_arch = "x86_64", not(target_feature = "popcnt"))) {
        u32::from(BYTE_ONES[(list & 0xFF) as usize] + BYTE_ONES[((list >> 8) & 0xFF) as usize])
    } else {
        list.count_ones()
    }
}

impl<const V5: bool> Core<V5> {
    #[inline(always)]
    pub(crate) fn load_value(
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
            (Width::Half, false) if V5 => {
                self.load(bus, addr & !1, Width::Half, Access::NonSequential)
            }
            (Width::Half, true) if V5 => {
                let value = self.load(bus, addr & !1, Width::Half, Access::NonSequential);
                value as u16 as i16 as u32
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

    #[inline(always)]
    pub(super) fn near_block(&mut self, bus: &mut impl CpuBus, block: Block, width: Width) {
        if !V5 {
            if !bus.near_blocks() {
                return self.block_transfer(bus, block, width);
            }
            return self.near_block_v4(bus, block, width);
        }
        let Block {
            base: rn,
            list,
            load,
            up,
            pre,
            writeback,
            user,
        } = block;
        if user || list == 0 || rn == 15 {
            return self.block_elsewhere(bus, block, width);
        }
        let count = list_len(list);
        let base = self.regs.r[rn];
        let new_base = offset_base(base, count * 4, up);
        let mut addr = if up { base } else { new_base };
        if pre == up {
            addr = addr.wrapping_add(4);
        }
        let pc = self.regs.r[15];
        let Some(near) = self.near_next(bus, pc, width) else {
            return self.block_elsewhere(bus, block, width);
        };
        let count = count as usize;
        let cycles = if load {
            let Some((cycles, words)) = bus.near_words(addr, count) else {
                return self.block_elsewhere(bus, block, width);
            };
            self.take_next(pc, width, near);
            let mut left = list;
            for word in words {
                self.regs.r[left.trailing_zeros() as usize] = u32::from_le_bytes(*word);
                left &= left - 1;
            }
            cycles
        } else {
            let Some((cycles, words)) = bus.near_words_mut(addr, count) else {
                return self.block_elsewhere(bus, block, width);
            };
            self.take_next(pc, width, near);
            let mut left = list;
            for word in words {
                let reg = left.trailing_zeros() as usize;
                left &= left - 1;
                let value = if reg == rn { base } else { self.regs.r[reg] };
                *word = value.to_le_bytes();
            }
            cycles
        };
        self.data += cycles;
        self.next_fetch = Access::NonSequential;
        let loaded_base = load && list != 1 << rn && list >> rn == 1;
        if writeback && !loaded_base {
            self.regs.r[rn] = new_base;
        }
        if !load {
            return;
        }
        self.ldm_glitch = false;
        if list & (1 << 15) != 0 {
            let target = self.regs.r[15];
            if !self.exchange_landed(bus, target) {
                self.load_pc_exchange(bus, target);
            }
            self.take_refetch();
        }
    }

    #[inline(always)]
    fn near_block_v4(&mut self, bus: &mut impl CpuBus, block: Block, width: Width) {
        let Block {
            base: rn,
            list,
            load,
            up,
            pre,
            writeback,
            user,
        } = block;
        if user || list == 0 || rn == 15 {
            return self.block_elsewhere(bus, block, width);
        }
        let count = list_len(list);
        let base = self.operand(rn);
        let new_base = offset_base(base, count * 4, up);
        let mut addr = if up { base } else { new_base };
        if pre == up {
            addr = addr.wrapping_add(4);
        }
        let pc = self.regs.r[15];
        let count = count as usize;
        let access = self.next_fetch;
        if load {
            let Some((cycles, next, words)) = bus.near_block_words(pc, width, access, addr, count)
            else {
                return self.block_elsewhere(bus, block, width);
            };
            self.advanced_v4(pc, width, cycles, next);
            if writeback {
                self.regs.r[rn] = new_base;
            }
            let mut left = list;
            for word in words {
                self.regs.r[left.trailing_zeros() as usize] = u32::from_le_bytes(*word);
                left &= left - 1;
            }
        } else {
            let Some((cycles, next, words)) =
                bus.near_block_words_mut(pc, width, access, addr, count)
            else {
                return self.block_elsewhere(bus, block, width);
            };
            self.advanced_v4(pc, width, cycles, next);
            let mut left = list;
            for (index, word) in words.iter_mut().enumerate() {
                let reg = left.trailing_zeros() as usize;
                left &= left - 1;
                let value = if reg == rn && writeback && index != 0 {
                    new_base
                } else {
                    self.regs.r[reg]
                };
                *word = value.to_le_bytes();
            }
            if writeback {
                self.regs.r[rn] = new_base;
            }
        }
        self.next_fetch = Access::NonSequential;
        if !load {
            return;
        }
        self.load_idle(bus);
        self.ldm_glitch = false;
        if list & (1 << 15) != 0 {
            bus.jump(Jump::BlockLoad);
            self.reload(bus);
        }
    }

    #[inline(always)]
    fn advanced_v4(&mut self, pc: u32, width: Width, cycles: u32, next: u32) {
        self.cycles += cycles;
        self.pipeline = [self.pipeline[1], next];
        self.regs.r[15] = pc.wrapping_add(width.bytes() as u32);
    }

    #[inline(never)]
    fn block_elsewhere(&mut self, bus: &mut impl CpuBus, block: Block, width: Width) {
        self.block_transfer(bus, block, width);
    }

    #[inline(always)]
    pub(super) fn block_transfer(&mut self, bus: &mut impl CpuBus, block: Block, width: Width) {
        let Block {
            base: rn,
            list,
            load,
            up,
            pre,
            writeback,
            user,
        } = block;
        let (list, count, bytes) = match list {
            0 if V5 => (0, 0, 0x40),
            0 => (1 << 15, 1, 0x40),
            _ => {
                let count = list_len(list);
                (list, count, count * 4)
            }
        };

        let base = self.operand(rn);
        let new_base = offset_base(base, bytes, up);
        let mut addr = if up { base } else { new_base };
        if pre == up {
            addr = addr.wrapping_add(4);
        }

        let user_bank = user && !(load && list & (1 << 15) != 0);
        self.advance(bus, width);
        let bank = self.regs.bank();
        if user_bank {
            self.regs.switch_bank(Bank::User);
        }

        let count = count as usize;
        if count != 0 {
            let mut words = [0; 16];
            let words = &mut words[..count];
            let cycles = if load {
                let cycles = bus.timed_read_block(addr, words);
                if writeback && !V5 {
                    self.regs.r[rn] = new_base;
                }
                let mut left = list;
                for &value in words.iter() {
                    self.regs.r[left.trailing_zeros() as usize] = value;
                    left &= left - 1;
                }
                cycles
            } else {
                let mut left = list;
                for (index, value) in words.iter_mut().enumerate() {
                    let reg = left.trailing_zeros() as usize;
                    left &= left - 1;
                    *value = if reg != rn || (V5 && user_bank && rn != 15) {
                        self.regs.r[reg]
                    } else if V5 {
                        base
                    } else if writeback && index != 0 {
                        new_base
                    } else {
                        self.regs.r[reg]
                    };
                }
                let cycles = bus.timed_write_block(addr, words);
                if writeback && !V5 {
                    self.regs.r[rn] = new_base;
                }
                cycles
            };
            if V5 {
                self.data += cycles;
            } else {
                self.cycles += cycles;
            }
            self.next_fetch = Access::NonSequential;
        }

        if user_bank {
            self.regs.switch_bank(bank);
        }
        let loaded_base = load && list != 1 << rn && list >> rn == 1;
        if V5 && writeback && !loaded_base {
            self.regs.r[rn] = new_base;
        }
        let loads_pc = load && list & (1 << 15) != 0;
        if load {
            self.load_idle(bus);
            self.ldm_glitch = !V5 && user_bank && bank != Bank::User;
        }
        if writeback && rn == 15 && !loads_pc {
            self.reload(bus);
        } else if loads_pc {
            if user {
                self.restore_cpsr();
            } else if V5 {
                self.load_pc_exchange(bus, self.regs.r[15]);
                self.take_refetch();
                return;
            }
            if !V5 {
                bus.jump(Jump::BlockLoad);
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

    #[test]
    fn every_register_list_counts_its_registers() {
        for list in 0..=0xFFFF_u32 {
            assert_eq!(super::list_len(list), list.count_ones());
        }
    }
}
