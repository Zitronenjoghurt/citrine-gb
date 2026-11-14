pub type R = u32;

pub const ZR: R = 31;
pub const SP: R = 31;
pub const LR: R = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Lsl = 0,
    Lsr = 1,
    Asr = 2,
    Ror = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label(usize);

#[derive(Clone, Copy)]
enum Patch {
    Branch,
    Cond,
    Test,
}

#[derive(Default)]
pub struct Asm {
    words: Vec<u32>,
    labels: Vec<Option<usize>>,
    patches: Vec<(usize, Label, Patch)>,
}

pub fn logical_immediate(value: u32) -> Option<(u32, u32)> {
    if value == 0 || value == u32::MAX {
        return None;
    }
    let mut size = 32;
    while size > 2 {
        let half = size / 2;
        let mask = (1u32 << half) - 1;
        if value & mask != (value >> half) & mask {
            break;
        }
        size = half;
    }
    let mask = if size == 32 {
        u32::MAX
    } else {
        (1 << size) - 1
    };
    let element = value & mask;
    let ones = element.count_ones();
    let run = if ones == 32 {
        u32::MAX
    } else {
        (1 << ones) - 1
    };
    let rotate_left = |value: u32, by: u32| {
        if by == 0 {
            value
        } else {
            ((value << by) | (value >> (size - by))) & mask
        }
    };
    let immr = (0..size).find(|&by| rotate_left(element, by) == run)?;
    let imms = ((!(size - 1) << 1) & 0x3F) | (ones - 1);
    Some((immr, imms))
}

pub fn invert(cond: u32) -> u32 {
    cond ^ 1
}

impl Asm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn emit(&mut self, word: u32) {
        self.words.push(word);
    }

    pub fn here(&self) -> usize {
        self.words.len()
    }

    pub fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    pub fn bind(&mut self, label: Label) {
        self.labels[label.0] = Some(self.words.len());
    }

    pub fn bound(&self, label: Label) -> Option<usize> {
        self.labels[label.0]
    }

    pub fn finish(mut self) -> Option<Vec<u8>> {
        for (at, label, patch) in std::mem::take(&mut self.patches) {
            let target = self.labels[label.0]?;
            let delta = target as i64 - at as i64;
            let word = &mut self.words[at];
            match patch {
                Patch::Branch => {
                    if !(-(1 << 25)..1 << 25).contains(&delta) {
                        return None;
                    }
                    *word |= (delta as u32) & 0x03FF_FFFF;
                }
                Patch::Cond => {
                    if !(-(1 << 18)..1 << 18).contains(&delta) {
                        return None;
                    }
                    *word |= ((delta as u32) & 0x7_FFFF) << 5;
                }
                Patch::Test => {
                    if !(-(1 << 13)..1 << 13).contains(&delta) {
                        return None;
                    }
                    *word |= ((delta as u32) & 0x3FFF) << 5;
                }
            }
        }
        Some(
            self.words
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect(),
        )
    }

    pub fn movz(&mut self, wide: bool, rd: R, imm: u32, hw: u32) {
        self.emit(sf(wide) | 0x5280_0000 | hw << 21 | (imm & 0xFFFF) << 5 | rd);
    }

    pub fn movk(&mut self, wide: bool, rd: R, imm: u32, hw: u32) {
        self.emit(sf(wide) | 0x7280_0000 | hw << 21 | (imm & 0xFFFF) << 5 | rd);
    }

    pub fn movn(&mut self, rd: R, imm: u32, hw: u32) {
        self.emit(0x1280_0000 | hw << 21 | (imm & 0xFFFF) << 5 | rd);
    }

    pub fn mov_imm(&mut self, rd: R, value: u32) {
        let low = value & 0xFFFF;
        let high = value >> 16;
        if high == 0 {
            self.movz(false, rd, low, 0);
        } else if low == 0 {
            self.movz(false, rd, high, 1);
        } else if high == 0xFFFF {
            self.movn(rd, !value & 0xFFFF, 0);
        } else if low == 0xFFFF {
            self.movn(rd, (!value >> 16) & 0xFFFF, 1);
        } else if let Some((immr, imms)) = logical_immediate(value) {
            self.logical_imm(Logic::Orr, rd, ZR, immr, imms);
        } else {
            self.movz(false, rd, low, 0);
            self.movk(false, rd, high, 1);
        }
    }

    pub fn mov_imm64(&mut self, rd: R, value: u64) {
        self.movz(true, rd, value as u32 & 0xFFFF, 0);
        for hw in 1..4 {
            let part = (value >> (16 * hw)) as u32 & 0xFFFF;
            if part != 0 {
                self.movk(true, rd, part, hw);
            }
        }
    }

    pub fn mov(&mut self, rd: R, rm: R) {
        self.logical(Logic::Orr, false, rd, ZR, rm, Shift::Lsl, 0);
    }

    pub fn mov64(&mut self, rd: R, rm: R) {
        if rd == SP || rm == SP {
            self.add_imm(true, false, rd, rm, 0);
        } else {
            self.logical(Logic::Orr, true, rd, ZR, rm, Shift::Lsl, 0);
        }
    }

    pub fn add_imm(&mut self, wide: bool, flags: bool, rd: R, rn: R, imm: u32) {
        self.arith_imm(wide, false, flags, rd, rn, imm);
    }

    pub fn sub_imm(&mut self, wide: bool, flags: bool, rd: R, rn: R, imm: u32) {
        self.arith_imm(wide, true, flags, rd, rn, imm);
    }

    pub fn fits_arith(imm: u32) -> bool {
        imm < 0x1000 || (imm & 0xFFF == 0 && imm < 0x100_0000)
    }

    pub fn arith_imm(&mut self, wide: bool, sub: bool, flags: bool, rd: R, rn: R, imm: u32) {
        debug_assert!(Self::fits_arith(imm));
        let (shift, imm) = if imm < 0x1000 {
            (0, imm)
        } else {
            (1, imm >> 12)
        };
        self.emit(
            sf(wide)
                | 0x1100_0000
                | u32::from(sub) << 30
                | u32::from(flags) << 29
                | shift << 22
                | imm << 10
                | rn << 5
                | rd,
        );
    }

    #[expect(clippy::too_many_arguments, reason = "one encoding")]
    pub fn arith(
        &mut self,
        wide: bool,
        sub: bool,
        flags: bool,
        rd: R,
        rn: R,
        rm: R,
        shift: Shift,
        amount: u32,
    ) {
        debug_assert!(shift != Shift::Ror);
        self.emit(
            sf(wide)
                | 0x0B00_0000
                | u32::from(sub) << 30
                | u32::from(flags) << 29
                | (shift as u32) << 22
                | rm << 16
                | amount << 10
                | rn << 5
                | rd,
        );
    }

    pub fn add_ext(&mut self, rd: R, rn: R, rm: R, shift: u32) {
        self.emit(0x8B20_4000 | rm << 16 | shift << 10 | rn << 5 | rd);
    }

    pub fn carry(&mut self, sub: bool, flags: bool, rd: R, rn: R, rm: R) {
        self.emit(
            0x1A00_0000 | u32::from(sub) << 30 | u32::from(flags) << 29 | rm << 16 | rn << 5 | rd,
        );
    }

    #[expect(clippy::too_many_arguments, reason = "one encoding")]
    pub fn logical(
        &mut self,
        logic: Logic,
        wide: bool,
        rd: R,
        rn: R,
        rm: R,
        shift: Shift,
        amount: u32,
    ) {
        let (opc, not) = logic.bits();
        self.emit(
            sf(wide)
                | 0x0A00_0000
                | opc << 29
                | (shift as u32) << 22
                | not << 21
                | rm << 16
                | amount << 10
                | rn << 5
                | rd,
        );
    }

    pub fn logical_imm(&mut self, logic: Logic, rd: R, rn: R, immr: u32, imms: u32) {
        let (opc, not) = logic.bits();
        debug_assert!(not == 0);
        self.emit(0x1200_0000 | opc << 29 | immr << 16 | imms << 10 | rn << 5 | rd);
    }

    pub fn ubfm(&mut self, wide: bool, rd: R, rn: R, immr: u32, imms: u32) {
        let base = if wide { 0xD340_0000 } else { 0x5300_0000 };
        self.emit(base | immr << 16 | imms << 10 | rn << 5 | rd);
    }

    pub fn sbfm(&mut self, rd: R, rn: R, immr: u32, imms: u32) {
        self.emit(0x1300_0000 | immr << 16 | imms << 10 | rn << 5 | rd);
    }

    pub fn bfm(&mut self, rd: R, rn: R, immr: u32, imms: u32) {
        self.emit(0x3300_0000 | immr << 16 | imms << 10 | rn << 5 | rd);
    }

    pub fn lsl(&mut self, rd: R, rn: R, amount: u32) {
        self.ubfm(false, rd, rn, (32 - amount) & 31, 31 - amount);
    }

    pub fn lsr(&mut self, rd: R, rn: R, amount: u32) {
        self.ubfm(false, rd, rn, amount, 31);
    }

    pub fn lsr64(&mut self, rd: R, rn: R, amount: u32) {
        self.ubfm(true, rd, rn, amount, 63);
    }

    pub fn asr(&mut self, rd: R, rn: R, amount: u32) {
        self.sbfm(rd, rn, amount, 31);
    }

    pub fn ror(&mut self, rd: R, rn: R, amount: u32) {
        self.emit(0x1380_0000 | rn << 16 | amount << 10 | rn << 5 | rd);
    }

    pub fn ubfx(&mut self, rd: R, rn: R, lsb: u32, width: u32) {
        self.ubfm(false, rd, rn, lsb, lsb + width - 1);
    }

    pub fn bfi(&mut self, rd: R, rn: R, lsb: u32, width: u32) {
        self.bfm(rd, rn, (32 - lsb) & 31, width - 1);
    }

    pub fn mul(&mut self, rd: R, rn: R, rm: R, ra: R) {
        self.emit(0x1B00_0000 | rm << 16 | ra << 10 | rn << 5 | rd);
    }

    pub fn mul_long(&mut self, signed: bool, rd: R, rn: R, rm: R, ra: R) {
        let base = if signed { 0x9B20_0000 } else { 0x9BA0_0000 };
        self.emit(base | rm << 16 | ra << 10 | rn << 5 | rd);
    }

    pub fn csel(&mut self, rd: R, rn: R, rm: R, cond: u32) {
        self.emit(0x1A80_0000 | rm << 16 | cond << 12 | rn << 5 | rd);
    }

    pub fn cset(&mut self, rd: R, cond: u32) {
        self.emit(0x1A80_0400 | ZR << 16 | invert(cond) << 12 | ZR << 5 | rd);
    }

    pub fn cmp_imm(&mut self, rn: R, imm: u32) {
        self.sub_imm(false, true, ZR, rn, imm);
    }

    pub fn cmp(&mut self, rn: R, rm: R) {
        self.arith(false, true, true, ZR, rn, rm, Shift::Lsl, 0);
    }

    pub fn tst(&mut self, rn: R, rm: R) {
        self.logical(Logic::Ands, false, ZR, rn, rm, Shift::Lsl, 0);
    }

    pub fn ldr(&mut self, wide: bool, rt: R, rn: R, offset: u32) {
        let scale = if wide { 8 } else { 4 };
        debug_assert!(offset.is_multiple_of(scale) && offset / scale < 0x1000);
        let base = if wide { 0xF940_0000 } else { 0xB940_0000 };
        self.emit(base | (offset / scale) << 10 | rn << 5 | rt);
    }

    pub fn str(&mut self, wide: bool, rt: R, rn: R, offset: u32) {
        let scale = if wide { 8 } else { 4 };
        debug_assert!(offset.is_multiple_of(scale) && offset / scale < 0x1000);
        let base = if wide { 0xF900_0000 } else { 0xB900_0000 };
        self.emit(base | (offset / scale) << 10 | rn << 5 | rt);
    }

    pub fn strb(&mut self, rt: R, rn: R, offset: u32) {
        debug_assert!(offset < 0x1000);
        self.emit(0x3900_0000 | offset << 10 | rn << 5 | rt);
    }

    pub fn ldrb(&mut self, rt: R, rn: R, offset: u32) {
        debug_assert!(offset < 0x1000);
        self.emit(0x3940_0000 | offset << 10 | rn << 5 | rt);
    }

    pub fn ldr_index(&mut self, rt: R, rn: R, rm: R) {
        self.emit(0xB860_5800 | rm << 16 | rn << 5 | rt);
    }

    pub fn indexed(&mut self, op: Indexed, rt: R, rn: R, rm: R) {
        self.emit(op as u32 | rm << 16 | rn << 5 | rt);
    }

    pub fn pair(&mut self, load: bool, wide: bool, rt: R, rt2: R, rn: R, offset: i32) {
        let scale = if wide { 8 } else { 4 };
        debug_assert!(offset % scale == 0 && (-64..64).contains(&(offset / scale)));
        let base = match (wide, load) {
            (false, false) => 0x2900_0000,
            (false, true) => 0x2940_0000,
            (true, false) => 0xA900_0000,
            (true, true) => 0xA940_0000,
        };
        let imm = ((offset / scale) as u32) & 0x7F;
        self.emit(base | imm << 15 | rt2 << 10 | rn << 5 | rt);
    }

    pub fn push_pair(&mut self, rt: R, rt2: R, bytes: i32) {
        let imm = ((-bytes / 8) as u32) & 0x7F;
        self.emit(0xA980_0000 | imm << 15 | rt2 << 10 | SP << 5 | rt);
    }

    pub fn pop_pair(&mut self, rt: R, rt2: R, bytes: i32) {
        let imm = ((bytes / 8) as u32) & 0x7F;
        self.emit(0xA8C0_0000 | imm << 15 | rt2 << 10 | SP << 5 | rt);
    }

    pub fn mrs_nzcv(&mut self, rt: R) {
        self.emit(0xD53B_4200 | rt);
    }

    pub fn msr_nzcv(&mut self, rt: R) {
        self.emit(0xD51B_4200 | rt);
    }

    pub fn b(&mut self, label: Label) {
        self.patch(label, Patch::Branch);
        self.emit(0x1400_0000);
    }

    pub fn b_cond(&mut self, cond: u32, label: Label) {
        self.patch(label, Patch::Cond);
        self.emit(0x5400_0000 | cond);
    }

    pub fn cbz(&mut self, nonzero: bool, rt: R, label: Label) {
        self.patch(label, Patch::Cond);
        self.emit(0x3400_0000 | u32::from(nonzero) << 24 | rt);
    }

    pub fn cbz_wide(&mut self, nonzero: bool, rt: R, label: Label) {
        self.patch(label, Patch::Cond);
        self.emit(0xB400_0000 | u32::from(nonzero) << 24 | rt);
    }

    pub fn tbz(&mut self, nonzero: bool, rt: R, bit: u32, label: Label) {
        self.patch(label, Patch::Test);
        self.emit(
            0x3600_0000 | u32::from(nonzero) << 24 | (bit >> 5) << 31 | (bit & 31) << 19 | rt,
        );
    }

    pub fn br(&mut self, rn: R) {
        self.emit(0xD61F_0000 | rn << 5);
    }

    pub fn bl(&mut self, label: Label) {
        self.patch(label, Patch::Branch);
        self.emit(0x9400_0000);
    }

    pub fn lslv(&mut self, rd: R, rn: R, rm: R) {
        self.emit(0x1AC0_2000 | rm << 16 | rn << 5 | rd);
    }

    pub fn lslv64(&mut self, rd: R, rn: R, rm: R) {
        self.emit(0x9AC0_2000 | rm << 16 | rn << 5 | rd);
    }

    pub fn lsrv64(&mut self, rd: R, rn: R, rm: R) {
        self.emit(0x9AC0_2400 | rm << 16 | rn << 5 | rd);
    }

    pub fn asrv64(&mut self, rd: R, rn: R, rm: R) {
        self.emit(0x9AC0_2800 | rm << 16 | rn << 5 | rd);
    }

    pub fn sxtw(&mut self, rd: R, rn: R) {
        self.emit(0x9340_7C00 | rn << 5 | rd);
    }

    pub fn rorv(&mut self, rd: R, rn: R, rm: R) {
        self.emit(0x1AC0_2C00 | rm << 16 | rn << 5 | rd);
    }

    pub fn clz(&mut self, rd: R, rn: R) {
        self.emit(0x5AC0_1000 | rn << 5 | rd);
    }

    pub fn csinc(&mut self, rd: R, rn: R, rm: R, cond: u32) {
        self.emit(0x1A80_0400 | rm << 16 | cond << 12 | rn << 5 | rd);
    }

    pub fn blr(&mut self, rn: R) {
        self.emit(0xD63F_0000 | rn << 5);
    }

    pub fn ret(&mut self) {
        self.emit(0xD65F_03C0);
    }

    fn patch(&mut self, label: Label, patch: Patch) {
        self.patches.push((self.words.len(), label, patch));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Indexed {
    LoadByte = 0x3860_4800,
    LoadSignedByte = 0x38E0_4800,
    LoadHalf = 0x7860_4800,
    LoadSignedHalf = 0x78E0_4800,
    LoadWord = 0xB860_4800,
    StoreByte = 0x3820_4800,
    StoreHalf = 0x7820_4800,
    StoreWord = 0xB820_4800,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Logic {
    And,
    Bic,
    Orr,
    Orn,
    Eor,
    Eon,
    Ands,
    Bics,
}

impl Logic {
    pub fn inverted(self) -> Self {
        match self {
            Logic::And => Logic::Bic,
            Logic::Bic => Logic::And,
            Logic::Orr => Logic::Orn,
            Logic::Orn => Logic::Orr,
            Logic::Eor => Logic::Eon,
            Logic::Eon => Logic::Eor,
            Logic::Ands => Logic::Bics,
            Logic::Bics => Logic::Ands,
        }
    }

    fn bits(self) -> (u32, u32) {
        match self {
            Logic::And => (0, 0),
            Logic::Bic => (0, 1),
            Logic::Orr => (1, 0),
            Logic::Orn => (1, 1),
            Logic::Eor => (2, 0),
            Logic::Eon => (2, 1),
            Logic::Ands => (3, 0),
            Logic::Bics => (3, 1),
        }
    }
}

fn sf(wide: bool) -> u32 {
    u32::from(wide) << 31
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(build: impl FnOnce(&mut Asm)) -> u32 {
        let mut asm = Asm::new();
        build(&mut asm);
        let bytes = asm.finish().expect("labels bind");
        u32::from_le_bytes(bytes[..4].try_into().unwrap())
    }

    #[test]
    fn encodings_match_the_reference_assembler() {
        assert_eq!(one(|a| a.movz(false, 0, 7, 0)), 0x5280_00E0);
        assert_eq!(one(|a| a.movk(false, 3, 0x1234, 1)), 0x72A2_4683);
        assert_eq!(one(|a| a.add_imm(false, false, 1, 2, 4)), 0x1100_1041);
        assert_eq!(one(|a| a.sub_imm(false, true, ZR, 19, 1)), 0x7100_067F);
        assert_eq!(
            one(|a| a.arith(false, false, true, 19, 20, 21, Shift::Lsl, 2)),
            0x2B15_0A93
        );
        assert_eq!(one(|a| a.carry(true, true, 0, 1, 2)), 0x7A02_0020);
        assert_eq!(one(|a| a.clz(3, 3)), 0x5AC0_1063);
        assert_eq!(
            one(|a| a.logical(Logic::Orr, false, 0, ZR, 1, Shift::Lsl, 0)),
            0x2A01_03E0
        );
        assert_eq!(
            one(|a| a.logical(Logic::Bic, false, 8, 16, 16, Shift::Asr, 31)),
            0x0AB0_7E08
        );
        assert_eq!(one(|a| a.lsl(0, 1, 4)), 0x531C_6C20);
        assert_eq!(one(|a| a.lsr(0, 1, 4)), 0x5304_7C20);
        assert_eq!(one(|a| a.asr(0, 1, 31)), 0x131F_7C20);
        assert_eq!(one(|a| a.ror(0, 1, 8)), 0x1381_2020);
        assert_eq!(one(|a| a.ubfx(0, 1, 29, 1)), 0x531D_7420);
        assert_eq!(one(|a| a.bfi(17, 16, 29, 1)), 0x3303_0211);
        assert_eq!(one(|a| a.ldr(true, 16, 28, 64)), 0xF940_2390);
        assert_eq!(one(|a| a.str(false, 3, 0, 8)), 0xB900_0803);
        assert_eq!(one(|a| a.strb(3, 0, 5)), 0x3900_1403);
        assert_eq!(one(|a| a.ldrb(0, 3, 5)), 0x3940_1460);
        assert_eq!(one(|a| a.pair(true, false, 19, 20, 17, 0)), 0x2940_5233);
        assert_eq!(one(|a| a.pair(false, true, 8, 9, SP, 16)), 0xA901_27E8);
        assert_eq!(one(|a| a.push_pair(29, 30, 96)), 0xA9BA_7BFD);
        assert_eq!(one(|a| a.pop_pair(29, 30, 96)), 0xA8C6_7BFD);
        assert_eq!(one(|a| a.mrs_nzcv(16)), 0xD53B_4210);
        assert_eq!(one(|a| a.msr_nzcv(16)), 0xD51B_4210);
        assert_eq!(one(|a| a.blr(16)), 0xD63F_0200);
        assert_eq!(one(|a| a.br(17)), 0xD61F_0220);
        assert_eq!(one(|a| a.cset(1, 0)), 0x1A9F_17E1);
        assert_eq!(one(|a| a.ldr_index(16, 6, 3)), 0xB863_58D0);
        assert_eq!(one(|a| a.add_ext(16, 6, 16, 2)), 0x8B30_48D0);
        assert_eq!(one(|a| a.mul(0, 1, 2, ZR)), 0x1B02_7C20);
        assert_eq!(one(|a| a.mul_long(true, 0, 1, 2, ZR)), 0x9B22_7C20);
        assert_eq!(one(|a| a.mul_long(false, 0, 1, 2, 3)), 0x9BA2_0C20);
        assert_eq!(one(|a| a.lslv(5, 3, 6)), 0x1AC6_2065);
        assert_eq!(one(|a| a.csinc(2, 2, ZR, 0xA)), 0x1A9F_A442);
        assert_eq!(one(|a| a.indexed(Indexed::LoadByte, 6, 6, 5)), 0x3865_48C6);
        assert_eq!(
            one(|a| a.indexed(Indexed::LoadSignedByte, 6, 6, 5)),
            0x38E5_48C6
        );
        assert_eq!(one(|a| a.indexed(Indexed::LoadHalf, 6, 6, 5)), 0x7865_48C6);
        assert_eq!(
            one(|a| a.indexed(Indexed::LoadSignedHalf, 6, 6, 5)),
            0x78E5_48C6
        );
        assert_eq!(one(|a| a.indexed(Indexed::LoadWord, 6, 6, 5)), 0xB865_48C6);
        assert_eq!(one(|a| a.indexed(Indexed::StoreByte, 1, 6, 5)), 0x3825_48C1);
        assert_eq!(one(|a| a.indexed(Indexed::StoreHalf, 1, 6, 5)), 0x7825_48C1);
        assert_eq!(one(|a| a.indexed(Indexed::StoreWord, 1, 6, 5)), 0xB825_48C1);
        assert_eq!(one(|a| a.rorv(6, 6, 7)), 0x1AC7_2CC6);
        assert_eq!(one(|a| a.lslv64(3, 3, 17)), 0x9AD1_2063);
        assert_eq!(one(|a| a.lsrv64(6, 6, 0)), 0x9AC0_24C6);
        assert_eq!(one(|a| a.asrv64(6, 6, 0)), 0x9AC0_28C6);
        assert_eq!(one(|a| a.sxtw(6, 19)), 0x9340_7E66);
        let top = |a: &mut Asm| {
            let here = a.label();
            a.bind(here);
            a.tbz(false, 6, 63, here);
        };
        assert_eq!(one(top), 0xB6F8_0006);
        assert_eq!(
            one(|a| a.arith(true, true, false, 6, 5, 6, Shift::Lsl, 0)),
            0xCB06_00A6
        );
    }

    #[test]
    fn branches_reach_their_labels() {
        let mut asm = Asm::new();
        let back = asm.label();
        asm.bind(back);
        asm.emit(0xD503_201F);
        let ahead = asm.label();
        asm.b_cond(1, ahead);
        asm.tbz(true, 17, 31, ahead);
        asm.b(back);
        asm.bind(ahead);
        let bytes = asm.finish().expect("labels bind");
        let words: Vec<u32> = bytes
            .chunks(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
            .collect();
        assert_eq!(words[1], 0x5400_0061);
        assert_eq!(words[2], 0x37F8_0051);
        assert_eq!(words[3], 0x17FF_FFFD);
    }

    #[test]
    fn logical_immediates_cover_the_bitmask_patterns() {
        assert_eq!(logical_immediate(0xFF), Some((0, 7)));
        assert_eq!(logical_immediate(0xFFFF), Some((0, 15)));
        assert_eq!(logical_immediate(0x3000_0000), Some((4, 1)));
        assert_eq!(logical_immediate(0xFFFF_FFFE), Some((31, 30)));
        assert_eq!(logical_immediate(0x5555_5555), Some((0, 0b11_1100)));
        assert_eq!(logical_immediate(0x0F0F_0F0F), Some((0, 0b11_0011)));
        assert_eq!(logical_immediate(0x1234_5678), None);
        assert_eq!(logical_immediate(0), None);
    }
}
