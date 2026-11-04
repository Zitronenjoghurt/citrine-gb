use super::bar::{LineWidths, MIN_NAME, Tail, bar_line, fixed_label, line_widths};
use super::lines::{other_game_line, quality, short_name};
use super::*;
use crate::play::Ending;

#[test]
fn the_friends_game_comes_first_then_borrowing_then_the_cable_then_watching() {
    assert_eq!(fit(true, true, true, true), Fit::Open);
    assert_eq!(fit(false, true, true, true), Fit::Shelf);
    assert_eq!(fit(false, false, true, true), Fit::Borrow);
    assert_eq!(fit(false, false, false, true), Fit::Cable);
    assert_eq!(fit(false, false, false, false), Fit::Watch);
}

#[test]
fn connection_quality_has_three_words() {
    assert_eq!(quality(42, true).0, "Good");
    assert_eq!(quality(80, true).0, "OK");
    assert_eq!(quality(149, true).0, "OK");
    assert_eq!(quality(150, true).0, "Slow");
}

#[test]
fn the_short_name_is_the_first_word() {
    assert_eq!(short_name("Mia Sommer"), "Mia");
    assert_eq!(short_name("Jonas"), "Jonas");
    assert_eq!(short_name(""), "");
}

#[test]
fn an_ending_says_only_what_happened() {
    assert_eq!(
        ending_line("Mia", Ending::FriendLeft, true).as_deref(),
        Some("Mia left.")
    );
    assert_eq!(
        ending_line("Mia", Ending::Dropped, true).as_deref(),
        Some("Lost the connection to Mia.")
    );
    assert_eq!(
        ending_line("Mia", Ending::Failed, true).as_deref(),
        Some("Playing with Mia stopped.")
    );
    assert_eq!(
        ending_line("Mia", Ending::FriendLeft, false).as_deref(),
        Some("Mia left.")
    );
    assert_eq!(ending_line("Mia", Ending::Left, true), None);
    assert_eq!(
        ending_line("Mia", Ending::Declined, false).as_deref(),
        Some("Mia declined your invite.")
    );
}

#[test]
fn a_console_handed_back_in_a_room_shows_the_resume_banner() {
    let mut playing = PlayingUi::default();
    let jonas = OnlineId::random();
    playing.ended = Some(Ended {
        friend: jonas,
        line: "Jonas left.".into(),
        paused: false,
    });
    let mia = OnlineId::random();
    playing.handed_back(mia, "Mia");
    assert_eq!(
        playing.ended,
        Some(Ended {
            friend: mia,
            line: "Lost the connection to Mia.".into(),
            paused: true,
        })
    );
}

#[test]
fn the_other_game_line_gives_the_reason_and_a_number() {
    assert_eq!(
        other_game_line("Mia", false, "Petris", "Petris DX", true, Some(238)),
        "You don't have Petris DX, so you can't watch Mia's screen. \
         Each link cable exchange takes 238 ms."
    );
    assert_eq!(
        other_game_line("Mia", true, "Petris", "Petris DX", true, None),
        "Mia doesn't have Petris, so you can't watch each other's screens."
    );
    assert_eq!(
        other_game_line("Mia", false, "Petris", "Petris DX", false, None),
        "You don't have Petris DX, so you can't watch Mia's screen. \
         This relay doesn't share games."
    );
}

fn line_right_edge(width: f32, name: &str, title: &str) -> (f32, f32) {
    let ctx = egui::Context::default();
    let mut right = 0.0;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width + 100.0, 200.0),
        )),
        ..Default::default()
    };
    let mut start = 0.0;
    let _ = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.set_max_width(width);
            start = ui.min_rect().left();
            ui.horizontal(|ui| {
                ui.set_max_width(width);
                let tail = Tail {
                    full: "Good \u{b7} 42 ms".into(),
                    short: "Good".into(),
                    hover: Some("42 ms".into()),
                };
                bar_line(ui, name, Some(title), tail);
                right = ui.min_rect().right();
            });
        });
    });
    (start, right)
}

#[test]
fn the_bar_line_never_runs_past_its_room() {
    for width in [20.0, 60.0, 120.0, 200.0, 320.0, 900.0] {
        let (start, right) =
            line_right_edge(width, "Maximiliane", "Pok\u{e9}mon - Goldene Edition");
        assert!(
            right - start <= width + 0.5,
            "{width}: line ends at {}",
            right - start
        );
    }
}

#[test]
fn a_shortened_name_fills_its_room_and_shows_whole_on_hover() {
    let ctx = egui::Context::default();
    let mut shown = None;
    for _ in 0..2 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(40.0);
                    let response = fixed_label(ui, "Maximiliane".into(), 44.0);
                    shown = response.map(|r| r.rect.width());
                    assert!(fixed_label(ui, "Maximiliane".into(), 4.0).is_none());
                });
            });
        });
    }
    let width = shown.unwrap();
    assert!(width > 30.0 && width <= 44.0, "the name took {width}");
}

#[test]
fn the_connection_word_keeps_its_room_and_names_shorten() {
    let fits = line_widths(400.0, 8.0, 60.0, 200.0, 90.0, 40.0);
    assert_eq!(
        fits,
        LineWidths {
            name: 60.0,
            title: 200.0,
            tail: 90.0,
            full_tail: true
        }
    );
    let squeezed = line_widths(216.0, 8.0, 60.0, 200.0, 90.0, 40.0);
    assert!(squeezed.full_tail);
    assert!((squeezed.name - 55.0).abs() < 1e-3);
    assert!((squeezed.title - 55.0).abs() < 1e-3);
    let narrow = line_widths(152.0, 8.0, 60.0, 200.0, 90.0, 40.0);
    assert!(!narrow.full_tail);
    assert_eq!(narrow.tail, 40.0);
    assert!(narrow.name + narrow.title + narrow.tail + 16.0 <= 152.0 + 1e-3);
    assert!(narrow.name >= MIN_NAME && narrow.title >= MIN_NAME);
    let no_title = line_widths(90.0, 8.0, 60.0, 200.0, 90.0, 40.0);
    assert_eq!((no_title.title, no_title.tail), (0.0, 40.0));
    assert!((no_title.name - 42.0).abs() < 1e-3);
    let word_only = line_widths(60.0, 8.0, 60.0, 0.0, 90.0, 40.0);
    assert_eq!((word_only.name, word_only.tail), (0.0, 40.0));
    let cut_word = line_widths(30.0, 8.0, 60.0, 0.0, 90.0, 40.0);
    assert_eq!((cut_word.name, cut_word.tail), (0.0, 30.0));
}

#[test]
fn the_friends_mute_toggles_and_a_silent_slider_comes_back_at_half() {
    let playing = FriendSound {
        volume: 0.7,
        muted: false,
    };
    assert_eq!(
        playing.toggled(),
        FriendSound {
            volume: 0.7,
            muted: true
        }
    );
    assert_eq!(playing.toggled().toggled(), playing);
    let zero = FriendSound {
        volume: 0.0,
        muted: false,
    };
    assert_eq!(
        zero.toggled(),
        FriendSound {
            volume: 0.5,
            muted: false
        }
    );
}
