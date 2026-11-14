use super::super::{Core, CpuBus};
use super::alu::{IMMEDIATE, SHIFT_BY_IMMEDIATE, SHIFT_BY_REGISTER, operand_form};
use super::transfer::{ANY_FORM, FORM_BITS, OFFSET_DOWN, OFFSET_UP};

pub(super) const ENTRIES: usize = 4096;
const DEBUG_COPROCESSOR: u32 = 14;
const OUTSIDE_SAMPLES: [u32; 4] = [0x0000_0000, 0x000F_FF00, 0x0000_0E00, 0x0000_0100];
const BX_SLOT: usize = 0x121;

pub(super) type ArmHandler<const V5: bool, B> = fn(&mut Core<V5>, &mut B, u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ArmKind {
    DataProcessing,
    BranchExchange,
    Swap,
    Multiply,
    MultiplyLong,
    Halfword,
    V5Misc,
    PsrTransfer,
    SingleTransfer,
    BlockTransfer,
    Branch,
    SoftwareInterrupt,
    CoprocessorTransfer,
    DebugTransfer,
    Undefined,
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(super) fn index(op: u32) -> usize {
    ((op & 0x0FF0_00F0).wrapping_mul(0x0001_0010) >> 20) as usize
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
pub(super) fn index(op: u32) -> usize {
    (((op >> 16) & 0xFF0) | ((op >> 4) & 0xF)) as usize
}

const fn word(index: usize) -> u32 {
    ((index as u32 & 0xFF0) << 16) | ((index as u32 & 0xF) << 4)
}

impl<const V5: bool> Core<V5> {
    pub(super) const fn arm_kind(op: u32) -> ArmKind {
        match (op >> 25) & 7 {
            0b000 => {
                if op & 0x90 != 0x90 && op & 0x0190_0000 != 0x0100_0000 {
                    ArmKind::DataProcessing
                } else if op & 0x0FFF_FFF0 == 0x012F_FF10 {
                    ArmKind::BranchExchange
                } else if op & 0x0FB0_0FF0 == 0x0100_0090 {
                    ArmKind::Swap
                } else if op & 0x0FC0_00F0 == 0x0000_0090 {
                    ArmKind::Multiply
                } else if op & 0x0F80_00F0 == 0x0080_0090 {
                    ArmKind::MultiplyLong
                } else if op & 0x0000_0090 == 0x0000_0090 {
                    ArmKind::Halfword
                } else if V5 && op & 0xF0 != 0 {
                    ArmKind::V5Misc
                } else {
                    ArmKind::PsrTransfer
                }
            }
            0b001 => {
                if op & 0x0190_0000 == 0x0100_0000 {
                    ArmKind::PsrTransfer
                } else {
                    ArmKind::DataProcessing
                }
            }
            0b010 => ArmKind::SingleTransfer,
            0b011 if op & 0x10 == 0 => ArmKind::SingleTransfer,
            0b100 => ArmKind::BlockTransfer,
            0b101 => ArmKind::Branch,
            0b111 if op & (1 << 24) != 0 => ArmKind::SoftwareInterrupt,
            0b111 if V5 && op & 0x10 != 0 => ArmKind::CoprocessorTransfer,
            0b111 if op & 0x10 != 0 && (op >> 8) & 0xF == DEBUG_COPROCESSOR => {
                ArmKind::DebugTransfer
            }
            _ => ArmKind::Undefined,
        }
    }

    const fn settled_kind(index: usize) -> Option<ArmKind> {
        let word = word(index);
        let kind = Self::arm_kind(word);
        let mut sample = 1;
        while sample < OUTSIDE_SAMPLES.len() {
            if Self::arm_kind(word | OUTSIDE_SAMPLES[sample]) as u8 != kind as u8 {
                return None;
            }
            sample += 1;
        }
        Some(kind)
    }

    pub(super) const fn arm_table<B: CpuBus>() -> [ArmHandler<V5, B>; ENTRIES] {
        let mut table = [Self::execute_arm_decoded::<B> as ArmHandler<V5, B>; ENTRIES];
        let mut index = 0;
        while index < ENTRIES {
            table[index] = match Self::settled_kind(index) {
                Some(ArmKind::DataProcessing) => Self::alu_entry::<B>(word(index)),
                Some(ArmKind::SingleTransfer) => Self::load_store_entry::<B>(word(index)),
                Some(ArmKind::Halfword) => Self::halfword_entry::<B>(word(index)),
                Some(ArmKind::BlockTransfer) => Self::block_entry::<B>(word(index)),
                Some(kind) => Self::arm_handler::<B>(kind),
                None if index == BX_SLOT => Self::arm_bx_slot::<B>,
                None => Self::execute_arm_decoded::<B>,
            };
            index += 1;
        }
        table
    }

    const fn alu_entry<B: CpuBus>(op: u32) -> ArmHandler<V5, B> {
        match (op >> 21) & 0xF {
            0x0 => Self::alu_flags_entry::<B, 0x0>(op),
            0x1 => Self::alu_flags_entry::<B, 0x1>(op),
            0x2 => Self::alu_flags_entry::<B, 0x2>(op),
            0x3 => Self::alu_flags_entry::<B, 0x3>(op),
            0x4 => Self::alu_flags_entry::<B, 0x4>(op),
            0x5 => Self::alu_flags_entry::<B, 0x5>(op),
            0x6 => Self::alu_flags_entry::<B, 0x6>(op),
            0x7 => Self::alu_flags_entry::<B, 0x7>(op),
            0x8 => Self::alu_flags_entry::<B, 0x8>(op),
            0x9 => Self::alu_flags_entry::<B, 0x9>(op),
            0xA => Self::alu_flags_entry::<B, 0xA>(op),
            0xB => Self::alu_flags_entry::<B, 0xB>(op),
            0xC => Self::alu_flags_entry::<B, 0xC>(op),
            0xD => Self::alu_flags_entry::<B, 0xD>(op),
            0xE => Self::alu_flags_entry::<B, 0xE>(op),
            _ => Self::alu_flags_entry::<B, 0xF>(op),
        }
    }

    const fn alu_flags_entry<B: CpuBus, const OPCODE: u32>(op: u32) -> ArmHandler<V5, B> {
        if op & (1 << 20) != 0 {
            Self::alu_form_entry::<B, OPCODE, true>(op)
        } else {
            Self::alu_form_entry::<B, OPCODE, false>(op)
        }
    }

    const fn alu_form_entry<B: CpuBus, const OPCODE: u32, const FLAGS: bool>(
        op: u32,
    ) -> ArmHandler<V5, B> {
        match operand_form(op) {
            IMMEDIATE => Self::arm_alu::<B, OPCODE, FLAGS, IMMEDIATE>,
            SHIFT_BY_REGISTER => Self::arm_alu::<B, OPCODE, FLAGS, SHIFT_BY_REGISTER>,
            _ => Self::arm_alu::<B, OPCODE, FLAGS, SHIFT_BY_IMMEDIATE>,
        }
    }

    const fn load_store_entry<B: CpuBus>(op: u32) -> ArmHandler<V5, B> {
        match op & FORM_BITS {
            OFFSET_UP => Self::load_store_form_entry::<B, OFFSET_UP>(op),
            OFFSET_DOWN => Self::load_store_form_entry::<B, OFFSET_DOWN>(op),
            _ => Self::load_store_form_entry::<B, ANY_FORM>(op),
        }
    }

    const fn load_store_form_entry<B: CpuBus, const FORM: u32>(op: u32) -> ArmHandler<V5, B> {
        let register = op & (1 << 25) != 0;
        match (op & (1 << 20) != 0, op & (1 << 22) != 0) {
            (true, true) if register => Self::arm_load_store::<B, true, true, true, FORM>,
            (true, true) => Self::arm_load_store::<B, true, true, false, FORM>,
            (true, false) if register => Self::arm_load_store::<B, true, false, true, FORM>,
            (true, false) => Self::arm_load_store::<B, true, false, false, FORM>,
            (false, true) if register => Self::arm_load_store::<B, false, true, true, FORM>,
            (false, true) => Self::arm_load_store::<B, false, true, false, FORM>,
            (false, false) if register => Self::arm_load_store::<B, false, false, true, FORM>,
            (false, false) => Self::arm_load_store::<B, false, false, false, FORM>,
        }
    }

    const fn block_entry<B: CpuBus>(op: u32) -> ArmHandler<V5, B> {
        if op & (1 << 22) != 0 {
            return Self::arm_block_transfer;
        }
        let load = op & (1 << 20) != 0;
        let up = op & (1 << 23) != 0;
        let pre = op & (1 << 24) != 0;
        let writeback = op & (1 << 21) != 0;
        match (load, up, pre, writeback) {
            (false, false, false, false) => Self::arm_block::<B, false, false, false, false>,
            (false, false, false, true) => Self::arm_block::<B, false, false, false, true>,
            (false, false, true, false) => Self::arm_block::<B, false, false, true, false>,
            (false, false, true, true) => Self::arm_block::<B, false, false, true, true>,
            (false, true, false, false) => Self::arm_block::<B, false, true, false, false>,
            (false, true, false, true) => Self::arm_block::<B, false, true, false, true>,
            (false, true, true, false) => Self::arm_block::<B, false, true, true, false>,
            (false, true, true, true) => Self::arm_block::<B, false, true, true, true>,
            (true, false, false, false) => Self::arm_block::<B, true, false, false, false>,
            (true, false, false, true) => Self::arm_block::<B, true, false, false, true>,
            (true, false, true, false) => Self::arm_block::<B, true, false, true, false>,
            (true, false, true, true) => Self::arm_block::<B, true, false, true, true>,
            (true, true, false, false) => Self::arm_block::<B, true, true, false, false>,
            (true, true, false, true) => Self::arm_block::<B, true, true, false, true>,
            (true, true, true, false) => Self::arm_block::<B, true, true, true, false>,
            (true, true, true, true) => Self::arm_block::<B, true, true, true, true>,
        }
    }

    const fn halfword_entry<B: CpuBus>(op: u32) -> ArmHandler<V5, B> {
        match op & FORM_BITS {
            OFFSET_UP => Self::halfword_form_entry::<B, OFFSET_UP>(op),
            OFFSET_DOWN => Self::halfword_form_entry::<B, OFFSET_DOWN>(op),
            _ => Self::halfword_form_entry::<B, ANY_FORM>(op),
        }
    }

    const fn halfword_form_entry<B: CpuBus, const FORM: u32>(op: u32) -> ArmHandler<V5, B> {
        match (op & (1 << 20) != 0, op & (1 << 22) != 0) {
            (true, true) => Self::halfword_kind_entry::<B, true, true, FORM>(op),
            (true, false) => Self::halfword_kind_entry::<B, true, false, FORM>(op),
            (false, true) => Self::halfword_kind_entry::<B, false, true, FORM>(op),
            (false, false) => Self::halfword_kind_entry::<B, false, false, FORM>(op),
        }
    }

    const fn halfword_kind_entry<
        B: CpuBus,
        const LOADS: bool,
        const IMMEDIATE: bool,
        const FORM: u32,
    >(
        op: u32,
    ) -> ArmHandler<V5, B> {
        match (op >> 5) & 3 {
            0 => Self::arm_halfword::<B, LOADS, IMMEDIATE, 0, FORM>,
            1 => Self::arm_halfword::<B, LOADS, IMMEDIATE, 1, FORM>,
            2 => Self::arm_halfword::<B, LOADS, IMMEDIATE, 2, FORM>,
            _ => Self::arm_halfword::<B, LOADS, IMMEDIATE, 3, FORM>,
        }
    }

    pub(super) const fn arm_handler<B: CpuBus>(kind: ArmKind) -> ArmHandler<V5, B> {
        match kind {
            ArmKind::DataProcessing => Self::arm_data_processing,
            ArmKind::BranchExchange => Self::arm_bx,
            ArmKind::Swap => Self::arm_swap,
            ArmKind::Multiply => Self::arm_multiply,
            ArmKind::MultiplyLong => Self::arm_multiply_long,
            ArmKind::Halfword => Self::arm_halfword_transfer,
            ArmKind::V5Misc => Self::arm_v5_misc,
            ArmKind::PsrTransfer => Self::arm_psr_transfer,
            ArmKind::SingleTransfer => Self::arm_single_transfer,
            ArmKind::BlockTransfer => Self::arm_block_transfer,
            ArmKind::Branch => Self::arm_branch,
            ArmKind::SoftwareInterrupt => Self::arm_software_interrupt,
            ArmKind::CoprocessorTransfer => Self::arm_coprocessor_transfer,
            ArmKind::DebugTransfer => Self::arm_debug_transfer,
            ArmKind::Undefined => Self::arm_undefined_word,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::gba::bus::Bus;
    use crate::gba::cpu::CpuState;
    use crate::gba::wait::{Access, Width};

    fn random() -> impl Iterator<Item = u32> {
        let mut state = 0x9E37_79B9u32;
        std::iter::from_fn(move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            Some(state)
        })
    }

    fn fills() -> impl Iterator<Item = u32> {
        OUTSIDE_SAMPLES
            .into_iter()
            .chain([0x000F_FF0F, 0xF000_0000, 0x0000_E000])
            .chain(random().take(96))
    }

    fn check<const V5: bool>() {
        for index in 0..ENTRIES {
            assert_eq!(super::index(word(index)), index);
            let settled = Core::<V5>::settled_kind(index);
            for fill in fills() {
                let op = word(index) | (fill & !0x0FF0_00F0);
                assert_eq!(super::index(op), index);
                let kind = Core::<V5>::arm_kind(op);
                assert!(
                    settled.is_none_or(|settled| settled == kind),
                    "{op:08X}: table {settled:?}, decode {kind:?}"
                );
            }
        }
    }

    #[test]
    fn each_table_entry_matches_the_decode_of_every_word_it_covers() {
        check::<false>();
        check::<true>();
    }

    fn run<const V5: bool>(
        state: &CpuState,
        bus: &Bus,
        op: u32,
        table: bool,
    ) -> (Core<V5>, Vec<u32>) {
        let mut core = Core::<V5>::from_state(state);
        let mut bus = bus.clone();
        if table {
            core.execute_arm(&mut bus, op);
        } else {
            core.execute_arm_decoded(&mut bus, op);
        }
        let memory = (0x0200_0000..0x0200_2400)
            .step_by(4)
            .map(|addr| CpuBus::read(&mut bus, addr, Width::Word, Access::Sequential))
            .collect();
        (core, memory)
    }

    fn differential<const V5: bool>() {
        let bus = Bus::new(&[0x5A; 0x400]);
        let mut fill = random();
        for index in 0..ENTRIES {
            let bx_slot = index == BX_SLOT;
            if !bx_slot
                && !matches!(
                    Core::<V5>::settled_kind(index),
                    Some(
                        ArmKind::DataProcessing
                            | ArmKind::SingleTransfer
                            | ArmKind::BlockTransfer
                            | ArmKind::Halfword
                    )
                )
            {
                continue;
            }
            for round in 0..if bx_slot { 16 } else { 3 } {
                let seed = fill.next().unwrap();
                let sbo = if bx_slot && round % 2 == 0 {
                    0x000F_FF00
                } else {
                    0
                };
                let op = 0xE000_0000 | word(index) | (seed & 0x000F_FF0F) | sbo;
                let flags = seed << 28;
                let mode = if seed & 0x100 != 0 { 0x13 } else { 0x1F };
                let mut state = CpuState {
                    cpsr: flags | mode,
                    spsr: [flags | 0x1F; 5],
                    ..CpuState::default()
                };
                for (i, r) in state.r.iter_mut().enumerate().take(15) {
                    *r = 0x0200_0800 + (i as u32) * 0x44 + (seed.rotate_left(i as u32) & 0x1F);
                }
                state.r[15] = 0x0800_0008;
                let table = run::<V5>(&state, &bus, op, true);
                let decoded = run::<V5>(&state, &bus, op, false);
                assert!(table == decoded, "{op:08X}");
            }
        }
    }

    #[test]
    fn data_processing_transfer_and_bx_entries_run_like_the_full_decode() {
        differential::<false>();
        differential::<true>();
    }

    #[test]
    fn on_the_arm9_only_swap_and_bx_fall_back_to_the_full_decode() {
        let open = (0..ENTRIES)
            .filter(|&index| Core::<true>::settled_kind(index).is_none())
            .collect::<Vec<_>>();
        assert_eq!(open, [0x109, 0x121, 0x149]);
    }
}
