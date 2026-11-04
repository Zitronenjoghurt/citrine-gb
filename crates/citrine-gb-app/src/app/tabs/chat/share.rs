use super::bubble::save_button;
use super::{Action, ChatUi, NO_FILE, PREVIEW_MAX, UNREADABLE, Want};
use crate::app::tabs::TabViewer;
use crate::app::tabs::media::{game_name, thumbnail};
use crate::app::widgets::action::wide_button;
use crate::app::widgets::progress::ProgressBar;
use crate::app::widgets::text::faint;
use crate::emulator::Emulator;
use crate::game_name::GameNames;
use crate::icons;
use crate::media::{self, Frame, MediaItem, MediaKind, MediaStore, NewMedia};
use crate::online::chat::{ChatEntry, Fetch, NewShare};
use crate::online::{Online, chat_media};
use crate::progress::Progress;
use crate::replay::ReplayError;
use citrine_gb_net::{Body, OnlineId, Share, ShareKind};
use egui::{CursorIcon, Label, RichText, Ui, vec2};

#[allow(clippy::too_many_arguments)]
pub(super) fn share_card(
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
    let facts = ui.add(Label::new(faint(share_facts(share))).truncate());
    let own_tip = title.response.contains_pointer() || facts.contains_pointer();

    if entry.mine {
        if let Some((sent, total)) = online.uploading(friend, entry.id) {
            ui.add(ProgressBar::new(
                "Sending",
                Progress::bytes(sent as u64, total as u64),
            ));
        }
        return own_tip;
    }
    if share.file == NO_FILE {
        return own_tip;
    }
    let loading = match online.fetched(share.file) {
        Some(Fetch::Loading { done, total }) => Some(Progress::bytes(*done, *total)),
        _ => state
            .wanted
            .contains_key(&share.file)
            .then(Progress::unknown),
    };
    ui.add_space(2.0);
    if let Some(progress) = loading {
        ui.add(ProgressBar::new("Downloading", progress));
        return own_tip;
    }
    match share.kind {
        ShareKind::Screenshot | ShareKind::Recording => {
            if state.saved.contains(&entry.id) {
                ui.label(faint(format!("{} Saved to Media", icons::CHECK)));
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
                ui.add(Label::new(faint(format!("Load {game} to play it"))).wrap());
            }
        }
    }
    own_tip
}

pub(super) fn from_media(ctx: &egui::Context, viewer: &mut TabViewer, friend: OnlineId) {
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
        if wide_button(ui, egui::Button::new("Cancel")).clicked() {
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

pub(super) fn share_entry(online: &Online, friend: OnlineId, id: u64) -> Option<Share> {
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

pub(super) fn share_item(item: &MediaItem, bytes: Vec<u8>) -> Option<NewShare> {
    let info = &item.info;
    let (kind, data, width, height, preview) = match info.kind {
        MediaKind::Screenshot | MediaKind::Print => {
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

pub(super) fn replay_share(emulator: &Emulator) -> Option<NewShare> {
    let json = emulator.recorder.export_json()?;
    let recording = crate::replay::parse(json.as_bytes()).ok()?;
    let video = emulator.video();
    let frame = Frame {
        width: video.width as u32,
        height: video.height as u32,
        rgba: emulator.frame().to_vec(),
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

pub(super) fn replay_error(err: &ReplayError) -> String {
    match err {
        ReplayError::Unreadable => "This replay can't be read".to_owned(),
        ReplayError::OtherGame(game) => format!("Load {} to play it", game_name(game)),
    }
}

pub(super) fn kind_word(item: &MediaItem) -> &'static str {
    match item.info.kind {
        MediaKind::Screenshot => "screenshot",
        MediaKind::Recording => "recording",
        MediaKind::Print => "print",
    }
}

pub(super) fn share_icon(kind: ShareKind) -> &'static str {
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
