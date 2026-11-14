mod cache;
mod protection;

use crate::gba::cpu::CoprocessorRegister;
pub use cache::CacheMap;
use protection::Protection;

pub const MAIN_ID: u32 = 0x4105_9461;
pub const CACHE_TYPE: u32 = 0x0F0D_2112;
pub const TCM_SIZE: u32 = 0x0014_0180;
pub const DIRECT_BOOT_CONTROL: u32 = 0x0005_2078;
pub const DIRECT_BOOT_DTCM: u32 = 0x0300_000A;
pub const DIRECT_BOOT_ITCM: u32 = 0x0000_0020;

const CP15: u32 = 15;
const CONTROL_RESET: u32 = 0x0000_2078;
const CONTROL_FIXED: u32 = 0x0000_0078;
const CONTROL_WRITABLE: u32 = 0x000F_F085;
const PROTECTION_ON: u32 = 1 << 0;
const DCACHE_ON: u32 = 1 << 2;
const ICACHE_ON: u32 = 1 << 12;
const HIGH_VECTORS: u32 = 1 << 13;
const DTCM_ON: u32 = 1 << 16;
const DTCM_LOAD: u32 = 1 << 17;
const ITCM_ON: u32 = 1 << 18;
const ITCM_LOAD: u32 = 1 << 19;
const HIGH_VECTOR_BASE: u32 = 0xFFFF_0000;
const DTCM_MASK: u32 = 0xFFFF_F03E;
const ITCM_MASK: u32 = 0x0000_003E;
const TCM_BASE: u32 = 0xFFFF_F000;
const TCM_SMALLEST: u32 = 3;
const TCM_LARGEST: u32 = 23;
const LOCKDOWN_MASK: u32 = 0x8000_0003;
const TEST_STATE_MASK: u32 = 0x0000_1E00;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TcmRegion {
    pub base: u32,
    pub virtual_size: u64,
    pub enabled: bool,
    pub load_mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    Code,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cp15Write {
    Done,
    TcmMoved,
    Halt,
    Undefined,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct CacheInputs {
    control: u32,
    protection: Protection,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cp15 {
    control: u32,
    protection: Protection,
    lockdown: [u32; 2],
    dtcm: u32,
    itcm: u32,
    trace_process_id: u32,
    test_state: u32,
}

impl Default for Cp15 {
    fn default() -> Self {
        Self {
            control: CONTROL_RESET,
            protection: Protection::default(),
            lockdown: [0; 2],
            dtcm: 0,
            itcm: 0,
            trace_process_id: 0,
            test_state: 0,
        }
    }
}

impl Cp15 {
    pub fn read(&self, register: CoprocessorRegister) -> Option<u32> {
        if register.coprocessor != CP15 {
            return None;
        }
        let CoprocessorRegister {
            opcode1,
            crn,
            crm,
            opcode2,
            ..
        } = register;
        Some(match (opcode1, crn, crm, opcode2) {
            (0, 0, 0, 1) => CACHE_TYPE,
            (0, 0, 0, 2) => TCM_SIZE,
            (0, 0, 0, _) => MAIN_ID,
            (0, 1, 0, 0) => self.control,
            (0, 2 | 3 | 5 | 6, ..) => self.protection.read(crn, crm, opcode2),
            (0, 9, 0, side @ 0..=1) => self.lockdown[side as usize],
            (0, 9, 1, 0) => self.dtcm,
            (0, 9, 1, 1) => self.itcm,
            (0, 13, 0..=1, 1) => self.trace_process_id,
            (0, 15, 0, 0) => self.test_state,
            _ => 0,
        })
    }

    pub fn write(&mut self, register: CoprocessorRegister, value: u32) -> Cp15Write {
        if register.coprocessor != CP15 {
            return Cp15Write::Undefined;
        }
        let CoprocessorRegister {
            opcode1,
            crn,
            crm,
            opcode2,
            ..
        } = register;
        let placed = (self.itcm(), self.dtcm());
        match (opcode1, crn, crm, opcode2) {
            (0, 1, 0, 0) => self.control = (value & CONTROL_WRITABLE) | CONTROL_FIXED,
            (0, 2 | 3 | 5 | 6, ..) => self.protection.write(crn, crm, opcode2, value),
            (0, 7, 0, 4) | (0, 7, 8, 2) => return Cp15Write::Halt,
            (0, 9, 0, side @ 0..=1) => self.lockdown[side as usize] = value & LOCKDOWN_MASK,
            (0, 9, 1, 0) => self.dtcm = value & DTCM_MASK,
            (0, 9, 1, 1) => self.itcm = value & ITCM_MASK,
            (0, 13, 0..=1, 1) => self.trace_process_id = value,
            (0, 15, 0, 0) => self.test_state = value & TEST_STATE_MASK,
            _ => {}
        }
        if (self.itcm(), self.dtcm()) == placed {
            Cp15Write::Done
        } else {
            Cp15Write::TcmMoved
        }
    }

    pub fn control(&self) -> u32 {
        self.control
    }

    pub fn cached(&self, addr: u32, fetch: Fetch) -> bool {
        let cache = match fetch {
            Fetch::Code => ICACHE_ON,
            Fetch::Data => DCACHE_ON,
        };
        self.control & PROTECTION_ON != 0
            && self.control & cache != 0
            && self.protection.cacheable(addr, fetch)
    }

    fn cache_inputs(&self) -> CacheInputs {
        CacheInputs {
            control: self.control & (PROTECTION_ON | DCACHE_ON | ICACHE_ON),
            protection: self.protection.clone(),
        }
    }

    pub fn vector_base(&self) -> u32 {
        if self.control & HIGH_VECTORS != 0 {
            HIGH_VECTOR_BASE
        } else {
            0
        }
    }

    pub fn itcm(&self) -> TcmRegion {
        TcmRegion {
            base: 0,
            virtual_size: virtual_size(self.itcm),
            enabled: self.control & ITCM_ON != 0,
            load_mode: self.control & ITCM_LOAD != 0,
        }
    }

    #[cfg(jit)]
    pub(crate) fn dtcm_word(&self) -> *const u32 {
        &self.dtcm
    }

    pub fn dtcm(&self) -> TcmRegion {
        let virtual_size = virtual_size(self.dtcm);
        TcmRegion {
            base: self.dtcm & TCM_BASE & !(virtual_size - 1) as u32,
            virtual_size,
            enabled: self.control & DTCM_ON != 0,
            load_mode: self.control & DTCM_LOAD != 0,
        }
    }

    pub fn allows(&self, addr: u32, fetch: Fetch, write: bool, privileged: bool) -> bool {
        self.control & PROTECTION_ON == 0 || self.protection.allows(addr, fetch, write, privileged)
    }

    pub fn direct_boot(&mut self) {
        self.control = DIRECT_BOOT_CONTROL;
        self.protection.direct_boot();
        self.dtcm = DIRECT_BOOT_DTCM;
        self.itcm = DIRECT_BOOT_ITCM;
    }
}

fn virtual_size(setting: u32) -> u64 {
    512 << ((setting >> 1) & 0x1F).clamp(TCM_SMALLEST, TCM_LARGEST)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DTCM_AT_27C0000: u32 = 0x027C_000A;

    fn cp(coprocessor: u32, opcode1: u32, crn: u32, crm: u32, opcode2: u32) -> CoprocessorRegister {
        CoprocessorRegister {
            coprocessor,
            opcode1,
            crn,
            crm,
            opcode2,
        }
    }

    fn reg(crn: u32, crm: u32, opcode2: u32) -> CoprocessorRegister {
        cp(15, 0, crn, crm, opcode2)
    }

    fn read(cp15: &Cp15, crn: u32, crm: u32, opcode2: u32) -> u32 {
        cp15.read(reg(crn, crm, opcode2)).unwrap()
    }

    fn write(cp15: &mut Cp15, crn: u32, crm: u32, opcode2: u32, value: u32) -> Cp15Write {
        cp15.write(reg(crn, crm, opcode2), value)
    }

    fn control_after(value: u32) -> u32 {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 1, 0, 0, value);
        read(&cp15, 1, 0, 0)
    }

    fn with_control(bits: u32) -> Cp15 {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 1, 0, 0, bits);
        cp15
    }

    #[test]
    fn other_coprocessors_are_undefined() {
        let mut cp15 = Cp15::default();
        assert_eq!(cp15.read(cp(14, 0, 0, 0, 0)), None);
        assert_eq!(cp15.write(cp(14, 0, 0, 0, 0), 0), Cp15Write::Undefined);
    }

    #[test]
    fn main_id_is_an_arm946e_s_revision_1() {
        assert_eq!(read(&Cp15::default(), 0, 0, 0), 0x4105_9461);
    }

    #[test]
    fn cache_type_is_8kb_code_and_4kb_data() {
        assert_eq!(read(&Cp15::default(), 0, 0, 1), 0x0F0D_2112);
    }

    #[test]
    fn tcm_size_is_32kb_itcm_and_16kb_dtcm() {
        assert_eq!(read(&Cp15::default(), 0, 0, 2), 0x0014_0180);
    }

    #[test]
    fn reserved_id_registers_repeat_the_main_id() {
        let cp15 = Cp15::default();
        for opcode2 in 3..=7 {
            assert_eq!(read(&cp15, 0, 0, opcode2), MAIN_ID);
        }
    }

    #[test]
    fn id_registers_ignore_writes() {
        let mut cp15 = Cp15::default();
        assert_eq!(write(&mut cp15, 0, 0, 0, 0), Cp15Write::Done);
        assert_eq!(read(&cp15, 0, 0, 0), MAIN_ID);
    }

    #[test]
    fn control_resets_with_high_vectors_and_the_fixed_bits() {
        assert_eq!(read(&Cp15::default(), 1, 0, 0), 0x0000_2078);
    }

    #[test]
    fn control_bits_3_to_6_always_read_1() {
        assert_eq!(control_after(0), 0x0000_0078);
    }

    #[test]
    fn control_bits_1_8_to_11_and_20_up_always_read_0() {
        assert_eq!(control_after(0xFFF0_0F02), 0x0000_0078);
    }

    #[test]
    fn control_bit_0_turns_the_protection_unit_on() {
        assert_eq!(control_after(1 << 0), 0x79);
        assert!(Cp15::default().allows(0x0200_0000, Fetch::Data, true, false));
        assert!(!with_control(1).allows(0x0200_0000, Fetch::Data, true, false));
    }

    #[test]
    fn control_bit_2_data_cache_is_kept() {
        assert_eq!(control_after(1 << 2), 0x7C);
    }

    #[test]
    fn control_bit_7_big_endian_is_kept() {
        assert_eq!(control_after(1 << 7), 0xF8);
    }

    #[test]
    fn control_bit_12_code_cache_is_kept() {
        assert_eq!(control_after(1 << 12), 0x1078);
    }

    #[test]
    fn control_bit_13_moves_the_vectors_to_ffff0000() {
        assert_eq!(control_after(1 << 13), 0x2078);
        assert_eq!(with_control(1 << 13).vector_base(), 0xFFFF_0000);
        assert_eq!(with_control(0).vector_base(), 0);
    }

    #[test]
    fn control_bit_14_round_robin_is_kept() {
        assert_eq!(control_after(1 << 14), 0x4078);
    }

    #[test]
    fn control_bit_15_pre_armv5_mode_is_kept() {
        assert_eq!(control_after(1 << 15), 0x8078);
    }

    #[test]
    fn control_bit_16_turns_dtcm_on() {
        assert_eq!(control_after(1 << 16), 0x1_0078);
        assert!(with_control(1 << 16).dtcm().enabled);
        assert!(!with_control(1 << 16).itcm().enabled);
    }

    #[test]
    fn control_bit_17_is_dtcm_load_mode() {
        assert_eq!(control_after(1 << 17), 0x2_0078);
        assert!(with_control(1 << 17).dtcm().load_mode);
        assert!(!with_control(1 << 17).itcm().load_mode);
    }

    #[test]
    fn control_bit_18_turns_itcm_on() {
        assert_eq!(control_after(1 << 18), 0x4_0078);
        assert!(with_control(1 << 18).itcm().enabled);
        assert!(!with_control(1 << 18).dtcm().enabled);
    }

    #[test]
    fn control_bit_19_is_itcm_load_mode() {
        assert_eq!(control_after(1 << 19), 0x8_0078);
        assert!(with_control(1 << 19).itcm().load_mode);
        assert!(!with_control(1 << 19).dtcm().load_mode);
    }

    #[test]
    fn turning_a_tcm_on_or_off_moves_it() {
        let mut cp15 = Cp15::default();
        assert_eq!(write(&mut cp15, 1, 0, 0, ITCM_ON), Cp15Write::TcmMoved);
        assert_eq!(write(&mut cp15, 1, 0, 0, ITCM_ON | 1), Cp15Write::Done);
        assert_eq!(write(&mut cp15, 1, 0, 0, 1), Cp15Write::TcmMoved);
    }

    #[test]
    fn protection_registers_reach_the_protection_unit() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 2, 0, 0, 0x42);
        write(&mut cp15, 3, 0, 0, 0x02);
        write(&mut cp15, 5, 0, 2, 0x1511_1011);
        write(&mut cp15, 6, 7, 0, 0x027F_F017);
        assert_eq!(read(&cp15, 2, 0, 0), 0x42);
        assert_eq!(read(&cp15, 3, 0, 0), 0x02);
        assert_eq!(read(&cp15, 5, 0, 2), 0x1511_1011);
        assert_eq!(read(&cp15, 6, 7, 0), 0x027F_F017);
    }

    #[test]
    fn wait_for_interrupt_asks_for_a_halt() {
        let mut cp15 = Cp15::default();
        assert_eq!(write(&mut cp15, 7, 0, 4, 0), Cp15Write::Halt);
        assert_eq!(write(&mut cp15, 7, 8, 2, 0), Cp15Write::Halt);
    }

    #[test]
    fn cache_operations_do_nothing() {
        let mut cp15 = Cp15::default();
        cp15.direct_boot();
        let before = cp15.clone();
        for (crm, opcode2) in [(5, 0), (5, 1), (6, 0), (6, 1), (10, 1), (10, 2), (10, 4)] {
            assert_eq!(
                write(&mut cp15, 7, crm, opcode2, 0x0200_0000),
                Cp15Write::Done
            );
        }
        for (crm, opcode2) in [(13, 1), (14, 1), (14, 2)] {
            assert_eq!(
                write(&mut cp15, 7, crm, opcode2, 0x0200_0000),
                Cp15Write::Done
            );
        }
        assert_eq!(cp15, before);
    }

    #[test]
    fn cache_operations_read_0() {
        assert_eq!(read(&Cp15::default(), 7, 0, 4), 0);
        assert_eq!(read(&Cp15::default(), 7, 5, 0), 0);
    }

    #[test]
    fn cache_lockdown_keeps_the_load_bit_and_segment() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 9, 0, 0, 0xFFFF_FFFE);
        write(&mut cp15, 9, 0, 1, 0x0000_0001);
        assert_eq!(read(&cp15, 9, 0, 0), 0x8000_0002);
        assert_eq!(read(&cp15, 9, 0, 1), 0x0000_0001);
    }

    #[test]
    fn dtcm_region_reads_back_without_its_reserved_bits() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 9, 1, 0, 0x027C_0FFF);
        assert_eq!(read(&cp15, 9, 1, 0), 0x027C_003E);
    }

    #[test]
    fn itcm_region_reads_back_only_its_size() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 9, 1, 1, 0x0100_0FFF);
        assert_eq!(read(&cp15, 9, 1, 1), 0x0000_003E);
    }

    #[test]
    fn dtcm_sits_at_its_base_with_512_shl_n_bytes() {
        let mut cp15 = with_control(DTCM_ON);
        write(&mut cp15, 9, 1, 0, DTCM_AT_27C0000);
        assert_eq!(
            cp15.dtcm(),
            TcmRegion {
                base: 0x027C_0000,
                virtual_size: 0x4000,
                enabled: true,
                load_mode: false,
            }
        );
    }

    #[test]
    fn itcm_stays_at_0_whatever_base_is_written() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 9, 1, 1, 0x0100_000C);
        assert_eq!(cp15.itcm().base, 0);
        assert_eq!(cp15.itcm().virtual_size, 0x8000);
    }

    #[test]
    fn a_tcm_virtual_size_larger_than_the_chip_leaves_room_for_mirrors() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 9, 1, 1, 0x20);
        assert_eq!(cp15.itcm().virtual_size, 0x0200_0000);
    }

    #[test]
    fn tcm_sizes_below_4kb_count_as_4kb() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 9, 1, 0, 0x0300_0002);
        assert_eq!(cp15.dtcm().virtual_size, 0x1000);
        assert_eq!(cp15.dtcm().base, 0x0300_0000);
    }

    #[test]
    fn tcm_sizes_stop_at_4gb() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 9, 1, 0, 0x0300_003E);
        assert_eq!(cp15.dtcm().virtual_size, 0x1_0000_0000);
        assert_eq!(cp15.dtcm().base, 0);
    }

    #[test]
    fn a_dtcm_base_off_its_size_rounds_down() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 9, 1, 0, 0x027C_200A);
        assert_eq!(cp15.dtcm().base, 0x027C_0000);
    }

    #[test]
    fn moving_a_tcm_is_reported() {
        let mut cp15 = with_control(DTCM_ON);
        assert_eq!(
            write(&mut cp15, 9, 1, 0, DTCM_AT_27C0000),
            Cp15Write::TcmMoved
        );
        assert_eq!(write(&mut cp15, 9, 1, 0, DTCM_AT_27C0000), Cp15Write::Done);
        assert_eq!(write(&mut cp15, 9, 1, 1, 0x20), Cp15Write::TcmMoved);
    }

    #[test]
    fn fcse_process_id_reads_0() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 13, 0, 0, 0xFE00_0000);
        assert_eq!(read(&cp15, 13, 0, 0), 0);
    }

    #[test]
    fn trace_process_id_has_two_names() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 13, 0, 1, 0x1234_5678);
        assert_eq!(read(&cp15, 13, 1, 1), 0x1234_5678);
        write(&mut cp15, 13, 1, 1, 0x8765_4321);
        assert_eq!(read(&cp15, 13, 0, 1), 0x8765_4321);
    }

    #[test]
    fn test_state_keeps_the_linefill_and_streaming_bits() {
        let mut cp15 = Cp15::default();
        write(&mut cp15, 15, 0, 0, 0xFFFF_FFFF);
        assert_eq!(read(&cp15, 15, 0, 0), 0x0000_1E00);
    }

    #[test]
    fn bist_and_cache_debug_registers_read_0() {
        let mut cp15 = Cp15::default();
        for opcode1 in 0..=3 {
            assert_eq!(cp15.write(cp(15, opcode1, 15, 0, 1), 0x5), Cp15Write::Done);
            assert_eq!(cp15.read(cp(15, opcode1, 15, 0, 1)), Some(0));
        }
    }

    #[test]
    fn registers_the_chip_lacks_read_0_and_ignore_writes() {
        let mut cp15 = Cp15::default();
        let before = cp15.clone();
        for crn in [4, 8, 10, 11, 12, 14] {
            assert_eq!(write(&mut cp15, crn, 0, 0, u32::MAX), Cp15Write::Done);
            assert_eq!(read(&cp15, crn, 0, 0), 0);
        }
        assert_eq!(cp15.write(cp(15, 1, 1, 0, 0), u32::MAX), Cp15Write::Done);
        assert_eq!(cp15.read(cp(15, 1, 1, 0, 0)), Some(0));
        assert_eq!(cp15, before);
    }

    #[test]
    fn direct_boot_sets_control_to_00052078() {
        let mut cp15 = Cp15::default();
        cp15.direct_boot();
        assert_eq!(read(&cp15, 1, 0, 0), 0x0005_2078);
        assert_eq!(cp15.vector_base(), 0xFFFF_0000);
    }

    #[test]
    fn direct_boot_leaves_itcm_on_with_32mb_of_mirrors_at_0() {
        let mut cp15 = Cp15::default();
        cp15.direct_boot();
        assert_eq!(
            cp15.itcm(),
            TcmRegion {
                base: 0,
                virtual_size: 0x0200_0000,
                enabled: true,
                load_mode: false,
            }
        );
    }

    #[test]
    fn direct_boot_leaves_16kb_of_dtcm_on_at_03000000() {
        let mut cp15 = Cp15::default();
        cp15.direct_boot();
        assert_eq!(
            cp15.dtcm(),
            TcmRegion {
                base: 0x0300_0000,
                virtual_size: 0x4000,
                enabled: true,
                load_mode: false,
            }
        );
    }

    #[test]
    fn direct_boot_leaves_the_protection_unit_off() {
        let mut cp15 = Cp15::default();
        cp15.direct_boot();
        assert!(cp15.allows(0x0100_0000, Fetch::Data, true, false));
    }

    #[test]
    fn main_ram_is_cached_only_with_the_protection_unit_and_that_cache_on() {
        let mut cp15 = Cp15::default();
        cp15.direct_boot();
        assert!(!cp15.cached(0x0200_0000, Fetch::Code));
        cp15.write(
            reg(1, 0, 0),
            DIRECT_BOOT_CONTROL | PROTECTION_ON | ICACHE_ON,
        );
        assert!(cp15.cached(0x0200_0000, Fetch::Code));
        assert!(!cp15.cached(0x0200_0000, Fetch::Data));
        assert!(!cp15.cached(0x0400_0000, Fetch::Code));
        cp15.write(
            reg(1, 0, 0),
            DIRECT_BOOT_CONTROL | PROTECTION_ON | DCACHE_ON,
        );
        assert!(cp15.cached(0x0200_0000, Fetch::Data));
    }
}
