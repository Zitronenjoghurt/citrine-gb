use crate::app::Citrine;
use crate::app::mobile::nav::Place;
use crate::app::tabs::chat;
use crate::app::tests::{phone_app, remember_account};
use crate::app::widgets::soft_keyboard;
use crate::icons;
use crate::online::chat::{Change, ChatEntry, Delivery};
use citrine_gb_net::{Body, Friend, FriendState, OnlineId, Presence, PublicKey};
use egui::{Context, Event, Pos2, Rect, Vec2, pos2, vec2};

const PORTRAIT: Vec2 = vec2(412.0, 915.0);
const TOP: f32 = 32.0;
const NAV: f32 = 24.0;
const KEYBOARD: f32 = 300.0;
const LANDSCAPE_KEYBOARD: f32 = 200.0;
const KEY_STEP: f32 = 70.0;
const SETTLE: usize = 24;
const COMPOSER_GAP: f32 = 12.0;

const WORDS: [&str; 6] = [
    "see you at the lemon tree",
    "did you beat the second castle yet? I keep falling into the lava right before the boss door",
    "ok",
    "my brother says the secret room behind the waterfall has an extra life but nobody ever found the switch for it",
    "trade tomorrow?",
    "sure, bring the link cable",
];
const NEWEST: &str = "talk later";
const LATE: &str = "one more thing";

fn ana() -> OnlineId {
    OnlineId::new(7).unwrap()
}

struct Phone {
    app: Citrine,
    ctx: Context,
    size: Vec2,
    keyboard: f32,
    shown: f32,
    top: f32,
    up: bool,
    arrived: u64,
}

#[derive(Debug, Clone)]
struct Seen {
    text: String,
    full: Rect,
    rect: Rect,
}

struct Shot {
    output: egui::FullOutput,
    content: Rect,
    keyboard: f32,
    bottom: Rect,
    field: Option<Rect>,
    emoji: f32,
}

impl Shot {
    fn field(&self) -> Rect {
        self.field.expect("the message field")
    }
}

impl Phone {
    fn chat(name: &str, messages: u64) -> Self {
        let mut app = phone_app(name);
        remember_account(&mut app);
        app.online.befriend_for_test(Friend {
            online_id: ana(),
            display_name: "Ana".into(),
            public_key: PublicKey([4; 32]),
            state: FriendState::Friend,
            presence: Some(Presence::Online { away: false }),
        });
        let mut entries: Vec<ChatEntry> = (0..messages)
            .map(|id| entry(id + 1, WORDS[id as usize % WORDS.len()]))
            .collect();
        entries.push(entry(messages + 1, NEWEST));
        app.online.add_chat_for_test(ana(), entries);
        app.mobile.nav.go(Place::Friends);
        app.ui.friends.chat.open(ana());
        let mut phone = Self {
            app,
            ctx: Context::default(),
            size: PORTRAIT,
            keyboard: KEYBOARD,
            shown: 0.0,
            top: TOP,
            up: false,
            arrived: 0,
        };
        phone.settle();
        phone
    }

    fn rotate(&mut self) {
        self.size = vec2(self.size.y, self.size.x);
        self.keyboard = if self.size.x > self.size.y {
            LANDSCAPE_KEYBOARD
        } else {
            KEYBOARD
        };
        if self.up {
            self.shown = self.keyboard;
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> Shot {
        let target = if self.up { self.keyboard } else { 0.0 };
        self.shown = if self.shown < target {
            (self.shown + KEY_STEP).min(target)
        } else {
            (self.shown - KEY_STEP).max(target)
        };
        let full = if self.up { self.keyboard } else { 0.0 };
        soft_keyboard::note(&self.ctx, self.shown, full, self.size.x > self.size.y);
        let bottom = NAV + self.shown;
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            safe_area_insets: Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
                left: 0.0,
                right: 0.0,
                top: self.top,
                bottom,
            })),
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &self.ctx, &mut input);
        let app = &mut self.app;
        let output = self.ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            super::touch_spacing(ctx, touch);
            app.render(ctx);
            app.mobile_escape(ctx);
            app.handle_event_queue(touch);
        });
        self.up = output.platform_output.ime.is_some();
        let content =
            Rect::from_min_max(pos2(0.0, self.top), pos2(self.size.x, self.size.y - bottom));
        Shot {
            output,
            content,
            keyboard: self.shown,
            bottom: self.bottom(),
            field: self.field_now(),
            emoji: self.emoji(),
        }
    }

    fn settle(&mut self) -> Shot {
        let mut last = self.frame(vec![]);
        for _ in 0..SETTLE {
            last = self.frame(vec![]);
        }
        last
    }

    fn tap(&mut self, at: Pos2) -> Vec<Shot> {
        let mut shots = vec![
            self.frame(vec![Event::PointerMoved(at)]),
            self.frame(vec![button(at, true)]),
            self.frame(vec![button(at, false)]),
            self.frame(vec![Event::PointerGone]),
        ];
        for _ in 0..8 {
            shots.push(self.frame(vec![]));
        }
        shots
    }

    fn tap_text(&mut self, text: &str) -> Vec<Shot> {
        let shot = self.settle();
        let at = seen(&shot.output)
            .into_iter()
            .find(|seen| seen.text == text)
            .unwrap_or_else(|| panic!("{text:?} on screen"))
            .rect
            .center();
        self.tap(at)
    }

    fn tap_field(&mut self) -> Vec<Shot> {
        let at = self.field().center();
        self.tap(at)
    }

    fn field(&self) -> Rect {
        self.field_now().expect("the message field")
    }

    fn field_now(&self) -> Option<Rect> {
        let id = chat::draft_id(ana());
        self.ctx
            .viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.rect))
    }

    fn bottom(&self) -> Rect {
        egui::containers::panel::PanelState::load(&self.ctx, egui::Id::new(("chat_bottom", ana())))
            .expect("the chat's bottom panel")
            .rect
    }

    fn emoji(&self) -> f32 {
        self.app.ui.friends.chat.emoji_room(&self.ctx)
    }

    fn arrive(&mut self, text: &str) {
        self.arrived += 1;
        let id = 1_000 + self.arrived;
        let mut late = entry(id, text);
        late.mine = false;
        late.delivery = Delivery::Received;
        self.app.online.add_chat_for_test(ana(), vec![late]);
    }

    fn scroll_up(&mut self, by: f32) {
        let at = self.field().center() - vec2(0.0, 200.0);
        self.frame(vec![Event::PointerMoved(at)]);
        for _ in 0..(by / 50.0) as usize {
            self.frame(vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 50.0),
                modifiers: Default::default(),
            }]);
        }
        self.frame(vec![Event::PointerGone]);
        self.settle();
    }
}

fn entry(id: u64, text: &str) -> ChatEntry {
    let mine = id.is_multiple_of(3);
    ChatEntry {
        id,
        mine,
        at: 1_761_800_000 + id * 600,
        body: Body::Text(text.into()),
        delivery: if mine {
            Delivery::Read(1_761_900_000)
        } else {
            Delivery::Received
        },
        change: Change::Kept,
        reply: None,
    }
}

fn button(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

fn walk(shape: &egui::Shape, clip: Rect, out: &mut Vec<Seen>) {
    match shape {
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                walk(shape, clip, out);
            }
        }
        egui::Shape::Text(text) => {
            let full = text.visual_bounding_rect();
            let rect = full.intersect(clip);
            if rect.is_positive() {
                out.push(Seen {
                    text: text.galley.text().trim().to_owned(),
                    full,
                    rect,
                });
            }
        }
        _ => {}
    }
}

fn seen(output: &egui::FullOutput) -> Vec<Seen> {
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, clipped.clip_rect, &mut out);
    }
    out
}

fn messages(shot: &Shot) -> Vec<Seen> {
    seen(&shot.output)
        .into_iter()
        .filter(|s| WORDS.contains(&s.text.as_str()) || s.text == NEWEST || s.text == LATE)
        .collect()
}

fn fully_shown(shot: &Shot, text: &str) -> bool {
    messages(shot)
        .iter()
        .any(|s| s.text == text && (s.rect.height() - s.full.height()).abs() < 0.5)
}

fn assert_shown(shot: &Shot, text: &str, label: &str) {
    assert!(
        fully_shown(shot, text),
        "{label}: {text:?} is not fully in view; content {:?}, bottom {:?}, keyboard {}, emoji {}, messages {:?}",
        shot.content,
        shot.bottom,
        shot.keyboard,
        shot.emoji,
        messages(shot)
            .iter()
            .map(|s| (s.text.chars().take(10).collect::<String>(), s.full, s.rect))
            .collect::<Vec<_>>()
    );
}

fn assert_nothing_overlaps(shot: &Shot, label: &str) {
    let bottom = shot.bottom;
    assert!(
        bottom.bottom() <= shot.content.bottom() + 0.5,
        "{label}: the composer ends at {} under the keyboard top {}",
        bottom.bottom(),
        shot.content.bottom()
    );
    for message in messages(shot) {
        assert!(
            message.rect.bottom() <= bottom.top() + 0.5,
            "{label}: {:?} at {:?} reaches into the composer and emoji panel from {}, content {:?}, keyboard {}, emoji {}",
            message.text,
            message.rect,
            bottom.top(),
            shot.content,
            shot.keyboard,
            shot.emoji
        );
        assert!(
            message.rect.top() >= shot.content.top() - 0.5,
            "{label}: {:?} at {:?} is above the screen",
            message.text,
            message.rect
        );
    }
    let field = shot.field();
    assert!(
        field.bottom() <= shot.content.bottom() + 0.5,
        "{label}: the field {field:?} sits under the keyboard at {}",
        shot.content.bottom()
    );
}

fn pictures(output: &egui::FullOutput) -> Vec<(Rect, Rect)> {
    fn walk(shape: &egui::Shape, clip: Rect, out: &mut Vec<(Rect, Rect)>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, clip, out);
                }
            }
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                out.push((mesh.calc_bounds(), clip));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, clipped.clip_rect, &mut out);
    }
    out
}

fn assert_whole_rows(shot: &Shot, label: &str) {
    let below = shot.field().bottom();
    let panel: Vec<_> = pictures(&shot.output)
        .into_iter()
        .filter(|(rect, clip)| rect.center().y > below && rect.intersects(*clip))
        .collect();
    assert!(!panel.is_empty(), "{label}: no emoji in the panel");
    for (rect, clip) in panel {
        assert!(
            rect.top() >= clip.top() - 0.5 && rect.bottom() <= clip.bottom() + 0.5,
            "{label}: the emoji at {rect:?} is cut by the panel's edge {clip:?}"
        );
    }
}

fn bar_shown(shot: &Shot) -> bool {
    seen(&shot.output).iter().any(|s| s.text == "Games")
}

fn assert_whole_rail(shot: &Shot, label: &str) {
    let shown = seen(&shot.output);
    for place in [Place::Games, Place::Friends, Place::Media, Place::Settings] {
        let icon = shown
            .iter()
            .find(|s| s.text == place.icon())
            .unwrap_or_else(|| panic!("{label}: no {} in the rail", place.label()));
        assert!(
            (icon.rect.height() - icon.full.height()).abs() < 0.5
                && icon.rect.bottom() <= shot.content.bottom() + 0.5,
            "{label}: {} at {:?} is cut off above {}",
            place.label(),
            icon.rect,
            shot.content.bottom()
        );
    }
}

#[test]
fn with_the_keyboard_open_nothing_overlaps_and_the_place_bar_steps_aside() {
    let mut phone = Phone::chat("chat-feel-keyboard", 40);
    let shot = phone.settle();
    assert!(bar_shown(&shot));
    assert_nothing_overlaps(&shot, "closed");
    for (step, shot) in phone.tap_field().iter().enumerate() {
        assert_nothing_overlaps(shot, &format!("opening, frame {step}"));
    }
    let shot = phone.settle();
    assert_eq!(shot.keyboard, KEYBOARD);
    assert!(!bar_shown(&shot), "the place bar sits on the keyboard");
    let content = shot.content;
    assert!(
        (0.0..=COMPOSER_GAP).contains(&(content.bottom() - shot.bottom.bottom())),
        "the composer {:?} sits on the keyboard top {}",
        shot.bottom,
        content.bottom()
    );
}

#[test]
fn the_newest_message_stays_in_view_while_the_keyboard_opens_and_closes() {
    let mut phone = Phone::chat("chat-feel-newest", 40);
    assert_shown(&phone.settle(), NEWEST, "closed");
    for (step, shot) in phone.tap_field().iter().enumerate() {
        assert_shown(shot, NEWEST, &format!("opening, frame {step}"));
    }
    let shot = phone.frame(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    assert_shown(&shot, NEWEST, "back");
    for step in 0..8 {
        let shot = phone.frame(vec![]);
        assert_shown(&shot, NEWEST, &format!("closing, frame {step}"));
    }
    assert_eq!(phone.shown, 0.0, "the keyboard went down");
}

#[test]
fn the_emoji_panel_takes_the_keyboards_place_and_gives_it_back() {
    let mut phone = Phone::chat("chat-feel-emoji", 40);
    phone.tap_field();
    phone.settle();
    let field = phone.field();
    for (step, shot) in phone.tap_text(icons::SMILEY).iter().enumerate() {
        let label = format!("keyboard to emoji, frame {step}");
        assert!(
            shot.keyboard + shot.emoji <= KEYBOARD + 0.5,
            "{label}: keyboard {} and emoji panel {} both take space",
            shot.keyboard,
            shot.emoji
        );
        assert_nothing_overlaps(shot, &label);
        assert!(
            (shot.field().top() - field.top()).abs() < 1.0,
            "{label}: the field moved from {field:?} to {:?}",
            shot.field()
        );
    }
    let shot = phone.settle();
    assert_eq!(shot.keyboard, 0.0, "the keyboard went down");
    assert!((phone.emoji() - KEYBOARD).abs() < 0.5, "{}", phone.emoji());
    assert_shown(&shot, NEWEST, "emoji panel open");
    assert!(!bar_shown(&shot));
    for (step, shot) in phone.tap_text(icons::KEYBOARD).iter().enumerate() {
        let label = format!("emoji to keyboard, frame {step}");
        assert!(
            shot.keyboard + shot.emoji <= KEYBOARD + 0.5,
            "{label}: keyboard {} and emoji panel {} both take space",
            shot.keyboard,
            shot.emoji
        );
        assert_nothing_overlaps(shot, &label);
        assert!(
            (shot.field().top() - field.top()).abs() < 1.0,
            "{label}: the field moved from {field:?} to {:?}",
            shot.field()
        );
    }
    let shot = phone.settle();
    assert_eq!(shot.keyboard, KEYBOARD);
    assert_eq!(phone.emoji(), 0.0);
}

#[test]
fn a_tap_on_the_field_closes_the_emoji_panel_as_the_keyboard_comes() {
    let mut phone = Phone::chat("chat-feel-emoji-field", 40);
    phone.tap_field();
    phone.settle();
    phone.frame(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    assert_eq!(phone.settle().keyboard, 0.0);
    phone.tap_text(icons::SMILEY);
    let shot = phone.settle();
    assert_eq!(shot.keyboard, 0.0);
    assert!(phone.emoji() > 100.0);
    let field = phone.field();
    for (step, shot) in phone.tap_field().iter().enumerate() {
        let label = format!("emoji to field, frame {step}");
        assert_nothing_overlaps(shot, &label);
        assert!(
            (shot.field().top() - field.top()).abs() < 1.0,
            "{label}: the field moved from {field:?} to {:?}",
            shot.field()
        );
    }
    let shot = phone.settle();
    assert_eq!(shot.keyboard, KEYBOARD);
    assert_eq!(phone.emoji(), 0.0);
}

#[test]
fn back_closes_the_emoji_panel_before_the_chat() {
    let mut phone = Phone::chat("chat-feel-emoji-back", 40);
    phone.tap_text(icons::SMILEY);
    phone.settle();
    assert!(phone.app.ui.friends.chat.panel_open());
    phone.frame(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    let shot = phone.settle();
    assert!(!phone.app.ui.friends.chat.panel_open());
    assert!(phone.app.ui.friends.chat.current().is_some());
    assert!(bar_shown(&shot));
    assert_nothing_overlaps(&shot, "after back");
}

#[test]
fn turned_sideways_with_the_keyboard_open_nothing_overlaps() {
    let mut phone = Phone::chat("chat-feel-rotate", 40);
    phone.tap_field();
    phone.settle();
    phone.rotate();
    let shot = phone.settle();
    assert_nothing_overlaps(&shot, "sideways with the keyboard");
    assert_shown(&shot, NEWEST, "sideways with the keyboard");
    assert_whole_rail(&shot, "sideways with the keyboard");
    let column = shot.bottom.x_range();
    let field = phone.field();
    phone.tap_text(icons::SMILEY);
    let shot = phone.settle();
    assert_nothing_overlaps(&shot, "sideways with the emoji panel");
    assert!(phone.emoji() > 0.0);
    assert!(
        bar_shown(&shot),
        "sideways the rail stays with the emoji panel"
    );
    assert!(
        (phone.field().top() - field.top()).abs() < 0.5,
        "sideways the field moved from {field:?} with the keyboard to {:?} with the emoji panel",
        phone.field()
    );
    assert!(
        (shot.bottom.left() - column.min).abs() < 0.5
            && (shot.bottom.right() - column.max).abs() < 0.5,
        "the chat column moved from {column:?} to {:?}",
        shot.bottom.x_range()
    );
    phone.rotate();
    let shot = phone.settle();
    assert_nothing_overlaps(&shot, "upright with the emoji panel");
}

#[test]
fn the_emoji_card_runs_down_through_the_gesture_bar() {
    let mut phone = Phone::chat("chat-feel-emoji-foot", 40);
    phone.tap_text(icons::SMILEY);
    let shot = phone.settle();
    assert_emoji_card_foot(&phone, &shot, "upright");
    phone.rotate();
    let shot = phone.settle();
    assert_emoji_card_foot(&phone, &shot, "sideways");
}

fn assert_emoji_card_foot(phone: &Phone, shot: &Shot, label: &str) {
    assert!(
        phone.emoji() > 100.0,
        "{label}: emoji panel {}",
        phone.emoji()
    );
    let fill = phone.ctx.style().visuals.faint_bg_color;
    let screen = phone.size.y;
    let run = shot
        .output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect)
                if rect.fill == fill
                    && (rect.rect.bottom() - screen).abs() < 0.5
                    && clipped.clip_rect.bottom() >= screen - 0.5 =>
            {
                Some(rect.rect)
            }
            _ => None,
        });
    let run = run.unwrap_or_else(|| {
        panic!("{label}: the emoji card's colour misses the screen's bottom edge")
    });
    let card = shot
        .output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect)
                if rect.fill == fill
                    && rect.corner_radius.nw == 8
                    && rect.corner_radius.sw == 0 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("the emoji card");
    assert!(
        (run.top() - card.bottom()).abs() < 0.5,
        "{label}: the run {run:?} starts where the card {card:?} ends"
    );
    let glyphs = shot
        .output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                let rect = mesh.calc_bounds();
                (card.union(run).contains_rect(rect) && clipped.clip_rect.contains_rect(rect))
                    .then_some(rect)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut tops: Vec<f32> = glyphs.iter().map(|r| r.top()).collect();
    tops.sort_by(f32::total_cmp);
    tops.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    let rows = glyphs
        .iter()
        .copied()
        .reduce(|a, b| a.union(b))
        .unwrap_or_else(|| panic!("{label}: no emoji in the card"));
    let handle = shot.content.bottom() + (NAV - chat::GESTURE_HANDLE) / 2.0;
    let over = rows.top() - card.top();
    let under = handle - rows.bottom();
    let pixel = 1.0 / phone.ctx.pixels_per_point();
    assert!(
        (over - under).abs() <= pixel + 0.5,
        "{label}: the rows {rows:?} sit {over} below the card's top and {under} above the gesture handle at {handle}"
    );
    assert!(tops.len() >= 2, "{label}: one row of emoji");
    let between = tops[1] - (tops[0] + glyphs[0].height());
    assert!(
        (between - over).abs() <= pixel + 0.5,
        "{label}: {between} between rows against {over} above the first"
    );
}

#[test]
fn scrolled_up_the_list_stays_put_and_offers_a_way_to_the_newest() {
    let mut phone = Phone::chat("chat-feel-scrolled", 60);
    phone.scroll_up(1200.0);
    let shot = phone.settle();
    assert!(!fully_shown(&shot, NEWEST));
    let before = messages(&shot);
    let anchor = before.last().expect("messages in view").clone();
    phone.arrive(LATE);
    let shot = phone.settle();
    let after = messages(&shot);
    let kept = after
        .iter()
        .find(|s| s.text == anchor.text && (s.full.top() - anchor.full.top()).abs() < 0.5);
    assert!(kept.is_some(), "a new message moved the list");
    assert!(
        seen(&shot.output).iter().any(|s| s.text == "1 new message"),
        "the way down counts the new message"
    );
    phone.tap_field();
    let shot = phone.settle();
    assert!(
        fully_shown(&shot, &anchor.text),
        "the keyboard hid {:?}, the last message in view",
        anchor.text
    );
    let jump = seen(&shot.output)
        .into_iter()
        .find(|s| s.text == icons::CARET_DOWN)
        .expect("the way down");
    phone.tap(jump.rect.center());
    let shot = phone.settle();
    assert_shown(&shot, LATE, "after the way down");
    assert!(
        !seen(&shot.output)
            .iter()
            .any(|s| s.text == icons::CARET_DOWN)
    );
}

#[test]
fn sending_shows_the_sent_message() {
    let mut phone = Phone::chat("chat-feel-send", 60);
    phone.scroll_up(1200.0);
    phone.tap_field();
    phone.frame(vec![Event::Text("on my way".into())]);
    phone.frame(vec![Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    let shot = phone.settle();
    assert!(
        seen(&shot.output)
            .iter()
            .any(|s| s.text.starts_with("on my way")),
        "the sent message is in view: draft {:?}, last {:?}, seen {:?}",
        phone.app.ui.friends.chat.composing(),
        phone
            .app
            .online
            .conversation(ana())
            .and_then(|c| c.entries.last().map(|e| e.body.clone())),
        seen(&shot.output)
            .iter()
            .map(|s| (s.text.clone(), s.rect.top()))
            .collect::<Vec<_>>()
    );
    assert_nothing_overlaps(&shot, "after sending");
}

#[test]
fn the_emoji_panel_ends_on_a_whole_row() {
    let mut phone = Phone::chat("chat-feel-rows", 40);
    phone.tap_field();
    phone.tap_text(icons::SMILEY);
    let shot = phone.settle();
    assert_whole_rows(&shot, "upright");
    phone.rotate();
    let shot = phone.settle();
    assert_whole_rows(&shot, "sideways");
}

fn assert_jump_covers_nothing(phone: &Phone, shot: &Shot, label: &str) -> Rect {
    let id = chat::jump_id(ana());
    let jump = phone
        .ctx
        .viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.rect))
        .expect("the way down");
    for text in seen(&shot.output) {
        if text.text == icons::CARET_DOWN || text.text.ends_with("new message") {
            continue;
        }
        assert!(
            !text.rect.intersects(jump),
            "{label}: the way down at {jump:?} covers {:?} at {:?}",
            text.text,
            text.rect
        );
    }
    jump
}

#[test]
fn the_way_down_covers_no_message_its_time_or_ticks() {
    let mut phone = Phone::chat("chat-feel-jump-room", 60);
    phone.scroll_up(1200.0);
    phone.arrive(LATE);
    let shot = phone.settle();
    let jump = assert_jump_covers_nothing(&phone, &shot, "scrolled up");
    assert!(jump.bottom() <= shot.bottom.top() + 0.5);
    phone.tap_field();
    let shot = phone.settle();
    assert_jump_covers_nothing(&phone, &shot, "scrolled up with the keyboard");
}

#[test]
fn turned_sideways_with_the_keyboard_open_back_stays_on_screen() {
    let mut phone = Phone::chat("chat-feel-sideways-back", 40);
    phone.tap_field();
    phone.settle();
    phone.rotate();
    let shot = phone.settle();
    let back = seen(&shot.output)
        .into_iter()
        .find(|s| s.text == icons::ARROW_LEFT)
        .expect("a way back sideways with the keyboard");
    assert!(shot.content.contains_rect(back.full));
    assert_nothing_overlaps(&shot, "sideways with the keyboard and a way back");
    phone.tap_text(icons::SMILEY);
    let shot = phone.settle();
    assert!(
        seen(&shot.output)
            .iter()
            .any(|s| s.text == icons::ARROW_LEFT),
        "a way back sideways with the emoji panel"
    );
    phone.tap_text(icons::ARROW_LEFT);
    phone.settle();
    assert_eq!(phone.app.ui.friends.chat.current(), None);
}

fn rects(output: &egui::FullOutput) -> Vec<egui::epaint::RectShape> {
    fn walk(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            egui::Shape::Rect(rect) => out.push(rect.clone()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn the_focused_message_field_has_a_border_that_reads_in_every_theme() {
    use crate::app::ui_state::ui_theme::{UiTheme, contrast};
    use strum::IntoEnumIterator;
    for theme in UiTheme::iter().filter(|theme| *theme != UiTheme::System) {
        let mut phone = Phone::chat(&format!("chat-feel-border-{theme:?}"), 4);
        theme.apply(&phone.ctx);
        phone.tap_field();
        let shot = phone.settle();
        let field = shot.field();
        let frame = rects(&shot.output)
            .into_iter()
            .find(|r| {
                r.rect.contains_rect(field.shrink(1.0)) && r.rect.width() < field.width() + 4.0
            })
            .expect("the field's frame");
        assert!(frame.stroke.width >= 1.0, "{theme:?}: no border");
        let ratio = contrast(frame.stroke.color, frame.fill);
        assert!(
            ratio >= 3.0,
            "{theme:?}: border {:?} on {:?} is {ratio:.2}",
            frame.stroke.color,
            frame.fill
        );
    }
}

#[test]
fn turned_sideways_the_header_keeps_a_face_and_the_date_stays_with_its_message() {
    let mut phone = Phone::chat("chat-feel-sideways-date", 40);
    phone.top = 0.0;
    phone
        .app
        .online
        .add_chat_for_test(ana(), vec![entry(900, "\u{1F60B}\u{1F602}")]);
    phone.tap_field();
    phone.settle();
    phone.rotate();
    let shot = phone.settle();
    let texts = seen(&shot.output);
    let find = |want: &dyn Fn(&str) -> bool, what: &str| {
        texts
            .iter()
            .find(|s| want(s.text.as_str()))
            .unwrap_or_else(|| {
                let all: Vec<_> = texts.iter().map(|s| (&s.text, s.full)).collect();
                panic!("{what} on screen: {all:?}")
            })
            .full
    };
    let back = find(&|t| t == icons::ARROW_LEFT, "back");
    let name = find(&|t| t == "Ana", "the name");
    let initials = find(&|t| t == "A", "the avatar's initials");
    assert!(back.right() <= initials.left() && initials.right() <= name.left());
    let date = find(&|t| t.contains("2025"), "the date");
    let message = pictures(&shot.output)
        .into_iter()
        .map(|(rect, _)| rect)
        .filter(|rect| rect.top() > date.bottom())
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("the emoji message");
    let header = name.bottom().max(initials.center().y + 11.0);
    let before = texts
        .iter()
        .map(|s| s.rect)
        .chain(
            pictures(&shot.output)
                .into_iter()
                .map(|(rect, clip)| rect.intersect(clip)),
        )
        .filter(|rect| rect.height() > 0.0 && rect.bottom() <= date.top())
        .map(|rect| rect.bottom())
        .fold(header, f32::max);
    let above = date.top() - before;
    let below = message.top() - date.bottom();
    assert!(
        above > below,
        "the date is {above:.1} pt under what comes before it and {below:.1} pt over its message"
    );
}

#[test]
fn a_deleted_message_has_its_time_at_the_end_like_every_bubble() {
    let mut phone = Phone::chat("chat-feel-deleted", 4);
    let mut mine = entry(6, "on my way");
    mine.change = Change::Deleted { told: true };
    let mut theirs = entry(7, "see you there");
    theirs.change = Change::Deleted { told: true };
    phone
        .app
        .online
        .add_chat_for_test(ana(), vec![mine, theirs]);
    let shot = phone.settle();
    let seen = seen(&shot.output);
    let deleted: Vec<_> = seen
        .iter()
        .filter(|s| s.text.ends_with("Deleted"))
        .collect();
    assert_eq!(deleted.len(), 2);
    for label in deleted {
        let time = seen
            .iter()
            .find(|s| {
                s.text.contains(':')
                    && s.full.min.y < label.full.max.y
                    && s.full.max.y > label.full.min.y
            })
            .expect("a time beside Deleted");
        assert!(
            time.full.left() > label.full.right(),
            "the time {:?} comes after Deleted {:?}",
            time.full,
            label.full
        );
        assert!(
            (time.full.center().y - label.full.center().y).abs() <= 1.5,
            "the time {:?} sits level with Deleted {:?}",
            time.full,
            label.full
        );
    }
}
