use crate::gba::bus::Bus;
use crate::gba::ppu::rgba;
use crate::gba::ppu::tests::{bus, draw, pixel, to_line};

const DISPCNT: u32 = 0x0400_0000;
const BG2CNT: u32 = 0x0400_000C;
const BG3CNT: u32 = 0x0400_000E;
const BG2PA: u32 = 0x0400_0020;
const BG2PB: u32 = 0x0400_0022;
const BG2PC: u32 = 0x0400_0024;
const BG2PD: u32 = 0x0400_0026;
const BG2X: u32 = 0x0400_0028;
const BG2Y: u32 = 0x0400_002C;
const BG3X: u32 = 0x0400_0038;
const PALETTE: u32 = 0x0500_0000;
const VRAM: u32 = 0x0600_0000;
const MAP_BLOCK: u16 = 16;
const MAP: u32 = VRAM + 16 * 0x800;
const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;
const BG2: u16 = 1 << 10;
const BG3: u16 = 1 << 11;
const WRAP: u16 = 1 << 13;

fn tile(bus: &mut Bus, tile: u32, index: u8) {
    for half in 0..32 {
        bus.write16(VRAM + tile * 64 + half * 2, u16::from(index) * 0x0101);
    }
}

fn entry(bus: &mut Bus, map: u32, at: u32, tile: u8) {
    let address = map + (at & !1);
    let old = bus.read16(address);
    let shift = 8 * (at & 1);
    bus.write16(address, (old & !(0xFF << shift)) | u16::from(tile) << shift);
}

fn ready(size: u16) -> Bus {
    let mut bus = bus();
    bus.write16(PALETTE, BLUE);
    bus.write16(PALETTE + 2, RED);
    bus.write16(PALETTE + 4, GREEN);
    tile(&mut bus, 1, 1);
    tile(&mut bus, 2, 2);
    bus.write16(BG2CNT, MAP_BLOCK << 8 | size << 14);
    bus.write16(DISPCNT, 2 | BG2);
    bus
}

#[test]
fn bg2_in_mode_2_reads_one_byte_per_map_entry_and_8bpp_tiles() {
    let mut bus = ready(0);
    entry(&mut bus, MAP, 1, 1);
    entry(&mut bus, MAP, 16, 2);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 7, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 8, 0), rgba(RED));
    assert_eq!(pixel(&bus, 15, 7), rgba(RED));
    assert_eq!(pixel(&bus, 0, 8), rgba(GREEN));
}

#[test]
fn pa_scales_the_line_and_the_reference_point_moves_it() {
    let mut bus = ready(0);
    entry(&mut bus, MAP, 1, 1);
    bus.write16(BG2PA, 0x80);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 15, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 16, 0), rgba(RED));
    assert_eq!(pixel(&bus, 31, 0), rgba(RED));
    bus.write16(BG2PA, 0x100);
    bus.write32(BG2X, 4 << 8);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 3, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 4, 0), rgba(RED));
}

#[test]
fn pc_moves_down_the_map_along_the_line() {
    let mut bus = ready(0);
    entry(&mut bus, MAP, 16, 2);
    bus.write16(BG2PA, 0);
    bus.write16(BG2PC, 0x100);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 7, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 8, 0), rgba(GREEN));
}

#[test]
fn pb_and_pd_step_the_reference_point_each_line() {
    let mut bus = ready(0);
    entry(&mut bus, MAP, 16, 2);
    entry(&mut bus, MAP, 1, 1);
    bus.write16(BG2PD, 0x200);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 3), rgba(BLUE));
    assert_eq!(pixel(&bus, 0, 4), rgba(GREEN));
    bus.write16(BG2PD, 0);
    bus.write16(BG2PB, 0x200);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 3), rgba(BLUE));
    assert_eq!(pixel(&bus, 0, 4), rgba(RED));
}

#[test]
fn the_reference_point_reloads_each_frame_and_on_a_write_mid_frame() {
    let mut bus = ready(0);
    entry(&mut bus, MAP, 16, 2);
    draw(&mut bus);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 0, 8), rgba(GREEN));
    to_line(&mut bus, 100);
    bus.write32(BG2Y, 8 << 8);
    to_line(&mut bus, 0);
    assert_eq!(pixel(&bus, 0, 99), rgba(BLUE));
    assert_eq!(pixel(&bus, 0, 100), rgba(GREEN));
    assert_eq!(pixel(&bus, 0, 107), rgba(GREEN));
    assert_eq!(pixel(&bus, 0, 108), rgba(BLUE));
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
}

#[test]
fn the_reference_point_is_28_bits_signed_and_takes_byte_writes() {
    let mut bus = ready(0);
    entry(&mut bus, MAP, 0, 1);
    bus.write16(BG2CNT, MAP_BLOCK << 8 | WRAP);
    bus.write32(BG2X, 0x0FFF_FF00);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 1, 0), rgba(RED));
    bus.write8(BG2X + 1, 0xFE);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 1, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 2, 0), rgba(RED));
}

#[test]
fn outside_the_map_is_transparent_unless_the_bg_wraps() {
    let mut bus = ready(0);
    entry(&mut bus, MAP, 0, 1);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    assert_eq!(pixel(&bus, 128, 0), rgba(BLUE));
    bus.write16(BG2CNT, MAP_BLOCK << 8 | WRAP);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 128, 0), rgba(RED));
    assert_eq!(pixel(&bus, 128, 128), rgba(RED));
}

#[test]
fn the_four_map_sizes_are_16_to_128_tiles_square() {
    for (size, tiles) in [(0u16, 16u32), (1, 32), (2, 64), (3, 128)] {
        let mut bus = ready(size);
        entry(&mut bus, MAP, tiles, 1);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 8), rgba(RED), "size {size}");
        assert_eq!(pixel(&bus, 0, 0), rgba(BLUE), "size {size}");
    }
}

#[test]
fn mode_2_draws_bg3_and_mode_1_draws_only_bg2_as_affine() {
    let mut bus = ready(0);
    entry(&mut bus, MAP, 0, 1);
    bus.write16(BG3CNT, MAP_BLOCK << 8);
    for (dispcnt, expected) in [(2 | BG3, RED), (1 | BG3, BLUE), (1 | BG2, RED)] {
        bus.write16(DISPCNT, dispcnt);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(expected), "{dispcnt:#x}");
    }
    bus.write32(BG3X, 8 << 8);
    bus.write16(DISPCNT, 2 | BG3);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
}

#[test]
fn bitmap_modes_use_bg2_affine_parameters() {
    let mut bus = bus();
    bus.write16(PALETTE, BLUE);
    bus.write16(VRAM + 10 * 2, RED);
    bus.write16(DISPCNT, 5 | BG2);
    bus.write32(BG2X, 5 << 8);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 5, 0), rgba(RED));
    assert_eq!(pixel(&bus, 10, 0), rgba(0));
    bus.write32(BG2X, 0);
    bus.write16(BG2PA, 0x200);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 5, 0), rgba(RED));
    assert_eq!(pixel(&bus, 80, 0), rgba(BLUE));
    bus.write16(DISPCNT, 3 | BG2);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 5, 0), rgba(RED));
    assert_eq!(pixel(&bus, 120, 0), rgba(BLUE));
}

#[test]
fn affine_registers_are_write_only() {
    let mut bus = ready(0);
    bus.write16(BG2PA, 0x1234);
    assert_eq!(bus.read16(BG2PA), 0);
    assert_eq!(bus.ppu().affine_matrix(0), [0x1234, 0, 0, 0x100]);
}
