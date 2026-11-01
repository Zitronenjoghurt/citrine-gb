use crate::app::events::AppEvent;
use crate::app::tabs::friends::subtitle;
use crate::app::tabs::media::{Browse, game_name, thumbnail};
use crate::app::tabs::{Tab, TabViewer};
use crate::app::viewer::{ItemKey, Source};
use crate::app::widgets::{avatar, confirm, emoji, scroll};
use crate::emulator::Emulator;
use crate::game_name::GameNames;
use crate::icons;
use crate::media::{self, Frame, MediaItem, MediaKind, MediaStore, NewMedia};
use crate::online::chat::{ChatEntry, Delivery, Failure, Fetch, NewShare};
use crate::online::{Online, chat_media, seen};
use crate::replay::ReplayError;
use citrine_gb_net::{Body, FileId, Friend, MAX_TEXT_CHARS, OnlineId, Share, ShareKind};
use egui::{
    Align, Color32, CursorIcon, Key, Label, Layout, Modifiers, Rect, RichText, Sense,
    TextureHandle, TextureOptions, Ui, vec2,
};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};

const HEADER_AVATAR: f32 = 34.0;
const BUBBLE_SHARE: f32 = 0.8;
const INLINE_EMOJI: f32 = 20.0;
const BIG_EMOJI: f32 = 40.0;
const BIG_EMOJI_MAX: usize = 6;
const BIG_EMOJI_GAP: f32 = 2.0;
const STAMP_GAP: f32 = 6.0;
const TEXT_STAMP_GAP: f32 = 8.0;
const BUBBLE_MARGIN: egui::Margin = egui::Margin::symmetric(10, 6);
const PICKER_EMOJI: f32 = 28.0;
const PICKER_HEIGHT: f32 = 176.0;
const PREVIEW_MAX: f32 = 320.0;
const LONG_PRESS_SECS: f64 = 0.5;
const LONG_PRESS_SLOP: f32 = 8.0;
const COMPOSER_ROWS: f32 = 5.0;
const FIELD_MARGIN: egui::Margin = egui::Margin::symmetric(4, 2);
const NO_FILE: FileId = FileId([0; 16]);
const UNREADABLE: &str = "This file can't be opened";

struct Loaded {
    to: OnlineId,
    item: MediaItem,
    bytes: Option<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Want {
    Save,
    Play,
}

enum Action {
    Retry(u64),
    Forget(u64),
    Fetch(Want, FileId, u64),
    View(u64),
}

pub struct ChatUi {
    open: Option<OnlineId>,
    drafts: HashMap<OnlineId, String>,
    picker: bool,
    from_media: bool,
    media: Browse,
    jump: bool,
    reading: usize,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
    previews: HashMap<u64, Option<TextureHandle>>,
    wanted: HashMap<FileId, (Want, OnlineId, u64)>,
    saved: HashSet<u64>,
    notes: HashMap<u64, String>,
    receipt: Option<u64>,
}

impl Default for ChatUi {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            open: None,
            drafts: HashMap::new(),
            picker: false,
            from_media: false,
            media: Browse::default(),
            jump: false,
            reading: 0,
            tx,
            rx,
            previews: HashMap::new(),
            wanted: HashMap::new(),
            saved: HashSet::new(),
            notes: HashMap::new(),
            receipt: None,
        }
    }
}

impl ChatUi {
    pub fn open(&mut self, friend: OnlineId) {
        self.open = Some(friend);
        self.picker = false;
        self.from_media = false;
        self.jump = true;
    }

    pub fn close(&mut self) {
        self.open = None;
        self.picker = false;
        self.from_media = false;
    }

    pub fn current(&self) -> Option<OnlineId> {
        self.open
    }

    pub fn escape(&mut self) -> bool {
        if self.picker {
            self.picker = false;
            return true;
        }
        if self.open.is_some() {
            self.close();
            return true;
        }
        false
    }

    pub fn is_saved(&self, id: u64) -> bool {
        self.saved.contains(&id)
    }

    pub fn mark_saved(&mut self, id: u64) {
        self.saved.insert(id);
        self.notes.remove(&id);
    }

    pub fn want_save(&mut self, online: &mut Online, friend: OnlineId, file: FileId, id: u64) {
        self.want(online, Want::Save, friend, file, id);
    }

    pub fn want_play(&mut self, online: &mut Online, friend: OnlineId, file: FileId, id: u64) {
        self.want(online, Want::Play, friend, file, id);
    }

    fn want(&mut self, online: &mut Online, want: Want, friend: OnlineId, file: FileId, id: u64) {
        self.notes.remove(&id);
        self.wanted.insert(file, (want, friend, id));
        online.fetch(friend, file);
    }

    pub fn send_media(&mut self, store: &MediaStore, to: OnlineId, item: &MediaItem) {
        self.reading += 1;
        self.jump = true;
        let tx = self.tx.clone();
        let read = item.clone();
        store.read_bytes(&item.id, move |bytes| {
            let _ = tx.send(Loaded {
                to,
                item: read,
                bytes,
            });
        });
    }

    fn preview(&mut self, ctx: &egui::Context, id: u64, bytes: &[u8]) -> Option<TextureHandle> {
        self.previews
            .entry(id)
            .or_insert_with(|| {
                let frame = chat_media::decode_image(bytes)?;
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width as usize, frame.height as usize],
                    &frame.rgba,
                );
                Some(ctx.load_texture(format!("chat:{id}"), image, TextureOptions::NEAREST))
            })
            .clone()
    }
}

pub fn take_escape(ctx: &egui::Context) -> bool {
    let free = !ctx.wants_keyboard_input()
        && ctx.memory(|m| m.top_modal_layer().is_none())
        && !egui::Popup::is_any_open(ctx)
        && !confirm::is_open(ctx);
    free && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
}

pub fn pump(viewer: &mut TabViewer) {
    let state = &mut viewer.ui.friends.chat;
    while let Ok(loaded) = state.rx.try_recv() {
        state.reading = state.reading.saturating_sub(1);
        let shared = loaded
            .bytes
            .and_then(|bytes| share_item(&loaded.item, bytes))
            .is_some_and(|new| viewer.online.share(loaded.to, new));
        if !shared {
            viewer
                .events
                .notify_error(format!("Couldn't share this {}", kind_word(&loaded.item)));
        }
    }

    let done: Vec<FileId> = state
        .wanted
        .keys()
        .filter(|file| !matches!(viewer.online.fetched(**file), Some(Fetch::Loading { .. })))
        .copied()
        .collect();
    for file in done {
        let Some((want, friend, id)) = state.wanted.remove(&file) else {
            continue;
        };
        let bytes = match viewer.online.take_fetched(file) {
            Some(Fetch::Ready(bytes)) => bytes,
            Some(Fetch::Failed(text)) => {
                state.notes.insert(id, text.to_owned());
                continue;
            }
            _ => continue,
        };
        let Some(share) = share_entry(viewer.online, friend, id) else {
            continue;
        };
        match want {
            Want::Save => match save_received(viewer.media, &share, &bytes) {
                Ok(item) => {
                    state.saved.insert(id);
                    state.notes.remove(&id);
                    viewer.events.push(AppEvent::MediaAdded { id: item.id });
                }
                Err(text) => {
                    state.notes.insert(id, text.to_owned());
                }
            },
            Want::Play => {
                let played = crate::replay::parse(&bytes)
                    .and_then(|recording| viewer.emulator.play_replay(recording, &share.game));
                match played {
                    Ok(()) => {
                        state.notes.remove(&id);
                        viewer.events.open_tab(Tab::GameBoy);
                    }
                    Err(err) => {
                        state.notes.insert(id, replay_error(&err));
                    }
                }
            }
        }
    }
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) -> bool {
    let Some(friend) = viewer.ui.friends.chat.open else {
        return false;
    };
    let Some(person) = viewer.online.friend(friend).cloned() else {
        viewer.ui.friends.chat.close();
        return false;
    };
    let mobile = crate::app::mobile_layout(viewer.ui.settings.layout, ui.ctx());
    viewer.online.mark_read(friend);

    let mut profile = false;
    match header(ui, viewer.online, &person, !mobile) {
        Some(Header::Back) => {
            viewer.ui.friends.chat.close();
            return false;
        }
        Some(Header::Profile) => profile = true,
        None => {}
    }
    ui.separator();

    let can_chat = viewer.online.can_chat(friend);
    let bottom = egui::TopBottomPanel::bottom(egui::Id::new(("chat_bottom", friend)))
        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
            bottom: if mobile { 10 } else { 4 },
            ..Default::default()
        }))
        .show_separator_line(false)
        .resizable(false)
        .show_inside(ui, |ui| {
            if can_chat && viewer.ui.friends.chat.picker {
                picker(ui, viewer, friend);
            }
            if can_chat {
                composer(ui, viewer, friend);
            } else {
                ui.add_space(4.0);
                ui.label(RichText::new("You aren't friends anymore").weak());
            }
        });
    let height_id = egui::Id::new(("chat_bottom_height", friend));
    let height = bottom.response.rect.height();
    let was = ui.data(|d| d.get_temp::<f32>(height_id));
    ui.data_mut(|d| d.insert_temp(height_id, height));
    if was.is_some_and(|was| (was - height).abs() >= 0.5) {
        ui.ctx().request_discard("chat composer height");
    }
    messages(ui, viewer, friend);
    from_media(ui.ctx(), viewer, friend);
    profile
}

enum Header {
    Back,
    Profile,
}

fn header(ui: &mut Ui, online: &Online, person: &Friend, back: bool) -> Option<Header> {
    let mut action = None;
    let unix_now = crate::storage::now_unix();
    let height = HEADER_AVATAR.max(ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::left_to_right(Align::Center),
        |ui| {
            if back
                && ui
                    .button(icons::ARROW_LEFT)
                    .on_hover_text("Back to friends")
                    .clicked()
            {
                action = Some(Header::Back);
            }
            let presence = online.is_connected().then(|| seen(person.presence));
            if avatar::avatar(
                ui,
                &person.display_name,
                person.online_id.value(),
                HEADER_AVATAR,
                presence,
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
            {
                action = Some(Header::Profile);
            }
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                let strong = ui.visuals().strong_text_color();
                let name = emoji::line(ui, &person.display_name, egui::TextStyle::Body, strong)
                    .on_hover_cursor(CursorIcon::PointingHand);
                let weak = ui.visuals().weak_text_color();
                let line = emoji::line(
                    ui,
                    &subtitle(online, person, unix_now),
                    egui::TextStyle::Small,
                    weak,
                )
                .on_hover_cursor(CursorIcon::PointingHand);
                if name.clicked() || line.clicked() {
                    action = Some(Header::Profile);
                }
            });
        },
    );
    action
}

fn composer(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId) {
    let row = ui.text_style_height(&egui::TextStyle::Body);
    let side = ui.spacing().interact_size.y.max(row + FIELD_MARGIN.sum().y);
    let gap = ui.spacing().item_spacing.x / 2.0;
    let id = egui::Id::new(("chat_draft", friend));
    let height_id = egui::Id::new(("chat_field_height", friend));
    let tall = ui
        .data(|d| d.get_temp::<f32>(height_id))
        .unwrap_or(side)
        .max(side);
    let mut send = false;
    ui.add_space(2.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        at_foot(ui, side, tall, |ui| attach_menu(ui, viewer, friend, side));
        let state = &mut viewer.ui.friends.chat;
        let enter = ui.memory(|m| m.has_focus(id)) && ui.input_mut(enter_sends);
        send |= enter;
        let draft = state.drafts.entry(friend).or_default();
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let ready = !draft.trim().is_empty();
            let send_button = at_foot(ui, side, tall, |ui| {
                ui.add_enabled_ui(ready, |ui| {
                    icon_button(ui, egui::Button::new(icons::PAPER_PLANE_RIGHT), side)
                })
                .inner
            })
            .on_hover_text("Send");
            send |= send_button.clicked();
            let picker = state.picker;
            let label = if picker { "Hide emoji" } else { "Emoji" };
            if at_foot(ui, side, tall, |ui| {
                icon_button(ui, egui::Button::new(icons::SMILEY).selected(picker), side)
            })
            .on_hover_text(label)
            .clicked()
            {
                state.picker = !picker;
            }
            let bar = ui.spacing().scroll.allocated_width();
            let width = (ui.available_width() - bar).max(side);
            let hint = "Message";
            let hint = if measure(ui, RichText::new(hint)) + FIELD_MARGIN.sum().x <= width {
                hint
            } else {
                ""
            };
            let (field, shown) = draft_field(ui, draft, id, width, side, hint);
            if (shown - tall).abs() >= 0.5 {
                ui.data_mut(|d| d.insert_temp(height_id, shown));
                ui.ctx().request_discard("chat field height");
            }
            if field.changed() && ready == draft.trim().is_empty() {
                ui.ctx().request_repaint();
            }
        });
    });
    if !send {
        return;
    }
    let state = &mut viewer.ui.friends.chat;
    let draft = state.drafts.entry(friend).or_default();
    if viewer.online.send_text(friend, draft) {
        draft.clear();
        state.jump = true;
    }
    ui.memory_mut(|m| m.request_focus(id));
}

fn draft_field(
    ui: &mut Ui,
    draft: &mut String,
    id: egui::Id,
    width: f32,
    side: f32,
    hint: &str,
) -> (egui::Response, f32) {
    let row = ui.text_style_height(&egui::TextStyle::Body);
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
        emoji::field_galley(ui, text.as_str(), wrap)
    };
    let scroll = egui::ScrollArea::vertical()
        .id_salt(id.with("scroll"))
        .max_height(row * COMPOSER_ROWS + 8.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            let output = egui::TextEdit::multiline(draft)
                .id(id)
                .hint_text(hint)
                .desired_rows(1)
                .char_limit(MAX_TEXT_CHARS)
                .margin(FIELD_MARGIN)
                .desired_width(width - FIELD_MARGIN.sum().x)
                .min_size(vec2(width, side))
                .vertical_align(Align::Center)
                .layouter(&mut layouter)
                .show(ui);
            emoji::paint_field(ui, &output, draft);
            output.response
        });
    let shown = scroll.inner.rect.height().min(scroll.inner_rect.height());
    (scroll.inner, shown)
}

fn enter_sends(input: &mut egui::InputState) -> bool {
    !input.modifiers.shift && input.consume_key(Modifiers::NONE, Key::Enter)
}

fn at_foot<R>(ui: &mut Ui, side: f32, tall: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(vec2(side, tall), Layout::bottom_up(Align::Center), add)
        .inner
}

fn icon_button(ui: &mut Ui, button: egui::Button, side: f32) -> egui::Response {
    ui.add_sized(vec2(side, side), button)
}

fn attach_menu(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId, side: f32) {
    let button = icon_button(ui, egui::Button::new(icons::PLUS), side).on_hover_text("Share");
    let has_rom = viewer.emulator.gb.cartridge.has_rom_loaded;
    let has_media = !viewer.media.items().is_empty();
    let has_replay = viewer.emulator.recorder.has_data();
    egui::Popup::menu(&button).show(|ui| {
        if ui
            .add_enabled(
                has_rom,
                egui::Button::new(format!("{}  Screenshot now", icons::CAMERA)),
            )
            .on_disabled_hover_text("Load a ROM first")
            .clicked()
        {
            let scale = viewer.ui.settings.screenshot_scale;
            match media::save_screenshot(viewer.emulator, viewer.media, scale) {
                Ok(item) => {
                    viewer
                        .ui
                        .friends
                        .chat
                        .send_media(viewer.media, friend, &item);
                    viewer.events.push(AppEvent::MediaAdded { id: item.id });
                }
                Err(err) => viewer.events.notify_error(err),
            }
        }
        if ui
            .add_enabled(
                has_media,
                egui::Button::new(format!("{}  From Media", icons::IMAGES)),
            )
            .on_disabled_hover_text("Media has no screenshots or recordings yet")
            .clicked()
        {
            viewer.ui.friends.chat.from_media = true;
        }
        if has_replay
            && ui
                .button(format!("{}  Input replay", icons::GAME_CONTROLLER))
                .clicked()
        {
            let shared =
                replay_share(viewer.emulator).is_some_and(|new| viewer.online.share(friend, new));
            if shared {
                viewer.ui.friends.chat.jump = true;
            } else {
                viewer.events.notify_error("Couldn't share the replay");
            }
        }
    });
}

fn picker(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId) {
    ui.add_space(4.0);
    let mut picked = None;
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(8.0)
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("chat_emoji_scroll")
                .max_height(PICKER_HEIGHT)
                .show(ui, |ui| picked = emoji::picker(ui, PICKER_EMOJI));
        });
    if let Some(glyph) = picked {
        let state = &mut viewer.ui.friends.chat;
        let draft = state.drafts.entry(friend).or_default();
        if draft.chars().count() + glyph.chars().count() <= MAX_TEXT_CHARS {
            draft.push_str(glyph);
        }
    }
}

fn messages(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId) {
    let state = &mut viewer.ui.friends.chat;
    let names = &viewer.ui.settings.game_names;
    let online = &*viewer.online;
    let rom = viewer
        .emulator
        .gb
        .cartridge
        .has_rom_loaded
        .then(|| viewer.emulator.rom_key())
        .flatten();
    let jump = std::mem::take(&mut state.jump);
    let clock = Clock::new();
    let mut actions = Vec::new();
    let mut seen = Vec::new();
    let (pressed, focused) =
        ui.input(|i| (i.pointer.any_pressed(), i.viewport().focused != Some(false)));
    if pressed {
        state.receipt = None;
    }
    scroll::gutter(ui);
    egui::ScrollArea::vertical()
        .id_salt(("chat_scroll", friend))
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let entries = online
                .conversation(friend)
                .map(|c| c.entries.as_slice())
                .unwrap_or_default();
            if entries.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("No messages yet").weak()));
            }
            let mut last_day = None;
            for entry in entries {
                let day = clock.day(entry.at);
                if day != last_day {
                    if let Some(day) = day {
                        date_row(ui, &clock, day);
                    }
                    last_day = day;
                }
                let bubble = entry_ui(
                    ui,
                    state,
                    online,
                    friend,
                    entry,
                    &clock,
                    rom,
                    names,
                    &mut actions,
                );
                if focused && entry.delivery == Delivery::Received && ui.is_rect_visible(bubble) {
                    seen.push(entry.id);
                }
            }
            if state.reading > 0 {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ui.label(RichText::new("Getting the file ready…").small().weak());
                });
            }
            ui.add_space(4.0);
            if jump {
                ui.scroll_to_cursor(Some(Align::BOTTOM));
            }
        });
    viewer.online.mark_seen(friend, &seen);
    for action in actions {
        match action {
            Action::Retry(id) => viewer.online.retry(friend, id),
            Action::Forget(id) => viewer.online.forget(friend, id),
            Action::Fetch(want, file, id) => {
                viewer
                    .ui
                    .friends
                    .chat
                    .want(viewer.online, want, friend, file, id);
            }
            Action::View(id) => viewer
                .ui
                .viewer
                .open(Source::Chat(friend), ItemKey::Chat(id)),
        }
    }
}

fn date_row(ui: &mut Ui, clock: &Clock, day: Date) {
    ui.add_space(6.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(clock.day_label(day)).small().weak());
    });
    ui.add_space(2.0);
}

#[allow(clippy::too_many_arguments)]
fn entry_ui(
    ui: &mut Ui,
    state: &mut ChatUi,
    online: &Online,
    friend: OnlineId,
    entry: &ChatEntry,
    clock: &Clock,
    rom: Option<&str>,
    names: &GameNames,
    actions: &mut Vec<Action>,
) -> Rect {
    let room = ui.available_width();
    let max = (room * BUBBLE_SHARE).max(120.0).min(room);
    let layout = if entry.mine {
        Layout::right_to_left(Align::Min)
    } else {
        Layout::left_to_right(Align::Min)
    };
    let mut own_tip = false;
    let bubble = ui.with_layout(layout, |ui| {
        ui.set_max_width(max);
        let big = match &entry.body {
            Body::Text(text) => emoji::only_emoji(&emoji::pieces(text))
                .filter(|n| *n <= BIG_EMOJI_MAX)
                .map(|count| BigRow::fit(count, max, stamp_width(ui, entry, clock))),
            _ => None,
        };
        let margin = if big.is_some() {
            egui::Margin::symmetric(0, BUBBLE_MARGIN.top)
        } else {
            BUBBLE_MARGIN
        };
        let fit_id = ui.id().with(("bubble_fit", entry.id));
        let wraps = match &entry.body {
            Body::Text(text) => {
                let needed = text_width(ui, text, big.as_ref(), entry, clock) + margin.sum().x;
                let fitted = ui
                    .data(|d| d.get_temp::<(f32, f32)>(fit_id))
                    .filter(|(at, _)| (at - max).abs() < 0.5)
                    .map(|(_, content)| content + margin.sum().x);
                let width = needed.min(fitted.unwrap_or(max));
                if width < max {
                    ui.set_max_width(width);
                }
                needed >= max
            }
            _ => false,
        };
        let fill = match big {
            Some(_) => Color32::TRANSPARENT,
            None => bubble_fill(ui.visuals(), entry.mine),
        };
        let first = paint_index(ui);
        let shown = egui::Frame::new()
            .fill(fill)
            .corner_radius(10.0)
            .inner_margin(margin)
            .show(ui, |ui| {
                let content =
                    ui.with_layout(Layout::top_down(Align::Min), |ui| match &entry.body {
                        Body::Text(text) => text_body(ui, text, big.as_ref(), entry, clock),
                        Body::Share(share) => {
                            own_tip = share_card(
                                ui, state, online, friend, entry, share, rom, names, actions,
                            );
                            stamp(ui, entry, clock);
                        }
                        Body::Receipt(_) => {}
                    });
                if wraps {
                    let width = content.response.rect.width();
                    let known = ui.data(|d| d.get_temp::<(f32, f32)>(fit_id));
                    if known.is_none_or(|(at, was)| {
                        (at - max).abs() >= 0.5 || (was - width).abs() >= 0.5
                    }) {
                        ui.data_mut(|d| d.insert_temp(fit_id, (max, width)));
                        ui.ctx().request_discard("chat bubble width");
                    }
                }
                content.response.rect
            });
        if wraps {
            hug_words(ui, first, shown.inner, entry.mine);
        }
        shown.response.rect
    });
    let bubble = bubble.inner;
    if entry.mine
        && let Some(text) = receipt_text(entry.delivery, clock)
    {
        let delay = ui.style().interaction.tooltip_delay;
        let (held, resting) = ui.input(|i| {
            let resting = !i.any_touches() && i.pointer.time_since_last_movement() >= delay;
            (long_pressed(i, bubble), resting)
        });
        if held {
            state.receipt = Some(entry.id);
        }
        if resting && !own_tip && ui.rect_contains_pointer(bubble) {
            let corner = bubble.right_bottom() - BUBBLE_MARGIN.right_bottom();
            let ticks = Rect::from_min_max(
                corner
                    - vec2(
                        stamp_width(ui, entry, clock),
                        ui.text_style_height(&egui::TextStyle::Small),
                    ),
                corner,
            );
            let mut tip = egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::new(("chat_receipt", entry.id)),
                ticks,
            )
            .gap(BUBBLE_MARGIN.bottom as f32 + 4.0);
            tip.popup = tip.popup.align(egui::RectAlign::BOTTOM_END);
            tip.show(|ui| ui.add(egui::Label::new(&text).extend()));
        }
        if state.receipt == Some(entry.id) {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.label(RichText::new(text).small().weak());
            });
        }
    }
    if let Delivery::Failed(failure) = entry.delivery {
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if failure == Failure::Gone {
                if ui.small_button("Remove").clicked() {
                    actions.push(Action::Forget(entry.id));
                }
            } else if ui
                .small_button(format!("{} Retry", icons::ARROW_CLOCKWISE))
                .clicked()
            {
                actions.push(Action::Retry(entry.id));
            }
            ui.label(
                RichText::new(format!("{} {}", icons::WARNING, failure.text()))
                    .small()
                    .color(ui.visuals().warn_fg_color),
            );
        });
    }
    if let Some(note) = state.notes.get(&entry.id) {
        let layout = if entry.mine {
            Layout::right_to_left(Align::Min)
        } else {
            Layout::left_to_right(Align::Min)
        };
        ui.with_layout(layout, |ui| {
            ui.label(
                RichText::new(format!("{} {note}", icons::WARNING))
                    .small()
                    .color(ui.visuals().warn_fg_color),
            );
        });
    }
    ui.add_space(4.0);
    bubble
}

fn paint_index(ui: &Ui) -> usize {
    ui.ctx().graphics(|layers| {
        layers
            .get(ui.layer_id())
            .map_or(0, |list| list.next_idx().0)
    })
}

fn hug_words(ui: &Ui, first: usize, content: egui::Rect, mine: bool) {
    let layer = ui.layer_id();
    ui.ctx().graphics_mut(|layers| {
        let list = layers.entry(layer);
        let end = list.next_idx();
        let right = list
            .all_entries()
            .skip(first + 1)
            .map(|clipped| clipped.shape.visual_bounding_rect().right())
            .fold(f32::NEG_INFINITY, f32::max);
        let slack = content.right() - right;
        if !(0.5..content.width()).contains(&slack) {
            return;
        }
        list.mutate_shape(egui::layers::ShapeIdx(first), |clipped| {
            if let egui::Shape::Rect(bubble) = &mut clipped.shape {
                if mine {
                    bubble.rect.min.x += slack;
                } else {
                    bubble.rect.max.x -= slack;
                }
            }
        });
        if mine {
            for index in first + 1..end.0 {
                list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                    clipped.shape.translate(vec2(slack, 0.0));
                });
            }
        }
    });
}

fn bubble_fill(visuals: &egui::Visuals, mine: bool) -> Color32 {
    if mine {
        visuals.selection.bg_fill
    } else {
        visuals.faint_bg_color
    }
}

fn text_body(ui: &mut Ui, text: &str, big: Option<&BigRow>, entry: &ChatEntry, clock: &Clock) {
    let pieces = emoji::pieces(text);
    if let Some(big) = big {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = BIG_EMOJI_GAP;
            for piece in &pieces {
                if let emoji::Piece::Emoji(index) = piece {
                    emoji::image(ui, *index, big.size);
                }
            }
            if big.beside {
                ui.add_space(STAMP_GAP - BIG_EMOJI_GAP);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), big.size),
                    Layout::bottom_up(Align::Max),
                    |ui| stamp_label(ui, entry, clock, false),
                );
            }
        });
        if !big.beside {
            let align = if entry.mine { Align::Max } else { Align::Min };
            ui.with_layout(Layout::top_down(align), |ui| {
                stamp_label(ui, entry, clock, false);
            });
        }
        return;
    }
    ui.spacing_mut().interact_size.y = 0.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for piece in &pieces {
            match piece {
                emoji::Piece::Text(part) => {
                    ui.label(RichText::new(*part).color(ui.visuals().strong_text_color()));
                }
                emoji::Piece::Emoji(index) => {
                    ui.add_space(1.0);
                    emoji::image(ui, *index, INLINE_EMOJI);
                    ui.add_space(1.0);
                }
            }
        }
        let stamp = stamp_width(ui, entry, clock);
        if ui.available_size_before_wrap().x >= TEXT_STAMP_GAP + stamp {
            ui.add_space(TEXT_STAMP_GAP);
        } else {
            let text = ui.min_rect().width();
            ui.end_row();
            ui.add_space((text - stamp).max(0.0));
        }
        stamp_label(ui, entry, clock, true);
    });
}

fn save_button(ui: &mut Ui) -> egui::Response {
    let full = format!("{} Save to Media", icons::DOWNLOAD_SIMPLE);
    let padding = ui.spacing().button_padding.x * 2.0;
    if measure(ui, RichText::new(&full)) + padding <= ui.available_width() {
        return ui.add(egui::Button::new(full).wrap_mode(egui::TextWrapMode::Extend));
    }
    let short = format!("{} Save", icons::DOWNLOAD_SIMPLE);
    ui.add(egui::Button::new(short).wrap_mode(egui::TextWrapMode::Extend))
        .on_hover_text("Save to Media")
}

fn stamp(ui: &mut Ui, entry: &ChatEntry, clock: &Clock) {
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        stamp_label(ui, entry, clock, true);
    });
}

fn measure(ui: &Ui, text: RichText) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

fn stamp_width(ui: &Ui, entry: &ChatEntry, clock: &Clock) -> f32 {
    measure(ui, RichText::new(stamp_text(entry, clock)).small())
}

struct BigRow {
    size: f32,
    beside: bool,
    width: f32,
}

impl BigRow {
    fn fit(count: usize, room: f32, stamp: f32) -> Self {
        let count = count as f32;
        let gaps = (count - 1.0) * BIG_EMOJI_GAP;
        let full = count * BIG_EMOJI + gaps;
        if full + STAMP_GAP + stamp + 1.0 <= room {
            return Self {
                size: BIG_EMOJI,
                beside: true,
                width: full + STAMP_GAP + stamp,
            };
        }
        let size = ((room - gaps - 1.0) / count)
            .floor()
            .clamp(INLINE_EMOJI, BIG_EMOJI);
        Self {
            size,
            beside: false,
            width: (count * size + gaps).max(stamp),
        }
    }
}

fn text_width(ui: &Ui, text: &str, big: Option<&BigRow>, entry: &ChatEntry, clock: &Clock) -> f32 {
    if let Some(big) = big {
        return big.width + 1.0;
    }
    let stamp = stamp_width(ui, entry, clock);
    let body: f32 = emoji::pieces(text)
        .iter()
        .map(|piece| match piece {
            emoji::Piece::Text(part) => measure(ui, RichText::new(*part)),
            emoji::Piece::Emoji(_) => INLINE_EMOJI + 2.0,
        })
        .sum();
    body + TEXT_STAMP_GAP + stamp + 1.0
}

fn stamp_mark(delivery: Delivery) -> Option<&'static str> {
    match delivery {
        Delivery::Sending => Some(icons::CLOCK),
        Delivery::Sent => Some(icons::CHECK),
        Delivery::Delivered(_) | Delivery::Read(_) => Some(icons::CHECKS),
        Delivery::Failed(_) => Some(icons::WARNING),
        Delivery::Received | Delivery::Seen => None,
    }
}

fn stamp_text(entry: &ChatEntry, clock: &Clock) -> String {
    match stamp_mark(entry.delivery) {
        Some(mark) => format!("{} {mark}", clock.time(entry.at)),
        None => clock.time(entry.at),
    }
}

fn stamp_label(ui: &mut Ui, entry: &ChatEntry, clock: &Clock, in_bubble: bool) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let base = if in_bubble && entry.mine {
        ui.visuals().strong_text_color().gamma_multiply(0.75)
    } else {
        ui.visuals().weak_text_color()
    };
    let mut job = egui::text::LayoutJob::default();
    let format = |color| egui::TextFormat::simple(font.clone(), color);
    job.append(&clock.time(entry.at), 0.0, format(base));
    if let Some(mark) = stamp_mark(entry.delivery) {
        let color = match entry.delivery {
            Delivery::Read(_) => read_color(ui.visuals()),
            Delivery::Sent | Delivery::Delivered(_) if in_bubble && entry.mine => {
                base.gamma_multiply(0.5)
            }
            _ => base,
        };
        job.append(&format!(" {mark}"), 0.0, format(color));
    }
    let label = ui.add(Label::new(job).extend());
    if let Delivery::Failed(failure) = entry.delivery {
        label.on_hover_text(failure.text());
    }
}

fn read_color(visuals: &egui::Visuals) -> Color32 {
    visuals.selection.stroke.color
}

fn receipt_text(delivery: Delivery, clock: &Clock) -> Option<String> {
    let when = |at: u64| match clock.day(at) {
        Some(day) if day != clock.today => clock
            .zoned(at)
            .map(|z| z.strftime("%-d %b, %H:%M").to_string())
            .unwrap_or_default(),
        _ => clock.time(at),
    };
    match delivery {
        Delivery::Sending => Some("Sending".to_owned()),
        Delivery::Sent => Some("Sent".to_owned()),
        Delivery::Delivered(at) => Some(format!("Delivered {}", when(at))),
        Delivery::Read(at) => Some(format!("Read {}", when(at))),
        Delivery::Failed(_) | Delivery::Received | Delivery::Seen => None,
    }
}

fn long_pressed(input: &egui::InputState, rect: Rect) -> bool {
    input.any_touches()
        && input.pointer.primary_down()
        && input
            .pointer
            .press_origin()
            .is_some_and(|at| rect.contains(at))
        && input
            .pointer
            .total_drag_delta()
            .is_none_or(|moved| moved.length() < LONG_PRESS_SLOP)
        && input
            .pointer
            .press_start_time()
            .is_some_and(|start| input.time - start >= LONG_PRESS_SECS)
}

#[allow(clippy::too_many_arguments)]
fn share_card(
    ui: &mut Ui,
    state: &mut ChatUi,
    online: &Online,
    friend: OnlineId,
    entry: &ChatEntry,
    share: &Share,
    rom: Option<&str>,
    names: &GameNames,
    actions: &mut Vec<Action>,
) -> bool {
    let game = names.name_or(share.rom_sha256.as_deref(), game_name(&share.game));
    let width = ui.available_width().min(PREVIEW_MAX);
    if let Some(texture) = state.preview(ui.ctx(), entry.id, &share.preview) {
        let size = texture.size_vec2();
        let scale = (width / size.x).min(2.0);
        let image = egui::Image::new((texture.id(), size * scale))
            .corner_radius(6.0)
            .sense(egui::Sense::click());
        if ui
            .add(image)
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
        {
            actions.push(Action::View(entry.id));
        }
    }
    ui.add_space(2.0);
    let title = ui.horizontal(|ui| {
        ui.label(share_icon(share.kind));
        ui.add(Label::new(RichText::new(game).strong()).truncate());
    });
    let facts = ui.add(Label::new(RichText::new(share_facts(share)).small().weak()).truncate());
    let own_tip = title.response.contains_pointer() || facts.contains_pointer();

    if entry.mine {
        if let Some((sent, total)) = online.uploading(friend, entry.id) {
            ui.label(
                RichText::new(format!("Sending {}%", percent(sent as u64, total as u64)))
                    .small()
                    .weak(),
            );
        }
        return own_tip;
    }
    if share.file == NO_FILE {
        return own_tip;
    }
    let loading = match online.fetched(share.file) {
        Some(Fetch::Loading { done, total }) => Some(percent(*done, *total)),
        _ => state.wanted.contains_key(&share.file).then_some(0),
    };
    ui.add_space(2.0);
    if let Some(done) = loading {
        ui.label(RichText::new(format!("Downloading {done}%")).small().weak());
        return own_tip;
    }
    match share.kind {
        ShareKind::Screenshot | ShareKind::Recording => {
            if state.saved.contains(&entry.id) {
                ui.label(
                    RichText::new(format!("{} Saved to Media", icons::CHECK))
                        .small()
                        .weak(),
                );
            } else if save_button(ui).clicked() {
                actions.push(Action::Fetch(Want::Save, share.file, entry.id));
            }
        }
        ShareKind::Replay => {
            let same = matches!(
                (rom, share.rom_sha256.as_deref()),
                (Some(loaded), Some(wanted)) if loaded.eq_ignore_ascii_case(wanted)
            );
            if same {
                if ui.button(format!("{} Play", icons::PLAY)).clicked() {
                    actions.push(Action::Fetch(Want::Play, share.file, entry.id));
                }
            } else {
                ui.add(
                    Label::new(
                        RichText::new(format!("Load {game} to play it"))
                            .small()
                            .weak(),
                    )
                    .wrap(),
                );
            }
        }
    }
    own_tip
}

fn from_media(ctx: &egui::Context, viewer: &mut TabViewer, friend: OnlineId) {
    if !viewer.ui.friends.chat.from_media {
        return;
    }
    let mut chosen = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("chat_from_media")).show(ctx, |ui| {
        let screen = ctx.content_rect();
        let width = (screen.width() - 48.0).clamp(240.0, 520.0);
        ui.set_width(width);
        ui.label(RichText::new("Share from Media").strong());
        ui.add_space(6.0);
        let gap = ui.spacing().item_spacing.x;
        let columns = ((width + gap) / (110.0 + gap)).floor().max(2.0);
        let cell_width = (width - gap * (columns - 1.0)) / columns;
        let cell = vec2(cell_width, cell_width * 0.9);
        let mut browse = std::mem::take(&mut viewer.ui.friends.chat.media);
        let height = (screen.height() * 0.6).max(cell.y);
        browse.show(
            ui,
            "chat_media",
            viewer.media.items(),
            Some(height),
            |ui, page| {
                ui.horizontal_wrapped(|ui| {
                    for item in page {
                        let texture = viewer.ui.media.texture(ctx, item);
                        let hint = format!(
                            "{}, {}",
                            game_name(&item.info.game),
                            crate::app::tabs::media::facts_line(item)
                        );
                        if thumbnail(ui, texture, item, cell)
                            .on_hover_text(hint)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            chosen = Some(item.clone());
                        }
                    }
                });
            },
        );
        viewer.ui.friends.chat.media = browse;
        ui.add_space(8.0);
        let size = vec2(ui.available_width(), ui.spacing().interact_size.y.max(32.0));
        if ui.add_sized(size, egui::Button::new("Cancel")).clicked() {
            close = true;
        }
    });
    if let Some(item) = chosen {
        viewer
            .ui
            .friends
            .chat
            .send_media(viewer.media, friend, &item);
        close = true;
    }
    if close || modal.should_close() {
        viewer.ui.friends.chat.from_media = false;
    }
}

pub fn pick_friend(ctx: &egui::Context, online: &Online, id: &str) -> Option<Option<OnlineId>> {
    let mut answer = None;
    let unix_now = crate::storage::now_unix();
    let modal = egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.set_width(300.0);
        ui.label(RichText::new("Send to").strong());
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .max_height(ctx.content_rect().height() * 0.6)
            .show(ui, |ui| {
                for person in online.friends() {
                    let height = 34.0_f32.max(ui.spacing().interact_size.y);
                    let row = ui
                        .allocate_ui_with_layout(
                            vec2(ui.available_width(), height),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                let presence = online.is_connected().then(|| seen(person.presence));
                                avatar::avatar(
                                    ui,
                                    &person.display_name,
                                    person.online_id.value(),
                                    30.0,
                                    presence,
                                );
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    ui.add(
                                        Label::new(RichText::new(&person.display_name).strong())
                                            .truncate(),
                                    );
                                    ui.add(
                                        Label::new(
                                            RichText::new(subtitle(online, person, unix_now))
                                                .small()
                                                .weak(),
                                        )
                                        .truncate(),
                                    );
                                });
                            },
                        )
                        .response
                        .interact(Sense::click())
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if row.hovered() {
                        ui.painter().rect_filled(
                            row.rect.expand(2.0),
                            6.0,
                            ui.visuals()
                                .widgets
                                .hovered
                                .weak_bg_fill
                                .gamma_multiply(0.4),
                        );
                    }
                    if row.clicked() {
                        answer = Some(Some(person.online_id));
                    }
                    ui.add_space(2.0);
                }
            });
        ui.add_space(8.0);
        let size = vec2(ui.available_width(), ui.spacing().interact_size.y.max(32.0));
        if ui.add_sized(size, egui::Button::new("Cancel")).clicked() {
            answer = Some(None);
        }
    });
    if modal.should_close() && answer.is_none() {
        answer = Some(None);
    }
    answer
}

pub fn unread_badge(ui: &Ui, avatar: Rect, count: usize) {
    count_pill(ui, avatar.right_top() + vec2(-3.0, 5.0), count);
}

pub fn count_pill(ui: &Ui, center: egui::Pos2, count: usize) {
    let text = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    let galley =
        ui.painter()
            .layout_no_wrap(text, egui::FontId::proportional(11.0), Color32::WHITE);
    let size = vec2((galley.size().x + 8.0).max(16.0), 16.0);
    let rect = Rect::from_center_size(center, size);
    ui.painter()
        .rect_filled(rect.expand(1.5), 9.5, ui.visuals().panel_fill);
    ui.painter().rect_filled(rect, 8.0, avatar::DOT);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

pub fn attention_hint(online: &Online, invited: bool) -> String {
    let mut parts = Vec::new();
    match online.waiting_requests() {
        0 => {}
        1 => parts.push("1 request".to_owned()),
        n => parts.push(format!("{n} requests")),
    }
    match online.total_unread() {
        0 => {}
        1 => parts.push("1 new message".to_owned()),
        n => parts.push(format!("{n} new messages")),
    }
    if invited {
        parts.push("invite to play".to_owned());
    }
    if parts.is_empty() {
        "Friends".to_owned()
    } else {
        format!("Friends ({})", parts.join(", "))
    }
}

fn share_entry(online: &Online, friend: OnlineId, id: u64) -> Option<Share> {
    online
        .conversation(friend)?
        .entries
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| match &e.body {
            Body::Share(share) => Some(share.clone()),
            _ => None,
        })
}

fn share_item(item: &MediaItem, bytes: Vec<u8>) -> Option<NewShare> {
    let info = &item.info;
    let (kind, data, width, height, preview) = match info.kind {
        MediaKind::Screenshot => {
            let packed = chat_media::pack_screenshot(&bytes)?;
            (
                ShareKind::Screenshot,
                packed.data,
                packed.width,
                packed.height,
                packed.preview,
            )
        }
        MediaKind::Recording => (
            ShareKind::Recording,
            bytes,
            info.width,
            info.height,
            item.thumbnail
                .as_ref()
                .map(chat_media::preview_of)
                .unwrap_or_default(),
        ),
    };
    Some(NewShare {
        kind,
        data,
        game: info.game.clone(),
        rom_sha256: info.rom_key.clone(),
        created_at: info.created_at,
        width,
        height,
        length_ms: info.length_ms,
        preview,
    })
}

fn replay_share(emulator: &Emulator) -> Option<NewShare> {
    let json = emulator.recorder.export_json()?;
    let recording = crate::replay::parse(json.as_bytes()).ok()?;
    let frame = Frame {
        width: media::THUMB_WIDTH,
        height: media::THUMB_HEIGHT,
        rgba: emulator.gb.frame().as_slice().to_vec(),
    };
    Some(NewShare {
        kind: ShareKind::Replay,
        game: emulator.game_name().to_string(),
        rom_sha256: Some(recording.rom_sha256.clone()),
        created_at: crate::storage::now_unix(),
        width: frame.width,
        height: frame.height,
        length_ms: Some(crate::replay::length_ms(&recording)),
        preview: chat_media::preview_of(&frame),
        data: json.into_bytes(),
    })
}

pub(crate) fn save_received(
    store: &mut MediaStore,
    share: &Share,
    bytes: &[u8],
) -> Result<MediaItem, &'static str> {
    let new = |kind, frame| NewMedia {
        kind,
        game: share.game.clone(),
        rom_key: share.rom_sha256.clone(),
        model: None,
        created_at: share.created_at,
        width: share.width,
        height: share.height,
        length_ms: share.length_ms,
        frame,
    };
    let saved = match share.kind {
        ShareKind::Screenshot => {
            let png = chat_media::unpack_screenshot(bytes, share.width, share.height)
                .ok_or(UNREADABLE)?;
            let frame = media::decode_png(&png).ok_or(UNREADABLE)?;
            store.add_screenshot(new(MediaKind::Screenshot, frame), &png)
        }
        ShareKind::Recording => {
            let frame = chat_media::decode_image(&share.preview).unwrap_or(Frame {
                width: 0,
                height: 0,
                rgba: Vec::new(),
            });
            store.add_file(new(MediaKind::Recording, frame), bytes)
        }
        ShareKind::Replay => return Err(UNREADABLE),
    };
    saved.map_err(|_| "Couldn't save it to Media")
}

fn replay_error(err: &ReplayError) -> String {
    match err {
        ReplayError::Unreadable => "This replay can't be read".to_owned(),
        ReplayError::OtherGame(game) => format!("Load {} to play it", game_name(game)),
    }
}

fn kind_word(item: &MediaItem) -> &'static str {
    match item.info.kind {
        MediaKind::Screenshot => "screenshot",
        MediaKind::Recording => "recording",
    }
}

fn share_icon(kind: ShareKind) -> &'static str {
    match kind {
        ShareKind::Screenshot => icons::CAMERA,
        ShareKind::Recording => icons::VIDEO_CAMERA,
        ShareKind::Replay => icons::GAME_CONTROLLER,
    }
}

pub fn share_label(kind: ShareKind) -> &'static str {
    match kind {
        ShareKind::Screenshot => "Screenshot",
        ShareKind::Recording => "Recording",
        ShareKind::Replay => "Input replay",
    }
}

pub fn share_facts(share: &Share) -> String {
    let mut facts = vec![share_label(share.kind).to_owned()];
    if let Some(ms) = share.length_ms {
        facts.push(media::format_length(ms));
    }
    if share.kind != ShareKind::Replay {
        facts.push(media::format_size(share.size));
    }
    facts.join(" · ")
}

fn percent(done: u64, total: u64) -> u64 {
    (done * 100).checked_div(total).unwrap_or(0).min(100)
}

struct Clock {
    zone: TimeZone,
    today: Date,
}

impl Clock {
    fn new() -> Self {
        let zone = TimeZone::system();
        let today = jiff::Timestamp::now().to_zoned(zone.clone()).date();
        Self { zone, today }
    }

    fn zoned(&self, at: u64) -> Option<jiff::Zoned> {
        jiff::Timestamp::from_second(at as i64)
            .ok()
            .map(|t| t.to_zoned(self.zone.clone()))
    }

    fn day(&self, at: u64) -> Option<Date> {
        self.zoned(at).map(|z| z.date())
    }

    fn time(&self, at: u64) -> String {
        self.zoned(at)
            .map(|z| z.strftime("%H:%M").to_string())
            .unwrap_or_default()
    }

    fn day_label(&self, day: Date) -> String {
        if day == self.today {
            "Today".to_owned()
        } else if self.today.yesterday().is_ok_and(|y| y == day) {
            "Yesterday".to_owned()
        } else {
            day.strftime("%-d %b %Y").to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn shapes_of(
        entry: &ChatEntry,
        names: &GameNames,
        width: f32,
    ) -> Vec<egui::epaint::ClippedShape> {
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
                "Are you free on Saturday? I finally got past the ice cave and want to trade"
                    .into(),
            ),
            delivery: Delivery::Sent,
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
            };
            for width in (200..=520).step_by(4) {
                let width = width as f32;
                let ctx = egui::Context::default();
                ctx.style_mut(|style| style.spacing.interact_size.y = 44.0);
                let shapes = shapes_in(ctx, &entry, &GameNames::default(), width);
                let painted = texts(&shapes);
                let Some((_, words)) = painted.iter().find(|(text, _)| text.starts_with("Are"))
                else {
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
                        egui::Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => {
                            Some(rect.rect)
                        }
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
                                header(ui, &online, &person, true);
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
}
