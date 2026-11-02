use crate::app::events::AppEvent;
use crate::app::tabs::friends::subtitle;
use crate::app::tabs::media::Browse;
use crate::app::tabs::{Tab, TabViewer};
use crate::app::viewer::{ItemKey, Source};
use crate::app::widgets::action::wide_button;
use crate::app::widgets::row::row_highlight;
use crate::app::widgets::text::faint;
use crate::app::widgets::{avatar, confirm, emoji, scroll};
use crate::icons;
use crate::media::{MediaItem, MediaStore};
use crate::online::chat::{ChatEntry, Delivery, Fetch};
use crate::online::{Online, chat_media};
use bubble::entry_ui;
use citrine_gb_net::{Body, FileId, Friend, OnlineId, ShareKind};
use clock::Clock;
use composer::{composer, picker};
use egui::{
    Align, CursorIcon, Key, Label, Layout, Modifiers, RichText, Sense, TextureHandle,
    TextureOptions, Ui, vec2,
};
use jiff::civil::Date;
use share::{from_media, kind_word, replay_error, save_received, share_entry, share_item};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};

mod bubble;
mod clock;
mod composer;
mod menu;
mod palette;
pub(crate) mod share;

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
const PALETTE_WIDTH: f32 = 200.0;
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
    Edit(u64),
    AskDelete(u64),
    AddPalette(u64),
}

struct Editing {
    friend: OnlineId,
    id: u64,
    stash: String,
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
    touch: bool,
    held: Option<u64>,
    editing: Option<Editing>,
    focus_draft: bool,
    deleting: Option<u64>,
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
            touch: false,
            held: None,
            editing: None,
            focus_draft: false,
            deleting: None,
        }
    }
}

impl ChatUi {
    pub fn open(&mut self, friend: OnlineId) {
        if self.open != Some(friend) {
            self.end_edit();
        }
        self.open = Some(friend);
        self.picker = false;
        self.from_media = false;
        self.jump = true;
    }

    pub fn close(&mut self) {
        self.end_edit();
        self.deleting = None;
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
        if self.editing.is_some() {
            self.end_edit();
            return true;
        }
        if self.open.is_some() {
            self.close();
            return true;
        }
        false
    }

    fn start_edit(&mut self, friend: OnlineId, id: u64, text: &str) {
        self.end_edit();
        let draft = self.drafts.entry(friend).or_default();
        let stash = std::mem::replace(draft, text.to_owned());
        self.editing = Some(Editing { friend, id, stash });
        self.picker = false;
        self.focus_draft = true;
    }

    fn end_edit(&mut self) {
        if let Some(editing) = self.editing.take() {
            self.drafts.insert(editing.friend, editing.stash);
        }
    }

    fn editing(&self, friend: OnlineId) -> Option<u64> {
        self.editing
            .as_ref()
            .filter(|e| e.friend == friend)
            .map(|e| e.id)
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
    let mobile = viewer.ui.settings.layout.is_touch(ui.ctx());
    viewer.ui.friends.chat.touch = mobile;
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
    ask_delete(ui.ctx(), viewer, &person);
    profile
}

fn ask_delete(ctx: &egui::Context, viewer: &mut TabViewer, person: &Friend) {
    let state = &mut viewer.ui.friends.chat;
    let Some(id) = state.deleting else {
        return;
    };
    let friend = person.online_id;
    let Some(entry) = viewer
        .online
        .conversation(friend)
        .and_then(|c| c.entries.iter().find(|e| e.id == id))
    else {
        state.deleting = None;
        return;
    };
    let question = delete_question(entry, &person.display_name);
    match confirm::confirm(ctx, "chat_delete", &question, "Delete") {
        Some(true) => {
            if state.editing(friend) == Some(id) {
                state.end_edit();
            }
            state.deleting = None;
            viewer.online.delete_message(friend, id);
        }
        Some(false) => state.deleting = None,
        None => {}
    }
}

fn delete_question(entry: &ChatEntry, name: &str) -> String {
    if matches!(entry.delivery, Delivery::Failed(_)) {
        return "Delete this unsent message?".to_owned();
    }
    let what = match &entry.body {
        Body::Share(share) => match share.kind {
            ShareKind::Screenshot => "screenshot",
            ShareKind::Recording => "recording",
            ShareKind::Replay => "replay",
        },
        Body::Palette(_) => "palette",
        _ => "message",
    };
    format!("Delete this {what} for you and {name}?")
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
            let presence = online.presence(person);
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

fn messages(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId) {
    let state = &mut viewer.ui.friends.chat;
    let names = &viewer.ui.settings.game_names;
    let palettes = &viewer.ui.settings.palettes;
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
                    palettes,
                    &mut actions,
                );
                if focused && entry.delivery == Delivery::Received && ui.is_rect_visible(bubble) {
                    seen.push(entry.id);
                }
            }
            if state.reading > 0 {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ui.label(faint("Getting the file ready…"));
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
            Action::Edit(id) => {
                let text = viewer
                    .online
                    .conversation(friend)
                    .and_then(|c| c.entries.iter().find(|e| e.id == id))
                    .and_then(|e| match &e.body {
                        Body::Text(text) => Some(text.clone()),
                        _ => None,
                    });
                if let Some(text) = text {
                    viewer.ui.friends.chat.start_edit(friend, id, &text);
                }
            }
            Action::AskDelete(id) => viewer.ui.friends.chat.deleting = Some(id),
            Action::AddPalette(id) => {
                let shared = viewer
                    .online
                    .conversation(friend)
                    .and_then(|c| c.entries.iter().find(|e| e.id == id))
                    .and_then(palette::shared_palette);
                if let Some(shared) = shared {
                    viewer.ui.settings.palettes.add_shared(&shared);
                    viewer.ui.settings.dirty = true;
                }
            }
        }
    }
}

fn date_row(ui: &mut Ui, clock: &Clock, day: Date) {
    ui.add_space(6.0);
    ui.vertical_centered(|ui| {
        ui.label(faint(clock.day_label(day)));
    });
    ui.add_space(2.0);
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
                                let presence = online.presence(person);
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
                                        Label::new(faint(subtitle(online, person, unix_now)))
                                            .truncate(),
                                    );
                                });
                            },
                        )
                        .response
                        .interact(Sense::click())
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if row.hovered() {
                        ui.painter().add(row_highlight(ui, row.rect));
                    }
                    if row.clicked() {
                        answer = Some(Some(person.online_id));
                    }
                    ui.add_space(2.0);
                }
            });
        ui.add_space(8.0);
        if wide_button(ui, egui::Button::new("Cancel")).clicked() {
            answer = Some(None);
        }
    });
    if modal.should_close() && answer.is_none() {
        answer = Some(None);
    }
    answer
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

#[cfg(test)]
mod tests;
