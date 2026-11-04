use super::sprite::SIZES;
use super::{HBLANK_FREE, LINE_CYCLES, LINES, OBJ, Ppu, VBLANK_LINE};

const GBAHAWK_CYCLE_OFFSET: u32 = 1;
const EVAL_START: u32 = 40;
const EVAL_CALLS: u32 = LINE_CYCLES / 2;
const SPRITES: usize = 128;
const ENTRY_HALVES: usize = 4;
const ATTR_HALVES: usize = 2;
const VRAM_LIMIT: u32 = 1230;
const OAM_LIMIT: u32 = 1230;
const FREE_VRAM_LIMIT: u32 = 958;
const FREE_OAM_LIMIT: u32 = 960;
const WORDS: usize = (LINE_CYCLES as usize).div_ceil(64);
const NO_LINE: u16 = u16::MAX;

#[derive(Clone)]
pub struct ObjFetch {
    line: u16,
    busy: [u64; WORDS],
    oam_busy: [u64; WORDS],
}

impl Default for ObjFetch {
    fn default() -> Self {
        Self {
            line: NO_LINE,
            busy: [0; WORDS],
            oam_busy: [0; WORDS],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct OamWrite {
    pos: u16,
    half: u16,
    old: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OamWrites {
    line: u16,
    writes: Vec<OamWrite>,
}

impl Default for OamWrites {
    fn default() -> Self {
        Self {
            line: NO_LINE,
            writes: Vec::new(),
        }
    }
}

impl OamWrites {
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }

    fn of(&self, line: u16) -> &[OamWrite] {
        if self.line == line { &self.writes } else { &[] }
    }

    fn start(&mut self, line: u16) {
        if self.line != line {
            self.line = line;
            self.writes.clear();
        }
    }

    pub fn retire(&mut self, vcount: u16) {
        if !self.is_empty() && self.line != previous_line(vcount) {
            *self = Self::default();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    line: u16,
    pos: u32,
}

struct Found {
    x: u32,
    width: u32,
    affine: bool,
}

struct Target<'a> {
    line: u16,
    obj_on: bool,
    hblank_free: bool,
    oam: &'a [u8],
    writes: &'a [OamWrite],
}

impl ObjFetch {
    pub fn forget(&mut self) {
        self.line = NO_LINE;
    }

    fn mark(&mut self, at: u32) {
        self.busy[at as usize / 64] |= 1 << (at % 64);
    }

    fn mark_oam(&mut self, at: u32) {
        self.oam_busy[at as usize / 64] |= 1 << (at % 64);
    }

    fn oam_busy(&self, at: u32) -> bool {
        at < LINE_CYCLES && self.oam_busy[at as usize / 64] & 1 << (at % 64) != 0
    }

    fn busy(&self, at: u32) -> bool {
        at < LINE_CYCLES && self.busy[at as usize / 64] & 1 << (at % 64) != 0
    }

    fn stall(&self, at: u32, base: u32, word: bool) -> u32 {
        let mut wait = base;
        let mut in_use = word;
        if self.busy(at) {
            wait += 1;
            in_use = true;
        }
        let mut cycle = 1;
        while cycle < wait {
            if in_use && self.busy(at + cycle) {
                wait += 1;
            }
            cycle += 1;
        }
        wait
    }

    fn build(&mut self, line: u16, target: &Target) {
        self.line = line;
        self.busy = [0; WORDS];
        self.oam_busy = [0; WORDS];
        let (vram_limit, oam_limit) = if target.hblank_free {
            (FREE_VRAM_LIMIT, FREE_OAM_LIMIT)
        } else {
            (VRAM_LIMIT, OAM_LIMIT)
        };
        let mut fetch_vram = false;
        let mut count = 0;
        let mut length = 0;
        let mut affine = false;
        let mut oam0 = true;
        let mut oam2 = false;
        let mut params = false;
        let mut params_count = 0;
        let mut next_fetch = 3;
        let mut current = 0;
        let mut processed = 0;
        let mut pending = None;
        let mut fetching = None;
        let mut oam_on = true;
        for call in 0..EVAL_CALLS {
            let render = call * 2;
            if fetch_vram {
                if render <= vram_limit {
                    self.mark(render + 1);
                }
                count += if affine { 1 } else { 2 };
                if count == length {
                    if processed == SPRITES - 1 || next_fetch == 4 {
                        return;
                    } else if next_fetch == 0 {
                        oam0 = true;
                    } else {
                        oam2 = true;
                    }
                    fetch_vram = false;
                    next_fetch = 3;
                }
            }
            if oam0 {
                if render > oam_limit {
                    oam_on = false;
                }
                let found = if target.obj_on && oam_on {
                    self.mark_oam(render + 1);
                    target.in_range(current, render + 1)
                } else {
                    None
                };
                current += 1;
                match found {
                    None if current == SPRITES => {
                        oam0 = false;
                        if !fetch_vram && !params {
                            return;
                        }
                        next_fetch = 4;
                    }
                    None => {
                        if fetch_vram || params {
                            oam0 = false;
                            next_fetch = 0;
                        }
                    }
                    Some(found) => {
                        oam0 = false;
                        pending = Some((current - 1, found));
                        if fetch_vram || params {
                            next_fetch = 2;
                        } else {
                            oam2 = true;
                        }
                    }
                }
            } else if oam2 {
                oam2 = false;
                self.mark_oam(render + 1);
                let (index, found) = pending.take().unwrap_or((
                    0,
                    Found {
                        x: 0,
                        width: 0,
                        affine: false,
                    },
                ));
                processed = index;
                if found.affine {
                    params = true;
                    params_count = 0;
                    fetching = Some(found);
                } else {
                    fetch_vram = true;
                    if current < SPRITES {
                        oam0 = true;
                    }
                    (count, length, affine) = start(&found);
                }
            }
            if params {
                if (1..=4).contains(&params_count) {
                    self.mark_oam(render + 1);
                }
                if params_count == 4 && current < SPRITES {
                    oam0 = true;
                }
                params_count += 1;
                if params_count == 6 {
                    params = false;
                    fetch_vram = true;
                    if let Some(found) = fetching.take() {
                        (count, length, affine) = start(&found);
                    }
                }
            }
        }
    }
}

fn start(found: &Found) -> (u32, u32, bool) {
    let mut count = 0;
    if found.x >= 240 {
        count = 512 - found.x;
        if !found.affine {
            count &= !1;
        }
    }
    (count, found.width, found.affine)
}

fn previous_line(line: u16) -> u16 {
    (line + LINES - 1) % LINES
}

impl Target<'_> {
    fn attr(&self, sprite: usize, which: usize, pos: u32) -> u16 {
        let half = sprite * ENTRY_HALVES + which;
        let mut value = u16::from_le_bytes([self.oam[half * 2], self.oam[half * 2 + 1]]);
        for write in self.writes.iter().rev() {
            if usize::from(write.half) == half && u32::from(write.pos) >= pos {
                value = write.old;
            }
        }
        value
    }

    fn in_range(&self, sprite: usize, pos: u32) -> Option<Found> {
        let attr0 = self.attr(sprite, 0, pos);
        let attr1 = self.attr(sprite, 1, pos);
        let affine = attr0 & 1 << 8 != 0;
        let double = attr0 & 1 << 9 != 0;
        if double && !affine {
            return None;
        }
        let shape = usize::from(attr0 >> 14);
        if shape == 3 {
            return None;
        }
        let (width, height) = SIZES[shape][usize::from(attr1 >> 14)];
        let (width, height) = (width as u32, height as u32);
        let scale = if double && affine { 2 } else { 1 };
        let top = u32::from(attr0 & 0xFF);
        let bottom = top + height * scale;
        let ly = u32::from(self.line);
        let outside = if bottom > 0xFF {
            ly >= bottom & 0xFF
        } else {
            ly < top || ly >= bottom
        };
        let x = u32::from(attr1 & 0x1FF);
        if outside || (x >= 240 && x + width * scale <= 512) {
            return None;
        }
        Some(Found {
            x,
            width: width * scale,
            affine,
        })
    }
}

impl Ppu {
    fn obj_window(&self, ahead: u32) -> Option<Window> {
        let dot = self.dot + ahead + GBAHAWK_CYCLE_OFFSET;
        let lines = dot / LINE_CYCLES % u32::from(LINES);
        let line = (u32::from(self.vcount) + lines) as u16 % LINES;
        let dot = dot % LINE_CYCLES;
        let line = if dot >= EVAL_START {
            line
        } else {
            previous_line(line)
        };
        if line >= VBLANK_LINE && line != LINES - 1 {
            return None;
        }
        Some(Window {
            line,
            pos: (dot + LINE_CYCLES - EVAL_START) % LINE_CYCLES,
        })
    }

    pub fn obj_vram_stall(&mut self, oam: &[u8], ahead: u32, base: u32, word: bool) -> u32 {
        match self.obj_fetch_at(oam, ahead) {
            Some(pos) => self.obj_fetch.stall(pos, base, word),
            None => base,
        }
    }

    pub fn oam_stall(&mut self, oam: &[u8], ahead: u32, base: u32) -> u32 {
        match self.obj_fetch_at(oam, ahead) {
            Some(pos) => base + u32::from(self.obj_fetch.oam_busy(pos)),
            None => base,
        }
    }

    fn obj_fetch_at(&mut self, oam: &[u8], ahead: u32) -> Option<u32> {
        let window = self.obj_window(ahead)?;
        if self.obj_fetch.line != window.line {
            let target = Target {
                line: (window.line + 1) % LINES,
                obj_on: self.dispcnt & OBJ != 0,
                hblank_free: self.dispcnt & HBLANK_FREE != 0,
                oam,
                writes: self.oam_writes.of(window.line),
            };
            self.obj_fetch.build(window.line, &target);
        }
        Some(window.pos)
    }

    pub fn oam_written(&mut self, oam: &[u8], ahead: u32, offset: usize, bytes: usize) {
        self.obj_fetch.forget();
        let Some(window) = self.obj_window(ahead) else {
            return;
        };
        for half in offset / 2..(offset + bytes) / 2 {
            if half % ENTRY_HALVES < ATTR_HALVES {
                self.oam_writes.start(window.line);
                self.oam_writes.writes.push(OamWrite {
                    pos: window.pos as u16,
                    half: half as u16,
                    old: u16::from_le_bytes([oam[half * 2], oam[half * 2 + 1]]),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OAM_BYTES: usize = SPRITES * ENTRY_HALVES * 2;
    const HIDDEN: u16 = 1 << 9;

    fn hidden_oam() -> Vec<u8> {
        let mut oam = vec![0; OAM_BYTES];
        for sprite in 0..SPRITES {
            set_attr(&mut oam, sprite, 0, HIDDEN);
        }
        oam
    }

    fn set_attr(oam: &mut [u8], sprite: usize, which: usize, value: u16) {
        let at = (sprite * ENTRY_HALVES + which) * 2;
        oam[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn schedule(oam: &[u8], line: u16, hblank_free: bool, writes: &[OamWrite]) -> ObjFetch {
        let mut fetch = ObjFetch::default();
        let target = Target {
            line,
            obj_on: true,
            hblank_free,
            oam,
            writes,
        };
        fetch.build(line - 1, &target);
        fetch
    }

    fn busy_positions(fetch: &ObjFetch) -> Vec<u32> {
        (0..LINE_CYCLES).filter(|&at| fetch.busy(at)).collect()
    }

    #[test]
    fn hidden_sprites_read_no_vram() {
        let fetch = schedule(&hidden_oam(), 20, false, &[]);
        assert!(busy_positions(&fetch).is_empty());
    }

    #[test]
    fn plain_sprite_reads_two_pixels_per_call_after_its_attributes() {
        let mut oam = hidden_oam();
        set_attr(&mut oam, 0, 0, 16);
        set_attr(&mut oam, 0, 1, 8);
        let fetch = schedule(&oam, 20, false, &[]);
        assert_eq!(busy_positions(&fetch), [5, 7, 9, 11]);
    }

    #[test]
    fn affine_sprite_reads_parameters_before_one_pixel_per_call() {
        let mut oam = hidden_oam();
        set_attr(&mut oam, 0, 0, 16 | 1 << 8);
        set_attr(&mut oam, 0, 1, 8);
        let fetch = schedule(&oam, 20, false, &[]);
        let busy = busy_positions(&fetch);
        assert_eq!(busy.len(), 8);
        assert_eq!(busy[0], 15);
        assert!(busy.windows(2).all(|pair| pair[1] - pair[0] == 2));
    }

    #[test]
    fn hblank_free_stops_vram_reads_at_its_limit() {
        let mut oam = hidden_oam();
        for sprite in 0..SPRITES {
            set_attr(&mut oam, sprite, 0, 1 << 8 | 1 << 9);
            set_attr(&mut oam, sprite, 1, 3 << 14);
        }
        let free = busy_positions(&schedule(&oam, 20, true, &[]));
        let full = busy_positions(&schedule(&oam, 20, false, &[]));
        assert_eq!(free.last(), Some(&(FREE_VRAM_LIMIT + 1)));
        assert_eq!(full.last(), Some(&(VRAM_LIMIT + 1)));
    }

    #[test]
    fn access_on_a_busy_cycle_waits_and_holds_the_bus() {
        let mut fetch = ObjFetch::default();
        fetch.mark(10);
        fetch.mark(12);
        assert_eq!(fetch.stall(10, 1, false), 2);
        assert_eq!(fetch.stall(9, 1, false), 1);
        assert_eq!(fetch.stall(9, 2, false), 2);
        assert_eq!(fetch.stall(9, 2, true), 3);
        assert_eq!(fetch.stall(10, 2, true), 4);
    }

    #[test]
    fn oam_write_counts_only_for_reads_after_it_lands() {
        let mut oam = hidden_oam();
        set_attr(&mut oam, 0, 0, 16);
        set_attr(&mut oam, 0, 1, 8);
        let undo = |pos| {
            [OamWrite {
                pos,
                half: 0,
                old: HIDDEN,
            }]
        };
        assert!(busy_positions(&schedule(&oam, 20, false, &undo(1))).is_empty());
        assert_eq!(
            busy_positions(&schedule(&oam, 20, false, &undo(0))),
            [5, 7, 9, 11]
        );
    }

    fn ppu_at(vcount: u16, dot: u32) -> Ppu {
        Ppu {
            vcount,
            dot,
            dispcnt: OBJ,
            ..Ppu::default()
        }
    }

    #[test]
    fn window_starts_at_dot_forty_counted_in_gbahawk_cycles() {
        let at = |vcount, dot| ppu_at(vcount, dot).obj_window(0);
        let start = EVAL_START - GBAHAWK_CYCLE_OFFSET;
        assert_eq!(at(19, start), Some(Window { line: 19, pos: 0 }));
        assert_eq!(
            at(19, start - 1),
            Some(Window {
                line: 18,
                pos: LINE_CYCLES - 1
            })
        );
        assert_eq!(at(160, start), None);
        assert_eq!(at(227, start), Some(Window { line: 227, pos: 0 }));
        assert_eq!(
            ppu_at(19, start).obj_window(LINE_CYCLES * 3),
            Some(Window { line: 22, pos: 0 })
        );
    }

    #[test]
    fn stall_follows_the_schedule_of_the_access_line() {
        let mut oam = hidden_oam();
        set_attr(&mut oam, 0, 0, 20);
        set_attr(&mut oam, 0, 1, 8);
        let mut ppu = ppu_at(19, EVAL_START - GBAHAWK_CYCLE_OFFSET);
        assert_eq!(ppu.obj_vram_stall(&oam, 5, 1, false), 2);
        assert_eq!(ppu.obj_vram_stall(&oam, 4, 1, false), 1);
        assert_eq!(ppu.obj_vram_stall(&oam, LINE_CYCLES * 8 + 5, 1, false), 1);
    }

    #[test]
    fn oam_write_in_the_window_keeps_earlier_reads() {
        let mut oam = hidden_oam();
        let mut ppu = ppu_at(19, EVAL_START - GBAHAWK_CYCLE_OFFSET);
        assert_eq!(ppu.obj_vram_stall(&oam, 5, 1, false), 1);
        ppu.oam_written(&oam, 3, 0, 4);
        set_attr(&mut oam, 0, 0, 20);
        set_attr(&mut oam, 0, 1, 8);
        assert_eq!(ppu.obj_vram_stall(&oam, 5, 1, false), 1);
        ppu.oam_writes.retire(20);
        assert!(!ppu.oam_writes.is_empty());
        ppu.oam_writes.retire(21);
        assert!(ppu.oam_writes.is_empty());
        ppu.obj_fetch.forget();
        assert_eq!(ppu.obj_vram_stall(&oam, 5, 1, false), 2);
    }

    #[test]
    fn oam_write_before_the_read_counts_for_it() {
        let mut oam = hidden_oam();
        let mut ppu = ppu_at(19, EVAL_START - GBAHAWK_CYCLE_OFFSET);
        assert_eq!(ppu.obj_vram_stall(&oam, 5, 1, false), 1);
        ppu.oam_written(&oam, 0, 0, 4);
        set_attr(&mut oam, 0, 0, 20);
        set_attr(&mut oam, 0, 1, 8);
        assert_eq!(ppu.obj_vram_stall(&oam, 5, 1, false), 2);
    }

    #[test]
    fn dispcnt_obj_change_rebuilds_the_schedule() {
        let mut oam = hidden_oam();
        set_attr(&mut oam, 0, 0, 20);
        set_attr(&mut oam, 0, 1, 8);
        let mut ppu = ppu_at(19, EVAL_START - GBAHAWK_CYCLE_OFFSET);
        assert_eq!(ppu.obj_vram_stall(&oam, 5, 1, false), 2);
        ppu.write_dispcnt(0, OBJ);
        assert_eq!(ppu.obj_vram_stall(&oam, 5, 1, false), 1);
    }

    fn oam_positions(fetch: &ObjFetch) -> Vec<u32> {
        (0..LINE_CYCLES).filter(|&at| fetch.oam_busy(at)).collect()
    }

    #[test]
    fn hidden_sprites_read_oam_on_every_call_until_the_last_sprite() {
        let fetch = schedule(&hidden_oam(), 20, false, &[]);
        let odd: Vec<u32> = (0..SPRITES as u32).map(|call| call * 2 + 1).collect();
        assert_eq!(oam_positions(&fetch), odd);
    }

    #[test]
    fn last_plain_sprite_reads_attr2_after_its_attributes() {
        let mut oam = hidden_oam();
        set_attr(&mut oam, SPRITES - 1, 0, 16);
        set_attr(&mut oam, SPRITES - 1, 1, 8);
        let reads = oam_positions(&schedule(&oam, 20, false, &[]));
        assert_eq!(reads.len(), SPRITES + 1);
        assert_eq!(reads.last(), Some(&257));
    }

    #[test]
    fn last_affine_sprite_reads_four_parameters_after_attr2() {
        let mut oam = hidden_oam();
        set_attr(&mut oam, SPRITES - 1, 0, 16 | 1 << 8);
        set_attr(&mut oam, SPRITES - 1, 1, 8);
        let reads = oam_positions(&schedule(&oam, 20, false, &[]));
        assert_eq!(reads[SPRITES - 1..], [255, 257, 259, 261, 263, 265]);
    }

    #[test]
    fn oam_access_waits_one_cycle_while_the_sprite_scan_reads_oam() {
        let oam = hidden_oam();
        let mut ppu = ppu_at(19, EVAL_START - GBAHAWK_CYCLE_OFFSET);
        assert_eq!(ppu.oam_stall(&oam, 1, 1), 2);
        assert_eq!(ppu.oam_stall(&oam, 2, 1), 1);
        assert_eq!(ppu.oam_stall(&oam, 3, 2), 3);
        ppu.write_dispcnt(0, OBJ);
        assert_eq!(ppu.oam_stall(&oam, 1, 1), 1);
    }
}
