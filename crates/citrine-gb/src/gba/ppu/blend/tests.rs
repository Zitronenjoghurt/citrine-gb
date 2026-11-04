use super::{Blend, Effect};
use crate::gba::bus::Bus;
use crate::gba::ppu::rgba;
use crate::gba::ppu::tests::{bus, draw, pixel, solid_layers};

const DISPCNT: u32 = 0x0400_0000;
const BG0CNT: u32 = 0x0400_0008;
const BG1CNT: u32 = 0x0400_000A;
const BLDCNT: u32 = 0x0400_0050;
const BLDALPHA: u32 = 0x0400_0052;
const BLDY: u32 = 0x0400_0054;
const PALETTE: u32 = 0x0500_0000;
const OAM: u32 = 0x0700_0000;
const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;
const LAYERS: u16 = 0b11 << 8;
const OBJ: u16 = 1 << 12;
const SEMI: u16 = 1 << 10;

fn blend(control: u16, alpha: u16, brightness: u16) -> Blend {
    let mut blend = Blend::default();
    blend.write(0, control, 0xFFFF);
    blend.write(2, alpha, 0xFFFF);
    blend.write(4, brightness, 0xFFFF);
    blend
}

#[test]
fn alpha_weighs_each_channel_in_sixteenths_and_saturates() {
    let half = blend(0, 8 | 8 << 8, 0);
    assert_eq!(half.alpha(RED, GREEN), 15 | 15 << 5);
    let full = blend(0, 16 | 16 << 8, 0);
    assert_eq!(full.alpha(0x7FFF, 0x7FFF), 0x7FFF);
    let over = blend(0, 31 | 31 << 8, 0);
    assert_eq!(over.alpha(10, 10), 20);
    let top_only = blend(0, 16, 0);
    assert_eq!(top_only.alpha(RED | 0x8000, BLUE), RED);
}

#[test]
fn brighten_and_darken_move_each_channel_by_evy_sixteenths() {
    let quarter = blend(0, 0, 4);
    assert_eq!(quarter.brighten(15), 19 | 7 << 5 | 7 << 10);
    assert_eq!(quarter.darken(15), 12);
    let over = blend(0, 0, 31);
    assert_eq!(over.brighten(RED), 0x7FFF);
    assert_eq!(over.darken(0x7FFF), 0);
}

#[test]
fn bldcnt_picks_the_effect_and_targets() {
    let blend = blend(0b10 << 6 | 1 << 2 | 1 << 13, 0, 0);
    assert_eq!(blend.effect(), Effect::Brighten);
    assert!(blend.first(2) && !blend.first(0));
    assert!(blend.second(5) && !blend.second(2));
}

fn ready() -> Bus {
    let mut bus = bus();
    solid_layers(&mut bus);
    bus.write16(OAM, 1 << 9);
    bus.write16(BG0CNT, 8 << 8);
    bus.write16(BG1CNT, 9 << 8 | 1);
    bus.write16(BLDALPHA, 8 | 8 << 8);
    bus.write16(DISPCNT, LAYERS);
    bus
}

#[test]
fn alpha_blends_a_first_target_over_a_second_target() {
    let mut bus = ready();
    for (control, expected) in [
        (1 | 1 << 6 | 2 << 8, 15 | 15 << 5),
        (1 | 1 << 6, RED),
        (2 | 1 << 6 | 2 << 8, RED),
        (1 | 2 << 8, RED),
    ] {
        bus.write16(BLDCNT, control);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(expected), "{control:#x}");
    }
}

#[test]
fn the_backdrop_can_be_the_second_target() {
    let mut bus = ready();
    bus.write16(PALETTE, BLUE);
    bus.write16(DISPCNT, 1 << 8);
    bus.write16(BLDCNT, 1 | 1 << 6 | 1 << 13);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(15 | 15 << 10));
}

#[test]
fn brightness_applies_to_first_targets_only() {
    let mut bus = ready();
    bus.write16(BLDY, 16);
    bus.write16(BLDCNT, 1 | 3 << 6);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(0));
    bus.write16(BLDCNT, 2 | 3 << 6);
    draw(&mut bus);
    assert_eq!(pixel(&bus, 0, 0), rgba(RED));
}

#[test]
fn a_semi_transparent_sprite_blends_with_a_second_target_whatever_the_effect() {
    let mut bus = ready();
    bus.write16(OAM, SEMI);
    bus.write16(OAM + 4, 0);
    bus.write16(DISPCNT, LAYERS | OBJ);
    bus.write16(BLDY, 16);
    for (control, expected) in [
        (1 << 8, 15 << 10 | 15),
        (1 << 8 | 2 << 6, 15 << 10 | 15),
        (2 << 6, 0x7FFF),
        (0, BLUE),
    ] {
        bus.write16(BLDCNT, control);
        draw(&mut bus);
        assert_eq!(pixel(&bus, 0, 0), rgba(expected), "{control:#x}");
    }
}

#[test]
fn bldcnt_and_bldalpha_read_back_and_bldy_is_write_only() {
    let mut bus = ready();
    bus.write16(BLDCNT, 0xFFFF);
    bus.write16(BLDALPHA, 0xFFFF);
    bus.write16(BLDY, 0xFFFF);
    assert_eq!(bus.read16(BLDCNT), 0x3FFF);
    assert_eq!(bus.read16(BLDALPHA), 0x1F1F);
    assert_eq!(bus.read16(BLDY), 0);
}
