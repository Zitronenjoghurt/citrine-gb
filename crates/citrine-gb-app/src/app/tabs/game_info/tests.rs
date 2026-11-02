use super::parts::{card_with_margin, type_text_color};
use super::sprites::cut_out;
use super::status::{Clock, Status, status_line};
use super::*;
use crate::game_info::GameInfo;
use citrine_gb_gamedata::fixtures::{
    crystal_cartridge, crystal_live_wram, crystal_save, set_crystal_live_money,
};

const NOON: i64 = 1_761_912_000;

#[test]
fn type_tags_pick_the_text_colour_with_more_contrast() {
    assert_eq!(type_text_color(0), Color32::from_rgb(0x1a, 0x1a, 0x1a));
    assert_eq!(type_text_color(8), Color32::WHITE);
    assert_eq!(type_text_color(1), Color32::WHITE);
}

#[test]
fn selecting_a_card_keeps_its_content_in_place() {
    let ctx = egui::Context::default();
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            assert_eq!(
                card(ui, true).total_margin(),
                card(ui, false).total_margin()
            );
            assert_eq!(
                card_with_margin(ui, true, 2, 4).total_margin(),
                card_with_margin(ui, false, 2, 4).total_margin()
            );
        });
    });
}

fn clock() -> Clock {
    Clock::in_zone(
        jiff::tz::TimeZone::UTC,
        jiff::Timestamp::from_second(NOON).unwrap(),
    )
}

fn crystal(saved_at: Option<u64>) -> GameInfo {
    let mut info = GameInfo::default();
    info.set_rom(Some("crystal"), &crystal_cartridge(), saved_at);
    info
}

#[test]
fn the_line_says_whether_the_live_game_is_saved() {
    let save = crystal_save();
    let mut wram = crystal_live_wram(&save);
    let mut info = crystal(Some(NOON as u64 - 3600));
    info.read(&save, &wram, NOON as u64);
    assert_eq!(
        status_line(info.game().unwrap(), &clock()),
        (Status::Saved, "Playing, saved in game at 11:00".to_owned())
    );

    set_crystal_live_money(&mut wram, 4815);
    info.read(&save, &wram, NOON as u64);
    assert_eq!(
        status_line(info.game().unwrap(), &clock()),
        (
            Status::Behind,
            "Playing, not saved in game since 11:00".to_owned()
        )
    );
}

#[test]
fn the_line_shows_the_save_file_and_its_day_when_the_game_is_not_running() {
    let mut info = crystal(Some(NOON as u64 - 3 * 86_400));
    info.read(&crystal_save(), &vec![0u8; 0x2000], NOON as u64);
    assert_eq!(
        status_line(info.game().unwrap(), &clock()),
        (Status::File, "Save file from 28 Oct, 12:00".to_owned())
    );

    let mut empty = crystal(None);
    empty.read(&vec![0u8; 0x8000], &vec![0u8; 0x2000], NOON as u64);
    assert_eq!(
        status_line(empty.game().unwrap(), &clock()),
        (Status::File, "No saved game yet".to_owned())
    );
}

#[test]
fn a_sprite_is_clear_outside_its_outline_and_solid_inside() {
    let ring = |x: usize, y: usize| -> u8 {
        let edge = (1..=4).contains(&x) && (1..=4).contains(&y);
        let inner = (2..=3).contains(&x) && (2..=3).contains(&y);
        if edge && !inner { 3 } else { 0 }
    };
    let image = cut_out(6, 6, ring, |value| [value, value, value]);
    let alpha = |x: usize, y: usize| image.pixels[y * 6 + x].a();
    assert_eq!(alpha(0, 0), 0);
    assert_eq!(alpha(5, 3), 0);
    assert_eq!(alpha(1, 1), 0xFF);
    assert_eq!(alpha(2, 2), 0xFF);
    assert_eq!(alpha(3, 3), 0xFF);
}
