use crate::debug::registers::RegisterGroup;
use crate::gba::cpu::{Cpu, CpuState, FEATURES, Mode};

fn cpu_in(mode: Mode, flags: u32) -> Cpu {
    let mut r = [0; 16];
    for (index, value) in r.iter_mut().enumerate() {
        *value = index as u32;
    }
    Cpu::from_state(&CpuState {
        r,
        fiq: [0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86],
        svc: [0x90, 0x91],
        abt: [0xA0, 0xA1],
        irq: [0xB0, 0xB1],
        und: [0xC0, 0xC1],
        cpsr: flags | mode.bits(),
        spsr: [0x111, 0x113, 0x117, 0x112, 0x11B],
        ..CpuState::default()
    })
}

fn groups(cpu: &Cpu) -> Vec<RegisterGroup> {
    let mut out = vec![RegisterGroup {
        name: "stale",
        rows: Vec::new(),
    }];
    cpu.registers(&mut out);
    out
}

fn value(groups: &[RegisterGroup], group: &str, row: &str) -> u32 {
    groups
        .iter()
        .find(|g| g.name == group)
        .and_then(|g| g.rows.iter().find(|r| r.name == row))
        .unwrap_or_else(|| panic!("no {group} {row}"))
        .value
}

#[test]
fn the_groups_are_the_cpu_and_one_per_bank() {
    let names: Vec<&str> = groups(&cpu_in(Mode::System, 0))
        .iter()
        .map(|g| g.name)
        .collect();
    assert_eq!(
        names,
        [
            "CPU",
            "User and System",
            "FIQ",
            "Supervisor",
            "Abort",
            "IRQ",
            "Undefined"
        ]
    );
}

#[test]
fn the_cpu_group_shows_the_registers_of_the_current_mode() {
    let cases = [
        (Mode::User, 8, 13, 14),
        (Mode::System, 8, 13, 14),
        (Mode::Fiq, 0x80, 0x85, 0x86),
        (Mode::Supervisor, 8, 0x90, 0x91),
        (Mode::Abort, 8, 0xA0, 0xA1),
        (Mode::Irq, 8, 0xB0, 0xB1),
        (Mode::Undefined, 8, 0xC0, 0xC1),
    ];
    for (mode, r8, sp, lr) in cases {
        let all = groups(&cpu_in(mode, 0));
        assert_eq!(value(&all, "CPU", "R0"), 0, "{mode:?}");
        assert_eq!(value(&all, "CPU", "R8"), r8, "{mode:?}");
        assert_eq!(value(&all, "CPU", "SP"), sp, "{mode:?}");
        assert_eq!(value(&all, "CPU", "LR"), lr, "{mode:?}");
        assert_eq!(value(&all, "CPU", "PC"), 15, "{mode:?}");
        assert_eq!(value(&all, "User and System", "R8_usr"), 8, "{mode:?}");
        assert_eq!(value(&all, "User and System", "R13_usr"), 13, "{mode:?}");
        assert_eq!(value(&all, "FIQ", "R13_fiq"), 0x85, "{mode:?}");
        assert_eq!(value(&all, "IRQ", "R14_irq"), 0xB1, "{mode:?}");
    }
}

#[test]
fn each_bank_keeps_its_spsr() {
    let all = groups(&cpu_in(Mode::Irq, 0));
    assert_eq!(value(&all, "FIQ", "SPSR_fiq"), 0x111);
    assert_eq!(value(&all, "Supervisor", "SPSR_svc"), 0x113);
    assert_eq!(value(&all, "Abort", "SPSR_abt"), 0x117);
    assert_eq!(value(&all, "IRQ", "SPSR_irq"), 0x112);
    assert_eq!(value(&all, "Undefined", "SPSR_und"), 0x11B);
}

#[test]
fn cpsr_fields_read_the_mode_and_flags() {
    let all = groups(&cpu_in(Mode::Irq, 0xA000_00A0));
    let cpsr = all[0].rows.iter().find(|r| r.name == "CPSR").expect("cpsr");
    assert_eq!(cpsr.bits, 32);
    assert_eq!(cpsr.hex(), "A00000B2");
    let fields: Vec<(&str, u32)> = cpsr
        .fields
        .iter()
        .map(|field| (field.name, field.read(cpsr.value)))
        .collect();
    assert_eq!(
        fields,
        [
            ("N", 1),
            ("Z", 0),
            ("C", 1),
            ("V", 0),
            ("I", 1),
            ("F", 0),
            ("T", 1),
            ("Mode", 0x12),
        ]
    );
}

#[test]
fn features_name_the_vectors_and_eight_digit_addresses() {
    assert_eq!((FEATURES.pc_digits, FEATURES.thumb), (8, true));
    assert!(FEATURES.vectors.contains(&("IRQ", 0x18)));
}
