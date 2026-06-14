use crate::gb::bus::CpuBusInterface;
use crate::gb::cpu::Cpu;
use crate::gb::ic::{ICInterface, Interrupt};

struct HaltBus {
    mem: Vec<u8>,
    pending: bool,
}

impl HaltBus {
    fn new(pending: bool) -> Self {
        Self {
            mem: vec![0; 0x10000],
            pending,
        }
    }
}

impl CpuBusInterface for HaltBus {
    fn cycle(&mut self) {}

    fn read(&mut self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.mem[addr as usize] = value;
    }
}

impl ICInterface for HaltBus {
    fn request_interrupt(&mut self, _interrupt: Interrupt) {}

    fn has_pending_interrupt(&self) -> bool {
        self.pending
    }
}

#[cfg(feature = "debug")]
impl crate::debug::DebuggerInterface for HaltBus {}

fn run(pending: bool) -> Cpu {
    let mut bus = HaltBus::new(pending);
    bus.mem[0x0200] = 0x76; // HALT
    bus.mem[0x0201] = 0x3C; // INC A
    bus.mem[0x0202] = 0x00; // NOP

    let mut cpu = Cpu::new_dmg(0x01);
    cpu.a = 0x00;
    cpu.ime = false;
    cpu.pc = 0x0201;
    cpu.ir = 0x76;

    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu
}

#[test]
fn halt_bug_executes_following_opcode_twice() {
    let cpu = run(true);
    assert_eq!(cpu.a, 0x02, "INC A after HALT should execute twice");
    assert!(!cpu.halted);
    assert!(!cpu.halt_bug);
}

#[test]
fn no_halt_bug_without_pending_interrupt() {
    let cpu = run(false);
    assert!(cpu.halted, "HALT without a pending interrupt should halt");
}
