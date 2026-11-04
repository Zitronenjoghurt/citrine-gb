use crate::app::tabs::TabViewer;
use crate::app::tabs::media::{detail_line, game_name};
use crate::icons;
use crate::media::playback::Player;
use crate::media::{MediaId, MediaKind};
use acts::{apply, ask_delete, pick_friend};
use bar::{controls, top_bar};
use citrine_gb_net::{FileId, OnlineId, ShareKind};
use egui::{
    Align2, Color32, Context, Id, Key, Modifiers, Rect, RichText, Sense, TextureHandle, Ui, Vec2,
    pos2, vec2,
};
use load::{begin, chat_entry, keys, local_copy, media_item, pump, tick};
use std::sync::mpsc::{Receiver, Sender, channel};

mod acts;
mod bar;
mod load;

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
    Loading(Option<crate::progress::Progress>),
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
    Pass(crate::media::clipboard::Way),
    Send,
    Delete,
    PlayReplay,
    DownloadCodec,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    Reveal,
}

pub struct ViewerState {
    open: Option<(Source, ItemKey)>,
    shown: Option<Box<Shown>>,
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
    pass: Option<crate::media::clipboard::Way>,
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
                    MediaKind::Print => icons::PRINTER,
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
                pass: crate::media::clipboard::way(item.info.kind),
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
                .has_game()
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
                pass: None,
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
    let mobile = viewer.ui.settings.layout.is_touch(ctx);
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

    let loading = match shown.map(|s| &s.status) {
        Some(Status::Loading(Some(progress))) => Some(("Downloading", *progress)),
        Some(Status::Loading(None)) => crate::media::recording::codec_progress()
            .map(|progress| ("Downloading the video codec", progress)),
        _ => None,
    };
    let note = match shown.map(|s| &s.status) {
        Some(Status::Note(text)) => Some(text.clone()),
        _ if loading.is_some() => None,
        _ => facts.replay_note.clone(),
    };
    let needs_codec = matches!(shown.map(|s| &s.status), Some(Status::NeedsCodec));
    if note.is_some() || loading.is_some() || needs_codec || facts.replay {
        let at = Rect::from_center_size(
            pos2(room.center().x, room.bottom() - 40.0),
            vec2(room.width().min(360.0), 64.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some((label, progress)) = loading {
                        ui.add(crate::app::widgets::progress::ProgressBar::new(label, progress));
                    }
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

pub fn depth(state: &ViewerState) -> usize {
    usize::from(state.is_open())
}

#[cfg(test)]
mod tests;
