use crate::storage::{SaveStore, THUMB_HEIGHT, THUMB_WIDTH};
use egui::{ColorImage, Context, TextureHandle, TextureOptions};

#[derive(Default)]
pub struct SnapshotsState {
    loaded_key: Option<String>,
    slots: Vec<SlotView>,
    renaming: Option<(usize, String)>,
}

pub struct SlotView {
    pub slot: usize,
    pub saved_at: u64,
    pub texture: TextureHandle,
    pub name: Option<String>,
}

impl SlotView {
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => crate::storage::default_slot_name(self.slot),
        }
    }
}

impl SnapshotsState {
    pub fn sync(&mut self, ctx: &Context, store: &SaveStore, rom_key: &str) {
        if self.loaded_key.as_deref() == Some(rom_key) {
            return;
        }
        self.reload(ctx, store, rom_key);
    }

    pub fn reload(&mut self, ctx: &Context, store: &SaveStore, rom_key: &str) {
        self.slots = store
            .snapshot_infos(rom_key)
            .into_iter()
            .map(|info| {
                let image = ColorImage::from_rgba_unmultiplied(
                    [THUMB_WIDTH, THUMB_HEIGHT],
                    &info.thumbnail,
                );
                SlotView {
                    slot: info.slot,
                    saved_at: info.saved_at,
                    texture: ctx.load_texture(
                        format!("snapshot_{rom_key}_{}", info.slot),
                        image,
                        TextureOptions::NEAREST,
                    ),
                    name: info.name,
                }
            })
            .collect();
        self.loaded_key = Some(rom_key.to_string());
        self.renaming = None;
    }

    pub fn invalidate(&mut self) {
        self.loaded_key = None;
    }

    pub fn slots(&self) -> &[SlotView] {
        &self.slots
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn begin_rename(&mut self, slot: usize, current: String) {
        self.renaming = Some((slot, current));
    }

    pub fn cancel_rename(&mut self) {
        self.renaming = None;
    }

    pub fn rename_buffer(&mut self, slot: usize) -> Option<&mut String> {
        match &mut self.renaming {
            Some((editing, text)) if *editing == slot => Some(text),
            _ => None,
        }
    }

    pub fn take_rename(&mut self) -> Option<(usize, String)> {
        self.renaming.take()
    }
}

pub fn relative_time(saved_at: u64) -> String {
    let now = web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let elapsed = now.saturating_sub(saved_at);
    match elapsed {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", elapsed / 60),
        3600..=86399 => format!("{}h ago", elapsed / 3600),
        _ => format!("{}d ago", elapsed / 86400),
    }
}
