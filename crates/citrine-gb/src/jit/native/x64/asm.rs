pub type R = u8;

pub const RAX: R = 0;
pub const RCX: R = 1;
pub const RDX: R = 2;
pub const RBX: R = 3;
pub const RSP: R = 4;
pub const RBP: R = 5;
pub const RSI: R = 6;
pub const RDI: R = 7;
pub const R8: R = 8;
pub const R9: R = 9;
pub const R10: R = 10;
pub const R11: R = 11;
pub const R12: R = 12;
pub const R13: R = 13;
pub const R14: R = 14;
pub const R15: R = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alu {
    Add = 0,
    Or = 1,
    Adc = 2,
    Sbb = 3,
    And = 4,
    Sub = 5,
    Xor = 6,
    Cmp = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rot {
    Rol = 0,
    Ror = 1,
    Rcr = 3,
    Shl = 4,
    Shr = 5,
    Sar = 7,
}

pub mod cc {
    pub const O: u8 = 0x0;
    pub const NO: u8 = 0x1;
    pub const B: u8 = 0x2;
    pub const AE: u8 = 0x3;
    pub const E: u8 = 0x4;
    pub const NE: u8 = 0x5;
    pub const BE: u8 = 0x6;
    pub const A: u8 = 0x7;
    pub const S: u8 = 0x8;
    pub const NS: u8 = 0x9;
    pub const L: u8 = 0xC;
    pub const GE: u8 = 0xD;
    pub const LE: u8 = 0xE;
    pub const G: u8 = 0xF;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mem {
    pub base: R,
    pub index: Option<(R, u8)>,
    pub disp: i32,
}

pub fn at(base: R, disp: i32) -> Mem {
    Mem {
        base,
        index: None,
        disp,
    }
}

pub fn indexed(base: R, index: R, scale: u8, disp: i32) -> Mem {
    Mem {
        base,
        index: Some((index, scale)),
        disp,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label(usize);

struct Jump {
    start: usize,
    field: usize,
    label: Label,
    short: bool,
}

#[derive(Default)]
pub struct Asm {
    bytes: Vec<u8>,
    labels: Vec<Option<usize>>,
    jumps: Vec<Jump>,
    plan: Vec<bool>,
}

fn fits8(value: i32) -> bool {
    (-128..=127).contains(&value)
}

impl Asm {
    pub fn new(plan: Vec<bool>) -> Self {
        Self {
            plan,
            ..Self::default()
        }
    }

    pub fn here(&self) -> usize {
        self.bytes.len()
    }

    pub fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn word(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    pub fn bind(&mut self, label: Label) {
        self.labels[label.0] = Some(self.bytes.len());
    }

    pub fn bound(&self, label: Label) -> Option<usize> {
        self.labels[label.0]
    }

    pub fn shorts(&self) -> Option<Vec<bool>> {
        self.jumps
            .iter()
            .map(|jump| {
                let target = self.labels[jump.label.0]?;
                Some(if target > jump.start {
                    target - (jump.field + 4) <= 127
                } else {
                    jump.start - target + 2 <= 128
                })
            })
            .collect()
    }

    pub fn finish(mut self) -> Option<Vec<u8>> {
        for jump in std::mem::take(&mut self.jumps) {
            let target = self.labels[jump.label.0]? as i64;
            if jump.short {
                let delta = target - (jump.field as i64 + 1);
                let delta = i8::try_from(delta).ok()?;
                self.bytes[jump.field] = delta as u8;
            } else {
                let delta = target - (jump.field as i64 + 4);
                let delta = i32::try_from(delta).ok()?;
                self.bytes[jump.field..jump.field + 4].copy_from_slice(&delta.to_le_bytes());
            }
        }
        Some(self.bytes)
    }

    fn rex(&mut self, wide: bool, reg: R, index: R, base: R, force: bool) {
        let rex = 0x40 | u8::from(wide) << 3 | (reg >> 3) << 2 | (index >> 3) << 1 | (base >> 3);
        if rex != 0x40 || force {
            self.byte(rex);
        }
    }

    fn modrm(&mut self, mode: u8, reg: R, rm: R) {
        self.byte(mode << 6 | (reg & 7) << 3 | (rm & 7));
    }

    fn rr(&mut self, op: &[u8], wide: bool, reg: R, rm: R, bytes: bool) {
        let force = bytes && matches!(rm, 4..=7);
        self.rex(wide, reg, 0, rm, force);
        self.bytes.extend_from_slice(op);
        self.modrm(3, reg, rm);
    }

    fn rm(&mut self, op: &[u8], wide: bool, reg: R, mem: Mem, bytes: bool) {
        let index = mem.index.map_or(0, |(index, _)| index);
        let force = bytes && matches!(reg, 4..=7);
        self.rex(wide, reg, index, mem.base, force);
        self.bytes.extend_from_slice(op);
        self.address(reg, mem);
    }

    fn address(&mut self, reg: R, mem: Mem) {
        let mode = if mem.disp == 0 && mem.base & 7 != RBP {
            0
        } else if fits8(mem.disp) {
            1
        } else {
            2
        };
        match mem.index {
            None if mem.base & 7 != RSP => self.modrm(mode, reg, mem.base),
            index => {
                self.modrm(mode, reg, RSP);
                let (index, scale) = index.unwrap_or((RSP, 1));
                let bits = match scale {
                    1 => 0,
                    2 => 1,
                    4 => 2,
                    _ => 3,
                };
                self.byte(bits << 6 | (index & 7) << 3 | (mem.base & 7));
            }
        }
        match mode {
            0 => {}
            1 => self.byte(mem.disp as u8),
            _ => self.word(mem.disp as u32),
        }
    }

    pub fn alu(&mut self, op: Alu, wide: bool, dest: R, source: R) {
        self.rr(&[(op as u8) << 3 | 1], wide, source, dest, false);
    }

    pub fn alu_load(&mut self, op: Alu, dest: R, mem: Mem) {
        self.rm(&[(op as u8) << 3 | 3], false, dest, mem, false);
    }

    pub fn alu_load_wide(&mut self, op: Alu, dest: R, mem: Mem) {
        self.rm(&[(op as u8) << 3 | 3], true, dest, mem, false);
    }

    pub fn alu_store(&mut self, op: Alu, mem: Mem, source: R) {
        self.rm(&[(op as u8) << 3 | 1], false, source, mem, false);
    }

    pub fn alu_imm(&mut self, op: Alu, wide: bool, dest: R, imm: i32) {
        if fits8(imm) {
            self.rr(&[0x83], wide, op as u8, dest, false);
            self.byte(imm as u8);
        } else if dest == RAX {
            self.rex(wide, 0, 0, 0, false);
            self.byte((op as u8) << 3 | 5);
            self.word(imm as u32);
        } else {
            self.rr(&[0x81], wide, op as u8, dest, false);
            self.word(imm as u32);
        }
    }

    pub fn alu_store_wide(&mut self, op: Alu, mem: Mem, source: R) {
        self.rm(&[(op as u8) << 3 | 1], true, source, mem, false);
    }

    pub fn alu_mem_imm(&mut self, op: Alu, mem: Mem, imm: i32) {
        self.mem_imm(false, op, mem, imm);
    }

    pub fn alu_mem_imm_wide(&mut self, op: Alu, mem: Mem, imm: i32) {
        self.mem_imm(true, op, mem, imm);
    }

    fn mem_imm(&mut self, wide: bool, op: Alu, mem: Mem, imm: i32) {
        if fits8(imm) {
            self.rm(&[0x83], wide, op as u8, mem, false);
            self.byte(imm as u8);
        } else {
            self.rm(&[0x81], wide, op as u8, mem, false);
            self.word(imm as u32);
        }
    }

    pub fn test_mem8(&mut self, mem: Mem, imm: u8) {
        self.rm(&[0xF6], false, 0, mem, false);
        self.byte(imm);
    }

    pub fn alu_load8(&mut self, op: Alu, dest: R, mem: Mem) {
        self.rm(&[(op as u8) << 3 | 2], false, dest, mem, true);
    }

    pub fn alu_store8(&mut self, op: Alu, mem: Mem, source: R) {
        self.rm(&[(op as u8) << 3], false, source, mem, true);
    }

    pub fn test(&mut self, a: R, b: R) {
        self.rr(&[0x85], false, b, a, false);
    }

    pub fn test_imm(&mut self, rm: R, imm: u32) {
        if imm <= 0x7F {
            self.rr(&[0xF6], false, 0, rm, true);
            self.byte(imm as u8);
        } else if rm == RAX {
            self.byte(0xA9);
            self.word(imm);
        } else {
            self.rr(&[0xF7], false, 0, rm, false);
            self.word(imm);
        }
    }

    pub fn mov(&mut self, wide: bool, dest: R, source: R) {
        self.rr(&[0x89], wide, source, dest, false);
    }

    pub fn xchg(&mut self, wide: bool, a: R, b: R) {
        self.rr(&[0x87], wide, b, a, false);
    }

    pub fn load(&mut self, wide: bool, dest: R, mem: Mem) {
        self.rm(&[0x8B], wide, dest, mem, false);
    }

    pub fn store(&mut self, wide: bool, mem: Mem, source: R) {
        self.rm(&[0x89], wide, source, mem, false);
    }

    pub fn store8(&mut self, mem: Mem, source: R) {
        self.rm(&[0x88], false, source, mem, true);
    }

    pub fn store16(&mut self, mem: Mem, source: R) {
        self.byte(0x66);
        self.rm(&[0x89], false, source, mem, false);
    }

    pub fn store_imm(&mut self, mem: Mem, imm: u32) {
        self.rm(&[0xC7], false, 0, mem, false);
        self.word(imm);
    }

    pub fn store_imm_wide(&mut self, mem: Mem, imm: i32) {
        self.rm(&[0xC7], true, 0, mem, false);
        self.word(imm as u32);
    }

    pub fn store8_imm(&mut self, mem: Mem, imm: u8) {
        self.rm(&[0xC6], false, 0, mem, false);
        self.byte(imm);
    }

    pub fn load8(&mut self, signed: bool, dest: R, mem: Mem) {
        self.rm(
            &[0x0F, if signed { 0xBE } else { 0xB6 }],
            false,
            dest,
            mem,
            false,
        );
    }

    pub fn load16(&mut self, signed: bool, dest: R, mem: Mem) {
        self.rm(
            &[0x0F, if signed { 0xBF } else { 0xB7 }],
            false,
            dest,
            mem,
            false,
        );
    }

    pub fn extend8(&mut self, signed: bool, dest: R, source: R) {
        self.rr(
            &[0x0F, if signed { 0xBE } else { 0xB6 }],
            false,
            dest,
            source,
            true,
        );
    }

    pub fn extend16(&mut self, signed: bool, dest: R, source: R) {
        self.rr(
            &[0x0F, if signed { 0xBF } else { 0xB7 }],
            false,
            dest,
            source,
            false,
        );
    }

    pub fn mov_imm(&mut self, dest: R, imm: u32) {
        self.rex(false, 0, 0, dest, false);
        self.byte(0xB8 | (dest & 7));
        self.word(imm);
    }

    pub fn mov_imm64(&mut self, dest: R, imm: u64) {
        match u32::try_from(imm) {
            Ok(imm) => self.mov_imm(dest, imm),
            Err(_) => {
                self.rex(true, 0, 0, dest, false);
                self.byte(0xB8 | (dest & 7));
                self.bytes.extend_from_slice(&imm.to_le_bytes());
            }
        }
    }

    pub fn lea(&mut self, wide: bool, dest: R, mem: Mem) {
        self.rm(&[0x8D], wide, dest, mem, false);
    }

    pub fn rot(&mut self, kind: Rot, wide: bool, dest: R, amount: u8) {
        if amount == 1 {
            self.rr(&[0xD1], wide, kind as u8, dest, false);
        } else {
            self.rr(&[0xC1], wide, kind as u8, dest, false);
            self.byte(amount);
        }
    }

    pub fn rot_cl(&mut self, kind: Rot, wide: bool, dest: R) {
        self.rr(&[0xD3], wide, kind as u8, dest, false);
    }

    pub fn not(&mut self, dest: R) {
        self.rr(&[0xF7], false, 2, dest, false);
    }

    pub fn neg(&mut self, dest: R) {
        self.rr(&[0xF7], false, 3, dest, false);
    }

    pub fn inc(&mut self, dest: R) {
        self.rr(&[0xFF], false, 0, dest, false);
    }

    pub fn dec(&mut self, dest: R) {
        self.rr(&[0xFF], false, 1, dest, false);
    }

    pub fn imul(&mut self, wide: bool, dest: R, source: R) {
        self.rr(&[0x0F, 0xAF], wide, dest, source, false);
    }

    pub fn imul_load(&mut self, dest: R, mem: Mem) {
        self.rm(&[0x0F, 0xAF], false, dest, mem, false);
    }

    pub fn imul_imm(&mut self, dest: R, source: R, imm: i32) {
        if fits8(imm) {
            self.rr(&[0x6B], false, dest, source, false);
            self.byte(imm as u8);
        } else {
            self.rr(&[0x69], false, dest, source, false);
            self.word(imm as u32);
        }
    }

    pub fn movsxd(&mut self, dest: R, source: R) {
        self.rr(&[0x63], true, dest, source, false);
    }

    pub fn movsxd_load(&mut self, dest: R, mem: Mem) {
        self.rm(&[0x63], true, dest, mem, false);
    }

    pub fn al_imm(&mut self, op: Alu, imm: u8) {
        self.byte((op as u8) << 3 | 4);
        self.byte(imm);
    }

    pub fn ah_imm(&mut self, op: Alu, imm: u8) {
        self.byte(0x80);
        self.modrm(3, op as u8, 4);
        self.byte(imm);
    }

    pub fn stc(&mut self) {
        self.byte(0xF9);
    }

    pub fn setcc(&mut self, cond: u8, dest: R) {
        self.rr(&[0x0F, 0x90 | cond], false, 0, dest, true);
    }

    pub fn bsf(&mut self, dest: R, source: R) {
        self.rr(&[0x0F, 0xBC], false, dest, source, false);
    }

    pub fn bsr(&mut self, dest: R, source: R) {
        self.rr(&[0x0F, 0xBD], false, dest, source, false);
    }

    pub fn bt(&mut self, base: R, bit: R) {
        self.rr(&[0x0F, 0xA3], false, bit, base, false);
    }

    pub fn cmov(&mut self, cond: u8, dest: R, source: R) {
        self.rr(&[0x0F, 0x40 | cond], false, dest, source, false);
    }

    fn branch(&mut self, short: &[u8], long: &[u8], label: Label) {
        let short_form = self.plan.get(self.jumps.len()).copied().unwrap_or(false);
        let start = self.here();
        let op = if short_form { short } else { long };
        self.bytes.extend_from_slice(op);
        let field = self.here();
        if short_form {
            self.byte(0);
        } else {
            self.word(0);
        }
        self.jumps.push(Jump {
            start,
            field,
            label,
            short: short_form,
        });
    }

    pub fn jcc(&mut self, cond: u8, label: Label) {
        self.branch(&[0x70 | cond], &[0x0F, 0x80 | cond], label);
    }

    pub fn jmp(&mut self, label: Label) {
        self.branch(&[0xEB], &[0xE9], label);
    }

    pub fn jmp_reg(&mut self, rm: R) {
        self.rr(&[0xFF], false, 4, rm, false);
    }

    pub fn call_reg(&mut self, rm: R) {
        self.rr(&[0xFF], false, 2, rm, false);
    }

    pub fn ret(&mut self) {
        self.byte(0xC3);
    }

    pub fn push(&mut self, reg: R) {
        self.rex(false, 0, 0, reg, false);
        self.byte(0x50 | (reg & 7));
    }

    pub fn pop(&mut self, reg: R) {
        self.rex(false, 0, 0, reg, false);
        self.byte(0x58 | (reg & 7));
    }

    pub fn lahf(&mut self) {
        self.byte(0x9F);
    }

    pub fn sahf(&mut self) {
        self.byte(0x9E);
    }

    pub fn cmc(&mut self) {
        self.byte(0xF5);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(build: impl FnOnce(&mut Asm)) -> Vec<u8> {
        let mut asm = Asm::new(Vec::new());
        build(&mut asm);
        asm.finish().expect("assembles")
    }

    #[test]
    fn register_forms_pick_rex_only_when_needed() {
        assert_eq!(bytes(|a| a.alu(Alu::Add, false, RBX, RCX)), [0x01, 0xCB]);
        assert_eq!(
            bytes(|a| a.alu(Alu::Sub, false, R12, RSI)),
            [0x41, 0x29, 0xF4]
        );
        assert_eq!(
            bytes(|a| a.alu(Alu::Cmp, true, R9, R15)),
            [0x4D, 0x39, 0xF9]
        );
        assert_eq!(bytes(|a| a.mov(false, RDI, R8)), [0x44, 0x89, 0xC7]);
        assert_eq!(bytes(|a| a.xchg(false, RCX, RDX)), [0x87, 0xD1]);
        assert_eq!(bytes(|a| a.setcc(cc::E, RSI)), [0x40, 0x0F, 0x94, 0xC6]);
        assert_eq!(bytes(|a| a.setcc(cc::O, RAX)), [0x0F, 0x90, 0xC0]);
        assert_eq!(bytes(|a| a.inc(RBX)), [0xFF, 0xC3]);
        assert_eq!(bytes(|a| a.dec(R8)), [0x41, 0xFF, 0xC8]);
    }

    #[test]
    fn bit_scans_and_shifts_by_cl_match_the_manual() {
        assert_eq!(bytes(|a| a.bsr(RDX, R8)), [0x41, 0x0F, 0xBD, 0xD0]);
        assert_eq!(bytes(|a| a.bsr(RBX, RBP)), [0x0F, 0xBD, 0xDD]);
        assert_eq!(bytes(|a| a.bt(RSI, RDX)), [0x0F, 0xA3, 0xD6]);
        assert_eq!(bytes(|a| a.rot_cl(Rot::Shl, true, RDX)), [0x48, 0xD3, 0xE2]);
        assert_eq!(bytes(|a| a.rot_cl(Rot::Shr, true, RDX)), [0x48, 0xD3, 0xEA]);
        assert_eq!(bytes(|a| a.rot_cl(Rot::Sar, true, RDX)), [0x48, 0xD3, 0xFA]);
        assert_eq!(bytes(|a| a.rot_cl(Rot::Ror, false, RDX)), [0xD3, 0xCA]);
    }

    #[test]
    fn immediates_take_the_short_form_when_they_fit() {
        assert_eq!(
            bytes(|a| a.alu_imm(Alu::Add, false, RBX, 4)),
            [0x83, 0xC3, 0x04]
        );
        assert_eq!(
            bytes(|a| a.alu_imm(Alu::Cmp, false, RAX, 0x100)),
            [0x3D, 0x00, 0x01, 0x00, 0x00]
        );
        assert_eq!(
            bytes(|a| a.alu_imm(Alu::And, false, R10, 0x1000)),
            [0x41, 0x81, 0xE2, 0x00, 0x10, 0x00, 0x00]
        );
        assert_eq!(bytes(|a| a.test_imm(RDI, 1)), [0x40, 0xF6, 0xC7, 0x01]);
        assert_eq!(bytes(|a| a.mov_imm(R11, 7)), [0x41, 0xBB, 7, 0, 0, 0]);
    }

    #[test]
    fn memory_operands_cover_the_special_bases() {
        assert_eq!(
            bytes(|a| a.load(false, RAX, at(RSP, 8))),
            [0x8B, 0x44, 0x24, 0x08]
        );
        assert_eq!(
            bytes(|a| a.load(false, RCX, at(RBP, 0))),
            [0x8B, 0x4D, 0x00]
        );
        assert_eq!(
            bytes(|a| a.load(false, RDX, at(R13, 0))),
            [0x41, 0x8B, 0x55, 0x00]
        );
        assert_eq!(
            bytes(|a| a.load(false, RDX, at(R12, 4))),
            [0x41, 0x8B, 0x54, 0x24, 0x04]
        );
        assert_eq!(
            bytes(|a| a.store(true, at(RDI, 0x200), RSI)),
            [0x48, 0x89, 0xB7, 0x00, 0x02, 0x00, 0x00]
        );
        assert_eq!(
            bytes(|a| a.lea(false, RAX, indexed(RBX, R9, 4, -3))),
            [0x42, 0x8D, 0x44, 0x8B, 0xFD]
        );
    }

    #[test]
    fn wide_memory_immediates_and_byte_tests_match_the_manual() {
        assert_eq!(
            bytes(|a| a.alu_mem_imm_wide(Alu::Add, at(R11, 0x1234), 1)),
            [0x49, 0x83, 0x83, 0x34, 0x12, 0x00, 0x00, 0x01]
        );
        assert_eq!(
            bytes(|a| a.alu_mem_imm_wide(Alu::Add, at(R11, 0x10), 1)),
            [0x49, 0x83, 0x43, 0x10, 0x01]
        );
        assert_eq!(
            bytes(|a| a.alu_mem_imm_wide(Alu::Sub, at(RSP, 8), 0x100)),
            [0x48, 0x81, 0x6C, 0x24, 0x08, 0x00, 0x01, 0x00, 0x00]
        );
        assert_eq!(
            bytes(|a| a.alu_mem_imm_wide(Alu::Cmp, at(R12, 0), -1)),
            [0x49, 0x83, 0x3C, 0x24, 0xFF]
        );
        assert_eq!(
            bytes(|a| a.alu_mem_imm(Alu::Add, at(R11, 0x10), 1)),
            [0x41, 0x83, 0x43, 0x10, 0x01]
        );
        assert_eq!(
            bytes(|a| a.alu_store_wide(Alu::Add, at(R11, 0x1234), RCX)),
            [0x49, 0x01, 0x8B, 0x34, 0x12, 0x00, 0x00]
        );
        assert_eq!(
            bytes(|a| a.store(true, at(R11, 0x10), RDX)),
            [0x49, 0x89, 0x53, 0x10]
        );
        assert_eq!(
            bytes(|a| a.alu_store_wide(Alu::Add, at(RDI, 0x1234), R8)),
            [0x4C, 0x01, 0x87, 0x34, 0x12, 0x00, 0x00]
        );
        assert_eq!(
            bytes(|a| a.store_imm_wide(at(RSI, 0x40), 0x101)),
            [0x48, 0xC7, 0x46, 0x40, 0x01, 0x01, 0x00, 0x00]
        );
        assert_eq!(
            bytes(|a| a.store_imm_wide(at(R11, 0x1234), 0)),
            [
                0x49, 0xC7, 0x83, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00
            ]
        );
        assert_eq!(
            bytes(|a| a.store_imm_wide(at(RDI, 8), -1)),
            [0x48, 0xC7, 0x47, 0x08, 0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            bytes(|a| a.load8(false, R8, at(RAX, 9))),
            [0x44, 0x0F, 0xB6, 0x40, 0x09]
        );
        assert_eq!(
            bytes(|a| a.extend8(false, RAX, RSI)),
            [0x40, 0x0F, 0xB6, 0xC6]
        );
        assert_eq!(
            bytes(|a| a.test_mem8(at(RSI, 0x40), 0xFF)),
            [0xF6, 0x46, 0x40, 0xFF]
        );
        assert_eq!(
            bytes(|a| a.test_mem8(at(R11, 0x40), 0x80)),
            [0x41, 0xF6, 0x43, 0x40, 0x80]
        );
        assert_eq!(
            bytes(|a| a.test_mem8(at(R11, 0x1234), 0x04)),
            [0x41, 0xF6, 0x83, 0x34, 0x12, 0x00, 0x00, 0x04]
        );
        assert_eq!(
            bytes(|a| a.test_mem8(at(RSP, 0x200), 4)),
            [0xF6, 0x84, 0x24, 0x00, 0x02, 0x00, 0x00, 0x04]
        );
        assert_eq!(
            bytes(|a| a.test_mem8(at(R13, 0), 1)),
            [0x41, 0xF6, 0x45, 0x00, 0x01]
        );
    }

    #[test]
    fn byte_memory_alu_matches_the_manual() {
        assert_eq!(
            bytes(|a| a.alu_load8(Alu::Or, RCX, at(R11, 0x10))),
            [0x41, 0x0A, 0x4B, 0x10]
        );
        assert_eq!(
            bytes(|a| a.alu_load8(Alu::Or, RSI, at(RDI, 8))),
            [0x40, 0x0A, 0x77, 0x08]
        );
        assert_eq!(
            bytes(|a| a.alu_store8(Alu::Or, at(R11, 0x1234), RCX)),
            [0x41, 0x08, 0x8B, 0x34, 0x12, 0x00, 0x00]
        );
        assert_eq!(
            bytes(|a| a.alu_store8(Alu::And, at(RAX, 0), R8)),
            [0x44, 0x20, 0x00]
        );
    }

    #[test]
    fn jumps_shrink_on_the_second_pass() {
        let build = |plan: Vec<bool>| {
            let mut asm = Asm::new(plan);
            let ahead = asm.label();
            let back = asm.label();
            asm.bind(back);
            asm.jcc(cc::NE, ahead);
            asm.lahf();
            asm.jmp(back);
            asm.bind(ahead);
            asm
        };
        let first = build(Vec::new());
        let shorts = first.shorts().expect("bound");
        assert_eq!(shorts, [true, true]);
        assert_eq!(first.finish().expect("long").len(), 12);
        let second = build(shorts).finish().expect("short");
        assert_eq!(second, [0x75, 0x03, 0x9F, 0xEB, 0xFB]);
    }
}
