use super::parts::{card_with_margin, type_text_color};
use super::sprites::cut_out;
use super::status::{Clock, Status, status_line};
use super::*;
use crate::game_info::GameInfo;
use citrine_gb_gamedata::Ram;
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
    info.read(&save, Ram::Gb(&wram), NOON as u64);
    assert_eq!(
        status_line(info.game().unwrap(), &clock()),
        (Status::Saved, "Playing, saved in game at 11:00".to_owned())
    );

    set_crystal_live_money(&mut wram, 4815);
    info.read(&save, Ram::Gb(&wram), NOON as u64);
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
    info.read(&crystal_save(), Ram::Gb(&[0u8; 0x2000]), NOON as u64);
    assert_eq!(
        status_line(info.game().unwrap(), &clock()),
        (Status::File, "Save file from 28 Oct, 12:00".to_owned())
    );

    let mut empty = crystal(None);
    empty.read(&vec![0u8; 0x8000], Ram::Gb(&[0u8; 0x2000]), NOON as u64);
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

#[test]
fn every_page_fits_a_narrow_desktop_panel() {
    let save = crystal_save();
    let mut info = crystal(Some(NOON as u64));
    info.read(&save, Ram::Gb(&crystal_live_wram(&save)), NOON as u64);
    let game = info.game().unwrap();
    let ctx = egui::Context::default();
    let mut wide = Vec::new();
    for width in [
        280.0, 300.0, 320.0, 340.0, 360.0, 380.0, 400.0, 419.0, 460.0, 500.0, 540.0, 580.0, 640.0,
    ] {
        for page in [
            Page::Map,
            Page::Party,
            Page::Journey,
            Page::Bag,
            Page::Pokedex,
        ] {
            let pages = Pages {
                key: "crystal",
                name: "POKEMON_SLV",
                touch: 24.0,
                game,
                mobile: false,
                clock: clock(),
            };
            let mut view = View::default();
            view.page = page;
            for frame in 0..2 {
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let rect = Rect::from_min_size(ui.min_rect().min, vec2(width, 4000.0));
                        let used = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                pages.show(ui, &mut view, None);
                            })
                            .response
                            .rect
                            .width();
                        if frame == 1 && used > width + 0.5 {
                            wide.push(format!("{page:?} takes {used} in a {width} panel"));
                        }
                    });
                });
            }
        }
    }
    assert!(wide.is_empty(), "{wide:#?}");
}

#[test]
fn details_with_long_facts_fit_a_narrow_desktop_panel() {
    use citrine_gb_gamedata::{Caught, Gender, Place, TimeOfDay};
    let save = crystal_save();
    let mut info = crystal(Some(NOON as u64));
    info.read(&save, Ram::Gb(&crystal_live_wram(&save)), NOON as u64);
    let game = info.game().unwrap();
    let tables = &game.tables;
    let state = game.state().unwrap();
    let mut mon = state
        .party
        .iter()
        .find_map(|pokemon| match pokemon {
            Pokemon::Hatched(mon) => Some(mon.clone()),
            Pokemon::Egg => None,
        })
        .unwrap();
    let place_len = |location: u8| {
        let caught = Caught {
            time_of_day: TimeOfDay::Morning,
            level: 100,
            ot_gender: Gender::Male,
            location,
        };
        match tables.caught_place(&caught) {
            Some(Place::Landmark(name)) => name.len(),
            _ => 0,
        }
    };
    let location = (0..=0x7D)
        .max_by_key(|location| place_len(*location))
        .unwrap();
    mon.caught = Some(Caught {
        time_of_day: TimeOfDay::Morning,
        level: 100,
        ot_gender: Gender::Male,
        location,
    });
    mon.held_item = (1..=u16::from(u8::MAX))
        .max_by_key(|item| tables.item_name(*item).map_or(0, |name| name.len()));
    mon.ot_name = "WWWWWWW".to_owned();
    mon.ot_id = state.trainer.id.wrapping_add(1);
    let ctx = egui::Context::default();
    let mut wide = Vec::new();
    for width in [
        260.0, 280.0, 300.0, 320.0, 360.0, 400.0, 460.0, 520.0, 600.0,
    ] {
        let pages = Pages {
            key: "crystal",
            name: "POKEMON_SLV",
            touch: 24.0,
            game,
            mobile: false,
            clock: clock(),
        };
        for frame in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let rect = Rect::from_min_size(ui.min_rect().min, vec2(width, 4000.0));
                    let used = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                            pages.details(ui, &mon);
                        })
                        .response
                        .rect
                        .width();
                    if frame == 1 && used > width + 0.5 {
                        wide.push(format!("details take {used} in a {width} panel"));
                    }
                });
            });
        }
    }
    assert!(wide.is_empty(), "{wide:#?}");
}

#[test]
fn long_facts_wrap_inside_a_narrow_card() {
    let facts = [
        ("Held item", "CHARCOAL".to_owned()),
        ("Hidden Power", "FLYING, power 70".to_owned()),
        ("Original trainer", "WWWWWWW, ID 24602".to_owned()),
        ("Caught", "NEW BARK TOWN in the morning, Lv 5".to_owned()),
    ];
    let ctx = egui::Context::default();
    let mut wide = Vec::new();
    for width in [200.0, 240.0, 280.0, 320.0, 360.0] {
        for frame in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let rect = Rect::from_min_size(ui.min_rect().min, vec2(width, 4000.0));
                    let used = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                            party::fact_rows(ui, &facts, |ui, value: &String| {
                                ui.add(egui::Label::new(value.as_str()).wrap());
                            });
                        })
                        .response
                        .rect
                        .width();
                    if frame == 1 && used > width + 0.5 {
                        wide.push(format!("facts take {used} in {width}"));
                    }
                });
            });
        }
    }
    assert!(wide.is_empty(), "{wide:#?}");
}

#[test]
fn the_trainer_card_keeps_its_height_on_every_page() {
    let save = crystal_save();
    let mut info = crystal(Some(NOON as u64));
    info.read(&save, Ram::Gb(&crystal_live_wram(&save)), NOON as u64);
    let game = info.game().unwrap();
    let state = game.state().unwrap();
    let pages = Pages {
        key: "crystal",
        name: "POKEMON_SLV",
        touch: 24.0,
        game,
        mobile: false,
        clock: clock(),
    };
    let goal = pages.goal_text(state).expect("next goal");
    let place = "ROUTE 38, Night".to_owned();
    let ctx = egui::Context::default();
    let mut moved = Vec::new();
    for width in [280.0, 320.0, 360.0, 400.0, 460.0, 540.0, 640.0, 800.0] {
        let mut heights = Vec::new();
        for last in [("Next goal", goal.clone()), ("Now at", place.clone())] {
            let mut height = 0.0;
            for _ in 0..2 {
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let rect = Rect::from_min_size(ui.min_rect().min, vec2(width, 4000.0));
                        height = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                pages.trainer(ui, state, Some(last.clone()));
                            })
                            .response
                            .rect
                            .height();
                    });
                });
            }
            heights.push(height);
        }
        if (heights[0] - heights[1]).abs() > 0.5 {
            moved.push(format!("{heights:?} at {width}"));
        }
    }
    assert!(moved.is_empty(), "{moved:#?}");
}
