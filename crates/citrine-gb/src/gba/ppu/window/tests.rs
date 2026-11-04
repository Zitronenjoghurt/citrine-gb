use crate::gba::bus::Bus;
use crate::gba::ppu::rgba;
use crate::gba::ppu::tests::{bus, draw, pixel, solid_layers, to_line};

const DISPCNT: u32 = 0x0400_0000;
const BG0CNT: u32 = 0x0400_0008;
const BG1CNT: u32 = 0x0400_000A;
const WIN0H: u32 = 0x0400_0040;
const WIN1H: u32 = 0x0400_0042;
const WIN0V: u32 = 0x0400_0044;
const WIN1V: u32 = 0x0400_0046;
const WININ: u32 = 0x0400_0048;
const WINOUT: u32 = 0x0400_004A;
const BLDCNT: u32 = 0x0400_0050;
const BLDY: u32 = 0x0400_0054;
const OAM: u32 = 0x0700_0000;
const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;
const WHITE: u16 = 0x7FFF;
const LAYERS: u16 = 0b11 << 8;
const OBJ: u16 = 1 << 12;
const WIN0: u16 = 1 << 13;
const WIN1: u16 = 1 << 14;
const OBJ_WINDOW: u16 = 1 << 15;
const SHOW_BG0: u16 = 1;
const SHOW_BG1: u16 = 2;
const SHOW_OBJ: u16 = 1 << 4;
const SHOW_EFFECTS: u16 = 1 << 5;

fn ready() -> Bus {
    let mut bus = bus();
    solid_layers(&mut bus);
    bus.write16(OAM, 1 << 9);
    bus.write16(BG0CNT, 8 << 8);
    bus.write16(BG1CNT, 9 << 8 | 1);
    bus
}

#[test]
fn without_windows_every_layer_shows() {
    let mut bus = ready();
    bus.write16(WINOUT, 0);
    bus.write16(DISPCNT, LAYERS);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 100, 100), rgba(RED));
}

#[test]
fn win0_shows_its_layers_inside_its_rectangle_and_winout_outside() {
    let mut bus = ready();
    bus.write16(WIN0H, 8 << 8 | 16);
    bus.write16(WIN0V, 4 << 8 | 12);
    bus.write16(WININ, SHOW_BG1);
    bus.write16(WINOUT, SHOW_BG0);
    bus.write16(DISPCNT, LAYERS | WIN0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 8, 4), rgba(GREEN));
    assert_eq!(pixel(&bus, 15, 11), rgba(GREEN));
    for (x, y) in [(7, 4), (16, 4), (8, 3), (8, 12)] {
        assert_eq!(pixel(&bus, x, y), rgba(RED), "{x},{y}");
    }
}

#[test]
fn a_window_that_shows_nothing_shows_the_backdrop() {
    let mut bus = ready();
    bus.write16(WINOUT, 0);
    bus.write16(DISPCNT, LAYERS | WIN0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 100, 100), rgba(0));
}

#[test]
fn win0_wins_over_win1_where_they_overlap() {
    let mut bus = ready();
    bus.write16(WIN0H, 16);
    bus.write16(WIN0V, 16);
    bus.write16(WIN1H, 32);
    bus.write16(WIN1V, 32);
    bus.write16(WININ, SHOW_BG0 | SHOW_BG1 << 8);
    bus.write16(WINOUT, 0);
    bus.write16(DISPCNT, LAYERS | WIN0 | WIN1);
    draw(&mut bus);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    assert_eq!(pixel(&bus, 20, 0), rgba(GREEN));
    assert_eq!(pixel(&bus, 40, 0), rgba(0));
}

#[test]
fn a_right_edge_left_of_the_left_edge_or_past_240_reaches_the_screen_edge() {
    for h in [200 << 8 | 10, 200 << 8 | 250] {
        let mut bus = ready();
        bus.write16(WININ, SHOW_BG1);
        bus.write16(WINOUT, SHOW_BG0);
        bus.write16(DISPCNT, LAYERS | WIN0);
        bus.write16(WIN0H, h);
        bus.write16(WIN0V, 150 << 8 | 200);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 239, 159), rgba(GREEN), "{h:#x}");
        assert_eq!(pixel(&bus, 5, 159), rgba(RED), "{h:#x}");
        assert_eq!(pixel(&bus, 239, 5), rgba(RED), "{h:#x}");
    }
}

#[test]
fn a_window_opens_on_its_top_line_and_stays_open_until_its_bottom_line() {
    let mut bus = ready();
    bus.write16(WIN0H, 240);
    bus.write16(WIN0V, 150 << 8 | 20);
    bus.write16(WININ, SHOW_BG1);
    bus.write16(WINOUT, SHOW_BG0);
    bus.write16(DISPCNT, LAYERS | WIN0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 5), rgba(RED));
    assert_eq!(pixel(&bus, 0, 150), rgba(GREEN));
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 5), rgba(GREEN));
    assert_eq!(pixel(&bus, 0, 19), rgba(GREEN));
    assert_eq!(pixel(&bus, 0, 20), rgba(RED));
    assert_eq!(pixel(&bus, 0, 149), rgba(RED));
}

#[test]
fn a_top_line_that_has_already_passed_opens_the_window_only_next_frame() {
    let mut bus = ready();
    bus.write16(WIN0H, 240);
    bus.write16(WININ, SHOW_BG1);
    bus.write16(WINOUT, SHOW_BG0);
    bus.write16(DISPCNT, LAYERS | WIN0);
    to_line(&mut bus, 50);
    bus.write16(WIN0V, 40 << 8 | 100);
    to_line(&mut bus, 0);
    assert_eq!(pixel(&bus, 0, 60), rgba(RED));
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 39), rgba(RED));
    assert_eq!(pixel(&bus, 0, 60), rgba(GREEN));
    assert_eq!(pixel(&bus, 0, 100), rgba(RED));
}

#[test]
fn the_object_window_is_the_opaque_pixels_of_window_sprites() {
    let mut bus = ready();
    bus.write16(OAM, 2 << 10);
    bus.write16(OAM + 2, 0);
    bus.write16(OAM + 4, 0);
    bus.write16(WINOUT, SHOW_BG0 | SHOW_BG1 << 8);
    bus.write16(DISPCNT, LAYERS | OBJ | OBJ_WINDOW);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(GREEN));
    assert_eq!(pixel(&bus, 7, 7), rgba(GREEN));
    assert_eq!(pixel(&bus, 8, 0), rgba(RED));
    bus.write16(DISPCNT, LAYERS | OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
}

#[test]
fn windows_hide_sprites_through_the_obj_bit() {
    let mut bus = ready();
    bus.write16(OAM, 0);
    bus.write16(OAM + 4, 0);
    bus.write16(WINOUT, SHOW_BG0);
    bus.write16(DISPCNT, LAYERS | OBJ | WIN0);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    bus.write16(WINOUT, SHOW_BG0 | SHOW_OBJ);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(BLUE));
}

#[test]
fn the_effects_bit_switches_blending_per_region() {
    let mut bus = ready();
    bus.write16(BLDCNT, 1 | 2 << 6);
    bus.write16(BLDY, 16);
    bus.write16(WIN0H, 16);
    bus.write16(WIN0V, 16);
    bus.write16(WININ, SHOW_BG0);
    bus.write16(WINOUT, SHOW_BG0 | SHOW_EFFECTS);
    bus.write16(DISPCNT, LAYERS | WIN0);
    draw(&mut bus);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
    assert_eq!(pixel(&bus, 20, 0), rgba(WHITE));
}

#[test]
fn winin_and_winout_read_back_six_bits_per_region_and_the_edges_are_write_only() {
    let mut bus = ready();
    bus.write16(WININ, 0xFFFF);
    bus.write16(WINOUT, 0xFFFF);
    bus.write16(WIN0H, 0x1234);
    assert_eq!(bus.read16(WININ), 0x3F3F);
    assert_eq!(bus.read16(WINOUT), 0x3F3F);
    assert_eq!(bus.read16(WIN0H), 0);
    bus.write8(WINOUT + 1, 0x01);
    assert_eq!(bus.read16(WINOUT), 0x013F);
}
