use super::Core;
use crate::debug::Features;
use crate::debug::registers::{BitField, Register, RegisterGroup};
use crate::gba::disasm::{self, Decoded};

pub const FEATURES: Features = Features {
    pc_digits: 8,
    thumb: true,
    vectors: &[
        ("Reset", 0x00),
        ("Undefined", 0x04),
        ("SWI", 0x08),
        ("Prefetch abort", 0x0C),
        ("Data abort", 0x10),
        ("IRQ", 0x18),
        ("FIQ", 0x1C),
    ],
    cpus: &["ARM7TDMI"],
    cpu_groups: &[
        "CPU",
        "User and System",
        "FIQ",
        "Supervisor",
        "Abort",
        "IRQ",
        "Undefined",
    ],
    watchpoints: false,
    cycle: "cycle",
};

const MODES: &[(u32, &str)] = &[
    (0x10, "User"),
    (0x11, "FIQ"),
    (0x12, "IRQ"),
    (0x13, "Supervisor"),
    (0x17, "Abort"),
    (0x1B, "Undefined"),
    (0x1F, "System"),
];

const PSR: &[BitField] = &[
    BitField::bit("N", 31),
    BitField::bit("Z", 30),
    BitField::bit("C", 29),
    BitField::bit("V", 28),
    BitField::bit("I", 7),
    BitField::bit("F", 6),
    BitField::bit("T", 5),
    BitField::wide("Mode", 0, 5).named(MODES),
];

const PSR_V5: &[BitField] = &[
    BitField::bit("N", 31),
    BitField::bit("Z", 30),
    BitField::bit("C", 29),
    BitField::bit("V", 28),
    BitField::bit("Q", 27),
    BitField::bit("I", 7),
    BitField::bit("F", 6),
    BitField::bit("T", 5),
    BitField::wide("Mode", 0, 5).named(MODES),
];

const NAMES: [&str; 16] = [
    "R0", "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10", "R11", "R12", "SP", "LR",
    "PC",
];

const USER: [&str; 7] = [
    "R8_usr", "R9_usr", "R10_usr", "R11_usr", "R12_usr", "R13_usr", "R14_usr",
];

const FIQ: [&str; 7] = [
    "R8_fiq", "R9_fiq", "R10_fiq", "R11_fiq", "R12_fiq", "R13_fiq", "R14_fiq",
];

const BANKS: [(&str, [&str; 3]); 4] = [
    ("Supervisor", ["R13_svc", "R14_svc", "SPSR_svc"]),
    ("Abort", ["R13_abt", "R14_abt", "SPSR_abt"]),
    ("IRQ", ["R13_irq", "R14_irq", "SPSR_irq"]),
    ("Undefined", ["R13_und", "R14_und", "SPSR_und"]),
];

const SPSR_SLOTS: [usize; 4] = [1, 2, 3, 4];

fn word(name: &'static str, value: u32) -> Register {
    Register::new(name, value, 32)
}

impl<const V5: bool> Core<V5> {
    pub fn registers(&self, out: &mut Vec<RegisterGroup>) {
        out.clear();
        let psr = if V5 { PSR_V5 } else { PSR };
        let state = self.state();
        let mut rows: Vec<Register> = NAMES
            .iter()
            .enumerate()
            .map(|(index, &name)| word(name, self.reg(index)))
            .collect();
        rows.push(word("CPSR", self.cpsr()).with_fields(psr));
        out.push(RegisterGroup { name: "CPU", rows });
        out.push(RegisterGroup {
            name: "User and System",
            rows: USER
                .iter()
                .zip(&state.r[8..15])
                .map(|(&name, &value)| word(name, value))
                .collect(),
        });
        let mut fiq: Vec<Register> = FIQ
            .iter()
            .zip(&state.fiq)
            .map(|(&name, &value)| word(name, value))
            .collect();
        fiq.push(word("SPSR_fiq", state.spsr[0]).with_fields(psr));
        out.push(RegisterGroup {
            name: "FIQ",
            rows: fiq,
        });
        let pairs = [state.svc, state.abt, state.irq, state.und];
        for (((name, [sp, lr, spsr]), [sp_value, lr_value]), slot) in
            BANKS.into_iter().zip(pairs).zip(SPSR_SLOTS)
        {
            out.push(RegisterGroup {
                name,
                rows: vec![
                    word(sp, sp_value),
                    word(lr, lr_value),
                    word(spsr, state.spsr[slot]).with_fields(psr),
                ],
            });
        }
    }

    pub fn instruction(&self, paired: bool) -> Decoded {
        let [op, next] = self.pipeline;
        let next = paired.then_some(next as u16);
        match (self.thumb(), V5) {
            (true, true) => disasm::thumb_v5(self.pc(), op as u16, next),
            (true, false) => disasm::thumb(self.pc(), op as u16, next),
            (false, true) => disasm::arm_v5(self.pc(), op),
            (false, false) => disasm::arm(self.pc(), op),
        }
    }
}

#[cfg(test)]
mod tests {
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
}
