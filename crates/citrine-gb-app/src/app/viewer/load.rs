use super::{ItemKey, Shown, Source, Status};
use crate::app::tabs::TabViewer;
use crate::app::tabs::media::matches;
use crate::media::playback::{OpenError, Player};
use crate::media::{Frame, MediaId, MediaItem, MediaKind};
use crate::online::chat::{ChatEntry, Fetch};
use crate::online::chat_media;
use citrine_gb_net::{Body, FileId, OnlineId, Share, ShareKind};
use egui::{ColorImage, Context, TextureHandle, TextureOptions};

pub(super) fn keys(viewer: &TabViewer, source: Source) -> Vec<ItemKey> {
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

pub(super) fn media_item<'a>(viewer: &'a TabViewer, id: &MediaId) -> Option<&'a MediaItem> {
    viewer.media.items().iter().find(|item| &item.id == id)
}

pub(super) fn chat_entry<'a>(
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

pub(super) fn local_copy<'a>(viewer: &'a TabViewer, share: &Share) -> Option<&'a MediaItem> {
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

pub(super) fn texture_of(ctx: &Context, name: &str, frame: &Frame) -> TextureHandle {
    ctx.load_texture(name, color_image(frame), TextureOptions::NEAREST)
}

pub(super) fn color_image(frame: &Frame) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba)
}

pub(super) fn read_local(viewer: &mut TabViewer, key: &ItemKey, id: &MediaId) {
    let tx = viewer.ui.viewer.tx.clone();
    let key = key.clone();
    viewer.media.read_bytes(id, move |bytes| {
        let _ = tx.send((key, bytes));
    });
}

pub(super) fn begin(viewer: &mut TabViewer, ctx: &Context, source: Source, key: &ItemKey) {
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

pub(super) fn pump(viewer: &mut TabViewer, ctx: &Context, source: Source) {
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

pub(super) fn open_clip(shown: &mut Shown, bytes: Vec<u8>, muted: bool, volume: f32) {
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

pub(super) fn tick(ctx: &Context, shown: &mut Shown) {
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

pub(super) fn percent(done: u64, total: u64) -> u64 {
    (done * 100).checked_div(total).unwrap_or(0).min(100)
}
