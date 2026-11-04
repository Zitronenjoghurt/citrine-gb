use super::*;

fn rom(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn boot(words: &[u32]) -> (Cpu, Bus) {
    let mut bus = Bus::new(&rom(words));
    let mut cpu = Cpu::default();
    cpu.reset(&mut bus);
    let mut state = cpu.state();
    state.r[15] = 0x0800_0000;
    let mut cpu = Cpu::from_state(&state);
    cpu.reload(&mut bus);
    (cpu, bus)
}

#[test]
fn reset_enters_supervisor_mode_at_the_reset_vector() {
    let mut bus = Bus::new(&[]);
    let mut cpu = Cpu::default();
    cpu.reset(&mut bus);
    assert_eq!(cpu.mode(), Some(Mode::Supervisor));
    assert_eq!(cpu.cpsr() & 0xFF, 0xD3);
    assert_eq!(cpu.reg(15), 8);
}

#[test]
fn runs_a_counting_loop_from_rom() {
    let (mut cpu, mut bus) = boot(&[
        0xE3A0_0000,
        0xE280_0001,
        0xE350_0005,
        0x1AFF_FFFC,
        0xEAFF_FFFE,
    ]);
    for _ in 0..20 {
        cpu.step(&mut bus);
    }
    assert_eq!(cpu.reg(0), 5);
    assert_eq!(cpu.reg(15), 0x0800_0010 + 8);
}

#[test]
fn store_and_load_go_through_the_bus() {
    let (mut cpu, mut bus) = boot(&[0xE3A0_0403, 0xE3A0_102A, 0xE580_1004, 0xE590_2004]);
    for _ in 0..4 {
        cpu.step(&mut bus);
    }
    assert_eq!(bus.read32(0x0300_0004), 42);
    assert_eq!(cpu.reg(2), 42);
}

#[test]
fn bx_to_an_odd_address_runs_a_thumb_counting_loop() {
    let (mut cpu, mut bus) = boot(&[
        0xE28F_0005,
        0xE12F_FF10,
        0x0000_0000,
        0x3001_2000,
        0xD1FC_2805,
        0x0000_E7FE,
    ]);
    for _ in 0..30 {
        cpu.step(&mut bus);
    }
    assert!(cpu.thumb());
    assert_eq!(cpu.reg(0), 5);
    assert_eq!(cpu.reg(15), 0x0800_0014 + 4);
}

#[test]
fn banked_registers_survive_a_round_trip_through_irq_mode() {
    let (mut cpu, mut bus) = boot(&[0xE3A0_D00A, 0xE3A0_E00B]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    let mut state = cpu.state();
    state.cpsr &= !0x80;
    let mut cpu = Cpu::from_state(&state);
    cpu.interrupt(&mut bus, Exception::Irq);
    assert_eq!(cpu.mode(), Some(Mode::Irq));
    let state = cpu.state();
    assert_eq!(state.svc, [10, 11]);
    assert_eq!(state.irq[1], 0x0800_0008 + 4);
}
