use crate::gb::ppu::types::color::RGBA;
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;

const PROGRAM: [u8; 32] = [
    0xF0, 0x44, 0xFE, 0x90, 0x20, 0xFA, 0x3E, 0x00, 0xE0, 0x40, 0x3E, 0xFF, 0xE0, 0x47, 0x3E, 0x80,
    0xE0, 0x68, 0x3E, 0x00, 0xE0, 0x69, 0xE0, 0x69, 0x3E, 0x91, 0xE0, 0x40, 0x18, 0xFE, 0x00, 0x00,
];

fn lcd_on_rom(cgb: bool) -> Rom {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    if cgb {
        data[0x0143] = 0x80;
    }
    data[0x0150..0x0150 + PROGRAM.len()].copy_from_slice(&PROGRAM);
    Rom::new(&data)
}

fn next_frame(gb: &mut GameBoy) -> Vec<RGBA> {
    gb.ppu.frame_ready = false;
    while !gb.ppu.frame_ready {
        gb.step();
    }
    let frame = gb.ppu.frame();
    (0..144)
        .flat_map(|y| (0..160).map(move |x| (x, y)))
        .map(|(x, y)| frame.get_xy(x, y))
        .collect()
}

fn first_frames_after_lcd_on(model: GbModel) -> (RGBA, Vec<RGBA>, Vec<RGBA>) {
    let mut gb = GameBoy::new_empty(model);
    gb.load_rom(&lcd_on_rom(model.is_cgb())).expect("load");
    while gb.ppu.lcdc.lcd_enabled {
        gb.step();
    }
    let shown = gb.ppu.frame().get_xy(0, 0);
    while !gb.ppu.lcdc.lcd_enabled {
        gb.step();
    }
    let first = next_frame(&mut gb);
    let second = next_frame(&mut gb);
    (shown, first, second)
}

#[test]
fn dmg_shows_the_first_frame_after_lcd_on_blank() {
    let (_, first, second) = first_frames_after_lcd_on(GbModel::Dmg);
    let gb = GameBoy::new_empty(GbModel::Dmg);
    let blank = gb.ppu.backdrop_color();
    assert!(first.iter().all(|&px| px == blank));
    assert!(second.iter().all(|&px| px != blank));
}

#[test]
fn cgb_repeats_the_frame_before_lcd_on() {
    let (shown, first, second) = first_frames_after_lcd_on(GbModel::Cgb);
    let black = RGBA::rgb(0, 0, 0);
    assert_ne!(shown, black);
    assert!(first.iter().all(|&px| px == shown));
    assert!(second.iter().all(|&px| px == black));
}
