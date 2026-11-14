use citrine_gb::gba::cpu::{Arm9, CoprocessorRegister, CpuBus, CpuState, Mode};
use citrine_gb::gba::wait::{Access, Width};
use citrine_gb::nds::cp15::{Cp15, Cp15Write, MAIN_ID, TcmRegion};

const CODE: u32 = 0x1000;
const SYSTEM: u32 = 0x1F;
const ARM_NOP: u32 = 0xE1A0_0000;
const SWI: u32 = 0xEF00_0000;

const MRC_P15_R0_C0_C0_0: u32 = 0xEE10_0F10;
const MRC_P15_R0_C1_C0_0: u32 = 0xEE11_0F10;
const MCR_P15_R1_C1_C0_0: u32 = 0xEE01_1F10;
const MCR_P15_R1_C9_C1_0: u32 = 0xEE09_1F11;
const MRC_P15_R0_C9_C1_0: u32 = 0xEE19_0F11;
const MCR_P15_R0_C7_C0_4: u32 = 0xEE07_0F90;
const MCR_P15_R0_C7_C8_2: u32 = 0xEE07_0F58;
const MCR_P15_R0_C7_C5_0: u32 = 0xEE07_0F15;
const MRC_P14_R0_C0_C0_0: u32 = 0xEE10_0E10;

struct Ds9Bus {
    memory: Vec<u8>,
    cp15: Cp15,
    halted: bool,
    itcm: TcmRegion,
    dtcm: TcmRegion,
}

impl Ds9Bus {
    fn new() -> Self {
        Self {
            memory: vec![0; 0x1_0000],
            cp15: Cp15::default(),
            halted: false,
            itcm: TcmRegion::default(),
            dtcm: TcmRegion::default(),
        }
    }

    fn index(&self, addr: u32) -> usize {
        addr as usize & (self.memory.len() - 1)
    }

    fn get(&self, addr: u32, width: Width) -> u32 {
        let addr = addr & !(width.bytes() as u32 - 1);
        (0..width.bytes() as u32).fold(0, |value, byte| {
            value | (u32::from(self.memory[self.index(addr + byte)]) << (8 * byte))
        })
    }

    fn put(&mut self, addr: u32, width: Width, value: u32) {
        let addr = addr & !(width.bytes() as u32 - 1);
        for byte in 0..width.bytes() as u32 {
            let index = self.index(addr + byte);
            self.memory[index] = (value >> (8 * byte)) as u8;
        }
    }
}

impl CpuBus for Ds9Bus {
    fn fetch(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.get(addr, width)
    }

    fn read(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.get(addr, width)
    }

    fn write(&mut self, addr: u32, width: Width, value: u32, _access: Access) {
        self.put(addr, width, value);
    }

    fn idle(&mut self) {}

    fn cycles(&self, _addr: u32, _width: Width, _access: Access) -> u32 {
        1
    }

    fn bus_value(&self) -> u32 {
        0
    }

    fn drive(&mut self, _value: u32) {}

    fn coprocessor_read(&mut self, register: CoprocessorRegister) -> Option<u32> {
        self.cp15.read(register)
    }

    fn coprocessor_write(&mut self, register: CoprocessorRegister, value: u32) -> bool {
        match self.cp15.write(register, value) {
            Cp15Write::Undefined => return false,
            Cp15Write::Halt => self.halted = true,
            Cp15Write::TcmMoved => {
                self.itcm = self.cp15.itcm();
                self.dtcm = self.cp15.dtcm();
            }
            Cp15Write::Done => {}
        }
        true
    }

    fn vector_base(&self) -> u32 {
        self.cp15.vector_base()
    }
}

struct Rig {
    cpu: Arm9,
    bus: Ds9Bus,
}

fn run(bus: Ds9Bus, regs: &[(usize, u32)], program: &[u32]) -> Rig {
    let mut bus = bus;
    for (i, word) in program.iter().chain([ARM_NOP; 4].iter()).enumerate() {
        bus.put(CODE + 4 * i as u32, Width::Word, *word);
    }
    let mut state = CpuState {
        cpsr: SYSTEM,
        ..CpuState::default()
    };
    for &(index, value) in regs {
        state.r[index] = value;
    }
    state.r[15] = CODE + 8;
    state.pipeline = [bus.get(CODE, Width::Word), bus.get(CODE + 4, Width::Word)];
    state.next_fetch = Access::Sequential;
    let mut rig = Rig {
        cpu: Arm9::from_state(&state),
        bus,
    };
    for _ in 0..program.len() {
        rig.cpu.step(&mut rig.bus);
    }
    rig
}

#[test]
fn mrc_reads_the_main_id_from_cp15() {
    let rig = run(Ds9Bus::new(), &[], &[MRC_P15_R0_C0_C0_0]);
    assert_eq!(rig.cpu.reg(0), MAIN_ID);
    assert_eq!(rig.cpu.mode(), Some(Mode::System));
}

#[test]
fn mcr_then_mrc_of_control_keeps_only_the_writable_bits() {
    let rig = run(
        Ds9Bus::new(),
        &[(1, 0xFFFF_FFFF)],
        &[MCR_P15_R1_C1_C0_0, MRC_P15_R0_C1_C0_0],
    );
    assert_eq!(rig.cpu.reg(0), 0x000F_F0FD);
}

#[test]
fn high_vectors_send_a_swi_to_ffff0008() {
    let rig = run(Ds9Bus::new(), &[], &[SWI]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Supervisor));
    assert_eq!(rig.cpu.pc(), 0xFFFF_0008);
}

#[test]
fn low_vectors_send_a_swi_to_00000008() {
    let rig = run(Ds9Bus::new(), &[(1, 0x78)], &[MCR_P15_R1_C1_C0_0, SWI]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Supervisor));
    assert_eq!(rig.cpu.pc(), 0x0000_0008);
}

#[test]
fn direct_boot_control_through_mcr_turns_both_tcms_on() {
    let rig = run(Ds9Bus::new(), &[(1, 0x0005_2078)], &[MCR_P15_R1_C1_C0_0]);
    assert!(rig.bus.itcm.enabled);
    assert!(rig.bus.dtcm.enabled);
    assert_eq!(rig.bus.itcm.base, 0);
}

#[test]
fn mcr_to_the_dtcm_region_moves_the_dtcm() {
    let rig = run(
        Ds9Bus::new(),
        &[(1, 0x027C_000A)],
        &[MCR_P15_R1_C9_C1_0, MRC_P15_R0_C9_C1_0],
    );
    assert_eq!(rig.bus.dtcm.base, 0x027C_0000);
    assert_eq!(rig.bus.dtcm.virtual_size, 0x4000);
    assert_eq!(rig.cpu.reg(0), 0x027C_000A);
}

#[test]
fn wait_for_interrupt_halts_through_the_bus() {
    assert!(run(Ds9Bus::new(), &[], &[MCR_P15_R0_C7_C0_4]).bus.halted);
    assert!(run(Ds9Bus::new(), &[], &[MCR_P15_R0_C7_C8_2]).bus.halted);
}

#[test]
fn a_cache_invalidate_neither_halts_nor_traps() {
    let rig = run(Ds9Bus::new(), &[], &[MCR_P15_R0_C7_C5_0]);
    assert!(!rig.bus.halted);
    assert_eq!(rig.cpu.mode(), Some(Mode::System));
}

#[test]
fn another_coprocessor_is_still_undefined() {
    let rig = run(Ds9Bus::new(), &[], &[MRC_P14_R0_C0_C0_0]);
    assert_eq!(rig.cpu.mode(), Some(Mode::Undefined));
    assert_eq!(rig.cpu.pc(), 0xFFFF_0004);
}
