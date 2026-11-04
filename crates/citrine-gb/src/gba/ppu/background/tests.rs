use crate::gba::bus::Bus;
use crate::gba::ppu::rgba;
use crate::gba::ppu::tests::{bus, draw, pixel};

const DISPCNT: u32 = 0x0400_0000;
const BG0CNT: u32 = 0x0400_0008;
const BG0HOFS: u32 = 0x0400_0010;
const BG0VOFS: u32 = 0x0400_0012;
const PALETTE: u32 = 0x0500_0000;
const VRAM: u32 = 0x0600_0000;
const SCREEN_BLOCK: u32 = 0x800;
const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;
const BG0: u16 = 1 << 8;
const MAP: u16 = 8;
const MAP_BLOCK: u32 = 8 * SCREEN_BLOCK;

fn show(bus: &mut Bus, mode: u16, layers: u16) {
    bus.write16(DISPCNT, mode | layers);
}

fn solid_tile_4bpp(bus: &mut Bus, at: u32, index: u8) {
    let nibbles = u16::from(index) * 0x1111;
    for half in 0..16 {
        bus.write16(at + half * 2, nibbles);
    }
}

fn solid_tile_8bpp(bus: &mut Bus, at: u32, index: u8) {
    for half in 0..32 {
        bus.write16(at + half * 2, u16::from(index) * 0x0101);
    }
}

fn map(bus: &mut Bus, block: u32, x: u32, y: u32, entry: u16) {
    bus.write16(VRAM + block * SCREEN_BLOCK + (y * 32 + x) * 2, entry);
}

fn color(bus: &mut Bus, index: u32, value: u16) {
    bus.write16(PALETTE + index * 2, value);
}

#[test]
fn a_4bpp_tile_reads_its_palette_bank() {
    let mut bus = bus();
    solid_tile_4bpp(&mut bus, VRAM + 32, 3);
    color(&mut bus, 2 * 16 + 3, GREEN);
    map(&mut bus, 8, 1, 0, 1 | 2 << 12);
    bus.write16(BG0CNT, MAP << 8);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 8, 0), rgba(GREEN));
    assert_eq!(pixel(&bus, 15, 7), rgba(GREEN));
    assert_eq!(pixel(&bus, 16, 0), rgba(0));
}

#[test]
fn index_0_is_transparent_and_shows_the_backdrop() {
    let mut bus = bus();
    color(&mut bus, 0, BLUE);
    solid_tile_4bpp(&mut bus, VRAM, 0);
    bus.write16(BG0CNT, MAP << 8);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
}

#[test]
fn an_8bpp_tile_reads_the_whole_palette_from_its_char_block() {
    let mut bus = bus();
    solid_tile_8bpp(&mut bus, VRAM + 0x8000 + 2 * 64, 200);
    color(&mut bus, 200, RED);
    map(&mut bus, 8, 0, 0, 2 | 15 << 12);
    bus.write16(BG0CNT, 2 << 2 | 1 << 7 | MAP << 8);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    assert_eq!(pixel(&bus, 7, 7), rgba(RED));
}

#[test]
fn the_left_pixel_is_the_low_nibble_and_flips_mirror_the_tile() {
    let mut bus = bus();
    bus.write16(VRAM, 0x0001);
    color(&mut bus, 1, RED);
    map(&mut bus, 8, 0, 0, 0);
    map(&mut bus, 8, 1, 0, 1 << 10);
    map(&mut bus, 8, 2, 0, 1 << 11);
    map(&mut bus, 8, 3, 0, 3 << 10);
    bus.write16(BG0CNT, MAP << 8);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    assert_eq!(pixel(&bus, 1, 0), rgba(0));
    assert_eq!(pixel(&bus, 15, 0), rgba(RED));
    assert_eq!(pixel(&bus, 8, 0), rgba(0));
    assert_eq!(pixel(&bus, 16, 7), rgba(RED));
    assert_eq!(pixel(&bus, 16, 0), rgba(0));
    assert_eq!(pixel(&bus, 31, 7), rgba(RED));
}

#[test]
fn an_8bpp_row_flips_byte_by_byte() {
    let mut bus = bus();
    bus.write16(VRAM, 0x0009);
    color(&mut bus, 9, BLUE);
    map(&mut bus, 8, 0, 0, 1 << 10);
    bus.write16(BG0CNT, 1 << 7 | MAP << 8);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 7, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 0, 0), rgba(0));
}

#[test]
fn scrolling_moves_the_map_and_wraps_at_256() {
    let mut bus = bus();
    solid_tile_4bpp(&mut bus, VRAM + 32, 1);
    color(&mut bus, 1, RED);
    map(&mut bus, 8, 31, 31, 1);
    bus.write16(BG0CNT, MAP << 8);
    bus.write16(BG0HOFS, 248 + 256);
    bus.write16(BG0VOFS, 252);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    assert_eq!(pixel(&bus, 7, 3), rgba(RED));
    assert_eq!(pixel(&bus, 8, 0), rgba(0));
    assert_eq!(pixel(&bus, 0, 4), rgba(0));
}

fn four_blocks(bus: &mut Bus) {
    for (block, index) in [(8, 1), (9, 2), (10, 3), (11, 4)] {
        solid_tile_4bpp(bus, VRAM + index * 32, index as u8);
        map(bus, block, 0, 0, index as u16);
    }
    for (index, value) in [(1, RED), (2, GREEN), (3, BLUE), (4, 0x7FFF)] {
        color(bus, index, value);
    }
}

fn corner(bus: &mut Bus, size: u16, hofs: u16, vofs: u16) -> [u8; 4] {
    bus.write16(BG0CNT, MAP << 8 | size << 14);
    bus.write16(BG0HOFS, hofs);
    bus.write16(BG0VOFS, vofs);
    draw(bus);
    pixel(bus, 0, 0)
}

#[test]
fn the_four_screen_sizes_lay_out_their_blocks() {
    let mut bus = bus();
    four_blocks(&mut bus);
    show(&mut bus, 0, BG0);
    assert_eq!(corner(&mut bus, 0, 256, 256), rgba(RED));
    assert_eq!(corner(&mut bus, 1, 256, 0), rgba(GREEN));
    assert_eq!(corner(&mut bus, 1, 0, 256), rgba(RED));
    assert_eq!(corner(&mut bus, 2, 0, 256), rgba(GREEN));
    assert_eq!(corner(&mut bus, 2, 256, 0), rgba(RED));
    assert_eq!(corner(&mut bus, 3, 256, 0), rgba(GREEN));
    assert_eq!(corner(&mut bus, 3, 0, 256), rgba(BLUE));
    assert_eq!(corner(&mut bus, 3, 256, 256), rgba(0x7FFF));
}

#[test]
fn tiles_past_the_64k_bg_area_are_transparent() {
    let mut bus = bus();
    color(&mut bus, 0, BLUE);
    solid_tile_4bpp(&mut bus, VRAM + 0x1_0000, 1);
    color(&mut bus, 1, RED);
    map(&mut bus, 8, 0, 0, 512);
    bus.write16(BG0CNT, 3 << 2 | MAP << 8);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
}

#[test]
fn mode_1_draws_bg0_and_bg1_as_text_and_mode_2_none() {
    let mut bus = bus();
    color(&mut bus, 0, BLUE);
    solid_tile_4bpp(&mut bus, VRAM, 1);
    color(&mut bus, 1, RED);
    for bg in 0..4 {
        bus.write16(BG0CNT + bg * 2, MAP << 8);
    }
    for (mode, layer, expected) in [
        (1, 0, RED),
        (1, 1, RED),
        (1, 3, BLUE),
        (2, 0, BLUE),
        (2, 1, BLUE),
    ] {
        show(&mut bus, mode, BG0 << layer);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(expected), "mode {mode} bg{layer}");
    }
}

#[test]
fn bgcnt_reads_back_without_bit_13_on_bg0_and_bg1() {
    let mut bus = bus();
    for bg in 0..4 {
        bus.write16(BG0CNT + bg * 2, 0xFFFF);
    }
    assert_eq!(bus.read16(BG0CNT), 0xDFFF);
    assert_eq!(bus.read16(BG0CNT + 2), 0xDFFF);
    assert_eq!(bus.read16(BG0CNT + 4), 0xFFFF);
    assert_eq!(bus.read16(BG0CNT + 6), 0xFFFF);
}

#[test]
fn scroll_registers_are_write_only_and_byte_writes_land() {
    let mut bus = bus();
    solid_tile_4bpp(&mut bus, VRAM + 32, 1);
    color(&mut bus, 1, RED);
    map(&mut bus, 8, 1, 0, 1);
    bus.write16(BG0CNT, MAP << 8);
    bus.write8(BG0HOFS, 8);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(bus.read16(BG0HOFS), 0);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
}

#[test]
fn map_entries_come_from_the_selected_screen_block() {
    let mut bus = bus();
    solid_tile_4bpp(&mut bus, VRAM + 32, 1);
    color(&mut bus, 1, GREEN);
    bus.write16(VRAM + 31 * SCREEN_BLOCK, 1);
    bus.write16(VRAM + MAP_BLOCK, 0);
    bus.write16(BG0CNT, 31 << 8);
    show(&mut bus, 0, BG0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
}
