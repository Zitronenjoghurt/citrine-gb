use super::load::{chat_entry, local_copy, media_item};
use super::{Act, ItemKey, Source, Status, neighbour};
use crate::app::events::AppEvent;
use crate::app::tabs::{Tab, TabViewer, chat};
use crate::app::widgets::confirm;
use crate::online::chat::NewShare;
use citrine_gb_net::Share;
use egui::Context;

pub(super) fn apply(
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
        Act::Pass(way) => {
            if let ItemKey::Media(id) = key {
                crate::media::clipboard::run(way, viewer.media, id, viewer.files.copy_tx.clone());
            }
        }
        Act::Export => {
            if let ItemKey::Media(id) = key {
                crate::media::export(viewer.media, id, viewer.files.save_tx.clone());
            }
        }
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
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
                Some(bytes) => match chat::share::save_received(viewer.media, &share, &bytes) {
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

pub(super) fn ask_delete(viewer: &mut TabViewer, ctx: &Context, list: &[ItemKey]) {
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

pub(super) fn pick_friend(viewer: &mut TabViewer, ctx: &Context, source: Source) {
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

pub(super) fn forward(share: &Share, data: Vec<u8>) -> NewShare {
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
