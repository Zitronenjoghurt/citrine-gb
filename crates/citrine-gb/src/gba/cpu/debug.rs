use super::Cpu;
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

impl Cpu {
    pub fn registers(&self, out: &mut Vec<RegisterGroup>) {
        out.clear();
        let state = self.state();
        let mut rows: Vec<Register> = NAMES
            .iter()
            .enumerate()
            .map(|(index, &name)| word(name, self.reg(index)))
            .collect();
        rows.push(word("CPSR", self.cpsr()).with_fields(PSR));
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
        fiq.push(word("SPSR_fiq", state.spsr[0]).with_fields(PSR));
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
                    word(spsr, state.spsr[slot]).with_fields(PSR),
                ],
            });
        }
    }

    pub fn instruction(&self, paired: bool) -> Decoded {
        let [op, next] = self.pipeline;
        if self.thumb() {
            disasm::thumb(self.pc(), op as u16, paired.then_some(next as u16))
        } else {
            disasm::arm(self.pc(), op)
        }
    }
}

#[cfg(test)]
mod tests;
