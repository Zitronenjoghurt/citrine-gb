pub trait Bus {
    fn read8(&mut self, addr: u16) -> u8;
    fn write8(&mut self, addr: u16, value: u8);

    fn read16(&mut self, addr: u16) -> u16 {
        let a = addr & !1;
        u16::from_be_bytes([self.read8(a), self.read8(a | 1)])
    }

    fn write16(&mut self, addr: u16, value: u16) {
        let a = addr & !1;
        let [h, l] = value.to_be_bytes();
        self.write8(a, h);
        self.write8(a | 1, l);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    fn mask(self) -> u32 {
        match self {
            Size::Byte => 0xFF,
            Size::Word => 0xFFFF,
            Size::Long => 0xFFFF_FFFF,
        }
    }

    fn sign(self) -> u32 {
        match self {
            Size::Byte => 0x80,
            Size::Word => 0x8000,
            Size::Long => 0x8000_0000,
        }
    }

    fn half(self) -> u32 {
        match self {
            Size::Byte => 0xF,
            Size::Word => 0xFFF,
            Size::Long => 0xFFF_FFFF,
        }
    }

    fn bytes(self) -> u32 {
        match self {
            Size::Byte => 1,
            Size::Word => 2,
            Size::Long => 4,
        }
    }
}

pub const FLAG_I: u8 = 0x80;
pub const FLAG_H: u8 = 0x20;
pub const FLAG_N: u8 = 0x08;
pub const FLAG_Z: u8 = 0x04;
pub const FLAG_V: u8 = 0x02;
pub const FLAG_C: u8 = 0x01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub pc: u16,
    pub opcode: u16,
}

#[derive(Clone, Debug, Default)]
pub struct Cpu {
    pub er: [u32; 8],
    pub pc: u16,
    pub ccr: u8,
    pub sleeping: bool,
    pub cycles: u64,
}

impl Cpu {
    pub fn reset(&mut self, bus: &mut impl Bus) {
        self.ccr = FLAG_I;
        self.sleeping = false;
        self.pc = bus.read16(0);
    }

    pub fn r8(&self, n: u8) -> u8 {
        let n = n & 0xF;
        if n < 8 {
            (self.er[n as usize] >> 8) as u8
        } else {
            self.er[(n - 8) as usize] as u8
        }
    }

    pub fn set_r8(&mut self, n: u8, v: u8) {
        let n = n & 0xF;
        if n < 8 {
            let r = &mut self.er[n as usize];
            *r = (*r & !0xFF00) | ((v as u32) << 8);
        } else {
            let r = &mut self.er[(n - 8) as usize];
            *r = (*r & !0xFF) | v as u32;
        }
    }

    pub fn r16(&self, n: u8) -> u16 {
        let n = n & 0xF;
        if n < 8 {
            self.er[n as usize] as u16
        } else {
            (self.er[(n - 8) as usize] >> 16) as u16
        }
    }

    pub fn set_r16(&mut self, n: u8, v: u16) {
        let n = n & 0xF;
        if n < 8 {
            let r = &mut self.er[n as usize];
            *r = (*r & 0xFFFF_0000) | v as u32;
        } else {
            let r = &mut self.er[(n - 8) as usize];
            *r = (*r & 0xFFFF) | ((v as u32) << 16);
        }
    }

    fn reg(&self, size: Size, n: u8) -> u32 {
        match size {
            Size::Byte => self.r8(n) as u32,
            Size::Word => self.r16(n) as u32,
            Size::Long => self.er[(n & 7) as usize],
        }
    }

    fn set_reg(&mut self, size: Size, n: u8, v: u32) {
        match size {
            Size::Byte => self.set_r8(n, v as u8),
            Size::Word => self.set_r16(n, v as u16),
            Size::Long => self.er[(n & 7) as usize] = v,
        }
    }

    fn flag(&self, f: u8) -> bool {
        self.ccr & f != 0
    }

    fn set_flag(&mut self, f: u8, on: bool) {
        if on {
            self.ccr |= f;
        } else {
            self.ccr &= !f;
        }
    }

    fn set_nz(&mut self, v: u32, size: Size) {
        let v = v & size.mask();
        self.set_flag(FLAG_N, v & size.sign() != 0);
        self.set_flag(FLAG_Z, v == 0);
    }

    fn set_logic(&mut self, v: u32, size: Size) {
        self.set_nz(v, size);
        self.set_flag(FLAG_V, false);
    }

    fn add(&mut self, a: u32, b: u32, carry: u32, size: Size) -> u32 {
        let m = size.mask();
        let (a, b) = (a & m, b & m);
        let wide = a as u64 + b as u64 + carry as u64;
        let r = wide as u32 & m;
        let h = size.half();
        self.set_flag(FLAG_H, (a & h) + (b & h) + carry > h);
        self.set_nz(r, size);
        self.set_flag(FLAG_V, (a ^ r) & (b ^ r) & size.sign() != 0);
        self.set_flag(FLAG_C, wide > m as u64);
        r
    }

    fn sub(&mut self, a: u32, b: u32, borrow: u32, size: Size) -> u32 {
        let m = size.mask();
        let (a, b) = (a & m, b & m);
        let r = a.wrapping_sub(b).wrapping_sub(borrow) & m;
        let h = size.half();
        self.set_flag(FLAG_H, (b & h) + borrow > (a & h));
        self.set_nz(r, size);
        self.set_flag(FLAG_V, (a ^ b) & (a ^ r) & size.sign() != 0);
        self.set_flag(FLAG_C, b as u64 + borrow as u64 > a as u64);
        r
    }

    fn fetch(&mut self, bus: &mut impl Bus) -> u16 {
        let w = bus.read16(self.pc);
        self.pc = self.pc.wrapping_add(2);
        self.cycles += 2;
        w
    }

    fn read(&mut self, bus: &mut impl Bus, size: Size, addr: u32) -> u32 {
        let a = addr as u16;
        self.cycles += 2;
        match size {
            Size::Byte => bus.read8(a) as u32,
            Size::Word => bus.read16(a) as u32,
            Size::Long => {
                self.cycles += 2;
                ((bus.read16(a) as u32) << 16) | bus.read16(a.wrapping_add(2)) as u32
            }
        }
    }

    fn write(&mut self, bus: &mut impl Bus, size: Size, addr: u32, v: u32) {
        let a = addr as u16;
        self.cycles += 2;
        match size {
            Size::Byte => bus.write8(a, v as u8),
            Size::Word => bus.write16(a, v as u16),
            Size::Long => {
                self.cycles += 2;
                bus.write16(a, (v >> 16) as u16);
                bus.write16(a.wrapping_add(2), v as u16);
            }
        }
    }

    fn push16(&mut self, bus: &mut impl Bus, v: u16) {
        self.er[7] = self.er[7].wrapping_sub(2);
        self.write(bus, Size::Word, self.er[7], v as u32);
    }

    fn pop16(&mut self, bus: &mut impl Bus) -> u16 {
        let v = self.read(bus, Size::Word, self.er[7]) as u16;
        self.er[7] = self.er[7].wrapping_add(2);
        v
    }

    pub fn interrupt(&mut self, bus: &mut impl Bus, vector: u8) {
        self.push16(bus, self.pc);
        let ccr = self.ccr as u16;
        self.push16(bus, (ccr << 8) | ccr);
        self.ccr |= FLAG_I;
        self.sleeping = false;
        self.pc = bus.read16(vector as u16 * 2);
        self.cycles += 8;
    }

    fn cond(&self, c: u8) -> bool {
        let n = self.flag(FLAG_N);
        let z = self.flag(FLAG_Z);
        let v = self.flag(FLAG_V);
        let c_ = self.flag(FLAG_C);
        match c & 0xF {
            0x0 => true,
            0x1 => false,
            0x2 => !(c_ || z),
            0x3 => c_ || z,
            0x4 => !c_,
            0x5 => c_,
            0x6 => !z,
            0x7 => z,
            0x8 => !v,
            0x9 => v,
            0xA => !n,
            0xB => n,
            0xC => n == v,
            0xD => n != v,
            0xE => !z && n == v,
            _ => z || n != v,
        }
    }

    fn jump(&mut self, target: u32) {
        self.pc = target as u16;
        self.cycles += 2;
    }

    fn call(&mut self, bus: &mut impl Bus, target: u32) {
        self.push16(bus, self.pc);
        self.jump(target);
    }

    fn mov_load(&mut self, bus: &mut impl Bus, size: Size, addr: u32, rd: u8) {
        let v = self.read(bus, size, addr);
        self.set_reg(size, rd, v);
        self.set_logic(v, size);
    }

    fn mov_store(&mut self, bus: &mut impl Bus, size: Size, addr: u32, rs: u8) {
        let v = self.reg(size, rs);
        self.write(bus, size, addr, v);
        self.set_logic(v, size);
    }

    fn mov_mem(&mut self, bus: &mut impl Bus, size: Size, op: u8, b: u8) -> Result<(), ()> {
        let store = b & 0x80 != 0;
        let ptr = ((b >> 4) & 7) as usize;
        let reg = b & 0xF;
        match op {
            0x68 | 0x69 => {
                let addr = self.er[ptr];
                if store {
                    self.mov_store(bus, size, addr, reg);
                } else {
                    self.mov_load(bus, size, addr, reg);
                }
            }
            0x6C | 0x6D => {
                if store {
                    let v = self.reg(size, reg);
                    self.er[ptr] = self.er[ptr].wrapping_sub(size.bytes());
                    let addr = self.er[ptr];
                    self.write(bus, size, addr, v);
                    self.set_logic(v, size);
                } else {
                    let addr = self.er[ptr];
                    self.er[ptr] = self.er[ptr].wrapping_add(size.bytes());
                    self.mov_load(bus, size, addr, reg);
                }
            }
            0x6E | 0x6F => {
                let d = self.fetch(bus) as i16 as i32 as u32;
                let addr = self.er[ptr].wrapping_add(d);
                if store {
                    self.mov_store(bus, size, addr, reg);
                } else {
                    self.mov_load(bus, size, addr, reg);
                }
            }
            0x6A | 0x6B => {
                let addr = match b >> 4 {
                    0x0 | 0x8 => self.fetch(bus) as i16 as i32 as u32,
                    0x2 | 0xA => {
                        let hi = self.fetch(bus) as u32;
                        ((hi & 0xFF) << 16) | self.fetch(bus) as u32
                    }
                    _ => return Err(()),
                };
                if store {
                    self.mov_store(bus, size, addr, reg);
                } else {
                    self.mov_load(bus, size, addr, reg);
                }
            }
            _ => return Err(()),
        }
        Ok(())
    }

    fn mov_d24(&mut self, bus: &mut impl Bus, size: Size, b: u8) -> Result<(), ()> {
        let ptr = ((b >> 4) & 7) as usize;
        let w = self.fetch(bus);
        let sub = (w & 0xFF) as u8;
        let hi = self.fetch(bus) as u32;
        let lo = self.fetch(bus) as u32;
        let disp = ((hi & 0xFF) << 16) | lo;
        let disp = ((disp << 8) as i32 >> 8) as u32;
        let addr = self.er[ptr].wrapping_add(disp);
        let reg = sub & 0xF;
        match sub >> 4 {
            0x2 => self.mov_load(bus, size, addr, reg),
            0xA => self.mov_store(bus, size, addr, reg),
            _ => return Err(()),
        }
        Ok(())
    }

    fn shift(&mut self, op: u8, b: u8) -> Result<(), ()> {
        let size = match b >> 4 & 3 {
            0 => Size::Byte,
            1 => Size::Word,
            3 => Size::Long,
            _ => return Err(()),
        };
        let alt = b & 0x80 != 0;
        let n = b & 0xF;
        let v = self.reg(size, n);
        let m = size.mask();
        let s = size.sign();
        let c = self.flag(FLAG_C);
        let (r, carry) = match (op, alt) {
            (0x10, _) => ((v << 1) & m, v & s != 0),
            (0x11, false) => (v >> 1, v & 1 != 0),
            (0x11, true) => ((v >> 1) | (v & s), v & 1 != 0),
            (0x12, false) => (((v << 1) | c as u32) & m, v & s != 0),
            (0x12, true) => (((v << 1) | (v & s != 0) as u32) & m, v & s != 0),
            (0x13, false) => ((v >> 1) | if c { s } else { 0 }, v & 1 != 0),
            _ => ((v >> 1) | if v & 1 != 0 { s } else { 0 }, v & 1 != 0),
        };
        self.set_reg(size, n, r);
        self.set_nz(r, size);
        self.set_flag(FLAG_C, carry);
        let overflow = op == 0x10 && alt && (v ^ r) & s != 0;
        self.set_flag(FLAG_V, overflow);
        Ok(())
    }

    fn unary(&mut self, b: u8) -> Result<(), ()> {
        let n = b & 0xF;
        match b >> 4 {
            0x0 | 0x1 | 0x3 => {
                let size = [Size::Byte, Size::Word, Size::Word, Size::Long][(b >> 4) as usize];
                let r = !self.reg(size, n) & size.mask();
                self.set_reg(size, n, r);
                self.set_logic(r, size);
            }
            0x5 => {
                let r = self.r16(n) as u32 & 0xFF;
                self.set_r16(n, r as u16);
                self.set_logic(r, Size::Word);
            }
            0x7 => {
                let r = self.er[(n & 7) as usize] & 0xFFFF;
                self.er[(n & 7) as usize] = r;
                self.set_logic(r, Size::Long);
            }
            0x8 | 0x9 | 0xB => {
                let size = [Size::Byte, Size::Word, Size::Word, Size::Long][(b >> 4 & 3) as usize];
                let v = self.reg(size, n);
                let r = self.sub(0, v, 0, size);
                self.set_reg(size, n, r);
            }
            0xD => {
                let r = self.r16(n) as u8 as i8 as i16 as u16;
                self.set_r16(n, r);
                self.set_logic(r as u32, Size::Word);
            }
            0xF => {
                let r = self.er[(n & 7) as usize] as u16 as i16 as i32 as u32;
                self.er[(n & 7) as usize] = r;
                self.set_logic(r, Size::Long);
            }
            _ => return Err(()),
        }
        Ok(())
    }

    fn inc_dec(&mut self, op: u8, b: u8) -> Result<(), ()> {
        let n = b & 0xF;
        let dec = op == 0x1B;
        let (size, k) = match b >> 4 {
            0x0 | 0x8 | 0x9 => {
                let k = match b >> 4 {
                    0x0 => 1,
                    0x8 => 2,
                    _ => 4,
                };
                let r = &mut self.er[(n & 7) as usize];
                *r = if dec {
                    r.wrapping_sub(k)
                } else {
                    r.wrapping_add(k)
                };
                return Ok(());
            }
            0x5 => (Size::Word, 1),
            0xD => (Size::Word, 2),
            0x7 => (Size::Long, 1),
            0xF => (Size::Long, 2),
            _ => return Err(()),
        };
        self.step_value(size, n, k, dec);
        Ok(())
    }

    fn step_value(&mut self, size: Size, n: u8, k: u32, dec: bool) {
        let v = self.reg(size, n);
        let m = size.mask();
        let s = size.sign();
        let r = if dec {
            v.wrapping_sub(k) & m
        } else {
            v.wrapping_add(k) & m
        };
        self.set_reg(size, n, r);
        self.set_nz(r, size);
        let overflow = if dec { v & !r & s } else { !v & r & s };
        self.set_flag(FLAG_V, overflow != 0);
    }

    fn bit_op(&mut self, sub: u8, arg: u8, value: u8) -> Result<Option<u8>, ()> {
        let bit = match sub {
            0x60..=0x63 => self.r8(arg >> 4) & 7,
            _ => (arg >> 4) & 7,
        };
        let invert = matches!(sub, 0x67 | 0x74..=0x77) && arg & 0x80 != 0;
        let mask = 1u8 << bit;
        let set = value & mask != 0;
        let operand = set != invert;
        let c = self.flag(FLAG_C);
        Ok(match sub {
            0x60 | 0x70 => Some(value | mask),
            0x61 | 0x71 => Some(value ^ mask),
            0x62 | 0x72 => Some(value & !mask),
            0x63 | 0x73 => {
                self.set_flag(FLAG_Z, !set);
                None
            }
            0x67 => {
                let out = c != invert;
                Some(if out { value | mask } else { value & !mask })
            }
            0x74 => {
                self.set_flag(FLAG_C, c || operand);
                None
            }
            0x75 => {
                self.set_flag(FLAG_C, c != operand);
                None
            }
            0x76 => {
                self.set_flag(FLAG_C, c && operand);
                None
            }
            0x77 => {
                self.set_flag(FLAG_C, operand);
                None
            }
            _ => return Err(()),
        })
    }

    fn bit_mem(&mut self, bus: &mut impl Bus, addr: u16, writes: bool) -> Result<(), ()> {
        let w = self.fetch(bus);
        let sub = (w >> 8) as u8;
        let arg = w as u8;
        let ok = if writes {
            matches!(sub, 0x60..=0x62 | 0x67 | 0x70..=0x72)
        } else {
            matches!(sub, 0x63 | 0x73..=0x77)
        };
        if !ok {
            return Err(());
        }
        let v = self.read(bus, Size::Byte, addr as u32) as u8;
        if let Some(nv) = self.bit_op(sub, arg, v)? {
            self.write(bus, Size::Byte, addr as u32, nv as u32);
        }
        Ok(())
    }

    fn alu_reg(&mut self, op: u8, size: Size, rs: u32, rd: u8) {
        let d = self.reg(size, rd);
        let r = match op {
            0 => {
                self.set_logic(rs, size);
                rs
            }
            1 => self.add(d, rs, 0, size),
            2 => {
                self.sub(d, rs, 0, size);
                return;
            }
            3 => self.sub(d, rs, 0, size),
            4 => {
                let r = d | rs;
                self.set_logic(r, size);
                r
            }
            5 => {
                let r = d ^ rs;
                self.set_logic(r, size);
                r
            }
            _ => {
                let r = d & rs;
                self.set_logic(r, size);
                r
            }
        };
        self.set_reg(size, rd, r);
    }

    fn op01(&mut self, bus: &mut impl Bus, b: u8) -> Result<(), ()> {
        match b {
            0x00 => {
                let w = self.fetch(bus);
                let op = (w >> 8) as u8;
                let b2 = w as u8;
                if op == 0x78 {
                    self.mov_d24(bus, Size::Long, b2)
                } else if matches!(op, 0x69 | 0x6B | 0x6D | 0x6F) {
                    self.mov_mem(bus, Size::Long, op, b2)
                } else {
                    Err(())
                }
            }
            0x40 => {
                let w = self.fetch(bus);
                let op = (w >> 8) as u8;
                let b2 = w as u8;
                let store = b2 & 0x80 != 0;
                let ptr = ((b2 >> 4) & 7) as usize;
                let addr = match op {
                    0x69 => self.er[ptr],
                    0x6B => match b2 >> 4 {
                        0x0 | 0x8 => self.fetch(bus) as i16 as i32 as u32,
                        0x2 | 0xA => {
                            let hi = self.fetch(bus) as u32;
                            ((hi & 0xFF) << 16) | self.fetch(bus) as u32
                        }
                        _ => return Err(()),
                    },
                    0x6D => {
                        if store {
                            self.er[ptr] = self.er[ptr].wrapping_sub(2);
                            self.er[ptr]
                        } else {
                            let a = self.er[ptr];
                            self.er[ptr] = a.wrapping_add(2);
                            a
                        }
                    }
                    0x6F => self.er[ptr].wrapping_add(self.fetch(bus) as i16 as i32 as u32),
                    0x78 => {
                        let w2 = self.fetch(bus);
                        let hi = self.fetch(bus) as u32;
                        let lo = self.fetch(bus) as u32;
                        let disp = ((((hi & 0xFF) << 16) | lo) << 8) as i32 >> 8;
                        let store = w2 & 0x80 != 0;
                        let addr = self.er[ptr].wrapping_add(disp as u32);
                        return self.ldc_stc(bus, addr, store);
                    }
                    _ => return Err(()),
                };
                let store = if op == 0x6B { b2 & 0x80 != 0 } else { store };
                self.ldc_stc(bus, addr, store)
            }
            0x80 => {
                self.sleeping = true;
                Ok(())
            }
            0xC0 | 0xD0 => {
                let w = self.fetch(bus);
                let op = (w >> 8) as u8;
                let rs = (w >> 4) as u8 & 0xF;
                let rd = w as u8 & 0xF;
                match (b, op) {
                    (0xC0, 0x50) => {
                        let r = (self.r16(rd) as u8 as i8 as i16)
                            .wrapping_mul(self.r8(rs) as i8 as i16)
                            as u16;
                        self.set_r16(rd, r);
                        self.set_nz(r as u32, Size::Word);
                        self.cycles += 10;
                    }
                    (0xC0, 0x52) => {
                        let n = (rd & 7) as usize;
                        let r = (self.er[n] as u16 as i16 as i32)
                            .wrapping_mul(self.r16(rs) as i16 as i32)
                            as u32;
                        self.er[n] = r;
                        self.set_nz(r, Size::Long);
                        self.cycles += 18;
                    }
                    (0xD0, 0x51) => {
                        let divisor = self.r8(rs) as i8 as i32;
                        let dividend = self.r16(rd) as i16 as i32;
                        self.set_flag(FLAG_Z, divisor == 0);
                        if divisor != 0 {
                            let q = dividend.wrapping_div(divisor);
                            let r = dividend.wrapping_rem(divisor);
                            self.set_r16(rd, ((r as u8 as u16) << 8) | q as u8 as u16);
                            self.set_flag(FLAG_N, q < 0);
                        }
                        self.cycles += 10;
                    }
                    (0xD0, 0x53) => {
                        let n = (rd & 7) as usize;
                        let divisor = self.r16(rs) as i16 as i64;
                        let dividend = self.er[n] as i32 as i64;
                        self.set_flag(FLAG_Z, divisor == 0);
                        if divisor != 0 {
                            let q = dividend / divisor;
                            let r = dividend % divisor;
                            self.er[n] = ((r as u16 as u32) << 16) | q as u16 as u32;
                            self.set_flag(FLAG_N, q < 0);
                        }
                        self.cycles += 18;
                    }
                    _ => return Err(()),
                }
                Ok(())
            }
            0xF0 => {
                let w = self.fetch(bus);
                let op = (w >> 8) as u8;
                let rs = self.er[(w >> 4) as usize & 7];
                let rd = (w & 7) as u8;
                let kind = match op {
                    0x64 => 4,
                    0x65 => 5,
                    0x66 => 6,
                    _ => return Err(()),
                };
                self.alu_reg(kind, Size::Long, rs, rd);
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn ldc_stc(&mut self, bus: &mut impl Bus, addr: u32, store: bool) -> Result<(), ()> {
        if store {
            let v = (self.ccr as u32) << 8 | self.ccr as u32;
            self.write(bus, Size::Word, addr, v);
        } else {
            self.ccr = (self.read(bus, Size::Word, addr) >> 8) as u8;
        }
        Ok(())
    }

    pub fn step(&mut self, bus: &mut impl Bus) -> Result<u32, Fault> {
        let start = self.cycles;
        let pc = self.pc;
        let w0 = self.fetch(bus);
        if self.execute(bus, w0).is_err() {
            self.pc = pc;
            return Err(Fault { pc, opcode: w0 });
        }
        Ok((self.cycles - start) as u32)
    }

    fn execute(&mut self, bus: &mut impl Bus, w0: u16) -> Result<(), ()> {
        let op = (w0 >> 8) as u8;
        let b = w0 as u8;
        let hi = b >> 4;
        let lo = b & 0xF;
        match op {
            0x00 => {}
            0x01 => self.op01(bus, b)?,
            0x02 => self.set_r8(lo, self.ccr),
            0x03 => self.ccr = self.r8(lo),
            0x04 => self.ccr |= b,
            0x05 => self.ccr ^= b,
            0x06 => self.ccr &= b,
            0x07 => self.ccr = b,
            0x08 => self.alu_reg(1, Size::Byte, self.r8(hi) as u32, lo),
            0x09 => self.alu_reg(1, Size::Word, self.r16(hi) as u32, lo),
            0x0A => {
                if hi == 0 {
                    self.step_value(Size::Byte, lo, 1, false);
                } else if hi & 8 != 0 {
                    self.alu_reg(1, Size::Long, self.er[(hi & 7) as usize], lo);
                } else {
                    return Err(());
                }
            }
            0x0B | 0x1B => self.inc_dec(op, b)?,
            0x0C => self.alu_reg(0, Size::Byte, self.r8(hi) as u32, lo),
            0x0D => self.alu_reg(0, Size::Word, self.r16(hi) as u32, lo),
            0x0E | 0x1E => {
                let s = self.r8(hi) as u32;
                self.carry_op(op == 0x1E, s, lo);
            }
            0x0F => {
                if hi == 0 {
                    self.daa(lo, false);
                } else if hi & 8 != 0 {
                    self.alu_reg(0, Size::Long, self.er[(hi & 7) as usize], lo);
                } else {
                    return Err(());
                }
            }
            0x10..=0x13 => self.shift(op, b)?,
            0x14 => self.alu_reg(4, Size::Byte, self.r8(hi) as u32, lo),
            0x15 => self.alu_reg(5, Size::Byte, self.r8(hi) as u32, lo),
            0x16 => self.alu_reg(6, Size::Byte, self.r8(hi) as u32, lo),
            0x17 => self.unary(b)?,
            0x18 => self.alu_reg(3, Size::Byte, self.r8(hi) as u32, lo),
            0x19 => self.alu_reg(3, Size::Word, self.r16(hi) as u32, lo),
            0x1A => {
                if hi == 0 {
                    self.step_value(Size::Byte, lo, 1, true);
                } else if hi & 8 != 0 {
                    self.alu_reg(3, Size::Long, self.er[(hi & 7) as usize], lo);
                } else {
                    return Err(());
                }
            }
            0x1C => self.alu_reg(2, Size::Byte, self.r8(hi) as u32, lo),
            0x1D => self.alu_reg(2, Size::Word, self.r16(hi) as u32, lo),
            0x1F => {
                if hi == 0 {
                    self.daa(lo, true);
                } else if hi & 8 != 0 {
                    self.alu_reg(2, Size::Long, self.er[(hi & 7) as usize], lo);
                } else {
                    return Err(());
                }
            }
            0x20..=0x2F => self.mov_load(bus, Size::Byte, 0xFF00 | b as u32, op & 0xF),
            0x30..=0x3F => self.mov_store(bus, Size::Byte, 0xFF00 | b as u32, op & 0xF),
            0x40..=0x4F => {
                if self.cond(op) {
                    self.jump((self.pc as i32 + b as i8 as i32) as u32);
                }
            }
            0x50 => {
                let r = (self.r16(lo) & 0xFF) * self.r8(hi) as u16;
                self.set_r16(lo, r);
                self.cycles += 10;
            }
            0x52 => {
                let n = (lo & 7) as usize;
                self.er[n] = (self.er[n] & 0xFFFF) * self.r16(hi) as u32;
                self.cycles += 18;
            }
            0x51 => {
                let divisor = self.r8(hi) as u16;
                let dividend = self.r16(lo);
                self.set_flag(FLAG_N, divisor & 0x80 != 0);
                self.set_flag(FLAG_Z, divisor == 0);
                if let Some(q) = dividend.checked_div(divisor) {
                    let r = dividend % divisor;
                    self.set_r16(lo, (r << 8) | (q & 0xFF));
                }
                self.cycles += 10;
            }
            0x53 => {
                let n = (lo & 7) as usize;
                let divisor = self.r16(hi) as u32;
                self.set_flag(FLAG_N, divisor & 0x8000 != 0);
                self.set_flag(FLAG_Z, divisor == 0);
                if divisor != 0 {
                    let d = self.er[n];
                    self.er[n] = ((d % divisor) << 16) | ((d / divisor) & 0xFFFF);
                }
                self.cycles += 18;
            }
            0x54 => {
                let pc = self.pop16(bus);
                self.jump(pc as u32);
            }
            0x55 => {
                let target = (self.pc as i32 + b as i8 as i32) as u32;
                self.call(bus, target);
            }
            0x56 => {
                self.ccr = (self.pop16(bus) >> 8) as u8;
                let pc = self.pop16(bus);
                self.jump(pc as u32);
            }
            0x57 => {
                let vector = 8 + ((b >> 4) & 3);
                self.interrupt(bus, vector);
            }
            0x58 => {
                let d = self.fetch(bus) as i16 as i32;
                if self.cond(hi) {
                    self.jump((self.pc as i32 + d) as u32);
                }
            }
            0x59 => self.jump(self.er[(hi & 7) as usize]),
            0x5A => {
                let a = self.fetch(bus) as u32;
                self.jump(((b as u32) << 16) | a);
            }
            0x5B => {
                let a = self.read(bus, Size::Word, b as u32);
                self.jump(a);
            }
            0x5C => {
                let d = self.fetch(bus) as i16 as i32;
                let target = (self.pc as i32 + d) as u32;
                self.call(bus, target);
            }
            0x5D => {
                let target = self.er[(hi & 7) as usize];
                self.call(bus, target);
            }
            0x5E => {
                let a = self.fetch(bus) as u32;
                self.call(bus, ((b as u32) << 16) | a);
            }
            0x5F => {
                let a = self.read(bus, Size::Word, b as u32);
                self.call(bus, a);
            }
            0x60..=0x63 | 0x67 | 0x70..=0x77 => {
                let v = self.r8(lo);
                if let Some(nv) = self.bit_op(op, b & 0xF0, v)? {
                    self.set_r8(lo, nv);
                }
            }
            0x64 => self.alu_reg(4, Size::Word, self.r16(hi) as u32, lo),
            0x65 => self.alu_reg(5, Size::Word, self.r16(hi) as u32, lo),
            0x66 => self.alu_reg(6, Size::Word, self.r16(hi) as u32, lo),
            0x68 | 0x6A | 0x6C | 0x6E => self.mov_mem(bus, Size::Byte, op, b)?,
            0x69 | 0x6B | 0x6D | 0x6F => self.mov_mem(bus, Size::Word, op, b)?,
            0x78 => {
                let w = bus.read16(self.pc);
                let size = match w >> 8 {
                    0x6A => Size::Byte,
                    0x6B => Size::Word,
                    _ => return Err(()),
                };
                self.mov_d24(bus, size, b)?;
            }
            0x79 => {
                let imm = self.fetch(bus) as u32;
                if hi > 6 {
                    return Err(());
                }
                self.alu_reg(hi, Size::Word, imm, lo);
            }
            0x7A => {
                let h = self.fetch(bus) as u32;
                let l = self.fetch(bus) as u32;
                if hi > 6 {
                    return Err(());
                }
                self.alu_reg(hi, Size::Long, (h << 16) | l, lo);
            }
            0x7B => {
                let w = self.fetch(bus);
                if w != 0x598F {
                    return Err(());
                }
                let word = match b {
                    0x5C => false,
                    0xD4 => true,
                    _ => return Err(()),
                };
                self.eepmov(bus, word);
            }
            0x7C | 0x7D => {
                let addr = self.er[(hi & 7) as usize] as u16;
                self.bit_mem(bus, addr, op == 0x7D)?;
            }
            0x7E | 0x7F => self.bit_mem(bus, 0xFF00 | b as u16, op == 0x7F)?,
            0x80..=0xFF => {
                let r = op & 0xF;
                let imm = b as u32;
                match op >> 4 {
                    0x8 => self.alu_reg(1, Size::Byte, imm, r),
                    0x9 => self.carry_op(false, imm, r),
                    0xA => self.alu_reg(2, Size::Byte, imm, r),
                    0xB => self.carry_op(true, imm, r),
                    0xC => self.alu_reg(4, Size::Byte, imm, r),
                    0xD => self.alu_reg(5, Size::Byte, imm, r),
                    0xE => self.alu_reg(6, Size::Byte, imm, r),
                    _ => self.alu_reg(0, Size::Byte, imm, r),
                }
            }
        }
        Ok(())
    }

    fn carry_op(&mut self, subtract: bool, s: u32, rd: u8) {
        let z = self.flag(FLAG_Z);
        let c = self.flag(FLAG_C) as u32;
        let d = self.r8(rd) as u32;
        let r = if subtract {
            self.sub(d, s, c, Size::Byte)
        } else {
            self.add(d, s, c, Size::Byte)
        };
        self.set_r8(rd, r as u8);
        self.set_flag(FLAG_Z, z && r == 0);
    }

    fn daa(&mut self, rd: u8, subtract: bool) {
        let v = self.r8(rd);
        let c = self.flag(FLAG_C);
        let h = self.flag(FLAG_H);
        let mut adj = 0u8;
        let mut carry = c;
        if subtract {
            if h {
                adj |= 0x06;
            }
            if c {
                adj |= 0x60;
            }
            let r = v.wrapping_sub(adj);
            self.set_r8(rd, r);
            self.set_nz(r as u32, Size::Byte);
        } else {
            if h || v & 0xF > 9 {
                adj |= 0x06;
            }
            if c || v > 0x99 {
                adj |= 0x60;
                carry = true;
            }
            let r = v.wrapping_add(adj);
            self.set_r8(rd, r);
            self.set_nz(r as u32, Size::Byte);
        }
        self.set_flag(FLAG_C, carry);
    }

    fn eepmov(&mut self, bus: &mut impl Bus, word: bool) {
        loop {
            let count = if word {
                self.r16(4)
            } else {
                self.r8(0xC) as u16
            };
            if count == 0 {
                break;
            }
            let v = self.read(bus, Size::Byte, self.er[5]);
            self.write(bus, Size::Byte, self.er[6], v);
            self.er[5] = self.er[5].wrapping_add(1);
            self.er[6] = self.er[6].wrapping_add(1);
            if word {
                self.set_r16(4, count - 1);
            } else {
                self.set_r8(0xC, count as u8 - 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram(Vec<u8>);

    impl Bus for Ram {
        fn read8(&mut self, addr: u16) -> u8 {
            self.0[addr as usize]
        }

        fn write8(&mut self, addr: u16, value: u8) {
            self.0[addr as usize] = value;
        }
    }

    fn run(code: &[u8], steps: usize) -> (Cpu, Ram) {
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0] = 0x01;
        ram.0[1] = 0x00;
        ram.0[0x100..0x100 + code.len()].copy_from_slice(code);
        let mut cpu = Cpu::default();
        cpu.reset(&mut ram);
        cpu.er[7] = 0xFF00;
        for _ in 0..steps {
            cpu.step(&mut ram).expect("valid instruction");
        }
        (cpu, ram)
    }

    #[test]
    fn mov_immediates_set_registers_and_flags() {
        let (cpu, _) = run(
            &[
                0xF8, 0x80, 0x79, 0x03, 0x12, 0x34, 0x7A, 0x05, 0xDE, 0xAD, 0xBE, 0xEF,
            ],
            3,
        );
        assert_eq!(cpu.r8(0x8), 0x80);
        assert_eq!(cpu.r16(3), 0x1234);
        assert_eq!(cpu.er[5], 0xDEAD_BEEF);
        assert!(cpu.ccr & FLAG_N != 0);
        assert_eq!(cpu.pc, 0x10C);
    }

    #[test]
    fn byte_registers_split_the_low_word() {
        let mut cpu = Cpu::default();
        cpu.er[2] = 0x1122_3344;
        assert_eq!(cpu.r8(2), 0x33);
        assert_eq!(cpu.r8(0xA), 0x44);
        assert_eq!(cpu.r16(0xA), 0x1122);
        cpu.set_r8(0xA, 0xFF);
        cpu.set_r16(0xA, 0xAABB);
        assert_eq!(cpu.er[2], 0xAABB_33FF);
    }

    #[test]
    fn add_byte_sets_half_carry_overflow_and_carry() {
        let (cpu, _) = run(&[0xF8, 0x7F, 0x88, 0x01], 2);
        assert_eq!(cpu.r8(0x8), 0x80);
        assert_eq!(
            cpu.ccr & (FLAG_H | FLAG_N | FLAG_V | FLAG_C),
            FLAG_H | FLAG_N | FLAG_V
        );
        let (cpu, _) = run(&[0xF8, 0xFF, 0x88, 0x01], 2);
        assert_eq!(cpu.r8(0x8), 0);
        assert_eq!(cpu.ccr & (FLAG_Z | FLAG_C), FLAG_Z | FLAG_C);
    }

    #[test]
    fn compare_drives_signed_and_unsigned_branches() {
        let code = [
            0x79, 0x00, 0x00, 0x05, 0x79, 0x20, 0xFF, 0xFF, 0x4E, 0x02, 0xF9, 0x01, 0x42, 0x02,
            0xFA, 0x01,
        ];
        let (cpu, _) = run(&code, 5);
        assert_eq!(cpu.r8(0x9), 0x00);
        assert_eq!(cpu.r8(0xA), 0x01);
    }

    #[test]
    fn addx_keeps_zero_only_when_both_halves_are_zero() {
        let (cpu, _) = run(&[0x79, 0x00, 0x00, 0xFF, 0x88, 0x01, 0x90, 0xFF], 3);
        assert_eq!(cpu.r16(0), 0);
        assert_eq!(cpu.ccr & (FLAG_Z | FLAG_C), FLAG_Z | FLAG_C);
        let (cpu, _) = run(&[0x79, 0x00, 0x00, 0xFE, 0x88, 0x01, 0x90, 0x00], 3);
        assert_eq!(cpu.r16(0), 0x00FF);
        assert_eq!(cpu.ccr & FLAG_Z, 0);
        let (cpu, _) = run(&[0x79, 0x00, 0x01, 0x00, 0x18, 0x00, 0xB0, 0x01], 3);
        assert_eq!(cpu.r16(0), 0xFF00);
        assert_eq!(cpu.ccr & (FLAG_Z | FLAG_C), FLAG_C);
    }

    #[test]
    fn push_pop_and_subroutines() {
        let code = [
            0x7A, 0x03, 0x01, 0x02, 0x03, 0x04, 0x01, 0x00, 0x6D, 0xF3, 0x5E, 0x00, 0x01, 0x20,
            0x01, 0x00, 0x6D, 0x74, 0x40, 0xFE, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x79, 0x03,
            0x00, 0x00, 0x54, 0x70,
        ];
        let (cpu, ram) = run(&code, 6);
        assert_eq!(cpu.er[4], 0x0102_0304);
        assert_eq!(cpu.er[3], 0x0102_0000);
        assert_eq!(cpu.er[7], 0xFF00);
        assert_eq!(&ram.0[0xFEFC..0xFF00], &[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(cpu.pc, 0x112);
    }

    #[test]
    fn bsr_and_relative_branches_use_the_next_pc() {
        let (cpu, ram) = run(&[0x55, 0x02, 0x00, 0x00, 0x40, 0xFE], 1);
        assert_eq!(cpu.pc, 0x104);
        assert_eq!(&ram.0[0xFEFE..0xFF00], &[0x01, 0x02]);
        let (cpu, _) = run(&[0x58, 0x00, 0x00, 0x10], 1);
        assert_eq!(cpu.pc, 0x114);
    }

    #[test]
    fn multiply_and_divide() {
        let code = [
            0xF8, 0x10, 0xF9, 0x20, 0x50, 0x90, 0x79, 0x01, 0x00, 0x07, 0x7A, 0x02, 0x00, 0x01,
            0x00, 0x06, 0x53, 0x12,
        ];
        let (cpu, _) = run(&code, 6);
        assert_eq!(cpu.r16(0), 0x0200);
        assert_eq!(cpu.er[2], ((0x10006 % 7) << 16) | (0x10006 / 7));
    }

    #[test]
    fn signed_multiply_and_divide() {
        let code = [
            0x79, 0x00, 0x00, 0xFE, 0xF9, 0x03, 0x01, 0xC0, 0x50, 0x90, 0x79, 0x02, 0xFF, 0xF9,
            0xFB, 0x02, 0x01, 0xD0, 0x51, 0xB2,
        ];
        let (cpu, _) = run(&code, 6);
        assert_eq!(cpu.r16(0) as i16, -6);
        assert_eq!(cpu.r8(0xA) as i8, -3);
        assert_eq!(cpu.r8(2) as i8, -1);
    }

    #[test]
    fn shifts_and_rotates_move_the_carry() {
        let code = [0xF8, 0x81, 0x10, 0x08, 0x13, 0x08, 0x11, 0x88];
        let (cpu, _) = run(&code, 2);
        assert_eq!(cpu.r8(0x8), 0x02);
        assert!(cpu.ccr & FLAG_C != 0);
        let (cpu, _) = run(&code, 3);
        assert_eq!(cpu.r8(0x8), 0x81);
        let (cpu, _) = run(&code, 4);
        assert_eq!(cpu.r8(0x8), 0xC0);
        assert!(cpu.ccr & FLAG_C != 0);
    }

    #[test]
    fn extend_and_negate() {
        let code = [0xF8, 0x80, 0x17, 0xD0, 0x17, 0xF0, 0x17, 0x90, 0x17, 0x50];
        let (cpu, _) = run(&code, 3);
        assert_eq!(cpu.er[0], 0xFFFF_FF80);
        let (cpu, _) = run(&code, 5);
        assert_eq!(cpu.r16(0), 0x0080);
    }

    #[test]
    fn short_absolute_moves_and_bit_ops_reach_ff00() {
        let code = [
            0xF8, 0x5A, 0x38, 0xD4, 0x7F, 0xD4, 0x70, 0x00, 0x7E, 0xD4, 0x73, 0x00, 0x7F, 0xD4,
            0x72, 0x10, 0x28, 0xD4,
        ];
        let (_, ram) = run(&code, 3);
        assert_eq!(ram.0[0xFFD4], 0x5B);
        let (cpu2, _) = run(&code, 4);
        assert!(cpu2.ccr & FLAG_Z == 0);
        let (cpu3, _) = run(&code, 6);
        assert_eq!(cpu3.r8(0x8), 0x59);
    }

    #[test]
    fn bld_bst_bit_carry_round_trip() {
        let code = [0xF8, 0x04, 0x77, 0x28, 0x67, 0x78, 0x77, 0xA8, 0x67, 0x08];
        let (cpu, _) = run(&code, 3);
        assert_eq!(cpu.r8(0x8), 0x84);
        let (cpu, _) = run(&code, 5);
        assert_eq!(cpu.r8(0x8), 0x84);
    }

    #[test]
    fn indexed_and_displacement_moves() {
        let code = [
            0x7A, 0x01, 0x00, 0x00, 0xE0, 0x00, 0xF8, 0xAB, 0x6E, 0x98, 0x00, 0x10, 0x78, 0x10,
            0x6A, 0x2B, 0x00, 0x00, 0x00, 0x10, 0x6C, 0x1A, 0x6C, 0x9B,
        ];
        let (cpu, ram) = run(&code, 6);
        assert_eq!(ram.0[0xE010], 0xAB);
        assert_eq!(cpu.r8(0xB), 0xAB);
        assert_eq!(ram.0[0xE000], 0xAB);
        assert_eq!(cpu.er[1], 0xE000);
    }

    #[test]
    fn long_moves_through_memory() {
        let code = [
            0x7A, 0x00, 0x12, 0x34, 0x56, 0x78, 0x01, 0x00, 0x6B, 0x80, 0xE0, 0x00, 0x01, 0x00,
            0x6B, 0x03, 0xE0, 0x00,
        ];
        let (cpu, ram) = run(&code, 3);
        assert_eq!(&ram.0[0xE000..0xE004], &[0x12, 0x34, 0x56, 0x78]);
        assert_eq!(cpu.er[3], 0x1234_5678);
    }

    #[test]
    fn interrupt_and_rte_restore_pc_and_ccr() {
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0x0E..0x10].copy_from_slice(&[0x02, 0x00]);
        ram.0[0x200..0x202].copy_from_slice(&[0x56, 0x70]);
        let mut cpu = Cpu {
            pc: 0x1234,
            ccr: FLAG_C,
            ..Cpu::default()
        };
        cpu.er[7] = 0xFF00;
        cpu.interrupt(&mut ram, 7);
        assert_eq!(cpu.pc, 0x200);
        assert!(cpu.ccr & FLAG_I != 0);
        cpu.step(&mut ram).unwrap();
        assert_eq!(cpu.pc, 0x1234);
        assert_eq!(cpu.ccr, FLAG_C);
        assert_eq!(cpu.er[7], 0xFF00);
    }

    #[test]
    fn jsr_through_register_and_memory_indirect() {
        let mut code = vec![0x7A, 0x02, 0x00, 0x00, 0x01, 0x20, 0x5D, 0x20];
        code.resize(0x20, 0);
        code.extend_from_slice(&[0x5F, 0x40]);
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0..2].copy_from_slice(&[0x01, 0x00]);
        ram.0[0x40..0x42].copy_from_slice(&[0x03, 0x00]);
        ram.0[0x100..0x100 + code.len()].copy_from_slice(&code);
        let mut cpu = Cpu::default();
        cpu.reset(&mut ram);
        cpu.er[7] = 0xFF00;
        for _ in 0..3 {
            cpu.step(&mut ram).unwrap();
        }
        assert_eq!(cpu.pc, 0x300);
        assert_eq!(cpu.er[7], 0xFEFC);
    }

    #[test]
    fn eepmov_copies_r4l_bytes() {
        let code = [
            0x7A, 0x05, 0x00, 0x00, 0xE0, 0x00, 0x7A, 0x06, 0x00, 0x00, 0xE1, 0x00, 0xFC, 0x03,
            0x7B, 0x5C, 0x59, 0x8F,
        ];
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0..2].copy_from_slice(&[0x01, 0x00]);
        ram.0[0x100..0x100 + code.len()].copy_from_slice(&code);
        ram.0[0xE000..0xE003].copy_from_slice(&[7, 8, 9]);
        let mut cpu = Cpu::default();
        cpu.reset(&mut ram);
        for _ in 0..4 {
            cpu.step(&mut ram).unwrap();
        }
        assert_eq!(&ram.0[0xE100..0xE104], &[7, 8, 9, 0]);
        assert_eq!(cpu.r8(0xC), 0);
    }

    #[test]
    fn unknown_opcode_faults_without_moving_pc() {
        let (mut cpu, mut ram) = run(&[], 0);
        ram.0[0x100..0x102].copy_from_slice(&[0x01, 0x10]);
        let fault = cpu.step(&mut ram).unwrap_err();
        assert_eq!(
            fault,
            Fault {
                pc: 0x100,
                opcode: 0x0110
            }
        );
        assert_eq!(cpu.pc, 0x100);
    }

    #[test]
    fn sleep_marks_the_cpu_sleeping() {
        let (cpu, _) = run(&[0x01, 0x80], 1);
        assert!(cpu.sleeping);
    }
}
