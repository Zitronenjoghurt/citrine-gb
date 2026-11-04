use super::{HBLANK_START, LINE_CYCLES};
use crate::gba::bus::Bus;
use crate::gba::interrupt::Irq;

const DISPCNT: u32 = 0x0400_0000;
const DISPSTAT: u32 = 0x0400_0004;
const VCOUNT: u32 = 0x0400_0006;
const DMA0: u32 = 0x0400_00B0;
const PALETTE: u32 = 0x0500_0000;
const VRAM: u32 = 0x0600_0000;
const FRAME_CYCLES: u32 = 280_896;
const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;
const BG2: u16 = 1 << 10;

pub(super) fn bus() -> Bus {
    Bus::new(&[])
}

pub(super) fn to_line(bus: &mut Bus, line: u32) {
    let now = u32::from(bus.ppu().vcount());
    bus.step_video((line + 228 - now) % 228 * LINE_CYCLES);
}

pub(super) fn pixel(bus: &Bus, x: usize, y: usize) -> [u8; 4] {
    let at = (y * 240 + x) * 4;
    bus.ppu().frame()[at..at + 4].try_into().unwrap()
}

pub(super) fn draw(bus: &mut Bus) {
    bus.step_video(FRAME_CYCLES);
}

#[test]
fn vcount_counts_lines_of_1232_cycles_and_wraps_after_228() {
    let mut bus = bus();
    bus.step_video(LINE_CYCLES - 1);
    assert_eq!(bus.read16(VCOUNT), 0);
    bus.step_video(1);
    assert_eq!(bus.read16(VCOUNT), 1);
    bus.step_video(226 * LINE_CYCLES);
    assert_eq!(bus.read16(VCOUNT), 227);
    bus.step_video(LINE_CYCLES);
    assert_eq!(bus.read16(VCOUNT), 0);
}

#[test]
fn a_frame_is_280896_cycles() {
    let mut bus = bus();
    bus.step_video(FRAME_CYCLES - 1);
    assert_eq!(bus.read16(VCOUNT), 227);
    bus.step_video(1);
    assert_eq!(bus.read16(VCOUNT), 0);
    assert_eq!(bus.read16(DISPSTAT) & 0b11, 0);
}

#[test]
fn the_hblank_flag_rises_at_cycle_1007_and_falls_at_the_next_line() {
    let mut bus = bus();
    bus.step_video(HBLANK_START - 1);
    assert_eq!(bus.read16(DISPSTAT) & 0b10, 0);
    bus.step_video(1);
    assert_eq!(bus.read16(DISPSTAT) & 0b10, 0b10);
    bus.step_video(LINE_CYCLES - HBLANK_START - 1);
    assert_eq!(bus.read16(DISPSTAT) & 0b10, 0b10);
    bus.step_video(1);
    assert_eq!(bus.read16(DISPSTAT) & 0b10, 0);
}

#[test]
fn the_hblank_irq_fires_on_every_line_when_enabled() {
    let mut quiet = bus();
    quiet.step_video(HBLANK_START);
    assert_eq!(quiet.interrupts().if_(), 0);
    let mut bus = bus();
    bus.write16(DISPSTAT, 1 << 4);
    to_line(&mut bus, 200);
    bus.write16(0x0400_0202, 0xFFFF);
    bus.step_video(HBLANK_START + 1);
    assert_eq!(bus.interrupts().if_(), 0);
    bus.step_video(1);
    assert_eq!(bus.interrupts().if_(), Irq::HBLANK.bits());
}

#[test]
fn the_vblank_flag_covers_lines_160_to_226() {
    let mut bus = bus();
    to_line(&mut bus, 159);
    assert_eq!(bus.read16(DISPSTAT) & 1, 0);
    to_line(&mut bus, 160);
    assert_eq!(bus.read16(DISPSTAT) & 1, 1);
    to_line(&mut bus, 226);
    assert_eq!(bus.read16(DISPSTAT) & 1, 1);
    to_line(&mut bus, 227);
    assert_eq!(bus.read16(DISPSTAT) & 1, 0);
}

#[test]
fn the_vblank_irq_fires_once_at_line_160_when_enabled() {
    let mut bus = bus();
    to_line(&mut bus, 159);
    bus.step_video(LINE_CYCLES);
    assert_eq!(bus.interrupts().if_(), 0);
    bus.write16(DISPSTAT, 1 << 3);
    to_line(&mut bus, 159);
    bus.write16(0x0400_0202, 0xFFFF);
    bus.step_video(LINE_CYCLES + 1);
    assert_eq!(bus.interrupts().if_(), 0);
    bus.step_video(1);
    assert_eq!(bus.interrupts().if_(), Irq::VBLANK.bits());
    bus.write16(0x0400_0202, 0xFFFF);
    to_line(&mut bus, 159);
    assert_eq!(bus.interrupts().if_(), 0);
}

#[test]
fn the_vcount_flag_and_irq_follow_the_lyc_setting() {
    let mut bus = bus();
    bus.write16(DISPSTAT, (100 << 8) | (1 << 5));
    to_line(&mut bus, 99);
    assert_eq!(bus.read16(DISPSTAT) & 0b100, 0);
    assert_eq!(bus.interrupts().if_(), 0);
    to_line(&mut bus, 100);
    assert_eq!(bus.read16(DISPSTAT) & 0b100, 0);
    bus.step_video(1);
    assert_eq!(bus.read16(DISPSTAT) & 0b100, 0b100);
    bus.step_video(1);
    assert_eq!(bus.interrupts().if_(), 0);
    bus.step_video(1);
    assert_eq!(bus.interrupts().if_(), Irq::VCOUNT.bits());
    to_line(&mut bus, 101);
    bus.step_video(1);
    assert_eq!(bus.read16(DISPSTAT) & 0b100, 0);
}

#[test]
fn dispstat_keeps_its_flags_read_only_and_vcount_ignores_writes() {
    let mut bus = bus();
    to_line(&mut bus, 170);
    bus.write16(DISPSTAT, 0xFFFF);
    assert_eq!(bus.read16(DISPSTAT), 0xFF39);
    bus.write16(DISPSTAT, 0);
    assert_eq!(bus.read16(DISPSTAT), 1);
    bus.write16(VCOUNT, 5);
    assert_eq!(bus.read16(VCOUNT), 170);
}

#[test]
fn vblank_dma_starts_at_line_160() {
    let mut bus = bus();
    bus.write32(DMA0, 0x0300_0000);
    bus.write32(DMA0 + 4, 0x0300_0100);
    bus.write32(DMA0 + 8, 0x8000_0001 | (1 << 28));
    to_line(&mut bus, 159);
    assert!(!bus.dma_active());
    to_line(&mut bus, 160);
    assert!(bus.dma_active());
}

#[test]
fn hblank_dma_starts_on_visible_lines_only() {
    let mut bus = bus();
    bus.write32(DMA0, 0x0300_0000);
    bus.write32(DMA0 + 4, 0x0300_0100);
    bus.write32(DMA0 + 8, 0x8000_0001 | (2 << 28) | (1 << 25));
    bus.step_video(HBLANK_START - 1);
    assert!(!bus.dma_active());
    bus.step_video(1);
    assert!(bus.dma_active());
    to_line(&mut bus, 160);
    bus.run_dma();
    bus.step_video(HBLANK_START);
    assert!(!bus.dma_active());
}

#[test]
fn dma3_video_capture_runs_on_lines_2_to_161_once_armed_at_line_163() {
    let mut bus = bus();
    bus.write32(DMA0 + 36, 0x0300_0000);
    bus.write32(DMA0 + 40, 0x0300_0100);
    bus.write32(DMA0 + 44, 0x8000_0001 | (3 << 28) | (1 << 25));
    to_line(&mut bus, 2);
    bus.step_video(3);
    assert!(!bus.dma_active());
    bus.step_video(LINE_CYCLES - 3);
    to_line(&mut bus, 163);
    to_line(&mut bus, 2);
    bus.step_video(2);
    assert!(!bus.dma_active());
    bus.step_video(1);
    assert!(bus.dma_active());
    bus.run_dma();
    to_line(&mut bus, 161);
    bus.run_dma();
    to_line(&mut bus, 162);
    bus.step_video(3);
    assert!(!bus.dma_active());
    assert_eq!(bus.read16(DMA0 + 46) & 0x8000, 0);
}

#[test]
fn bitmap_modes_drop_vram_past_0x1c000_mirror_below() {
    let mut bus = bus();
    bus.write16(DISPCNT, 3);
    bus.write32(VRAM + 0x1_0000, 0xABAD_1DEA);
    bus.write32(VRAM + 0x1_8000, 0xDEAD_BEEF);
    assert_eq!(bus.read32(VRAM + 0x1_0000), 0xABAD_1DEA);
    assert_eq!(bus.read32(VRAM + 0x1_8000), 0);
    bus.write32(VRAM + 0x1_C000, 0x1234_5678);
    assert_eq!(bus.read32(VRAM + 0x1_4000), 0x1234_5678);
    bus.write16(DISPCNT, 0);
    bus.write32(VRAM + 0x1_8000, 0xDEAD_BEEF);
    assert_eq!(bus.read32(VRAM + 0x1_0000), 0xDEAD_BEEF);
}

#[test]
fn mode_3_draws_one_colour_per_halfword() {
    let mut bus = bus();
    bus.write16(DISPCNT, 3 | BG2);
    bus.write16(VRAM, RED);
    bus.write16(VRAM + 2 * (239 + 240 * 159), BLUE);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), [0xFF, 0, 0, 0xFF]);
    assert_eq!(pixel(&bus, 239, 159), [0, 0, 0xFF, 0xFF]);
    assert_eq!(pixel(&bus, 1, 0), [0, 0, 0, 0xFF]);
}

#[test]
fn mode_4_reads_palette_indices_from_the_selected_frame() {
    let mut bus = bus();
    bus.write16(PALETTE + 2, GREEN);
    bus.write16(PALETTE + 4, BLUE);
    bus.write16(VRAM + 240 * 10, 0x0201);
    bus.write16(VRAM + 0xA000 + 240 * 10, 0x0102);
    bus.write16(DISPCNT, 4 | BG2);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 10), [0, 0xFF, 0, 0xFF]);
    assert_eq!(pixel(&bus, 1, 10), [0, 0, 0xFF, 0xFF]);
    bus.write16(DISPCNT, 4 | BG2 | (1 << 4));
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 10), [0, 0, 0xFF, 0xFF]);
    assert_eq!(pixel(&bus, 1, 10), [0, 0xFF, 0, 0xFF]);
}

#[test]
fn mode_5_draws_160_by_128_and_the_backdrop_around_it() {
    let mut bus = bus();
    bus.write16(PALETTE, GREEN);
    bus.write16(VRAM + 2 * (159 + 160 * 127), RED);
    bus.write16(VRAM + 0xA000, BLUE);
    bus.write16(DISPCNT, 5 | BG2);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 159, 127), [0xFF, 0, 0, 0xFF]);
    assert_eq!(pixel(&bus, 160, 127), [0, 0xFF, 0, 0xFF]);
    assert_eq!(pixel(&bus, 0, 128), [0, 0xFF, 0, 0xFF]);
    bus.write16(DISPCNT, 5 | BG2 | (1 << 4));
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), [0, 0, 0xFF, 0xFF]);
}

#[test]
fn forced_blank_draws_white() {
    let mut bus = bus();
    bus.write16(VRAM, RED);
    bus.write16(DISPCNT, 3 | BG2 | (1 << 7));
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), [0xFF; 4]);
}

#[test]
fn tiled_modes_and_a_hidden_bg2_draw_the_backdrop() {
    let mut bus = bus();
    bus.write16(PALETTE, RED);
    bus.write16(VRAM, BLUE);
    bus.write16(DISPCNT, 3);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), [0xFF, 0, 0, 0xFF]);
    bus.write16(DISPCNT, BG2);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), [0xFF, 0, 0, 0xFF]);
}

#[test]
fn byte_writes_to_bitmap_vram_reach_the_second_frame() {
    let mut bus = bus();
    bus.write16(DISPCNT, 4 | BG2);
    bus.write8(VRAM + 0x1_2000, 0x05);
    assert_eq!(bus.read16(VRAM + 0x1_2000), 0x0505);
}

const BG0CNT: u32 = 0x0400_0008;
const OBJ_VRAM: u32 = 0x0601_0000;
const OAM: u32 = 0x0700_0000;
const BG0: u16 = 1 << 8;
const OBJ: u16 = 1 << 12;

pub(super) fn solid_layers(bus: &mut Bus) {
    for half in 0..16 {
        bus.write16(VRAM + 32 + half * 2, 0x1111);
        bus.write16(VRAM + 64 + half * 2, 0x2222);
        bus.write16(OBJ_VRAM + half * 2, 0x3333);
    }
    for (block, tile) in [(8, 1), (9, 2)] {
        for entry in 0..1024 {
            bus.write16(VRAM + block * 0x800 + entry * 2, tile);
        }
    }
    bus.write16(PALETTE + 2, RED);
    bus.write16(PALETTE + 4, GREEN);
    bus.write16(PALETTE + 0x200 + 6, BLUE);
    for index in 1..128 {
        bus.write16(OAM + index * 8, 1 << 9);
    }
}

fn top(bus: &mut Bus, bg0: u16, bg1: u16, obj: Option<u16>) -> [u8; 4] {
    bus.write16(BG0CNT, bg0 | 8 << 8);
    bus.write16(BG0CNT + 2, bg1 | 9 << 8);
    let layers = match obj {
        Some(priority) => {
            bus.write16(OAM + 4, priority << 10);
            BG0 | BG0 << 1 | OBJ
        }
        None => BG0 | BG0 << 1,
    };
    bus.write16(DISPCNT, layers);
    draw(bus);
    pixel(bus, 0, 0)
}

#[test]
fn the_bg_with_the_lower_priority_value_is_in_front() {
    let mut bus = bus();
    solid_layers(&mut bus);
    assert_eq!(top(&mut bus, 1, 0, None), super::rgba(GREEN));
    assert_eq!(top(&mut bus, 0, 1, None), super::rgba(RED));
}

#[test]
fn on_equal_priority_the_lower_bg_number_is_in_front() {
    let mut bus = bus();
    solid_layers(&mut bus);
    assert_eq!(top(&mut bus, 2, 2, None), super::rgba(RED));
}

#[test]
fn a_sprite_is_in_front_of_bgs_of_its_own_priority_or_lower() {
    let mut bus = bus();
    solid_layers(&mut bus);
    assert_eq!(top(&mut bus, 1, 1, Some(1)), super::rgba(BLUE));
    assert_eq!(top(&mut bus, 1, 3, Some(2)), super::rgba(RED));
    assert_eq!(top(&mut bus, 3, 1, Some(2)), super::rgba(GREEN));
    assert_eq!(top(&mut bus, 3, 3, Some(0)), super::rgba(BLUE));
}

#[test]
fn a_sprite_shows_through_mode_4_index_0() {
    let mut bus = bus();
    solid_layers(&mut bus);
    for half in 0..16 {
        bus.write16(OBJ_VRAM + 0x4000 + half * 2, 0x3333);
    }
    bus.write16(OAM + 4, 512 | 3 << 10);
    bus.write16(VRAM + 2, 0x0101);
    bus.write16(DISPCNT, 4 | BG2 | OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), super::rgba(BLUE));
    assert_eq!(pixel(&bus, 2, 0), super::rgba(RED));
    assert_eq!(pixel(&bus, 8, 0), super::rgba(0));
}

const MOSAIC: u32 = 0x0400_004C;

fn marked_tiles(bus: &mut Bus) {
    for row in 0..8 {
        let first = if row == 0 { 0x2221 } else { 0x2222 };
        bus.write16(VRAM + 96 + row * 4, first);
        bus.write16(VRAM + 96 + row * 4 + 2, 0x2222);
        bus.write16(OBJ_VRAM + 32 + row * 4, first);
        bus.write16(OBJ_VRAM + 32 + row * 4 + 2, 0x2222);
    }
    for entry in 0..1024 {
        bus.write16(VRAM + 10 * 0x800 + entry * 2, 3);
    }
    bus.write16(PALETTE + 0x200 + 2, BLUE);
    bus.write16(PALETTE + 0x200 + 4, GREEN);
}

#[test]
fn bg_mosaic_repeats_the_top_left_pixel_of_each_block() {
    let mut bus = bus();
    solid_layers(&mut bus);
    marked_tiles(&mut bus);
    bus.write16(MOSAIC, 7 | 7 << 4);
    bus.write16(DISPCNT, BG0);
    bus.write16(BG0CNT, 10 << 8);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 1, 0), super::rgba(GREEN));
    bus.write16(BG0CNT, 10 << 8 | 1 << 6);
    draw(&mut bus);
    for (x, y) in [(0, 0), (7, 0), (0, 7), (7, 7)] {
        assert_eq!(pixel(&bus, x, y), super::rgba(RED), "{x},{y}");
    }
    bus.write16(MOSAIC, 3 | 1 << 4);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 3, 1), super::rgba(RED));
    assert_eq!(pixel(&bus, 4, 0), super::rgba(GREEN));
    assert_eq!(pixel(&bus, 0, 2), super::rgba(GREEN));
}

#[test]
fn sprite_mosaic_uses_its_own_block_size() {
    let mut bus = bus();
    solid_layers(&mut bus);
    marked_tiles(&mut bus);
    bus.write16(OAM + 4, 1);
    bus.write16(MOSAIC, 3 << 8 | 3 << 12);
    bus.write16(DISPCNT, OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 1, 0), super::rgba(GREEN));
    bus.write16(OAM, 1 << 12);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 3, 3), super::rgba(BLUE));
    assert_eq!(pixel(&bus, 4, 0), super::rgba(GREEN));
    assert_eq!(pixel(&bus, 0, 4), super::rgba(GREEN));
}
