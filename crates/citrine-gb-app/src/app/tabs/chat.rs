use crate::app::big_screen::scroll::Following;
use crate::app::events::AppEvent;
use crate::app::tabs::friends::subtitle;
use crate::app::tabs::media::Browse;
use crate::app::tabs::{Tab, TabViewer};
use crate::app::ui_state::hotkeys::HotkeyAction;
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
const MESSAGES_PER_PAGE: usize = 200;
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
    older: usize,
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
    leave_draft: bool,
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
            older: 0,
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
            leave_draft: false,
            deleting: None,
        }
    }
}

impl ChatUi {
    pub fn open(&mut self, friend: OnlineId) {
        if self.open != Some(friend) {
            self.end_edit();
            self.older = 0;
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

    pub fn focus(&mut self) {
        self.focus_draft = self.open.is_some();
    }

    pub fn leave_field(&mut self) {
        self.leave_draft = true;
    }

    pub fn leaves_on_escape(&self) -> bool {
        !self.picker && self.composing().is_some()
    }

    pub fn field(&self) -> Option<egui::Id> {
        self.open.map(draft_id)
    }

    pub fn composing(&self) -> Option<(OnlineId, &str)> {
        let friend = self.open.filter(|_| self.editing.is_none())?;
        Some((friend, self.drafts.get(&friend).map_or("", String::as_str)))
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

pub fn draft_id(friend: OnlineId) -> egui::Id {
    egui::Id::new(("chat_draft", friend))
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
    let chip = if mobile {
        chip(viewer.play, viewer.online, friend)
    } else {
        None
    };
    match header(ui, viewer.online, &person, mobile, chip.as_ref()) {
        Some(Header::Back) => {
            viewer.ui.friends.chat.close();
            return false;
        }
        Some(Header::Profile) => profile = true,
        Some(Header::Chip) => match chip {
            Some(Chip::Play) => {
                let sent = crate::app::tabs::playing::invite(
                    viewer.play,
                    viewer.online,
                    viewer.emulator,
                    &mut viewer.ui.playing,
                    &viewer.ui.recent,
                    friend,
                );
                if !sent {
                    viewer.events.notify_error("Couldn't send the invite");
                }
            }
            Some(Chip::Join(invite)) => {
                if !crate::app::tabs::playing::join(
                    viewer.play,
                    viewer.online,
                    viewer.emulator,
                    &mut viewer.ui.playing,
                    &viewer.ui.recent,
                    &invite,
                ) {
                    viewer.events.notify_error("Couldn't join the game");
                }
            }
            Some(Chip::Playing) => viewer.events.open_tab(crate::app::tabs::Tab::GameBoy),
            None => {}
        },
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
            if can_chat {
                let typing = viewer.online.is_typing(friend, web_time::Instant::now());
                let typing = typing.then_some(person.display_name.as_str());
                let to_game =
                    !viewer.ui.friends.chat.touch && viewer.emulator.open_game().is_some();
                let field = if viewer.ui.friends.chat.editing(friend).is_some() {
                    Field::Editing
                } else if ui.memory(|m| m.has_focus(draft_id(friend))) {
                    Field::Focused
                } else {
                    Field::Away
                };
                let key = viewer.ui.settings.hotkeys.key_name(HotkeyAction::FocusChat);
                typing_slot(ui, slot(typing, to_game, field, key.as_deref()), to_game);
            }
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
    Chip,
}

#[derive(Debug, Clone)]
pub enum Chip {
    Play,
    Join(crate::play::IncomingInvite),
    Playing,
}

impl Chip {
    fn label(&self) -> &'static str {
        match self {
            Chip::Play => "Play",
            Chip::Join(_) => "Join",
            Chip::Playing => "Playing",
        }
    }
}

pub fn chip(play: &crate::play::Play, online: &Online, friend: OnlineId) -> Option<Chip> {
    if play
        .session()
        .is_some_and(|session| session.is_live() && session.friend() == friend)
    {
        return Some(Chip::Playing);
    }
    if let Some(invite) = play.invites().iter().find(|invite| invite.from == friend) {
        return Some(Chip::Join(invite.clone()));
    }
    if crate::app::tabs::playing::invite_closed(play, online, friend) {
        return None;
    }
    match crate::app::tabs::playing::play_button(play, online, friend) {
        Some(crate::app::tabs::playing::PlayButton::Ready) => Some(Chip::Play),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Field {
    Away,
    Focused,
    Editing,
}

#[derive(Debug, PartialEq)]
enum Slot {
    Typing(String),
    Tip(String),
}

fn slot(typing: Option<&str>, to_game: bool, field: Field, key: Option<&str>) -> Option<Slot> {
    if let Some(text) = typing.and_then(|name| crate::app::widgets::typing::chat_line(&[name])) {
        return Some(Slot::Typing(text));
    }
    if !to_game {
        return None;
    }
    match (field, key) {
        (Field::Away, Some(key)) => Some(Slot::Tip(format!("{key} to chat"))),
        (Field::Focused, Some(key)) => {
            Some(Slot::Tip(format!("{key} or Esc to go back to the game")))
        }
        (Field::Focused, None) => Some(Slot::Tip("Esc to go back to the game".to_owned())),
        _ => None,
    }
}

fn typing_slot(ui: &mut Ui, slot: Option<Slot>, keep: bool) {
    if slot.is_none() && !keep {
        return;
    }
    let style = egui::TextStyle::Small;
    ui.add_space(2.0);
    match slot {
        Some(Slot::Typing(text)) => {
            let color = ui.visuals().text_color();
            crate::app::widgets::typing::line(ui, &text, style, color);
        }
        tip => {
            let size = vec2(ui.available_width(), ui.text_style_height(&style));
            ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
                if let Some(Slot::Tip(text)) = tip {
                    ui.add(Label::new(RichText::new(text).text_style(style).weak()).truncate());
                }
            });
        }
    }
}

fn header(
    ui: &mut Ui,
    online: &Online,
    person: &Friend,
    mobile: bool,
    chip: Option<&Chip>,
) -> Option<Header> {
    let mut action = None;
    let unix_now = crate::storage::now_unix();
    let height = HEADER_AVATAR.max(ui.spacing().interact_size.y);
    let side = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::left_to_right(Align::Center),
        |ui| {
            let back = if mobile {
                crate::app::mobile::kit::header_button(icons::ARROW_LEFT, side)
            } else {
                egui::Button::new(icons::ARROW_LEFT)
            };
            if ui.add(back).on_hover_text("Back to friends").clicked() {
                action = Some(Header::Back);
            }
            if let Some(chip) = chip {
                let room = ui.available_width();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(4.0);
                    let button = crate::app::widgets::action::main_button(ui, chip.label())
                        .corner_radius(egui::CornerRadius::same(18))
                        .min_size(vec2(72.0, 36.0));
                    if ui.add(button).clicked() {
                        action = Some(Header::Chip);
                    }
                    ui.allocate_ui_with_layout(
                        vec2(ui.available_width().min(room), height),
                        Layout::left_to_right(Align::Center),
                        |ui| person_line(ui, online, person, unix_now, &mut action),
                    );
                });
                return;
            }
            person_line(ui, online, person, unix_now, &mut action);
        },
    );
    action
}

fn person_line(
    ui: &mut Ui,
    online: &Online,
    person: &Friend,
    unix_now: u64,
    action: &mut Option<Header>,
) {
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
        *action = Some(Header::Profile);
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
            *action = Some(Header::Profile);
        }
    });
}

fn messages(ui: &mut Ui, viewer: &mut TabViewer, friend: OnlineId) {
    let state = &mut viewer.ui.friends.chat;
    let names = &viewer.ui.settings.game_names;
    let palettes = &viewer.ui.settings.palettes;
    let online = &*viewer.online;
    let rom = viewer
        .emulator
        .has_game()
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
        .show_following(ui, |ui| {
            let all = online
                .conversation(friend)
                .map(|c| c.entries.as_slice())
                .unwrap_or_default();
            if all.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("No messages yet").weak()));
            }
            let start = all
                .len()
                .saturating_sub(MESSAGES_PER_PAGE * (state.older + 1));
            if start > 0 {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    if ui.button("Show older messages").clicked() {
                        state.older += 1;
                    }
                });
                ui.add_space(4.0);
            }
            let entries = &all[start..];
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
            .show_following(ui, |ui| {
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
mod tests {
    use super::*;
    use crate::app::ui_state::palette::PaletteLibrary;
    use crate::game_name::GameNames;
    use crate::online::chat::{Change, ChatEntry};
    use citrine_gb_net::{Body, Share, ShareKind};
    use egui::{Color32, Rect};
    use jiff::tz::TimeZone;

    pub(super) fn share(kind: ShareKind, size: u64, length_ms: Option<u64>) -> Share {
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

    pub(super) fn shapes_in(
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

    pub(super) fn test_clock() -> Clock {
        Clock {
            zone: TimeZone::UTC,
            today: jiff::civil::date(2026, 10, 31),
        }
    }

    pub(super) fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
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
                "Are you free on Saturday? I finally got past the ice cave and want to trade"
                    .into(),
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
}
