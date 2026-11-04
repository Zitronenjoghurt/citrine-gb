use crate::gba::bus::Bus;
use crate::gba::ppu::rgba;
use crate::gba::ppu::tests::{bus, draw, pixel};

const DISPCNT: u32 = 0x0400_0000;
const OBJ_PALETTE: u32 = 0x0500_0200;
const OBJ_VRAM: u32 = 0x0601_0000;
const OAM: u32 = 0x0700_0000;
const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;
const OBJ: u16 = 1 << 12;
const ONE_D: u16 = 1 << 6;
const HBLANK_FREE: u16 = 1 << 5;
const HIDDEN: u16 = 1 << 9;
const WIDE: u16 = 1 << 13;

fn sprite(bus: &mut Bus, index: u32, attr0: u16, attr1: u16, attr2: u16) {
    bus.write16(OAM + index * 8, attr0);
    bus.write16(OAM + index * 8 + 2, attr1);
    bus.write16(OAM + index * 8 + 4, attr2);
}

fn hide_all(bus: &mut Bus) {
    for index in 0..128 {
        bus.write16(OAM + index * 8, HIDDEN);
    }
}

fn solid_tile(bus: &mut Bus, tile: u32, index: u8) {
    for half in 0..16 {
        bus.write16(OBJ_VRAM + tile * 32 + half * 2, u16::from(index) * 0x1111);
    }
}

fn color(bus: &mut Bus, index: u32, value: u16) {
    bus.write16(OBJ_PALETTE + index * 2, value);
}

fn ready() -> Bus {
    let mut bus = bus();
    hide_all(&mut bus);
    color(&mut bus, 1, RED);
    bus
}

#[test]
fn a_4bpp_sprite_reads_its_bank_of_the_obj_palette() {
    let mut bus = ready();
    solid_tile(&mut bus, 1, 5);
    color(&mut bus, 3 * 16 + 5, GREEN);
    sprite(&mut bus, 0, 20, 16, 1 | 3 << 12);
    bus.write16(DISPCNT, OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 16, 20), rgba(GREEN));
    assert_eq!(pixel(&bus, 23, 27), rgba(GREEN));
    for (x, y) in [(24, 20), (15, 20), (16, 28), (16, 19)] {
        assert_eq!(pixel(&bus, x, y), rgba(0), "{x},{y}");
    }
}

#[test]
fn every_shape_and_size_covers_its_area() {
    let mut bus = ready();
    for tile in 0..1024 {
        solid_tile(&mut bus, tile, 1);
    }
    let sizes = [
        [(8, 8), (16, 16), (32, 32), (64, 64)],
        [(16, 8), (32, 8), (32, 16), (64, 32)],
        [(8, 16), (8, 32), (16, 32), (32, 64)],
    ];
    bus.write16(DISPCNT, OBJ | ONE_D);
    for (shape, row) in sizes.iter().enumerate() {
        for (size, &(width, height)) in row.iter().enumerate() {
            sprite(&mut bus, 0, (shape as u16) << 14, (size as u16) << 14, 0);
            draw(&mut bus);
            let case = format!("shape {shape} size {size}");
            assert_eq!(pixel(&bus, width - 1, height - 1), rgba(RED), "{case}");
            assert_eq!(pixel(&bus, width, 0), rgba(0), "{case}");
            assert_eq!(pixel(&bus, 0, height), rgba(0), "{case}");
        }
    }
}

#[test]
fn the_second_tile_row_follows_the_mapping_mode() {
    let mut bus = ready();
    solid_tile(&mut bus, 1, 1);
    solid_tile(&mut bus, 32, 2);
    color(&mut bus, 2, GREEN);
    sprite(&mut bus, 0, 2 << 14, 0, 0);
    bus.write16(DISPCNT, OBJ | ONE_D);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 8), rgba(RED));
    bus.write16(DISPCNT, OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 8), rgba(GREEN));
}

#[test]
fn an_8bpp_sprite_uses_two_tile_numbers_per_tile() {
    let mut bus = ready();
    for half in 0..32 {
        bus.write16(OBJ_VRAM + 2 * 32 + half * 2, 0x4D4D);
    }
    color(&mut bus, 0x4D, BLUE);
    sprite(&mut bus, 0, WIDE, 0, 2);
    bus.write16(DISPCNT, OBJ | ONE_D);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
    assert_eq!(pixel(&bus, 7, 7), rgba(BLUE));
}

#[test]
fn an_8bpp_sprite_in_2d_mapping_ignores_the_low_tile_bit() {
    let mut bus = ready();
    for half in 0..32 {
        bus.write16(OBJ_VRAM + 2 * 32 + half * 2, 0x0101);
    }
    sprite(&mut bus, 0, WIDE, 0, 3);
    bus.write16(DISPCNT, OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
}

#[test]
fn flips_mirror_the_whole_sprite() {
    let mut bus = ready();
    bus.write16(OBJ_VRAM, 0x0001);
    let cases = [
        (0, (0, 0)),
        (1 << 12, (15, 0)),
        (1 << 13, (0, 7)),
        (3 << 12, (15, 7)),
    ];
    bus.write16(DISPCNT, OBJ | ONE_D);
    for (flips, (x, y)) in cases {
        sprite(&mut bus, 0, 1 << 14, flips, 0);
        draw(&mut bus);
        assert_eq!(pixel(&bus, x, y), rgba(RED), "flips {flips:#x}");
        assert_eq!(pixel(&bus, 15 - x, 7 - y), rgba(0), "flips {flips:#x}");
    }
}

#[test]
fn sprites_wrap_past_the_left_and_top_edges() {
    let mut bus = ready();
    solid_tile(&mut bus, 0, 1);
    sprite(&mut bus, 0, 252, 511, 0);
    bus.write16(DISPCNT, OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    assert_eq!(pixel(&bus, 6, 3), rgba(RED));
    assert_eq!(pixel(&bus, 7, 0), rgba(0));
    assert_eq!(pixel(&bus, 0, 4), rgba(0));
}

#[test]
fn hidden_window_and_switched_off_sprites_draw_nothing() {
    let mut bus = ready();
    solid_tile(&mut bus, 0, 1);
    for (attr0, dispcnt) in [(HIDDEN, OBJ), (2 << 10, OBJ), (3 << 10, OBJ), (0, 0)] {
        sprite(&mut bus, 0, attr0, 0, 0);
        bus.write16(DISPCNT, dispcnt);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(0), "attr0 {attr0:#x}");
    }
}

#[test]
fn the_lower_oam_entry_wins_unless_the_other_has_higher_priority() {
    let mut bus = ready();
    solid_tile(&mut bus, 0, 1);
    solid_tile(&mut bus, 1, 2);
    color(&mut bus, 2, GREEN);
    sprite(&mut bus, 0, 0, 0, 1 << 10);
    sprite(&mut bus, 1, 0, 0, 1 | 1 << 10);
    bus.write16(DISPCNT, OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    sprite(&mut bus, 1, 0, 0, 1);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
}

#[test]
fn a_transparent_pixel_of_a_front_sprite_shows_the_one_behind() {
    let mut bus = ready();
    solid_tile(&mut bus, 1, 1);
    sprite(&mut bus, 0, 0, 0, 0);
    sprite(&mut bus, 1, 0, 0, 1 | 3 << 10);
    bus.write16(DISPCNT, OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
}

fn line_budget_case(dispcnt: u16, wide_sprites: u32) -> [u8; 4] {
    let mut bus = ready();
    solid_tile(&mut bus, 0, 1);
    for index in 0..wide_sprites {
        sprite(&mut bus, index, 0, 300 | 3 << 14, 0);
    }
    sprite(&mut bus, wide_sprites, 0, 0, 0);
    bus.write16(DISPCNT, OBJ | dispcnt);
    draw(&mut bus);
    pixel(&bus, 0, 0)
}

#[test]
fn a_line_draws_sprites_until_1210_cycles_are_spent() {
    assert_eq!(line_budget_case(0, 18), rgba(RED));
    assert_eq!(line_budget_case(0, 19), rgba(0));
}

#[test]
fn hblank_free_leaves_954_cycles_for_sprites() {
    assert_eq!(line_budget_case(HBLANK_FREE, 14), rgba(RED));
    assert_eq!(line_budget_case(HBLANK_FREE, 15), rgba(0));
}

#[test]
fn sprites_on_other_lines_cost_nothing() {
    let mut bus = ready();
    solid_tile(&mut bus, 0, 1);
    for index in 0..40 {
        sprite(&mut bus, index, 100, 300 | 3 << 14, 0);
    }
    sprite(&mut bus, 40, 0, 0, 0);
    bus.write16(DISPCNT, OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
}

#[test]
fn bitmap_modes_show_only_tiles_from_512() {
    let mut bus = ready();
    solid_tile(&mut bus, 0, 1);
    solid_tile(&mut bus, 512, 1);
    sprite(&mut bus, 0, 0, 0, 0);
    sprite(&mut bus, 1, 0, 16, 512);
    bus.write16(DISPCNT, 3 | OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(0));
    assert_eq!(pixel(&bus, 16, 0), rgba(RED));
}

const AFFINE: u16 = 1 << 8;
const DOUBLE: u16 = 1 << 9;

fn matrix(bus: &mut Bus, group: u32, values: [i16; 4]) {
    for (i, value) in values.into_iter().enumerate() {
        bus.write16(OAM + group * 32 + 6 + i as u32 * 8, value as u16);
    }
}

fn marked_tile(bus: &mut Bus, tile: u32) {
    for row in 0..8 {
        bus.write16(OBJ_VRAM + tile * 32 + row * 4, 0x2221);
        bus.write16(OBJ_VRAM + tile * 32 + row * 4 + 2, 0x2222);
    }
}

fn affine_ready() -> Bus {
    let mut bus = ready();
    color(&mut bus, 2, GREEN);
    marked_tile(&mut bus, 1);
    bus.write16(DISPCNT, OBJ);
    bus
}

#[test]
fn an_affine_sprite_with_the_identity_matrix_draws_like_a_normal_one() {
    let mut bus = affine_ready();
    matrix(&mut bus, 0, [0x100, 0, 0, 0x100]);
    sprite(&mut bus, 0, 10 | AFFINE, 20, 1);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 20, 10), rgba(RED));
    assert_eq!(pixel(&bus, 21, 10), rgba(GREEN));
    assert_eq!(pixel(&bus, 27, 17), rgba(GREEN));
    assert_eq!(pixel(&bus, 28, 10), rgba(0));
    assert_eq!(pixel(&bus, 20, 18), rgba(0));
}

#[test]
fn the_matrix_comes_from_the_parameter_group_not_the_flip_bits() {
    let mut bus = affine_ready();
    matrix(&mut bus, 0, [0x100, 0, 0, 0x100]);
    matrix(&mut bus, 24, [-0x100, 0, 0, 0x100]);
    sprite(&mut bus, 0, AFFINE, 24 << 9, 1);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(0));
    assert_eq!(pixel(&bus, 1, 0), rgba(GREEN));
}

#[test]
fn double_size_centres_the_sprite_in_twice_its_area() {
    let mut bus = affine_ready();
    matrix(&mut bus, 0, [0x100, 0, 0, 0x100]);
    sprite(&mut bus, 0, AFFINE | DOUBLE, 0, 1);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 3, 4), rgba(0));
    assert_eq!(pixel(&bus, 4, 4), rgba(RED));
    assert_eq!(pixel(&bus, 11, 11), rgba(GREEN));
    assert_eq!(pixel(&bus, 12, 11), rgba(0));
    matrix(&mut bus, 0, [0x80, 0, 0, 0x80]);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    assert_eq!(pixel(&bus, 1, 15), rgba(RED));
    assert_eq!(pixel(&bus, 2, 0), rgba(GREEN));
    assert_eq!(pixel(&bus, 15, 15), rgba(GREEN));
}

#[test]
fn without_double_size_a_scaled_sprite_is_clipped_to_its_size() {
    let mut bus = affine_ready();
    matrix(&mut bus, 0, [0x80, 0, 0, 0x80]);
    sprite(&mut bus, 0, AFFINE, 0, 1);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
    assert_eq!(pixel(&bus, 7, 7), rgba(GREEN));
    assert_eq!(pixel(&bus, 8, 0), rgba(0));
}

#[test]
fn rotation_maps_rows_to_columns() {
    let mut bus = affine_ready();
    matrix(&mut bus, 0, [0, 0x100, -0x100, 0]);
    sprite(&mut bus, 0, AFFINE, 0, 1);
    draw(&mut bus);
    for x in 1..8 {
        assert_eq!(pixel(&bus, x, 0), rgba(RED), "{x}");
        assert_eq!(pixel(&bus, x, 7), rgba(GREEN), "{x}");
    }
    assert_eq!(pixel(&bus, 0, 3), rgba(0));
}
