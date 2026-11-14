use super::cpu_access::IWRAM_PAGE;
use super::{Bus, read_le};
use crate::gba::cart::Cart;
use crate::gba::region::{EWRAM_SIZE, IWRAM_SIZE, Region};
use crate::gba::wait::Width;
use crate::jit::{CodeMarks, CodeRam, CodeSpan, Fast, FastBus, JitBus, Kept, Spec, WINDOWS};

const IO: u32 = 0x04;
const GAME_PAK: u32 = 0x08;
const EEPROM: u32 = 0x0D;

#[derive(Debug, Clone, Default)]
pub(super) struct Code {
    ewram: CodeMarks,
    iwram: CodeMarks,
    rom: u32,
    fast: Kept,
}

impl Code {
    #[inline(always)]
    pub(super) fn written(&mut self, region: Region, offset: usize) {
        match region {
            Region::Ewram => self.ewram.written(offset),
            Region::Iwram => self.iwram.written(offset),
            _ => {}
        }
    }

    pub(super) fn rom_changed(&mut self) {
        self.rom = self.rom.wrapping_add(1);
    }
}

impl Bus {
    pub(crate) fn jit_budget(&self) -> u32 {
        let line = self.interrupts.until_line().unwrap_or(u32::MAX);
        let event = if self.quiet > self.owed {
            self.quiet
        } else {
            self.until_event()
        };
        event
            .min(line)
            .min(self.dma_check)
            .saturating_sub(self.owed)
    }

    fn rom_span(&self, addr: u32) -> Option<usize> {
        if addr >> 24 == EEPROM {
            return None;
        }
        let offset = Region::of(addr).offset(addr);
        let word = offset & !3;
        let inside = word + 4 <= self.rom.len();
        (inside && !Cart::in_gpio(word) && !Cart::in_gpio(word + 3)).then_some(offset)
    }
}

impl JitBus for Bus {
    const TIMED_FETCH: bool = true;
    const WRITABLE: &'static [CodeRam] = &[CodeRam::Main, CodeRam::Wram, CodeRam::Rom];

    fn flat_code(pc: u32) -> bool {
        pc >> 24 == IWRAM_PAGE
    }

    fn leaves(addr: u32, store: bool) -> bool {
        let area = addr >> 24;
        area == IO || store && area >= GAME_PAK
    }

    fn code_layout(&self) -> u64 {
        0
    }

    fn code_span(&self, addr: u32) -> Option<CodeSpan> {
        let region = Region::of(addr);
        let (ram, offset) = match region {
            Region::Bios => (CodeRam::Bios, region.offset(addr)),
            Region::Ewram => (CodeRam::Main, region.offset(addr)),
            Region::Iwram => (CodeRam::Wram, region.offset(addr)),
            Region::Rom(_) => (CodeRam::Rom, self.rom_span(addr)?),
            _ => return None,
        };
        Some(CodeSpan {
            ram,
            offset: offset as u32,
        })
    }

    fn code_word(&self, span: CodeSpan) -> u32 {
        let offset = span.offset as usize & !3;
        match span.ram {
            CodeRam::Bios => read_le(&self.bios, offset, Width::Word),
            CodeRam::Main => read_le(&self.ewram, offset, Width::Word),
            CodeRam::Wram => read_le(&self.iwram, offset, Width::Word),
            CodeRam::Rom => read_le(&self.rom, offset, Width::Word),
            CodeRam::Itcm | CodeRam::SharedWram => 0,
        }
    }

    fn mark_code(&mut self, ram: CodeRam, page: usize) -> u32 {
        match ram {
            CodeRam::Main => self.code.ewram.mark(page, EWRAM_SIZE),
            CodeRam::Wram => self.code.iwram.mark(page, IWRAM_SIZE),
            CodeRam::Rom => self.code.rom,
            CodeRam::Bios | CodeRam::Itcm | CodeRam::SharedWram => 0,
        }
    }

    fn code_generation(&self, ram: CodeRam, page: usize) -> Option<u32> {
        match ram {
            CodeRam::Main => self.code.ewram.generation(page),
            CodeRam::Wram => self.code.iwram.generation(page),
            CodeRam::Rom => Some(self.code.rom),
            CodeRam::Bios => Some(0),
            CodeRam::Itcm | CodeRam::SharedWram => None,
        }
    }

    fn take_code_hit(&mut self) -> bool {
        let ewram = self.code.ewram.take_hit();
        let iwram = self.code.iwram.take_hit();
        ewram | iwram
    }

    fn take_stall(&mut self) -> u32 {
        0
    }

    #[inline(always)]
    fn fetch_flat(&mut self, addr: u32, width: Width, count: u32, room: u32) -> bool {
        self.iwram_run(addr, width, count, room)
    }

    fn code_stamp(&self, ram: CodeRam) -> u32 {
        match ram {
            CodeRam::Main => self.code.ewram.stamp(),
            CodeRam::Wram => self.code.iwram.stamp(),
            CodeRam::Rom => self.code.rom,
            CodeRam::Bios | CodeRam::Itcm | CodeRam::SharedWram => 0,
        }
    }

    fn written_page(&self, ram: CodeRam, stamp: u32) -> Option<usize> {
        match ram {
            CodeRam::Main => self.code.ewram.written_page(stamp),
            CodeRam::Wram => self.code.iwram.written_page(stamp),
            CodeRam::Bios | CodeRam::Itcm | CodeRam::SharedWram | CodeRam::Rom => None,
        }
    }
}

impl FastBus for Bus {
    const SPECS: [Spec; WINDOWS] = [Spec {
        len: 0,
        code: false,
        cache: None,
        half: [0; 2],
        word: [0; 2],
    }; WINDOWS];

    fn fast(&mut self) -> *mut Fast {
        self.code.fast.keep(Default::default(), Fast::CLOSED)
    }
}
