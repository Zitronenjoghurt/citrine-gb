use crate::app::events::AppEvent;
use crate::app::tabs::media::{detail_line, game_name, matches};
use crate::app::tabs::{Tab, TabViewer, chat};
use crate::app::widgets::{confirm, emoji};
use crate::icons;
use crate::media::playback::{self, OpenError, Player};
use crate::media::{Frame, MediaId, MediaItem, MediaKind};
use crate::online::chat::{ChatEntry, Fetch, NewShare};
use crate::online::chat_media;
use citrine_gb_net::{Body, FileId, OnlineId, Share, ShareKind};
use egui::{
    Align, Align2, Color32, ColorImage, Context, Id, Key, Layout, Modifiers, Rect, RichText, Sense,
    TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2,
};
use std::sync::mpsc::{Receiver, Sender, channel};

const BAR: f32 = 48.0;
const CONTROLS: f32 = 52.0;
const ARROW: f32 = 44.0;
const GAP: f32 = 12.0;
const SWIPE: f32 = 60.0;
const SWIPE_CLOSE: f32 = 110.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Media,
    Chat(OnlineId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKey {
    Media(MediaId),
    Chat(u64),
}

enum Status {
    Loading(Option<u64>),
    Ready,
    NeedsCodec,
    Note(String),
}

struct Shown {
    key: ItemKey,
    texture: Option<TextureHandle>,
    player: Option<Player>,
    bytes: Option<Vec<u8>>,
    pending: Option<Vec<u8>>,
    status: Status,
    fetching: Option<FileId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    Close,
    Step(isize),
    Toggle,
    Seek(f64),
    Mute,
    Volume(f32),
    Save,
    Export,
    Send,
    Delete,
    PlayReplay,
    DownloadCodec,
    #[cfg(not(target_arch = "wasm32"))]
    Reveal,
}

pub struct ViewerState {
    open: Option<(Source, ItemKey)>,
    shown: Option<Shown>,
    tx: Sender<(ItemKey, Option<Vec<u8>>)>,
    rx: Receiver<(ItemKey, Option<Vec<u8>>)>,
    drag: Vec2,
    paused_game: bool,
    asking_delete: Option<MediaId>,
    picking: bool,
    muted: bool,
    volume: f32,
}

impl Default for ViewerState {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            open: None,
            shown: None,
            tx,
            rx,
            drag: Vec2::ZERO,
            paused_game: false,
            asking_delete: None,
            picking: false,
            muted: false,
            volume: 1.0,
        }
    }
}

impl ViewerState {
    pub fn open(&mut self, source: Source, key: ItemKey) {
        self.open = Some((source, key));
        self.drag = Vec2::ZERO;
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
        self.shown = None;
        self.asking_delete = None;
        self.picking = false;
        self.drag = Vec2::ZERO;
    }

    pub fn current(&self) -> Option<&ItemKey> {
        self.open.as_ref().map(|(_, key)| key)
    }

    pub fn step(&mut self, keys: &[ItemKey], by: isize) -> bool {
        let Some((source, key)) = &self.open else {
            return false;
        };
        let Some(next) = neighbour(keys, key, by) else {
            return false;
        };
        self.open = Some((*source, next));
        true
    }
}

pub fn neighbour(keys: &[ItemKey], key: &ItemKey, by: isize) -> Option<ItemKey> {
    let at = keys.iter().position(|k| k == key)? as isize + by;
    usize::try_from(at).ok().and_then(|i| keys.get(i)).cloned()
}

pub fn swipe(drag: Vec2) -> Option<Act> {
    if drag.y > SWIPE_CLOSE && drag.y > drag.x.abs() * 1.5 {
        return Some(Act::Close);
    }
    if drag.x.abs() > SWIPE && drag.x.abs() > drag.y.abs() {
        return Some(Act::Step(if drag.x < 0.0 { 1 } else { -1 }));
    }
    None
}

pub fn keys_pressed(ctx: &Context, clip: bool) -> Vec<Act> {
    let mut acts = Vec::new();
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
            acts.push(Act::Step(-1));
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
            acts.push(Act::Step(1));
        }
        if clip && i.consume_key(Modifiers::NONE, Key::Space) {
            acts.push(Act::Toggle);
        }
        if clip && i.consume_key(Modifiers::NONE, Key::K) {
            acts.push(Act::Toggle);
        }
        if clip && i.consume_key(Modifiers::NONE, Key::M) {
            acts.push(Act::Mute);
        }
    });
    acts
}

fn keys(viewer: &TabViewer, source: Source) -> Vec<ItemKey> {
    match source {
        Source::Media => {
            let browse = &viewer.ui.media.browse;
            viewer
                .media
                .items()
                .iter()
                .filter(|item| matches(item, browse.kind, browse.game.as_deref()))
                .map(|item| ItemKey::Media(item.id.clone()))
                .collect()
        }
        Source::Chat(friend) => viewer
            .online
            .conversation(friend)
            .map(|c| {
                c.entries
                    .iter()
                    .filter(|e| matches!(e.body, Body::Share(_)))
                    .map(|e| ItemKey::Chat(e.id))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn media_item<'a>(viewer: &'a TabViewer, id: &MediaId) -> Option<&'a MediaItem> {
    viewer.media.items().iter().find(|item| &item.id == id)
}

fn chat_entry<'a>(
    viewer: &'a TabViewer,
    friend: OnlineId,
    id: u64,
) -> Option<(&'a ChatEntry, &'a Share)> {
    viewer
        .online
        .conversation(friend)?
        .entries
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| match &e.body {
            Body::Share(share) => Some((e, share)),
            _ => None,
        })
}

fn local_copy<'a>(viewer: &'a TabViewer, share: &Share) -> Option<&'a MediaItem> {
    let kind = match share.kind {
        ShareKind::Screenshot => MediaKind::Screenshot,
        ShareKind::Recording => MediaKind::Recording,
        ShareKind::Replay => return None,
    };
    viewer.media.items().iter().find(|item| {
        item.info.kind == kind
            && item.info.created_at == share.created_at
            && item.info.game == share.game
    })
}

fn texture_of(ctx: &Context, name: &str, frame: &Frame) -> TextureHandle {
    ctx.load_texture(name, color_image(frame), TextureOptions::NEAREST)
}

fn color_image(frame: &Frame) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba)
}

fn read_local(viewer: &mut TabViewer, key: &ItemKey, id: &MediaId) {
    let tx = viewer.ui.viewer.tx.clone();
    let key = key.clone();
    viewer.media.read_bytes(id, move |bytes| {
        let _ = tx.send((key, bytes));
    });
}

fn begin(viewer: &mut TabViewer, ctx: &Context, source: Source, key: &ItemKey) {
    let mut shown = Shown {
        key: key.clone(),
        texture: None,
        player: None,
        bytes: None,
        pending: None,
        status: Status::Loading(None),
        fetching: None,
    };
    match (source, key) {
        (Source::Media, ItemKey::Media(id)) => {
            if let Some(item) = media_item(viewer, id) {
                shown.texture = item
                    .thumbnail
                    .as_ref()
                    .map(|t| texture_of(ctx, "viewer", t));
                viewer.ui.viewer.shown = Some(shown);
                read_local(viewer, key, id);
                return;
            }
        }
        (Source::Chat(friend), ItemKey::Chat(id)) => {
            if let Some((entry, share)) = chat_entry(viewer, friend, *id) {
                shown.texture = chat_media::decode_image(&share.preview)
                    .map(|frame| texture_of(ctx, "viewer", &frame));
                let local = local_copy(viewer, share).map(|item| item.id.clone());
                let mine = entry.mine;
                let file = share.file;
                let kind = share.kind;
                if kind == ShareKind::Replay {
                    shown.status = Status::Ready;
                } else if let Some(local) = local {
                    viewer.ui.viewer.shown = Some(shown);
                    read_local(viewer, key, &local);
                    return;
                } else if mine || file == FileId([0; 16]) {
                    shown.status = match kind {
                        ShareKind::Recording => {
                            Status::Note("This recording isn't in your Media anymore".into())
                        }
                        _ => Status::Ready,
                    };
                } else {
                    match viewer.online.fetched(file) {
                        Some(Fetch::Ready(_)) | Some(Fetch::Failed(_)) => {
                            let _ = viewer.online.take_fetched(file);
                        }
                        _ => {}
                    }
                    viewer.online.fetch(friend, file);
                    shown.fetching = Some(file);
                }
            }
        }
        _ => {}
    }
    viewer.ui.viewer.shown = Some(shown);
}

fn pump(viewer: &mut TabViewer, ctx: &Context, source: Source) {
    let state = &mut viewer.ui.viewer;
    while let Ok((key, bytes)) = state.rx.try_recv() {
        if let Some(shown) = state.shown.as_mut().filter(|s| s.key == key) {
            match bytes {
                Some(bytes) => shown.pending = Some(bytes),
                None => shown.status = Status::Note("This file can't be opened".into()),
            }
        }
    }
    let fetched = state
        .shown
        .as_ref()
        .and_then(|s| s.fetching)
        .and_then(|file| match viewer.online.fetched(file) {
            Some(Fetch::Loading { done, total }) => Some((file, Some(percent(*done, *total)))),
            Some(_) => Some((file, None)),
            None => None,
        });
    if let Some((file, loading)) = fetched
        && let Some(shown) = viewer.ui.viewer.shown.as_mut()
    {
        match loading {
            Some(done) => shown.status = Status::Loading(Some(done)),
            None => {
                shown.fetching = None;
                match viewer.online.take_fetched(file) {
                    Some(Fetch::Ready(bytes)) => shown.pending = Some(bytes),
                    Some(Fetch::Failed(text)) => shown.status = Status::Note(text.to_owned()),
                    _ => {}
                }
            }
        }
    }

    let Some(pending) = viewer
        .ui
        .viewer
        .shown
        .as_mut()
        .and_then(|s| s.pending.take())
    else {
        return;
    };
    let Some(key) = viewer.ui.viewer.shown.as_ref().map(|s| s.key.clone()) else {
        return;
    };
    let (kind, share) = match (source, &key) {
        (Source::Media, ItemKey::Media(id)) => match media_item(viewer, id) {
            Some(item) => (item.info.kind, None),
            None => return,
        },
        (Source::Chat(friend), ItemKey::Chat(id)) => match chat_entry(viewer, friend, *id) {
            Some((entry, share)) => {
                let local = !entry.mine && local_copy(viewer, share).is_none();
                let kind = match share.kind {
                    ShareKind::Recording => MediaKind::Recording,
                    _ => MediaKind::Screenshot,
                };
                (kind, local.then(|| share.clone()))
            }
            None => return,
        },
        _ => return,
    };
    let shown = viewer.ui.viewer.shown.as_mut().expect("checked above");
    match kind {
        MediaKind::Screenshot => {
            let png = match &share {
                Some(share) => chat_media::unpack_screenshot(&pending, share.width, share.height),
                None => Some(pending.clone()),
            };
            match png.as_deref().and_then(crate::media::decode_png) {
                Some(frame) => {
                    shown.texture = Some(texture_of(ctx, "viewer", &frame));
                    shown.status = Status::Ready;
                }
                None => shown.status = Status::Note("This file can't be opened".into()),
            }
        }
        MediaKind::Recording => open_clip(
            shown,
            pending.clone(),
            viewer.ui.viewer.muted,
            viewer.ui.viewer.volume,
        ),
    }
    if let Some(shown) = viewer.ui.viewer.shown.as_mut() {
        shown.bytes = Some(pending);
    }
}

fn open_clip(shown: &mut Shown, bytes: Vec<u8>, muted: bool, volume: f32) {
    match Player::open(bytes) {
        Ok(mut player) => {
            player.set_muted(muted);
            player.set_volume(volume);
            shown.player = Some(player);
            shown.status = Status::Ready;
        }
        Err(OpenError::NeedsCodec) => shown.status = Status::NeedsCodec,
        Err(OpenError::Unreadable) => {
            shown.status = Status::Note("This recording can't be played".into())
        }
    }
}

fn tick(ctx: &Context, shown: &mut Shown) {
    let Some(player) = shown.player.as_mut() else {
        return;
    };
    player.update();
    if player.failed() {
        shown.player = None;
        shown.status = Status::Note("This browser can't play this recording".into());
        return;
    }
    if let Some(frame) = player.take_frame() {
        match &mut shown.texture {
            Some(texture) if texture.size() == [frame.width as usize, frame.height as usize] => {
                texture.set(color_image(frame), TextureOptions::NEAREST);
            }
            texture => *texture = Some(texture_of(ctx, "viewer", frame)),
        }
    }
    if player.is_playing() || !player.is_ready() {
        ctx.request_repaint();
    }
}

fn percent(done: u64, total: u64) -> u64 {
    (done * 100).checked_div(total).unwrap_or(0).min(100)
}

struct Facts {
    icon: &'static str,
    title: String,
    line: String,
    short: String,
    clip: bool,
    replay: bool,
    can_save: bool,
    saved: bool,
    can_send: bool,
    deletable: bool,
    replay_note: Option<String>,
}

fn facts(
    viewer: &TabViewer,
    source: Source,
    key: &ItemKey,
    place: (usize, usize),
) -> Option<Facts> {
    let count = (place.1 > 1).then(|| format!("{} of {}", place.0 + 1, place.1));
    let join = |facts: Vec<Option<String>>| {
        count
            .iter()
            .cloned()
            .chain(facts.into_iter().flatten())
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let friends = viewer.ui.settings.online.is_joined() && !viewer.online.friends().is_empty();
    let has_bytes = viewer
        .ui
        .viewer
        .shown
        .as_ref()
        .is_some_and(|s| s.bytes.is_some());
    match (source, key) {
        (Source::Media, ItemKey::Media(id)) => {
            let item = media_item(viewer, id)?;
            Some(Facts {
                icon: match item.info.kind {
                    MediaKind::Screenshot => icons::CAMERA,
                    MediaKind::Recording => icons::VIDEO_CAMERA,
                },
                title: game_name(&item.info.game).to_owned(),
                line: join(vec![Some(detail_line(item))]),
                short: join(vec![
                    Some(crate::media::local_stamp(
                        item.info.created_at,
                        "%-d %b %Y, %H:%M",
                    )),
                    item.info.length_ms.map(crate::media::format_length),
                ]),
                clip: item.info.kind == MediaKind::Recording,
                replay: false,
                can_save: false,
                saved: false,
                can_send: friends,
                deletable: true,
                replay_note: None,
            })
        }
        (Source::Chat(friend), ItemKey::Chat(id)) => {
            let (entry, share) = chat_entry(viewer, friend, *id)?;
            let from = if entry.mine {
                "You".to_owned()
            } else {
                viewer
                    .online
                    .friend(friend)
                    .map(|f| f.display_name.clone())
                    .unwrap_or_default()
            };
            let when = crate::media::local_stamp(entry.at, "%-d %b %Y, %H:%M");
            let local = local_copy(viewer, share).is_some();
            let rom = viewer
                .emulator
                .gb
                .cartridge
                .has_rom_loaded
                .then(|| viewer.emulator.rom_key())
                .flatten();
            let same_game = matches!(
                (rom, share.rom_sha256.as_deref()),
                (Some(loaded), Some(wanted)) if loaded.eq_ignore_ascii_case(wanted)
            );
            let replay = share.kind == ShareKind::Replay;
            Some(Facts {
                icon: match share.kind {
                    ShareKind::Screenshot => icons::CAMERA,
                    ShareKind::Recording => icons::VIDEO_CAMERA,
                    ShareKind::Replay => icons::GAME_CONTROLLER,
                },
                title: game_name(&share.game).to_owned(),
                line: join(vec![
                    Some(from),
                    Some(when.clone()),
                    share.length_ms.map(crate::media::format_length),
                    (!replay).then(|| crate::media::format_size(share.size)),
                ]),
                short: join(vec![
                    Some(when),
                    share.length_ms.map(crate::media::format_length),
                ]),
                clip: share.kind == ShareKind::Recording,
                replay,
                can_save: !entry.mine && !replay,
                saved: viewer.ui.friends.chat.is_saved(entry.id) || (!entry.mine && local),
                can_send: friends && !replay && (has_bytes || local),
                deletable: false,
                replay_note: (replay && !entry.mine && !same_game)
                    .then(|| format!("Load {} to play it", game_name(&share.game))),
            })
        }
        _ => None,
    }
}

pub fn show(viewer: &mut TabViewer, ctx: &Context) {
    let Some((source, key)) = viewer.ui.viewer.open.clone() else {
        if std::mem::take(&mut viewer.ui.viewer.paused_game) {
            viewer.emulator.running = true;
        }
        return;
    };
    let list = keys(viewer, source);
    let Some(index) = list.iter().position(|k| *k == key) else {
        viewer.ui.viewer.close();
        return;
    };
    if !viewer.ui.viewer.paused_game
        && viewer.emulator.running
        && viewer.emulator.link.is_none()
        && !viewer.play.is_live()
    {
        viewer.emulator.running = false;
        viewer.ui.viewer.paused_game = true;
    }
    if viewer.ui.viewer.shown.as_ref().is_none_or(|s| s.key != key) {
        begin(viewer, ctx, source, &key);
    }
    pump(viewer, ctx, source);
    if let Some(shown) = viewer.ui.viewer.shown.as_mut() {
        tick(ctx, shown);
    }
    let Some(facts) = facts(viewer, source, &key, (index, list.len())) else {
        viewer.ui.viewer.close();
        return;
    };

    let dialog_open = viewer.ui.viewer.asking_delete.is_some() || viewer.ui.viewer.picking;
    let mut acts = if dialog_open {
        Vec::new()
    } else {
        keys_pressed(ctx, facts.clip)
    };
    let mobile = crate::app::mobile_layout(viewer.ui.settings.layout, ctx);
    let screen = ctx.content_rect();
    let backdrop = ctx.style().visuals.extreme_bg_color.to_opaque();
    let modal = egui::Modal::new(Id::new("media_viewer"))
        .area(
            egui::Modal::default_area(Id::new("media_viewer"))
                .anchor(Align2::LEFT_TOP, screen.min.to_vec2()),
        )
        .frame(egui::Frame::NONE)
        .backdrop_color(backdrop)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size());
            ui.set_max_size(screen.size());
            draw(ui, viewer, &facts, (index, list.len()), mobile, &mut acts);
        });
    if !dialog_open && modal.should_close() {
        acts.push(Act::Close);
    }

    for act in acts {
        apply(viewer, ctx, source, &key, &list, act);
    }
    ask_delete(viewer, ctx, &list);
    pick_friend(viewer, ctx, source);
}

fn draw(
    ui: &mut Ui,
    viewer: &mut TabViewer,
    facts: &Facts,
    place: (usize, usize),
    mobile: bool,
    acts: &mut Vec<Act>,
) {
    let full = ui.max_rect();
    let back = ui.interact(full, Id::new("viewer_back"), Sense::click_and_drag());
    let state = &mut viewer.ui.viewer;
    if back.dragged() {
        state.drag += back.drag_delta();
    }
    if back.drag_stopped() {
        if let Some(act) = swipe(state.drag) {
            acts.push(act);
        }
        state.drag = Vec2::ZERO;
    } else if back.clicked() {
        acts.push(Act::Close);
    }

    let bottom = if facts.clip { CONTROLS } else { 0.0 };
    let stage = Rect::from_min_max(
        pos2(full.left() + GAP, full.top() + BAR + GAP),
        pos2(full.right() - GAP, full.bottom() - bottom - GAP),
    );
    let arrows = !mobile && place.1 > 1;
    let room = if arrows {
        stage.shrink2(vec2(ARROW + GAP, 0.0))
    } else {
        stage
    };
    let drag = if state.drag.x.abs() > state.drag.y.abs() {
        vec2(state.drag.x, 0.0)
    } else {
        vec2(0.0, state.drag.y.max(0.0))
    };

    let shown = state.shown.as_ref();
    if let Some(texture) = shown.and_then(|s| s.texture.as_ref()) {
        let rect = fit(texture.size_vec2(), room, ui.ctx().pixels_per_point()).translate(drag);
        let image = ui.interact(rect, Id::new("viewer_image"), Sense::click_and_drag());
        if image.dragged() {
            state.drag += image.drag_delta();
        }
        if image.drag_stopped() {
            if let Some(act) = swipe(state.drag) {
                acts.push(act);
            }
            state.drag = Vec2::ZERO;
        } else if image.clicked() && facts.clip {
            acts.push(Act::Toggle);
        }
        ui.painter().image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        let paused = shown
            .and_then(|s| s.player.as_ref())
            .is_some_and(|p| !p.is_playing() && p.is_ready());
        if paused {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                icons::PLAY_CIRCLE,
                egui::FontId::proportional(rect.height().min(rect.width()) * 0.22),
                Color32::from_white_alpha(230),
            );
        }
    }

    let note = match shown.map(|s| &s.status) {
        Some(Status::Loading(Some(done))) => Some(format!("Downloading {done}%")),
        Some(Status::Note(text)) => Some(text.clone()),
        _ => facts.replay_note.clone(),
    };
    let needs_codec = matches!(shown.map(|s| &s.status), Some(Status::NeedsCodec));
    if note.is_some() || needs_codec || facts.replay {
        let at = Rect::from_center_size(
            pos2(room.center().x, room.bottom() - 40.0),
            vec2(room.width().min(360.0), 64.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(note) = &note {
                        ui.label(note);
                    }
                    if needs_codec && cfg!(target_arch = "wasm32") {
                        ui.label("This browser can't play MP4 video.");
                    } else if needs_codec {
                        ui.label("Playing needs Cisco's video codec, a one-time download of about 600 KB.");
                        if ui.button(format!("{} Download", icons::DOWNLOAD_SIMPLE)).clicked() {
                            acts.push(Act::DownloadCodec);
                        }
                    }
                    if facts.replay
                        && facts.replay_note.is_none()
                        && note.is_none()
                        && ui.button(format!("{} Play in the Game Boy", icons::PLAY)).clicked()
                    {
                        acts.push(Act::PlayReplay);
                    }
                });
            });
        });
    }

    if arrows {
        let middle = stage.center().y;
        for (by, icon, x) in [
            (-1, icons::CARET_LEFT, stage.left() + ARROW / 2.0),
            (1, icons::CARET_RIGHT, stage.right() - ARROW / 2.0),
        ] {
            let next = place.0 as isize + by;
            if next < 0 || next >= place.1 as isize {
                continue;
            }
            let rect = Rect::from_center_size(pos2(x, middle), Vec2::splat(ARROW));
            let hint = if by < 0 { "Previous" } else { "Next" };
            let response = ui
                .put(
                    rect,
                    egui::Button::new(RichText::new(icon).size(22.0)).corner_radius(ARROW / 2.0),
                )
                .on_hover_text(hint);
            if response.clicked() {
                acts.push(Act::Step(by));
            }
        }
    }

    top_bar(
        ui,
        Rect::from_min_size(full.min, vec2(full.width(), BAR)),
        facts,
        mobile,
        acts,
    );
    if facts.clip {
        let bar = Rect::from_min_max(pos2(full.left(), full.bottom() - CONTROLS), full.max);
        controls(ui, bar, viewer, mobile, acts);
    }
}

pub fn fit(size: Vec2, room: Rect, pixels_per_point: f32) -> Rect {
    let scale = (room.width() / size.x)
        .min(room.height() / size.y)
        .max(0.01);
    let pixels = scale * pixels_per_point;
    let scale = if pixels >= 3.0 {
        pixels.floor() / pixels_per_point
    } else {
        scale
    };
    Rect::from_center_size(room.center(), size * scale)
}

fn top_bar(ui: &mut Ui, rect: Rect, facts: &Facts, mobile: bool, acts: &mut Vec<Act>) {
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().panel_fill.gamma_multiply(0.92));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(8.0, 4.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            let side = (BAR - 12.0).min(ui.spacing().interact_size.y.max(32.0));
            if ui
                .add(
                    egui::Button::new(RichText::new(icons::X).size(18.0))
                        .min_size(Vec2::splat(side)),
                )
                .on_hover_text("Close")
                .clicked()
            {
                acts.push(Act::Close);
            }
            ui.add_space(4.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let button = |ui: &mut Ui, icon: &str, label: &str| {
                    let text = if mobile {
                        RichText::new(icon).size(18.0)
                    } else {
                        RichText::new(format!("{icon} {label}"))
                    };
                    ui.add(egui::Button::new(text).min_size(vec2(side, side)))
                        .on_hover_text(label)
                        .clicked()
                };
                if facts.deletable && button(ui, icons::TRASH, "Delete") {
                    acts.push(Act::Delete);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if facts.deletable && button(ui, icons::FOLDER_OPEN, crate::media::reveal::LABEL) {
                    acts.push(Act::Reveal);
                }
                if facts.can_send && button(ui, icons::PAPER_PLANE_TILT, "Send") {
                    acts.push(Act::Send);
                }
                if facts.deletable && button(ui, icons::DOWNLOAD_SIMPLE, "Export") {
                    acts.push(Act::Export);
                }
                if facts.can_save {
                    if facts.saved {
                        let text = if mobile {
                            RichText::new(icons::CHECK).size(18.0)
                        } else {
                            RichText::new(format!("{} Saved to Media", icons::CHECK))
                        };
                        ui.add_enabled(false, egui::Button::new(text).min_size(vec2(side, side)))
                            .on_disabled_hover_text("Saved to Media");
                    } else if button(ui, icons::DOWNLOAD_SIMPLE, "Save to Media") {
                        acts.push(Act::Save);
                    }
                }
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let block = ui.text_style_height(&egui::TextStyle::Body)
                        + 1.0
                        + ui.text_style_height(&egui::TextStyle::Small);
                    ui.add_space(((ui.available_height() - block) / 2.0).max(0.0));
                    let strong = ui.visuals().strong_text_color();
                    let title = format!("{} {}", facts.icon, facts.title);
                    emoji::line(ui, &title, egui::TextStyle::Body, strong);
                    let line = if mobile { &facts.short } else { &facts.line };
                    let color = ui.visuals().text_color();
                    emoji::line(ui, line, egui::TextStyle::Small, color);
                });
            });
        },
    );
}

fn controls(ui: &mut Ui, rect: Rect, viewer: &mut TabViewer, mobile: bool, acts: &mut Vec<Act>) {
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().panel_fill.gamma_multiply(0.92));
    let state = &viewer.ui.viewer;
    let player = state.shown.as_ref().and_then(|s| s.player.as_ref());
    let enabled = player.is_some_and(|p| p.is_ready());
    let playing = player.is_some_and(|p| p.is_playing());
    let position = player.map_or(0.0, |p| p.position());
    let length = player.map_or(0.0, |p| p.length());
    let muted = state.muted;
    let volume = state.volume;
    let silent = player.is_some_and(|p| p.is_silent_here());
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(10.0, 6.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let side = ui.spacing().interact_size.y.max(32.0);
                let (icon, hint) = if playing {
                    (icons::PAUSE, "Pause")
                } else {
                    (icons::PLAY, "Play")
                };
                if ui
                    .add(
                        egui::Button::new(RichText::new(icon).size(18.0))
                            .min_size(Vec2::splat(side)),
                    )
                    .on_hover_text(hint)
                    .clicked()
                {
                    acts.push(Act::Toggle);
                }
                ui.label(RichText::new(playback::clock(position)).monospace().small());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !mobile && !silent {
                        let mut level = volume;
                        let slider = ui.add(
                            egui::Slider::new(&mut level, 0.0..=1.0)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        if slider.changed() {
                            acts.push(Act::Volume(level));
                        }
                    }
                    let (icon, hint) = if silent {
                        (
                            icons::SPEAKER_SLASH,
                            "This recording's sound can't play here",
                        )
                    } else if muted || volume == 0.0 {
                        (icons::SPEAKER_SLASH, "Sound on")
                    } else {
                        (icons::SPEAKER_HIGH, "Mute")
                    };
                    let mute = ui
                        .add_enabled(
                            !silent,
                            egui::Button::new(RichText::new(icon).size(18.0))
                                .min_size(Vec2::splat(side)),
                        )
                        .on_hover_text(hint)
                        .on_disabled_hover_text(hint);
                    if mute.clicked() {
                        acts.push(Act::Mute);
                    }
                    ui.label(RichText::new(playback::clock(length)).monospace().small());
                    let mut at = position as f32;
                    ui.spacing_mut().slider_width = ui.available_width() - 8.0;
                    let seek = ui.add(
                        egui::Slider::new(&mut at, 0.0..=(length.max(0.01) as f32))
                            .show_value(false)
                            .trailing_fill(true),
                    );
                    if seek.changed() {
                        acts.push(Act::Seek(at as f64));
                    }
                });
            });
        },
    );
}

fn apply(
    viewer: &mut TabViewer,
    ctx: &Context,
    source: Source,
    key: &ItemKey,
    list: &[ItemKey],
    act: Act,
) {
    match act {
        Act::Close => viewer.ui.viewer.close(),
        Act::Step(by) => {
            viewer.ui.viewer.step(list, by);
        }
        Act::Toggle => {
            if let Some(player) = viewer
                .ui
                .viewer
                .shown
                .as_mut()
                .and_then(|s| s.player.as_mut())
            {
                player.toggle();
            }
        }
        Act::Seek(at) => {
            if let Some(player) = viewer
                .ui
                .viewer
                .shown
                .as_mut()
                .and_then(|s| s.player.as_mut())
            {
                player.seek(at);
            }
        }
        Act::Mute => {
            let state = &mut viewer.ui.viewer;
            state.muted = !(state.muted || state.volume == 0.0);
            if !state.muted && state.volume == 0.0 {
                state.volume = 1.0;
            }
            let (muted, volume) = (state.muted, state.volume);
            if let Some(player) = state.shown.as_mut().and_then(|s| s.player.as_mut()) {
                player.set_muted(muted);
                player.set_volume(volume);
            }
        }
        Act::Volume(level) => {
            let state = &mut viewer.ui.viewer;
            state.volume = level;
            state.muted = false;
            if let Some(player) = state.shown.as_mut().and_then(|s| s.player.as_mut()) {
                player.set_muted(false);
                player.set_volume(level);
            }
        }
        Act::DownloadCodec => {
            if crate::media::recording::download_codec(true) {
                viewer.events.notify("Downloading the video codec");
                if let Some(shown) = viewer.ui.viewer.shown.as_mut() {
                    shown.status = Status::Loading(None);
                }
            }
        }
        Act::Export => {
            if let ItemKey::Media(id) = key {
                crate::media::export(viewer.media, id, viewer.files.save_tx.clone());
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Act::Reveal => {
            if let ItemKey::Media(id) = key
                && let Err(err) = crate::media::reveal::reveal(&viewer.media.path(id))
            {
                viewer
                    .events
                    .notify_error(format!("Could not open the folder: {err}"));
            }
        }
        Act::Delete => {
            if let ItemKey::Media(id) = key {
                viewer.ui.viewer.asking_delete = Some(id.clone());
            }
        }
        Act::Send => viewer.ui.viewer.picking = true,
        Act::Save => {
            let (Source::Chat(friend), ItemKey::Chat(id)) = (source, key) else {
                return;
            };
            let Some(share) = chat_entry(viewer, friend, *id).map(|(_, s)| s.clone()) else {
                return;
            };
            let bytes = viewer
                .ui
                .viewer
                .shown
                .as_ref()
                .and_then(|s| s.bytes.clone());
            match bytes {
                Some(bytes) => match chat::save_received(viewer.media, &share, &bytes) {
                    Ok(item) => {
                        viewer.ui.friends.chat.mark_saved(*id);
                        viewer.events.push(AppEvent::MediaAdded { id: item.id });
                    }
                    Err(text) => viewer.events.notify_error(text),
                },
                None => viewer
                    .ui
                    .friends
                    .chat
                    .want_save(viewer.online, friend, share.file, *id),
            }
        }
        Act::PlayReplay => {
            let (Source::Chat(friend), ItemKey::Chat(id)) = (source, key) else {
                return;
            };
            if let Some(file) = chat_entry(viewer, friend, *id).map(|(_, s)| s.file) {
                viewer
                    .ui
                    .friends
                    .chat
                    .want_play(viewer.online, friend, file, *id);
                viewer.ui.viewer.close();
            }
        }
    }
    let _ = ctx;
}

fn ask_delete(viewer: &mut TabViewer, ctx: &Context, list: &[ItemKey]) {
    let Some(id) = viewer.ui.viewer.asking_delete.clone() else {
        return;
    };
    let Some(item) = media_item(viewer, &id).cloned() else {
        viewer.ui.viewer.asking_delete = None;
        return;
    };
    let choice = confirm::confirm_with_detail(
        ctx,
        "viewer_delete_confirm",
        crate::app::tabs::media::delete_question(item.info.kind),
        Some(item.id.file_name()),
        "Delete",
    );
    let Some(confirmed) = choice else {
        return;
    };
    viewer.ui.viewer.asking_delete = None;
    if !confirmed {
        return;
    }
    let key = ItemKey::Media(id.clone());
    let next = neighbour(list, &key, 1).or_else(|| neighbour(list, &key, -1));
    match viewer.media.delete(&id) {
        Ok(()) => {
            viewer
                .events
                .notify(format!("Deleted {}", item.info.kind.label().to_lowercase()));
            match next {
                Some(next) => viewer.ui.viewer.open(Source::Media, next),
                None => viewer.ui.viewer.close(),
            }
        }
        Err(err) => viewer
            .events
            .notify_error(format!("Could not delete it: {err}")),
    }
}

fn pick_friend(viewer: &mut TabViewer, ctx: &Context, source: Source) {
    if !viewer.ui.viewer.picking {
        return;
    }
    let Some(answer) = chat::pick_friend(ctx, viewer.online, "viewer_send") else {
        return;
    };
    viewer.ui.viewer.picking = false;
    let Some(friend) = answer else {
        return;
    };
    let Some(key) = viewer.ui.viewer.current().cloned() else {
        return;
    };
    let item = match (&key, source) {
        (ItemKey::Media(id), _) => media_item(viewer, id).cloned(),
        (ItemKey::Chat(id), Source::Chat(from)) => chat_entry(viewer, from, *id)
            .and_then(|(_, share)| local_copy(viewer, share))
            .cloned(),
        _ => None,
    };
    let chat = &mut viewer.ui.friends.chat;
    match item {
        Some(item) => chat.send_media(viewer.media, friend, &item),
        None => {
            let (ItemKey::Chat(id), Source::Chat(from)) = (&key, source) else {
                return;
            };
            let share = chat_entry(viewer, from, *id).map(|(_, s)| s.clone());
            let bytes = viewer
                .ui
                .viewer
                .shown
                .as_ref()
                .and_then(|s| s.bytes.clone());
            let (Some(share), Some(data)) = (share, bytes) else {
                return;
            };
            let sent = viewer.online.share(friend, forward(&share, data));
            if !sent {
                viewer.events.notify_error("Couldn't share it");
                return;
            }
        }
    }
    viewer.ui.viewer.close();
    viewer.ui.friends.chat.open(friend);
    viewer.events.open_tab(Tab::Friends);
}

fn forward(share: &Share, data: Vec<u8>) -> NewShare {
    NewShare {
        kind: share.kind,
        data,
        game: share.game.clone(),
        rom_sha256: share.rom_sha256.clone(),
        created_at: share.created_at,
        width: share.width,
        height: share.height,
        length_ms: share.length_ms,
        preview: share.preview.clone(),
    }
}

pub fn depth(state: &ViewerState) -> usize {
    usize::from(state.is_open())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(n: u64) -> Vec<ItemKey> {
        (0..n).map(ItemKey::Chat).collect()
    }

    #[test]
    fn arrows_step_through_the_list_and_stop_at_its_ends() {
        let list = keys(3);
        let mut state = ViewerState::default();
        state.open(Source::Media, ItemKey::Chat(1));
        assert!(state.step(&list, 1));
        assert_eq!(state.current(), Some(&ItemKey::Chat(2)));
        assert!(!state.step(&list, 1), "the last item stays");
        assert!(state.step(&list, -1));
        assert!(state.step(&list, -1));
        assert!(!state.step(&list, -1), "the first item stays");
        assert_eq!(state.current(), Some(&ItemKey::Chat(0)));
    }

    #[test]
    fn the_arrow_keys_step_and_space_plays() {
        let ctx = Context::default();
        let press = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let input = egui::RawInput {
            events: vec![
                press(Key::ArrowRight),
                press(Key::ArrowLeft),
                press(Key::Space),
            ],
            ..Default::default()
        };
        let mut seen = Vec::new();
        let _ = ctx.run(input, |ctx| seen = keys_pressed(ctx, true));
        assert_eq!(seen, [Act::Step(-1), Act::Step(1), Act::Toggle]);
        let input = egui::RawInput {
            events: vec![press(Key::Space)],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| seen = keys_pressed(ctx, false));
        assert!(seen.is_empty(), "space is left alone for a picture");
    }

    #[test]
    fn escape_closes_the_viewer() {
        let ctx = Context::default();
        let mut state = ViewerState::default();
        state.open(Source::Media, ItemKey::Chat(0));
        let frame = |events: Vec<egui::Event>, state: &mut ViewerState| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                let modal = egui::Modal::new(Id::new("media_viewer"))
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| ui.label("shown"));
                if modal.should_close() {
                    state.close();
                }
            });
        };
        frame(Vec::new(), &mut state);
        assert!(state.is_open());
        assert_eq!(depth(&state), 1);
        let escape = egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        frame(vec![escape], &mut state);
        assert!(!state.is_open());
        assert_eq!(depth(&state), 0);
    }

    #[test]
    fn a_swipe_steps_and_a_pull_down_closes() {
        assert_eq!(swipe(vec2(-90.0, 10.0)), Some(Act::Step(1)));
        assert_eq!(swipe(vec2(90.0, -5.0)), Some(Act::Step(-1)));
        assert_eq!(swipe(vec2(10.0, 150.0)), Some(Act::Close));
        assert_eq!(swipe(vec2(20.0, 30.0)), None);
        assert_eq!(swipe(vec2(5.0, -150.0)), None);
    }

    #[test]
    fn pixels_scale_by_whole_steps_when_big() {
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0));
        let rect = fit(vec2(160.0, 144.0), room, 1.0);
        assert_eq!(rect.size(), vec2(640.0, 576.0));
        assert_eq!(rect.center(), room.center());
        let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 300.0));
        assert_eq!(
            fit(vec2(160.0, 144.0), small, 1.0).width(),
            300.0 * 160.0 / 160.0
        );
        let phone = fit(vec2(160.0, 144.0), small, 2.625);
        let pixels = phone.width() * 2.625;
        assert!((pixels - pixels.round()).abs() < 1e-3);
        assert_eq!(pixels.round(), 640.0);
    }
}
