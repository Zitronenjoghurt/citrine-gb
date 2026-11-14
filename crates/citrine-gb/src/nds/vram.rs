mod engine;
mod mapping;

use super::ram::{self, Ram};
use crate::gba::wait::Width;
pub use engine::EngineView;
use mapping::Mapping;
use std::sync::OnceLock;

const PAGE: usize = 0x2000;
static BLANK: [u8; PAGE] = [0; PAGE];
const FLAT_PAGES: usize = 0x8_0000 / PAGE;
const BLANK_PAGE: u8 = 0xFE;
const SHARED_PAGE: u8 = 0xFF;
const ARM7_C: u8 = 1 << 0;
const ARM7_D: u8 = 1 << 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bank {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
}

impl Bank {
    pub const ALL: [Self; 9] = [
        Self::A,
        Self::B,
        Self::C,
        Self::D,
        Self::E,
        Self::F,
        Self::G,
        Self::H,
        Self::I,
    ];

    pub fn size(self) -> usize {
        match self {
            Self::A | Self::B | Self::C | Self::D => 0x2_0000,
            Self::E => 0x1_0000,
            Self::F | Self::G | Self::I => 0x4000,
            Self::H => 0x8000,
        }
    }

    fn lcdc_offset(self) -> usize {
        match self {
            Self::A => 0x0_0000,
            Self::B => 0x2_0000,
            Self::C => 0x4_0000,
            Self::D => 0x6_0000,
            Self::E => 0x8_0000,
            Self::F => 0x9_0000,
            Self::G => 0x9_4000,
            Self::H => 0x9_8000,
            Self::I => 0xA_0000,
        }
    }

    fn mst_mask(self) -> u8 {
        match self {
            Self::A | Self::B | Self::H | Self::I => 0x03,
            _ => 0x07,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Purpose {
    Lcdc,
    BgA,
    ObjA,
    BgB,
    ObjB,
    Arm7,
    Texture,
    TexturePalette,
    BgExtPaletteA,
    ObjExtPaletteA,
    BgExtPaletteB,
    ObjExtPaletteB,
}

impl Purpose {
    pub const ALL: [Self; 12] = [
        Self::Lcdc,
        Self::BgA,
        Self::ObjA,
        Self::BgB,
        Self::ObjB,
        Self::Arm7,
        Self::Texture,
        Self::TexturePalette,
        Self::BgExtPaletteA,
        Self::ObjExtPaletteA,
        Self::BgExtPaletteB,
        Self::ObjExtPaletteB,
    ];

    pub fn window(self) -> usize {
        match self {
            Self::Lcdc => 0x10_0000,
            Self::BgA | Self::Texture => 0x8_0000,
            Self::ObjA | Self::Arm7 => 0x4_0000,
            Self::BgB | Self::ObjB => 0x2_0000,
            Self::TexturePalette => 0x1_8000,
            Self::BgExtPaletteA | Self::BgExtPaletteB => 0x8000,
            Self::ObjExtPaletteA | Self::ObjExtPaletteB => 0x2000,
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn holds_textures(self) -> bool {
        matches!(self, Self::Texture | Self::TexturePalette)
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vram {
    banks: Vec<Ram>,
    control: [u8; 9],
    mappings: [Option<Mapping>; 9],
    pages: Vec<Box<[u16]>>,
    #[cfg_attr(feature = "serde", serde(skip, default = "every_page"))]
    changed: [u64; 9],
    #[cfg_attr(feature = "serde", serde(skip))]
    texture_version: u64,
    #[cfg_attr(feature = "serde", serde(skip))]
    flat: OnceLock<Box<FlatCodes>>,
}

type FlatCodes = [[u8; FLAT_PAGES]; Purpose::ALL.len()];
type BankPages<'a> = [&'a [[u8; PAGE]]; 9];

impl Clone for Vram {
    fn clone(&self) -> Self {
        Self {
            banks: self.banks.clone(),
            control: self.control,
            mappings: self.mappings,
            pages: self.pages.clone(),
            changed: self.changed,
            texture_version: self.texture_version,
            flat: self.flat.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            banks,
            control,
            mappings,
            pages,
            changed,
            texture_version,
            flat,
        } = self;
        banks.clone_from(&source.banks);
        *control = source.control;
        *mappings = source.mappings;
        pages.clone_from(&source.pages);
        *changed = source.changed;
        *texture_version = source.texture_version;
        flat.clone_from(&source.flat);
    }
}

fn every_page() -> [u64; 9] {
    [u64::MAX; 9]
}

impl Default for Vram {
    fn default() -> Self {
        Self {
            banks: Bank::ALL.iter().map(|bank| Ram::new(bank.size())).collect(),
            control: [0; 9],
            mappings: [None; 9],
            pages: Purpose::ALL
                .iter()
                .map(|purpose| vec![0; purpose.window() / PAGE].into_boxed_slice())
                .collect(),
            changed: every_page(),
            texture_version: 0,
            flat: OnceLock::new(),
        }
    }
}

impl Vram {
    pub fn control(&self, bank: Bank) -> u8 {
        self.control[bank.index()]
    }

    pub fn write_control(&mut self, bank: Bank, value: u8) {
        let mapping = Mapping::decode(bank, value);
        let before = self.mappings[bank.index()];
        if mapping != before
            && [before, mapping]
                .into_iter()
                .flatten()
                .any(|mapping| mapping.purpose.holds_textures())
        {
            self.texture_version += 1;
        }
        self.control[bank.index()] = value;
        self.mappings[bank.index()] = mapping;
        self.rebuild_pages();
    }

    pub fn texture_version(&self) -> u64 {
        self.texture_version
    }

    pub fn arm7_status(&self) -> u8 {
        let arm7 = |bank: Bank| {
            matches!(
                self.mappings[bank.index()],
                Some(Mapping {
                    purpose: Purpose::Arm7,
                    ..
                })
            )
        };
        let mut status = 0;
        if arm7(Bank::C) {
            status |= ARM7_C;
        }
        if arm7(Bank::D) {
            status |= ARM7_D;
        }
        status
    }

    pub fn banks_at(&self, purpose: Purpose, offset: usize) -> impl Iterator<Item = Bank> + '_ {
        let mask = self.mask(purpose, offset);
        Bank::ALL
            .into_iter()
            .filter(move |bank| mask & 1 << bank.index() != 0)
    }

    pub fn read(&self, purpose: Purpose, offset: usize, width: Width) -> u32 {
        let window = purpose.window();
        let offset = if offset < window {
            offset
        } else {
            offset % window
        };
        let mask = self.pages[purpose.index()][offset / PAGE];
        if mask & mask.wrapping_sub(1) == 0 {
            if mask == 0 {
                return 0;
            }
            let index = mask.trailing_zeros() as usize;
            let start = self.mappings[index].map_or(0, |mapping| mapping.offset);
            return self.banks[index].read(offset - start, width);
        }
        self.banks_at(purpose, offset).fold(0, |value, bank| {
            value | self.banks[bank.index()].read(self.bank_offset(bank, offset), width)
        })
    }

    pub fn write(&mut self, purpose: Purpose, offset: usize, width: Width, value: u32) {
        let offset = offset % purpose.window();
        let mask = self.mask(purpose, offset);
        if purpose.holds_textures() && mask != 0 {
            self.texture_version += 1;
        }
        for bank in Bank::ALL {
            if mask & 1 << bank.index() != 0 {
                let at = self.bank_offset(bank, offset);
                self.banks[bank.index()].write(at, width, value);
                self.changed[bank.index()] |= 1 << (at / ram::PAGE);
            }
        }
    }

    pub fn copy_is_stale(&self, copy: &Vram) -> bool {
        self.control != copy.control
            || Bank::ALL
                .into_iter()
                .any(|bank| self.changed[bank.index()] != 0 && self.drawn(bank))
    }

    pub fn refresh(&mut self, copy: &mut Vram) {
        for bank in Bank::ALL {
            let index = bank.index();
            if copy.control[index] != self.control[index] {
                copy.write_control(bank, self.control[index]);
            }
            if self.changed[index] == 0 || !self.drawn(bank) {
                continue;
            }
            for page in 0..bank.size() / ram::PAGE {
                if self.changed[index] & 1 << page != 0 {
                    copy.banks[index].copy_page(&self.banks[index], page);
                }
            }
            self.changed[index] = 0;
        }
    }

    fn drawn(&self, bank: Bank) -> bool {
        self.mappings[bank.index()].is_some_and(|mapping| {
            matches!(
                mapping.purpose,
                Purpose::BgA
                    | Purpose::ObjA
                    | Purpose::BgB
                    | Purpose::ObjB
                    | Purpose::BgExtPaletteA
                    | Purpose::ObjExtPaletteA
                    | Purpose::BgExtPaletteB
                    | Purpose::ObjExtPaletteB
            )
        })
    }

    pub fn mapping(&self, bank: Bank) -> Option<(Purpose, usize)> {
        self.mappings[bank.index()].map(|mapping| (mapping.purpose, mapping.offset))
    }

    pub fn rams_mut(&mut self) -> impl Iterator<Item = &mut Ram> {
        self.banks.iter_mut()
    }

    pub fn bank(&self, bank: Bank) -> &[u8] {
        self.banks[bank.index()].bytes()
    }

    pub fn copy_window(&self, purpose: Purpose, out: &mut [u8]) {
        for (page, chunk) in out.chunks_mut(PAGE).enumerate() {
            let start = page * PAGE;
            let mask = self.mask(purpose, start);
            chunk.fill(0);
            for bank in Bank::ALL {
                if mask & 1 << bank.index() == 0 {
                    continue;
                }
                let at = self.bank_offset(bank, start);
                let bytes = &self.banks[bank.index()].bytes()[at..at + chunk.len()];
                for (target, byte) in chunk.iter_mut().zip(bytes) {
                    *target |= byte;
                }
            }
        }
    }

    fn bank_pages(&self) -> BankPages<'_> {
        std::array::from_fn(|bank| self.banks[bank].bytes().as_chunks::<PAGE>().0)
    }

    fn flat_pages<'a, const N: usize>(
        &self,
        purpose: Purpose,
        chunks: &BankPages<'a>,
    ) -> [Option<&'a [u8; PAGE]>; N] {
        let codes = &self.flat.get_or_init(|| self.flat_codes())[purpose.index()];
        let mut pages = [None; N];
        let used = (purpose.window() / PAGE).min(N);
        for (page, &code) in pages[..used].iter_mut().zip(&codes[..used]) {
            *page = match code {
                BLANK_PAGE => Some(&BLANK),
                SHARED_PAGE => None,
                code => chunks[usize::from(code >> 4)].get(usize::from(code & 0xF)),
            };
        }
        pages
    }

    fn flat_codes(&self) -> Box<FlatCodes> {
        let mut codes = Box::new([[SHARED_PAGE; FLAT_PAGES]; Purpose::ALL.len()]);
        for (codes, masks) in codes.iter_mut().zip(&self.pages) {
            for (code, &mask) in codes.iter_mut().zip(masks.iter()) {
                if mask == 0 {
                    *code = BLANK_PAGE;
                }
            }
        }
        for bank in Bank::ALL {
            let Some(mapping) = self.mappings[bank.index()] else {
                continue;
            };
            let only = 1 << bank.index();
            let chunks = bank.size() / PAGE;
            let masks = &self.pages[mapping.purpose.index()];
            let codes = &mut codes[mapping.purpose.index()];
            let first = mapping.offset / PAGE;
            for (chunk, page) in (first..first + mapping.len / PAGE).enumerate() {
                let chunk = chunk % chunks;
                if page < FLAT_PAGES && masks.get(page) == Some(&only) {
                    codes[page] = (bank.index() << 4 | chunk) as u8;
                }
            }
        }
        codes
    }

    fn mask(&self, purpose: Purpose, offset: usize) -> u16 {
        self.pages[purpose.index()][(offset % purpose.window()) / PAGE]
    }

    fn bank_offset(&self, bank: Bank, offset: usize) -> usize {
        let start = self.mappings[bank.index()].map_or(0, |mapping| mapping.offset);
        offset - start
    }

    fn rebuild_pages(&mut self) {
        self.flat = OnceLock::new();
        for pages in &mut self.pages {
            pages.fill(0);
        }
        for bank in Bank::ALL {
            let Some(mapping) = self.mappings[bank.index()] else {
                continue;
            };
            let pages = &mut self.pages[mapping.purpose.index()];
            let first = mapping.offset / PAGE;
            let last = (first + mapping.len / PAGE).min(pages.len());
            for page in &mut pages[first..last] {
                *page |= 1 << bank.index();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: u8 = 0x80;

    fn mapped(controls: &[(Bank, u8)]) -> Vram {
        let mut vram = Vram::default();
        for &(bank, control) in controls {
            vram.write_control(bank, ON | control);
        }
        vram
    }

    #[test]
    fn every_bank_starts_unmapped_and_reads_zero() {
        let vram = Vram::default();
        for purpose in Purpose::ALL {
            assert_eq!(vram.banks_at(purpose, 0).count(), 0);
            assert_eq!(vram.read(purpose, 0, Width::Word), 0);
        }
    }

    #[test]
    fn a_bank_shows_its_data_wherever_it_is_mapped() {
        let mut vram = mapped(&[(Bank::A, 0)]);
        vram.write(Purpose::Lcdc, 0x1_0000, Width::Word, 0x1234_5678);
        vram.write_control(Bank::A, ON | 1 << 3 | 1);
        assert_eq!(vram.read(Purpose::BgA, 0x3_0000, Width::Word), 0x1234_5678);
        assert_eq!(vram.read(Purpose::Lcdc, 0x1_0000, Width::Word), 0);
    }

    #[test]
    fn lcdc_lays_the_banks_end_to_end() {
        let all: Vec<(Bank, u8)> = Bank::ALL.iter().map(|&bank| (bank, 0)).collect();
        let mut vram = mapped(&all);
        let starts = [
            0x0_0000, 0x2_0000, 0x4_0000, 0x6_0000, 0x8_0000, 0x9_0000, 0x9_4000, 0x9_8000,
            0xA_0000,
        ];
        for (index, start) in starts.into_iter().enumerate() {
            vram.write(Purpose::Lcdc, start, Width::Half, index as u32 + 1);
        }
        for (bank, index) in Bank::ALL.into_iter().zip(1u8..) {
            assert_eq!(vram.bank(bank)[0], index);
        }
        assert_eq!(vram.banks_at(Purpose::Lcdc, 0xA_4000).count(), 0);
    }

    #[test]
    fn engine_a_bg_mirrors_every_512k() {
        let mut vram = mapped(&[(Bank::B, 1)]);
        vram.write(Purpose::BgA, 0x8_0010, Width::Half, 0xABCD);
        assert_eq!(vram.read(Purpose::BgA, 0x10, Width::Half), 0xABCD);
    }

    #[test]
    fn overlapping_banks_read_as_the_or_of_both() {
        let mut vram = mapped(&[(Bank::E, 0), (Bank::F, 0)]);
        vram.write(Purpose::Lcdc, 0x8_0000, Width::Word, 0x0000_00F0);
        vram.write(Purpose::Lcdc, 0x9_0000, Width::Word, 0x0000_000F);
        vram.write_control(Bank::E, ON | 1);
        vram.write_control(Bank::F, ON | 1);
        assert_eq!(vram.read(Purpose::BgA, 0, Width::Word), 0x0000_00FF);
    }

    #[test]
    fn a_write_to_overlapping_banks_lands_in_both() {
        let mut vram = mapped(&[(Bank::F, 1), (Bank::G, 1)]);
        vram.write(Purpose::BgA, 0x20, Width::Word, 0xDEAD_BEEF);
        assert_eq!(vram.bank(Bank::F)[0x20..0x24], [0xEF, 0xBE, 0xAD, 0xDE]);
        assert_eq!(vram.bank(Bank::G)[0x20..0x24], [0xEF, 0xBE, 0xAD, 0xDE]);
    }

    #[test]
    fn a_small_bank_leaves_the_rest_of_its_window_unmapped() {
        let vram = mapped(&[(Bank::I, 1)]);
        assert_eq!(vram.banks_at(Purpose::BgB, 0x7FFF).count(), 0);
        assert_eq!(
            vram.banks_at(Purpose::BgB, 0x8000).collect::<Vec<_>>(),
            [Bank::I]
        );
        assert_eq!(vram.banks_at(Purpose::BgB, 0xC000).count(), 0);
    }

    #[test]
    fn ext_palette_modes_map_only_the_lower_part_of_a_bank() {
        let vram = mapped(&[(Bank::E, 4), (Bank::I, 3)]);
        assert_eq!(
            vram.banks_at(Purpose::BgExtPaletteA, 0x7FFF)
                .collect::<Vec<_>>(),
            [Bank::E]
        );
        assert_eq!(
            vram.banks_at(Purpose::ObjExtPaletteB, 0x1FFF)
                .collect::<Vec<_>>(),
            [Bank::I]
        );
    }

    #[test]
    fn disabling_a_bank_unmaps_it() {
        let mut vram = mapped(&[(Bank::C, 4)]);
        vram.write_control(Bank::C, 4);
        assert_eq!(vram.banks_at(Purpose::BgB, 0).count(), 0);
    }

    #[test]
    fn arm7_status_shows_banks_c_and_d_in_mst_2_only() {
        assert_eq!(Vram::default().arm7_status(), 0);
        assert_eq!(mapped(&[(Bank::C, 2)]).arm7_status(), 0b01);
        assert_eq!(mapped(&[(Bank::D, 2 | 1 << 3)]).arm7_status(), 0b10);
        assert_eq!(mapped(&[(Bank::C, 2), (Bank::D, 2)]).arm7_status(), 0b11);
        assert_eq!(mapped(&[(Bank::C, 1), (Bank::D, 4)]).arm7_status(), 0);
    }

    #[test]
    fn control_reads_back_what_was_written() {
        let mut vram = Vram::default();
        vram.write_control(Bank::H, 0x81);
        assert_eq!(vram.control(Bank::H), 0x81);
    }

    #[test]
    fn a_refreshed_copy_reads_like_the_live_vram_until_a_drawn_bank_changes() {
        let mut live = mapped(&[(Bank::A, 1), (Bank::D, 0)]);
        live.write(Purpose::BgA, 0x1234, Width::Word, 0xDEAD_BEEF);
        let mut copy = Vram::default();
        assert!(live.copy_is_stale(&copy));
        live.refresh(&mut copy);
        assert!(!live.copy_is_stale(&copy));
        assert_eq!(copy.read(Purpose::BgA, 0x1234, Width::Word), 0xDEAD_BEEF);
        live.write(Purpose::Lcdc, 0x6_0000, Width::Word, 1);
        assert!(!live.copy_is_stale(&copy));
        live.write(Purpose::BgA, 0x8000, Width::Half, 0x5555);
        assert!(live.copy_is_stale(&copy));
        live.refresh(&mut copy);
        assert_eq!(copy.read(Purpose::BgA, 0x8000, Width::Half), 0x5555);
    }

    #[test]
    fn a_bank_mapped_for_drawing_later_brings_its_earlier_writes_to_the_copy() {
        let mut live = mapped(&[(Bank::E, 0)]);
        let mut copy = Vram::default();
        live.refresh(&mut copy);
        live.write(Purpose::Lcdc, 0x8_0010, Width::Word, 0x1234_5678);
        assert!(!live.copy_is_stale(&copy));
        live.write_control(Bank::E, ON | 1);
        assert!(live.copy_is_stale(&copy));
        live.refresh(&mut copy);
        assert_eq!(copy.read(Purpose::BgA, 0x10, Width::Word), 0x1234_5678);
    }
}
