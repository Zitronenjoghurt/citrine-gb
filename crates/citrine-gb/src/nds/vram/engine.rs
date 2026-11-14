use super::{BankPages, PAGE, Purpose, Vram};
use crate::gba::wait::Width;
use crate::nds::video::{EngineKind, EngineVram, LcdcRead, LcdcWrite, PALETTE_BANK};

const BG_PAGES: usize = 0x8_0000 / PAGE;
const OBJ_PAGES: usize = 0x4_0000 / PAGE;
const BG_PALETTE_PAGES: usize = 0x8000 / PAGE;
const OBJ_PALETTE_PAGES: usize = 0x2000 / PAGE;

#[derive(Clone, Copy)]
struct Pages<'a, const N: usize> {
    vram: &'a Vram,
    purpose: Purpose,
    mask: usize,
    pages: [Option<&'a [u8; PAGE]>; N],
}

impl<'a, const N: usize> Pages<'a, N> {
    fn new(vram: &'a Vram, purpose: Purpose, banks: Option<&BankPages<'a>>) -> Self {
        let window = purpose.window();
        debug_assert!(window <= N * PAGE && window.is_power_of_two());
        Self {
            vram,
            purpose,
            mask: window - 1,
            pages: match banks {
                Some(banks) => vram.flat_pages(purpose, banks),
                None => [None; N],
            },
        }
    }

    #[inline(always)]
    fn at(&self, address: u32) -> Option<(&'a [u8; PAGE], usize)> {
        let offset = address as usize & self.mask;
        self.pages[offset / PAGE].map(|page| (page, offset % PAGE))
    }

    #[inline(always)]
    fn slice(&self, address: u32, len: usize) -> Option<&'a [u8]> {
        let (page, start) = self.at(address)?;
        page.get(start..start + len)
    }

    #[inline(always)]
    fn byte(&self, address: u32) -> u8 {
        match self.at(address) {
            Some((page, at)) => page[at],
            None => self.unpaged(address, Width::Byte) as u8,
        }
    }

    #[inline(always)]
    fn half(&self, address: u32) -> u16 {
        match self.at(address) {
            Some((page, at)) if address & 1 == 0 => u16::from_le_bytes([page[at], page[at + 1]]),
            _ => self.half_elsewhere(address),
        }
    }

    #[inline(always)]
    fn word(&self, address: u32) -> u32 {
        match self.at(address) {
            Some((page, at)) if address & 3 == 0 => {
                u32::from_le_bytes([page[at], page[at + 1], page[at + 2], page[at + 3]])
            }
            _ => self.word_elsewhere(address),
        }
    }

    #[cold]
    #[inline(never)]
    fn unpaged(&self, address: u32, width: Width) -> u32 {
        self.vram
            .read(self.purpose, address as usize & self.mask, width)
    }

    #[cold]
    #[inline(never)]
    fn half_elsewhere(&self, address: u32) -> u16 {
        if address & 1 != 0 {
            return u16::from_le_bytes([self.byte(address), self.byte(address.wrapping_add(1))]);
        }
        self.unpaged(address, Width::Half) as u16
    }

    #[cold]
    #[inline(never)]
    fn word_elsewhere(&self, address: u32) -> u32 {
        if address & 3 != 0 {
            return u32::from_le_bytes(std::array::from_fn(|at| {
                self.byte(address.wrapping_add(at as u32))
            }));
        }
        self.unpaged(address, Width::Word)
    }
}

#[derive(Clone, Copy)]
pub struct EngineView<'a> {
    bg: Pages<'a, BG_PAGES>,
    obj: Pages<'a, OBJ_PAGES>,
    bg_palette: Pages<'a, BG_PALETTE_PAGES>,
    obj_palette: Pages<'a, OBJ_PALETTE_PAGES>,
}

impl<'a> EngineView<'a> {
    pub fn new(vram: &'a Vram, kind: EngineKind) -> Self {
        Self::with_pages(vram, kind, Some(&vram.bank_pages()))
    }

    pub fn unpaged(vram: &'a Vram, kind: EngineKind) -> Self {
        Self::with_pages(vram, kind, None)
    }

    fn with_pages(vram: &'a Vram, kind: EngineKind, banks: Option<&BankPages<'a>>) -> Self {
        let (bg, obj, bg_palette, obj_palette) = match kind {
            EngineKind::A => (
                Purpose::BgA,
                Purpose::ObjA,
                Purpose::BgExtPaletteA,
                Purpose::ObjExtPaletteA,
            ),
            EngineKind::B => (
                Purpose::BgB,
                Purpose::ObjB,
                Purpose::BgExtPaletteB,
                Purpose::ObjExtPaletteB,
            ),
        };
        Self {
            bg: Pages::new(vram, bg, banks),
            obj: Pages::new(vram, obj, banks),
            bg_palette: Pages::new(vram, bg_palette, banks),
            obj_palette: Pages::new(vram, obj_palette, banks),
        }
    }
}

impl EngineVram for EngineView<'_> {
    #[inline(always)]
    fn bg(&self, address: u32) -> u8 {
        self.bg.byte(address)
    }

    #[inline(always)]
    fn obj(&self, address: u32) -> u8 {
        self.obj.byte(address)
    }

    #[inline(always)]
    fn bg_palette(&self, address: u32) -> u8 {
        self.bg_palette.byte(address)
    }

    #[inline(always)]
    fn obj_palette(&self, address: u32) -> u8 {
        self.obj_palette.byte(address)
    }

    #[inline(always)]
    fn bg_u16(&self, address: u32) -> u16 {
        self.bg.half(address)
    }

    #[inline(always)]
    fn obj_u16(&self, address: u32) -> u16 {
        self.obj.half(address)
    }

    #[inline(always)]
    fn bg_palette_u16(&self, address: u32) -> u16 {
        self.bg_palette.half(address)
    }

    #[inline(always)]
    fn obj_palette_u16(&self, address: u32) -> u16 {
        self.obj_palette.half(address)
    }

    #[inline(always)]
    fn bg_palette_bank(&self, address: u32) -> Option<&[u8; PALETTE_BANK]> {
        self.bg_palette
            .slice(address, PALETTE_BANK)?
            .try_into()
            .ok()
    }

    #[inline(always)]
    fn bg_tile(&self, address: u32) -> Option<&[u8; 64]> {
        self.bg.slice(address, 64)?.try_into().ok()
    }

    #[inline(always)]
    fn bg_u32(&self, address: u32) -> u32 {
        self.bg.word(address)
    }

    #[inline(always)]
    fn obj_u32(&self, address: u32) -> u32 {
        self.obj.word(address)
    }
}

impl LcdcRead for Vram {
    fn lcdc_u16(&self, address: u32) -> u16 {
        self.read(Purpose::Lcdc, address as usize, Width::Half) as u16
    }
}

impl LcdcWrite for Vram {
    fn write_lcdc_u16(&mut self, address: u32, value: u16) {
        self.write(
            Purpose::Lcdc,
            address as usize,
            Width::Half,
            u32::from(value),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::vram::Bank;

    const ON: u8 = 0x80;
    const LCDC: u8 = 0;

    fn filled(bank: Bank, at: u32, value: u16, control: u8) -> Vram {
        let mut vram = Vram::default();
        vram.write_control(bank, ON | LCDC);
        vram.write_lcdc_u16(bank.lcdc_offset() as u32 + at, value);
        vram.write_control(bank, ON | control);
        vram
    }

    #[test]
    fn engine_a_reads_bank_a_as_background() {
        let vram = filled(Bank::A, 0x10, 0x1234, 1);
        let a = EngineView::new(&vram, EngineKind::A);
        assert_eq!(a.bg_u16(0x10), 0x1234);
        assert_eq!(a.bg(0x11), 0x12);
        assert_eq!(a.bg_u16(0x11), 0x0012);
        assert_eq!(EngineView::new(&vram, EngineKind::B).bg_u16(0x10), 0);
    }

    #[test]
    fn overlapping_banks_read_as_one_and_unmapped_pages_read_zero() {
        let mut vram = filled(Bank::A, 0x10, 0x00F0, 1);
        vram.write_control(Bank::B, ON | LCDC);
        vram.write_lcdc_u16(Bank::B.lcdc_offset() as u32 + 0x10, 0x0F00);
        vram.write_control(Bank::B, ON | 1);
        let a = EngineView::new(&vram, EngineKind::A);
        assert_eq!(a.bg_u16(0x10), 0x0FF0);
        assert_eq!(a.bg(0x11), 0x0F);
        assert_eq!(a.bg_u32(0x10), 0x0FF0);
        assert_eq!(a.bg_u32(0x4_0010), 0);
        assert_eq!(a.bg_u16(0x8_0010), 0x0FF0);
    }

    #[test]
    fn a_tile_comes_whole_from_its_page_unless_banks_overlap_there() {
        let mut vram = filled(Bank::A, 0x42, 0x1234, 1);
        let a = EngineView::new(&vram, EngineKind::A);
        let tile = a.bg_tile(0x40).expect("bank a alone holds the tile");
        assert_eq!(tile[2..4], [0x34, 0x12]);
        assert_eq!(a.bg_tile(0x2_0040), Some(&[0; 64]));
        vram.write_control(Bank::B, ON | 1);
        assert_eq!(EngineView::new(&vram, EngineKind::A).bg_tile(0x40), None);
    }

    #[test]
    fn engine_b_reads_bank_c_as_background_and_d_as_objects() {
        let vram = filled(Bank::C, 0x20, 0xBEEF, 4);
        let b = EngineView::new(&vram, EngineKind::B);
        assert_eq!(b.bg_u16(0x20), 0xBEEF);
        assert_eq!(b.obj_u16(0x20), 0);
        let vram = filled(Bank::D, 0x20, 0xCAFE, 4);
        assert_eq!(EngineView::new(&vram, EngineKind::B).obj_u16(0x20), 0xCAFE);
    }

    #[test]
    fn engine_a_reads_bank_b_as_objects() {
        let vram = filled(Bank::B, 0x40, 0x5A5A, 2);
        let a = EngineView::new(&vram, EngineKind::A);
        assert_eq!(a.obj_u16(0x40), 0x5A5A);
        assert_eq!(a.bg_u16(0x40), 0);
    }

    #[test]
    fn extended_palettes_come_from_their_own_banks() {
        let cases = [
            (Bank::E, 4, EngineKind::A, true),
            (Bank::H, 2, EngineKind::B, true),
            (Bank::F, 5, EngineKind::A, false),
            (Bank::I, 3, EngineKind::B, false),
        ];
        for (bank, mst, kind, bg) in cases {
            let vram = filled(bank, 0x202, 0x7C1F, mst);
            let view = EngineView::new(&vram, kind);
            let read = |at| {
                if bg {
                    view.bg_palette(at)
                } else {
                    view.obj_palette(at)
                }
            };
            assert_eq!(
                u16::from_le_bytes([read(0x202), read(0x203)]),
                0x7C1F,
                "{bank:?}"
            );
        }
    }

    #[test]
    fn every_engine_read_matches_a_plain_vram_read_for_random_bank_setups() {
        let mut seed = 0x6C07_8965u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let purposes = [
            (
                EngineKind::A,
                [
                    Purpose::BgA,
                    Purpose::ObjA,
                    Purpose::BgExtPaletteA,
                    Purpose::ObjExtPaletteA,
                ],
            ),
            (
                EngineKind::B,
                [
                    Purpose::BgB,
                    Purpose::ObjB,
                    Purpose::BgExtPaletteB,
                    Purpose::ObjExtPaletteB,
                ],
            ),
        ];
        for _ in 0..16 {
            let mut vram = Vram::default();
            for bank in Bank::ALL {
                for at in (0..bank.size()).step_by(4) {
                    vram.banks[bank.index()].write(at, Width::Word, next());
                }
            }
            for _ in 0..2 {
                for bank in Bank::ALL {
                    vram.write_control(bank, next() as u8);
                }
                let views = purposes.into_iter().flat_map(|(kind, purposes)| {
                    [
                        EngineView::new(&vram, kind),
                        EngineView::unpaged(&vram, kind),
                    ]
                    .map(|view| (kind, purposes, view))
                });
                for (kind, [bg, obj, bg_palette, obj_palette], view) in views {
                    let plain = |purpose: Purpose, at: u32| {
                        vram.read(purpose, at as usize & (purpose.window() - 1), Width::Byte) as u8
                    };
                    for _ in 0..2000 {
                        let at = next() & 0xF_FFFF;
                        assert_eq!(view.bg(at), plain(bg, at), "{kind:?} bg {at:x}");
                        assert_eq!(view.obj(at), plain(obj, at), "{kind:?} obj {at:x}");
                        assert_eq!(
                            view.bg_palette(at),
                            plain(bg_palette, at),
                            "{kind:?} {at:x}"
                        );
                        assert_eq!(
                            view.obj_palette(at),
                            plain(obj_palette, at),
                            "{kind:?} {at:x}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn lcdc_reads_and_writes_reach_only_a_bank_mapped_there() {
        let mut vram = Vram::default();
        vram.write_lcdc_u16(0x2_0000, 0x1111);
        assert_eq!(vram.lcdc_u16(0x2_0000), 0);
        vram.write_control(Bank::B, ON | LCDC);
        assert_eq!(vram.lcdc_u16(0x2_0000), 0);
        vram.write_lcdc_u16(0x2_0000, 0x2222);
        assert_eq!(vram.lcdc_u16(0x2_0000), 0x2222);
        assert_eq!(vram.lcdc_u16(0), 0);
    }
}
