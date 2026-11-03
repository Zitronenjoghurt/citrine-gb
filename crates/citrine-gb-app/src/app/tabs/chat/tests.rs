use super::bubble::{bubble_fill, long_pressed, receipt_text, stamp_mark};
use super::composer::{draft_field, enter_sends, icon_button};
use super::share::{percent, share_facts};
use super::*;
use crate::app::ui_state::palette::PaletteLibrary;
use crate::game_name::GameNames;
use crate::media;
use crate::online::chat::{Change, ChatEntry};
use citrine_gb_net::{Body, Share, ShareKind};
use egui::{Color32, Rect};
use jiff::tz::TimeZone;

#[test]
fn one_tick_on_the_relay_two_once_delivered_and_read_says_when() {
    let clock = Clock::new();
    let now = jiff::Timestamp::now().as_second() as u64;
    assert_eq!(stamp_mark(Delivery::Sent), Some(icons::CHECK));
    assert_eq!(stamp_mark(Delivery::Delivered(now)), Some(icons::CHECKS));
    assert_eq!(stamp_mark(Delivery::Read(now)), Some(icons::CHECKS));
    assert_eq!(stamp_mark(Delivery::Received), None);
    assert_eq!(stamp_mark(Delivery::Seen), None);
    assert_eq!(
        receipt_text(Delivery::Sent, &clock).as_deref(),
        Some("Sent")
    );
    assert_eq!(
        receipt_text(Delivery::Read(now), &clock),
        Some(format!("Read {}", clock.time(now)))
    );
    assert_eq!(
        receipt_text(Delivery::Delivered(now), &clock),
        Some(format!("Delivered {}", clock.time(now)))
    );
    let long_ago = now - 40 * 86_400;
    let text = receipt_text(Delivery::Read(long_ago), &clock).unwrap();
    assert!(text.starts_with("Read ") && text.contains(", "), "{text}");
    assert_eq!(receipt_text(Delivery::Seen, &clock), None);
}

#[test]
fn a_long_press_on_a_bubble_shows_its_receipt() {
    let ctx = egui::Context::default();
    let bubble = Rect::from_min_size(egui::pos2(10.0, 10.0), vec2(100.0, 40.0));
    let at = egui::pos2(40.0, 20.0);
    let touch = |phase, time| {
        let mut events = vec![egui::Event::Touch {
            device_id: egui::TouchDeviceId(1),
            id: egui::TouchId(1),
            phase,
            pos: at,
            force: None,
        }];
        if phase == egui::TouchPhase::Start {
            events.push(egui::Event::PointerMoved(at));
            events.push(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            });
        }
        egui::RawInput {
            time: Some(time),
            events,
            ..Default::default()
        }
    };
    let mut held = Vec::new();
    for (input, at) in [
        (touch(egui::TouchPhase::Start, 0.0), 0.0),
        (touch(egui::TouchPhase::Move, 0.2), 0.2),
        (
            egui::RawInput {
                time: Some(0.7),
                ..Default::default()
            },
            0.7,
        ),
    ] {
        let _ = ctx.run(input, |ctx| {
            held.push((at, ctx.input(|i| long_pressed(i, bubble))))
        });
    }
    assert_eq!(held, [(0.0, false), (0.2, false), (0.7, true)]);
    let elsewhere = Rect::from_min_size(egui::pos2(200.0, 10.0), vec2(100.0, 40.0));
    let later = egui::RawInput {
        time: Some(0.8),
        ..Default::default()
    };
    let _ = ctx.run(later, |ctx| {
        assert!(!ctx.input(|i| long_pressed(i, elsewhere)))
    });
}

fn share(kind: ShareKind, size: u64, length_ms: Option<u64>) -> Share {
    Share {
        file: NO_FILE,
        kind,
        size,
        game: "TETRIS".into(),
        rom_sha256: None,
        created_at: 0,
        width: 160,
        height: 144,
        length_ms,
        preview: Vec::new(),
    }
}

#[test]
fn share_cards_name_the_kind_length_and_size() {
    assert_eq!(
        share_facts(&share(ShareKind::Screenshot, 2_048, None)),
        format!("Screenshot · {}", media::format_size(2_048))
    );
    assert_eq!(
        share_facts(&share(ShareKind::Replay, 900, Some(83_000))),
        format!("Input replay · {}", media::format_length(83_000))
    );
}

fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

#[test]
fn message_text_reads_clearly_on_both_bubbles_in_both_themes() {
    for visuals in [egui::Visuals::light(), egui::Visuals::dark()] {
        for mine in [true, false] {
            let fill = bubble_fill(&visuals, mine);
            let back = if fill.a() == 255 {
                fill
            } else {
                let a = fill.a() as f32 / 255.0;
                let p = visuals.panel_fill;
                let mix = |f: u8, b: u8| (f as f32 + b as f32 * (1.0 - a)).min(255.0) as u8;
                Color32::from_rgb(
                    mix(fill.r(), p.r()),
                    mix(fill.g(), p.g()),
                    mix(fill.b(), p.b()),
                )
            };
            let (a, b) = (luminance(visuals.strong_text_color()), luminance(back));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(
                ratio >= 4.5,
                "dark {} mine {mine}: {ratio:.2}",
                visuals.dark_mode
            );
        }
    }
}

#[test]
fn progress_never_passes_a_hundred() {
    assert_eq!(percent(0, 0), 0);
    assert_eq!(percent(50, 200), 25);
    assert_eq!(percent(300, 200), 100);
}

#[test]
fn escape_closes_the_picker_before_the_chat() {
    let mut chat = ChatUi::default();
    chat.open(OnlineId::new(7).unwrap());
    chat.picker = true;
    assert!(chat.escape());
    assert_eq!(chat.current(), Some(OnlineId::new(7).unwrap()));
    assert!(chat.escape());
    assert_eq!(chat.current(), None);
    assert!(!chat.escape());
}

#[test]
fn days_read_as_today_yesterday_or_a_date() {
    let clock = Clock {
        zone: TimeZone::UTC,
        today: jiff::civil::date(2026, 10, 30),
    };
    assert_eq!(clock.day_label(jiff::civil::date(2026, 10, 30)), "Today");
    assert_eq!(
        clock.day_label(jiff::civil::date(2026, 10, 29)),
        "Yesterday"
    );
    assert_eq!(clock.day_label(jiff::civil::date(2026, 3, 2)), "2 Mar 2026");
}

fn shapes_of(entry: &ChatEntry, names: &GameNames, width: f32) -> Vec<egui::epaint::ClippedShape> {
    shapes_in(egui::Context::default(), entry, names, width)
}

fn shapes_in(
    ctx: egui::Context,
    entry: &ChatEntry,
    names: &GameNames,
    width: f32,
) -> Vec<egui::epaint::ClippedShape> {
    let online = Online::default();
    let mut state = ChatUi::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 600.0),
        )),
        ..Default::default()
    };
    let mut output = None;
    for _ in 0..2 {
        output = Some(ctx.run(input.clone(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                entry_ui(
                    ui,
                    &mut state,
                    &online,
                    OnlineId::new(7).unwrap(),
                    entry,
                    &test_clock(),
                    None,
                    names,
                    &PaletteLibrary::default(),
                    &mut Vec::new(),
                );
            });
        }));
    }
    output.unwrap().shapes
}

fn test_clock() -> Clock {
    Clock {
        zone: TimeZone::UTC,
        today: jiff::civil::date(2026, 10, 31),
    }
}

fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
    shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => {
                Some((text.galley.text().to_owned(), text.visual_bounding_rect()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn an_emoji_only_message_of_mine_sits_right_with_its_time_beside_it() {
    let entry = ChatEntry {
        id: 1,
        mine: true,
        at: 1_793_448_000,
        body: Body::Text("😋".into()),
        delivery: Delivery::Sent,
        change: Change::Kept,
    };
    let width = 400.0;
    let shapes = shapes_of(&entry, &GameNames::default(), width);
    let picture = shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                Some(mesh.calc_bounds())
            }
            _ => None,
        })
        .expect("the emoji picture");
    let time = test_clock().time(entry.at);
    let (_, stamp) = texts(&shapes)
        .into_iter()
        .find(|(text, _)| text.starts_with(&time))
        .expect("the time");
    assert!(picture.center().x > width / 2.0, "{picture:?}");
    assert!(stamp.left() >= picture.right(), "{stamp:?} {picture:?}");
    assert!(
        stamp.left() - picture.right() < 16.0,
        "{stamp:?} {picture:?}"
    );
    assert!(stamp.right() > width - 40.0, "{stamp:?}");
}

#[test]
fn a_short_message_of_mine_is_a_bubble_as_wide_as_its_words() {
    let entry = ChatEntry {
        id: 1,
        mine: true,
        at: 1_793_448_000,
        body: Body::Text("Hii".into()),
        delivery: Delivery::Sent,
        change: Change::Kept,
    };
    let width = 400.0;
    let shapes = shapes_of(&entry, &GameNames::default(), width);
    let (_, words) = texts(&shapes)
        .into_iter()
        .find(|(text, _)| text == "Hii")
        .expect("the words");
    let bubble = shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect)
                if rect.fill != Color32::TRANSPARENT && rect.rect.contains_rect(words) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the bubble");
    assert!(words.left() > width / 2.0, "{words:?}");
    assert!(words.left() - bubble.left() < 16.0, "{words:?} {bubble:?}");
    assert!(bubble.right() > width - 16.0, "{bubble:?}");
}

#[test]
fn a_long_message_of_mine_wraps_into_a_bubble_as_wide_as_its_longest_line() {
    let entry = ChatEntry {
        id: 1,
        mine: true,
        at: 1_793_448_000,
        body: Body::Text(
            "Are you free on Saturday? I finally got past the ice cave and want to trade".into(),
        ),
        delivery: Delivery::Sent,
        change: Change::Kept,
    };
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.spacing.interact_size.y = 44.0);
    let width = 300.0;
    let shapes = shapes_in(ctx, &entry, &GameNames::default(), width);
    let galley = shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.rows.len() > 1 => Some(text.galley.clone()),
            _ => None,
        })
        .expect("the wrapped words");
    let bubble = shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => Some(rect.rect),
            _ => None,
        })
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("the bubble");
    let inner = bubble.width() - BUBBLE_MARGIN.sum().x;
    assert!(inner - galley.size().x < 2.0, "{inner} {:?}", galley.size());
    assert!(bubble.right() > width - 16.0, "{bubble:?}");
    let rows = &galley.rows;
    let gap = rows[1].rect().top() - rows[0].rect().bottom();
    assert!(gap.abs() < 2.0, "{gap}");
}

#[test]
fn an_emoji_only_message_of_mine_ends_where_my_bubbles_end() {
    let entry = |id, text: &str| ChatEntry {
        id,
        mine: true,
        at: 1_793_448_000,
        body: Body::Text(text.into()),
        delivery: Delivery::Sent,
        change: Change::Kept,
    };
    for width in [412.0, 640.0] {
        let ctx = egui::Context::default();
        let text = shapes_in(ctx, &entry(1, "On my way"), &GameNames::default(), width);
        let bubble = text
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => Some(rect.rect),
                _ => None,
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .expect("the bubble");
        let ctx = egui::Context::default();
        let emoji = shapes_in(ctx, &entry(2, "😄"), &GameNames::default(), width);
        let right = emoji
            .iter()
            .map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => text.galley.rect.translate(text.pos.to_vec2()),
                shape => shape.visual_bounding_rect(),
            })
            .filter(|rect| rect.is_positive() && rect.width() < width / 2.0)
            .map(|rect| rect.right())
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (right - bubble.right()).abs() < 1.5,
            "{width}: {right} {bubble:?}"
        );
    }
}

#[test]
fn a_wrapped_bubble_ends_where_its_words_end() {
    let mut checked = 0;
    for mine in [false, true] {
        let entry = ChatEntry {
            id: 1,
            mine,
            at: 1_793_448_000,
            body: Body::Text(
                "Are you free on Saturday? I finally got past the ice cave and want to trade before the next gym"
                    .into(),
            ),
            delivery: if mine { Delivery::Sent } else { Delivery::Received },
            change: Change::Kept,
        };
        for width in (200..=520).step_by(4) {
            let width = width as f32;
            let ctx = egui::Context::default();
            ctx.style_mut(|style| style.spacing.interact_size.y = 44.0);
            let shapes = shapes_in(ctx, &entry, &GameNames::default(), width);
            let painted = texts(&shapes);
            let Some((_, words)) = painted.iter().find(|(text, _)| text.starts_with("Are")) else {
                panic!("{width}: the words");
            };
            let (_, stamp) = painted
                .iter()
                .find(|(text, _)| text.contains(':'))
                .expect("the time");
            if words.height() < 30.0 {
                continue;
            }
            checked += 1;
            let bubble = shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => Some(rect.rect),
                    _ => None,
                })
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .expect("the bubble");
            let inner = bubble.right() - BUBBLE_MARGIN.right as f32;
            let end = words.right().max(stamp.right());
            assert!(
                (inner - end).abs() < 1.5,
                "{mine} {width}: {bubble:?} {words:?}"
            );
            assert!(
                words.left() - bubble.left() >= BUBBLE_MARGIN.left as f32 - 1.5,
                "{mine} {width}: {bubble:?} {words:?}"
            );
            if mine {
                assert!(bubble.right() > width - 16.0, "{width}: {bubble:?}");
            }
        }
    }
    assert!(checked > 20, "{checked}");
}

#[test]
fn an_icon_button_in_the_message_row_has_its_icon_in_the_middle() {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(412.0, 200.0),
        )),
        ..Default::default()
    };
    let mut button = None;
    let output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                button = Some(icon_button(ui, egui::Button::new(icons::SMILEY), 44.0).rect);
            });
        });
    });
    let button = button.expect("the button");
    let icon = texts(&output.shapes)
        .into_iter()
        .find(|(text, _)| text == icons::SMILEY)
        .expect("the icon")
        .1;
    assert!(
        (icon.center().y - button.center().y).abs() < 2.0,
        "{icon:?} {button:?}"
    );
}

#[test]
fn the_time_in_my_bubble_reads_in_dark_mode() {
    let entry = ChatEntry {
        id: 1,
        mine: true,
        at: 1_793_448_000,
        body: Body::Text("See you at 3".into()),
        delivery: Delivery::Sent,
        change: Change::Kept,
    };
    let ctx = egui::Context::default();
    ctx.set_visuals(egui::Visuals::dark());
    let shapes = shapes_in(ctx, &entry, &GameNames::default(), 412.0);
    let fill = bubble_fill(&egui::Visuals::dark(), true);
    let stamp = shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().contains(':') => {
                text.galley.job.sections.first().map(|s| s.format.color)
            }
            _ => None,
        })
        .expect("the time");
    let shown = blend(stamp, fill);
    let ratio = contrast(shown, fill);
    assert!(ratio >= 4.5, "{ratio} {stamp:?} on {fill:?}");
}

#[test]
fn read_ticks_stand_out_from_delivered_ones_in_my_bubble() {
    let tick = |visuals: &egui::Visuals, delivery| {
        let entry = ChatEntry {
            id: 1,
            mine: true,
            at: 1_793_448_000,
            body: Body::Text("See you at 3".into()),
            delivery,
            change: Change::Kept,
        };
        let ctx = egui::Context::default();
        ctx.set_visuals(visuals.clone());
        shapes_in(ctx, &entry, &GameNames::default(), 412.0)
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().contains(icons::CHECKS) => {
                    text.galley.job.sections.last().map(|s| s.format.color)
                }
                _ => None,
            })
            .expect("the ticks")
    };
    for visuals in [egui::Visuals::light(), egui::Visuals::dark()] {
        let fill = bubble_fill(&visuals, true);
        let read = blend(tick(&visuals, Delivery::Read(1_793_448_060)), fill);
        let delivered = blend(tick(&visuals, Delivery::Delivered(1_793_448_060)), fill);
        assert!(contrast(read, fill) >= 4.5, "{read:?} on {fill:?}");
        assert!(
            contrast(read, delivered) >= 2.0,
            "{read:?} vs {delivered:?}"
        );
    }
}

fn blend(top: Color32, under: Color32) -> Color32 {
    let a = top.a() as f32 / 255.0;
    let mix = |t: u8, u: u8| (t as f32 + u as f32 * (1.0 - a)).round().min(255.0) as u8;
    Color32::from_rgb(
        mix(top.r(), under.r()),
        mix(top.g(), under.g()),
        mix(top.b(), under.b()),
    )
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let lin = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn every_message_fits_a_thin_panel_and_its_time_stays_on_one_line() {
    let entry = |id, mine, body, delivery| ChatEntry {
        id,
        mine,
        at: 1_793_448_000,
        body,
        delivery,
        change: Change::Kept,
    };
    let text = |t: &str| Body::Text(t.into());
    let mut received = share(ShareKind::Screenshot, 2_048, None);
    received.file = FileId([1; 16]);
    received.game = "Pokémon - Silver Version (USA, Europe)".into();
    let entries = [
        entry(1, true, text("😄😄😄"), Delivery::Read(1_793_448_060)),
        entry(2, false, text("😄😄😄😄😄😄"), Delivery::Received),
        entry(3, true, text("Hii"), Delivery::Delivered(1_793_448_060)),
        entry(
            4,
            true,
            text("Are you free on Saturday? I finally got past the ice cave"),
            Delivery::Sent,
        ),
        entry(5, false, Body::Share(received), Delivery::Received),
        entry(
            6,
            true,
            Body::Share(share(ShareKind::Recording, 2_048, Some(4_000))),
            Delivery::Sent,
        ),
    ];
    let time = test_clock().time(1_793_448_000);
    for width in [150.0, 200.0, 282.0, 412.0] {
        for entry in &entries {
            let shapes = shapes_of(entry, &GameNames::default(), width);
            for clipped in &shapes {
                let rect = clipped.shape.visual_bounding_rect();
                assert!(
                    !rect.is_positive() || rect.right() <= width + 0.5,
                    "{width}: message {} reaches {rect:?}",
                    entry.id
                );
                if let egui::Shape::Text(shown) = &clipped.shape
                    && shown.galley.text().starts_with(&time)
                {
                    assert_eq!(shown.galley.rows.len(), 1, "{width}: message {}", entry.id);
                }
            }
            let shown = texts(&shapes);
            let stamp = shown.iter().find(|(text, _)| text.starts_with(&time));
            let words = shown
                .iter()
                .filter(|(text, _)| entry.id == 4 && !text.starts_with(&time))
                .map(|(_, rect)| *rect)
                .reduce(|a, b| a.union(b));
            if let (Some((_, stamp)), Some(words)) = (stamp, words)
                && stamp.top() >= words.bottom() - 0.5
            {
                assert!(
                    (stamp.right() - words.right()).abs() < 2.0,
                    "{width}: a time on its own line sits right, {stamp:?} under {words:?}"
                );
            }
        }
    }
}

#[test]
fn enter_sends_and_shift_enter_starts_a_new_line() {
    for (modifiers, sends) in [(Modifiers::NONE, true), (Modifiers::SHIFT, false)] {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            modifiers,
            events: vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };
        let mut sent = None;
        let _ = ctx.run(input, |ctx| sent = Some(ctx.input_mut(enter_sends)));
        assert_eq!(sent, Some(sends), "{modifiers:?}");
    }
}

#[test]
fn the_empty_field_is_one_row_tall_and_grows_with_lines() {
    for (draft, grows) in [("", false), ("one\ntwo\nthree", true)] {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(2.0);
        let mut draft = draft.to_owned();
        let mut side = 0.0;
        let mut shown = 0.0;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let row = ui.text_style_height(&egui::TextStyle::Body);
                    side = ui.spacing().interact_size.y.max(row + FIELD_MARGIN.sum().y);
                    let id = egui::Id::new("draft");
                    shown = draft_field(ui, &mut draft, id, 200.0, side, "Message").1;
                });
            });
        }
        if grows {
            assert!(shown > side + 10.0, "{shown} grows past {side}");
        } else {
            assert!((shown - side).abs() < 1.0, "{shown} is one row of {side}");
        }
    }
}

#[test]
fn the_header_shows_the_name_in_a_thin_dock() {
    let person = Friend {
        online_id: OnlineId::new(7).unwrap(),
        display_name: "Mia Sommer".into(),
        public_key: citrine_gb_net::PublicKey([0; 32]),
        state: citrine_gb_net::FriendState::Friend,
        presence: None,
    };
    let online = Online::default();
    for width in [110.0, 120.0, 130.0, 400.0] {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(2.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 600.0))),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..2 {
            shapes = ctx
                .run(input.clone(), |ctx| {
                    egui::SidePanel::left("dock")
                        .exact_width(width)
                        .show(ctx, |ui| {
                            header(ui, &online, &person, false, None);
                        });
                })
                .shapes;
        }
        let shown = texts(&shapes);
        let name = shown
            .iter()
            .find(|(text, _)| text != "MS" && (text.starts_with('M') || text.starts_with('…')))
            .map(|(text, _)| text.clone());
        if width >= 400.0 {
            assert_eq!(name.as_deref(), Some("Mia Sommer"), "{width}: {shown:?}");
        } else {
            assert!(
                name.as_deref().is_some_and(|n| n.starts_with('M')),
                "{width}: a letter shows before the …: {shown:?}"
            );
        }
    }
}

#[test]
fn the_receipt_tip_stays_on_one_line_at_a_thin_dock_edge() {
    let ctx = egui::Context::default();
    let online = Online::default();
    let mut state = ChatUi::default();
    let entry = ChatEntry {
        id: 1,
        mine: true,
        at: 1_793_448_000,
        body: Body::Text("Hii".into()),
        delivery: Delivery::Read(1_793_448_060),
        change: Change::Kept,
    };
    let frame = |time: f64, events: Vec<egui::Event>, state: &mut ChatUi| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 600.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::SidePanel::left("dock")
                .exact_width(282.0)
                .show(ctx, |ui| {
                    entry_ui(
                        ui,
                        state,
                        &online,
                        OnlineId::new(7).unwrap(),
                        &entry,
                        &test_clock(),
                        None,
                        &GameNames::default(),
                        &PaletteLibrary::default(),
                        &mut Vec::new(),
                    );
                });
        })
    };
    frame(0.0, Vec::new(), &mut state);
    let shapes = frame(0.1, Vec::new(), &mut state).shapes;
    let hii = shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == "Hii" => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .expect("the message");
    let at = hii.center();
    frame(0.2, vec![egui::Event::PointerMoved(at)], &mut state);
    let mut tip = None;
    for time in [1.5, 1.6, 1.7] {
        for clipped in frame(time, Vec::new(), &mut state).shapes {
            if let egui::Shape::Text(text) = &clipped.shape
                && text.galley.text().starts_with("Read ")
            {
                tip = Some(text.galley.rows.len());
            }
        }
    }
    assert_eq!(tip, Some(1), "the tip shows, on one line");
}

#[test]
fn a_shared_screenshot_names_its_game_as_this_device_does() {
    let key = "ab".repeat(32);
    let mut names = GameNames::default();
    names.learn(&key, "Pokémon Silver Version");
    let mut screenshot = share(ShareKind::Screenshot, 2_048, None);
    screenshot.game = "POKEMON_SLVAAXE".into();
    screenshot.rom_sha256 = Some(key);
    let entry = ChatEntry {
        id: 2,
        mine: false,
        at: 1_793_448_000,
        body: Body::Share(screenshot),
        delivery: Delivery::Received,
        change: Change::Kept,
    };
    let shown: Vec<String> = texts(&shapes_of(&entry, &names, 400.0))
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert!(
        shown.iter().any(|t| t == "Pokémon Silver Version"),
        "{shown:?}"
    );
    assert!(!shown.iter().any(|t| t.contains("POKEMON")), "{shown:?}");
}

#[test]
fn the_typing_slot_shows_a_friend_typing_first_then_the_tab_tip() {
    let tip = |text: &str| Some(Slot::Tip(text.to_owned()));
    let typing = Some(Slot::Typing("Mia is typing".to_owned()));
    let key = Some("Tab");
    assert_eq!(slot(None, true, Field::Away, key), tip("Tab to chat"));
    assert_eq!(
        slot(None, true, Field::Focused, key),
        tip("Tab or Esc to go back to the game")
    );
    assert_eq!(slot(None, true, Field::Editing, key), None);
    assert_eq!(slot(Some("Mia"), true, Field::Away, key), typing);
    assert_eq!(slot(Some("Mia"), true, Field::Focused, key), typing);
    assert_eq!(slot(Some("Mia"), false, Field::Away, key), typing);
    assert_eq!(slot(None, false, Field::Away, key), None);
    assert_eq!(slot(None, false, Field::Focused, key), None);
    assert_eq!(slot(None, true, Field::Away, None), None);
    assert_eq!(
        slot(None, true, Field::Focused, None),
        tip("Esc to go back to the game")
    );
}
