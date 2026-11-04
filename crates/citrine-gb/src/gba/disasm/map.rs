use super::{Decoded, arm, thumb};
use crate::debug::code::{Address, BlockId, InstrClass, Line};
use crate::debug::disassembly::{CodeEntry, Confidence, Decode, Disassembly, Row};
use crate::gba::bus::Bus;
use crate::gba::region::Region;
use crate::gba::wait::Width;
use crate::instructions::FlowControl;
use std::ops::Range;

pub trait CodeSource {
    fn read_code(&self, addr: u32, width: Width) -> u32;
    fn rom_len(&self) -> usize;
}

impl CodeSource for Bus {
    fn read_code(&self, addr: u32, width: Width) -> u32 {
        self.read(addr, width)
    }

    fn rom_len(&self) -> usize {
        Bus::rom_len(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pc {
    pub addr: u32,
    pub thumb: bool,
}

impl Pc {
    fn aligned(self) -> Self {
        let mask = if self.thumb { !1 } else { !3 };
        Self {
            addr: self.addr & mask,
            ..self
        }
    }

    fn after(self, len: u8) -> Self {
        Self {
            addr: self.addr.wrapping_add(u32::from(len)),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GbaCode {
    pub at: Address,
    pub pc: Pc,
    len: u8,
    pub confidence: Confidence,
}

pub type CodeMap = Disassembly<GbaCode>;

impl CodeEntry for GbaCode {
    type Pc = Pc;
    const MAX_LEN: u32 = 4;

    fn at(&self) -> Address {
        self.at
    }

    fn size(&self) -> u32 {
        u32::from(self.len)
    }

    fn confidence(&self) -> Confidence {
        self.confidence
    }
}

impl<S: CodeSource + ?Sized> Decode<S> for GbaCode {
    fn locate(src: &S, pc: Pc) -> Option<Address> {
        code_address(pc.aligned().addr, src.rom_len())
    }

    fn decode(src: &S, pc: Pc, confidence: Confidence) -> Option<(Self, FlowControl<Pc>, Pc)> {
        let pc = pc.aligned();
        let at = Self::locate(src, pc)?;
        let decoded = decode_at(src, pc);
        if decoded.class == InstrClass::Invalid {
            return None;
        }
        let entry = GbaCode {
            at,
            pc,
            len: decoded.len,
            confidence,
        };
        Some((entry, decoded.flow, pc.after(decoded.len)))
    }
}

pub fn code_address(addr: u32, rom_len: usize) -> Option<Address> {
    let region = Region::of(addr);
    let offset = region.offset(addr);
    let block = match region {
        Region::Bios => BlockId::Bios,
        Region::Ewram => BlockId::Ewram,
        Region::Iwram => BlockId::Iwram,
        Region::Rom(_) if offset < rom_len => BlockId::GamePak,
        _ => return None,
    };
    Some(Address::gba(block, offset as u32))
}

pub fn decode_at(src: &(impl CodeSource + ?Sized), pc: Pc) -> Decoded {
    let pc = pc.aligned();
    if pc.thumb {
        let op = src.read_code(pc.addr, Width::Half) as u16;
        let next = src.read_code(pc.addr.wrapping_add(2), Width::Half) as u16;
        thumb(pc.addr, op, Some(next))
    } else {
        arm(pc.addr, src.read_code(pc.addr, Width::Word))
    }
}

pub fn line(src: &(impl CodeSource + ?Sized), pc: Pc, confidence: Confidence) -> Option<Line> {
    let pc = pc.aligned();
    let at = code_address(pc.addr, src.rom_len())?;
    let decoded = decode_at(src, pc);
    let width = if pc.thumb { Width::Half } else { Width::Word };
    let step = width.bytes() as u32;
    let bytes: Vec<u8> = (0..u32::from(decoded.len) / step)
        .flat_map(|i| {
            let value = src.read_code(pc.addr.wrapping_add(i * step), width);
            value.to_le_bytes().into_iter().take(step as usize)
        })
        .collect();
    Some(Line::code(
        at,
        &bytes,
        decoded.mnemonic,
        decoded.operands,
        decoded.class,
        confidence,
    ))
}

pub fn lines(
    map: &CodeMap,
    src: &(impl CodeSource + ?Sized),
    rows: Range<usize>,
    out: &mut Vec<Line>,
) {
    out.clear();
    for index in rows {
        let Some(Row::Rom(entry)) = map.row(index) else {
            break;
        };
        if let Some(line) = line(src, entry.pc, entry.confidence) {
            out.push(line);
        }
    }
}
