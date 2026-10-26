use crate::storage::{SaveStore, THUMB_HEIGHT, THUMB_WIDTH};
use egui::{ColorImage, Context, TextureHandle, TextureOptions};

#[derive(Default)]
pub struct SnapshotsState {
    loaded_key: Option<String>,
    slots: Vec<SlotView>,
    renaming: Option<(usize, String)>,
    renaming_battery: Option<String>,
    deleting: Option<(usize, String)>,
    autosaves: Vec<SlotView>,
    all_autosaves: bool,
    deleting_autosave: Option<(usize, String)>,
    loading_autosave: Option<PendingLoad>,
    stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLoad {
    pub id: usize,
    pub label: String,
    pub was_running: bool,
}

pub const FOLDED_AUTOSAVES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotNews {
    Saved,
    Overwrote,
    Loaded,
    Empty,
    Deleted,
    Renamed,
}

pub fn snapshot_message(news: SnapshotNews, name: &str) -> String {
    match news {
        SnapshotNews::Saved => format!("Saved '{name}'"),
        SnapshotNews::Overwrote => format!("Overwrote '{name}'"),
        SnapshotNews::Loaded => format!("Loaded '{name}'"),
        SnapshotNews::Empty => format!("'{name}' is empty"),
        SnapshotNews::Deleted => format!("Deleted '{name}'"),
        SnapshotNews::Renamed => format!("Renamed to '{name}'"),
    }
}

pub fn quick_slot_message(keys: Option<&str>, name: &str) -> String {
    match keys {
        Some(keys) => format!("{keys} now use '{name}'"),
        None => format!("Quick save now uses '{name}'"),
    }
}

pub enum NameEdit {
    Editing,
    Commit,
    Cancel,
}

pub fn name_editor(ui: &mut egui::Ui, buffer: &mut String, hint: &str) -> NameEdit {
    let side = crate::app::mobile::touch_square(ui);
    let width = if side > 0.0 {
        ui.available_width()
    } else {
        180.0
    };
    let response = ui.add(
        egui::TextEdit::singleline(buffer)
            .desired_width(width)
            .min_size(egui::vec2(0.0, side))
            .vertical_align(egui::Align::Center)
            .hint_text(hint)
            .char_limit(crate::storage::MAX_SAVE_NAME),
    );

    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        NameEdit::Cancel
    } else if response.lost_focus() {
        NameEdit::Commit
    } else {
        if !response.has_focus() {
            response.request_focus();
        }
        NameEdit::Editing
    }
}

pub fn editable_name(ui: &mut egui::Ui, name: &str) -> egui::Response {
    let text = egui::RichText::new(name).strong();
    let galley = egui::WidgetText::from(text.clone()).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let text_size = egui::vec2(galley.size().x.min(ui.available_width()), galley.size().y);
    let height = text_size.y.max(crate::app::mobile::touch_square(ui));
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(text_size.x, height), egui::Sense::click());
    ui.put(rect, egui::Label::new(text).truncate().selectable(false));
    if response.hovered() {
        let visuals = ui.visuals();
        let outline =
            egui::Rect::from_center_size(rect.center(), text_size).expand2(egui::vec2(4.0, 2.0));
        ui.painter().rect_stroke(
            outline,
            visuals.widgets.inactive.corner_radius,
            visuals.widgets.hovered.bg_stroke,
            egui::StrokeKind::Outside,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    response.on_hover_text("Rename")
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
        if !self.stale && self.loaded_key.as_deref() == Some(rom_key) {
            return;
        }
        self.reload(ctx, store, rom_key);
    }

    pub fn reload(&mut self, ctx: &Context, store: &SaveStore, rom_key: &str) {
        let same_game = self.loaded_key.as_deref() == Some(rom_key);
        self.slots = views(ctx, store.snapshot_infos(rom_key), "snapshot", rom_key);
        self.autosaves = views(ctx, store.autosave_infos(rom_key), "autosave", rom_key);
        self.loaded_key = Some(rom_key.to_string());
        self.stale = false;
        if !same_game {
            self.renaming = None;
            self.deleting = None;
            self.deleting_autosave = None;
            self.all_autosaves = false;
            self.loading_autosave = None;
        }
    }

    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    pub fn slots(&self) -> &[SlotView] {
        &self.slots
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn autosaves(&self) -> &[SlotView] {
        &self.autosaves
    }

    pub fn shown_autosaves(&self) -> usize {
        if self.all_autosaves {
            self.autosaves.len()
        } else {
            self.autosaves.len().min(FOLDED_AUTOSAVES)
        }
    }

    pub fn fold_label(&self) -> Option<String> {
        if self.autosaves.len() <= FOLDED_AUTOSAVES {
            None
        } else if self.all_autosaves {
            Some("Show fewer".to_string())
        } else {
            Some(format!("Show all {}", self.autosaves.len()))
        }
    }

    pub fn toggle_all_autosaves(&mut self) {
        self.all_autosaves = !self.all_autosaves;
    }

    pub fn ask_delete_autosave(&mut self, id: usize, label: String) {
        self.deleting_autosave = Some((id, label));
    }

    pub fn delete_autosave_question(&self) -> Option<String> {
        self.deleting_autosave
            .as_ref()
            .map(|(_, label)| format!("Delete the autosave from {label}? It can't be undone."))
    }

    pub fn answer_delete_autosave(&mut self, choice: Option<bool>) -> Option<(usize, String)> {
        match choice {
            None => None,
            Some(confirmed) => self.deleting_autosave.take().filter(|_| confirmed),
        }
    }

    pub fn ask_load_autosave(&mut self, load: PendingLoad) {
        self.loading_autosave = Some(load);
    }

    pub fn load_autosave_question(&self) -> Option<String> {
        self.loading_autosave.as_ref().map(|load| {
            format!(
                "Load the autosave from {}? Anything since your last save is lost.",
                load.label
            )
        })
    }

    pub fn answer_load_autosave(&mut self, choice: Option<bool>) -> Option<(bool, PendingLoad)> {
        let confirmed = choice?;
        self.loading_autosave.take().map(|load| (confirmed, load))
    }

    pub fn begin_rename(&mut self, slot: usize, current: String) {
        self.renaming_battery = None;
        self.renaming = Some((slot, current));
    }

    pub fn cancel_rename(&mut self) {
        self.renaming = None;
        self.renaming_battery = None;
    }

    pub fn begin_battery_rename(&mut self, current: String) {
        self.renaming = None;
        self.renaming_battery = Some(current);
    }

    pub fn battery_rename_buffer(&mut self) -> Option<&mut String> {
        self.renaming_battery.as_mut()
    }

    pub fn take_battery_rename(&mut self) -> Option<String> {
        self.renaming_battery.take()
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

    pub fn ask_delete(&mut self, slot: usize, name: String) {
        self.deleting = Some((slot, name));
    }

    pub fn delete_question(&self) -> Option<String> {
        self.deleting
            .as_ref()
            .map(|(_, name)| format!("Delete '{name}'? It can't be undone."))
    }

    pub fn answer_delete(&mut self, choice: Option<bool>) -> Option<usize> {
        match choice {
            None => None,
            Some(confirmed) => self
                .deleting
                .take()
                .filter(|_| confirmed)
                .map(|(slot, _)| slot),
        }
    }
}

fn views(
    ctx: &Context,
    infos: Vec<crate::storage::SnapshotInfo>,
    kind: &str,
    rom_key: &str,
) -> Vec<SlotView> {
    infos
        .into_iter()
        .map(|info| {
            let image =
                ColorImage::from_rgba_unmultiplied([THUMB_WIDTH, THUMB_HEIGHT], &info.thumbnail);
            SlotView {
                slot: info.slot,
                saved_at: info.saved_at,
                texture: ctx.load_texture(
                    format!("{kind}_{rom_key}_{}", info.slot),
                    image,
                    TextureOptions::NEAREST,
                ),
                name: info.name,
            }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_messages_use_the_name() {
        let all = [
            SnapshotNews::Saved,
            SnapshotNews::Overwrote,
            SnapshotNews::Loaded,
            SnapshotNews::Empty,
            SnapshotNews::Deleted,
            SnapshotNews::Renamed,
        ];
        for news in all {
            let message = snapshot_message(news, "Boss fight");
            assert!(message.contains("'Boss fight'"), "got {message}");
            assert!(!message.to_lowercase().contains("slot"), "got {message}");
        }
    }

    #[test]
    fn delete_waits_for_an_answer() {
        let mut state = SnapshotsState::default();
        state.ask_delete(1, "Slot 1".to_string());
        assert_eq!(
            state.delete_question().as_deref(),
            Some("Delete 'Slot 1'? It can't be undone.")
        );
        assert_eq!(state.answer_delete(None), None);
        assert_eq!(state.answer_delete(Some(true)), Some(1));
        assert_eq!(state.delete_question(), None);
    }

    #[test]
    fn cancel_deletes_nothing() {
        let mut state = SnapshotsState::default();
        state.ask_delete(2, "Boss fight".to_string());
        assert_eq!(
            state.delete_question().as_deref(),
            Some("Delete 'Boss fight'? It can't be undone.")
        );
        assert_eq!(state.answer_delete(Some(false)), None);
        assert_eq!(state.answer_delete(Some(true)), None);
    }

    #[test]
    fn long_autosave_lists_fold() {
        let ctx = egui::Context::default();
        let root = std::env::temp_dir().join(format!("citrine-fold-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = SaveStore::at(root.clone());
        let frame = vec![0u8; 160 * 144 * 4];
        let mut state = SnapshotsState::default();

        store
            .store_autosave("GAME", b"a", &frame, 10)
            .expect("autosave");
        state.reload(&ctx, &store, "GAME");
        assert_eq!(state.shown_autosaves(), 1);
        assert_eq!(state.fold_label(), None);

        for _ in 0..4 {
            store
                .store_autosave("GAME", b"a", &frame, 10)
                .expect("autosave");
        }
        state.reload(&ctx, &store, "GAME");
        assert_eq!(state.autosaves()[0].slot, 4, "newest first");
        assert_eq!(state.shown_autosaves(), FOLDED_AUTOSAVES);
        assert_eq!(state.fold_label().as_deref(), Some("Show all 5"));
        state.toggle_all_autosaves();
        assert_eq!(state.shown_autosaves(), 5);
        assert_eq!(state.fold_label().as_deref(), Some("Show fewer"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn deleting_an_autosave_waits_for_an_answer() {
        let mut state = SnapshotsState::default();
        state.ask_delete_autosave(7, "5m ago".to_string());
        assert_eq!(
            state.delete_autosave_question().as_deref(),
            Some("Delete the autosave from 5m ago? It can't be undone.")
        );
        assert_eq!(state.answer_delete_autosave(None), None);
        assert_eq!(
            state.answer_delete_autosave(Some(true)),
            Some((7, "5m ago".to_string()))
        );
        state.ask_delete_autosave(8, "1h ago".to_string());
        assert_eq!(state.answer_delete_autosave(Some(false)), None);
        assert_eq!(state.delete_autosave_question(), None);
    }

    #[test]
    fn a_cancelled_load_gives_back_the_pause_state() {
        let mut state = SnapshotsState::default();
        let load = PendingLoad {
            id: 3,
            label: "just now".to_string(),
            was_running: true,
        };
        state.ask_load_autosave(load.clone());
        assert!(state.load_autosave_question().is_some());
        assert_eq!(state.answer_load_autosave(None), None);
        assert_eq!(state.answer_load_autosave(Some(false)), Some((false, load)));
        assert_eq!(state.load_autosave_question(), None);
    }

    #[test]
    fn the_question_has_delete_and_cancel() {
        let ctx = egui::Context::default();
        let mut output = ctx.run(egui::RawInput::default(), |_| {});
        for _ in 0..3 {
            output = ctx.run(egui::RawInput::default(), |ctx| {
                crate::app::widgets::confirm::confirm(
                    ctx,
                    "test_confirm",
                    "Delete Slot 1?",
                    "Delete",
                );
            });
        }
        let texts: Vec<String> = output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        for wanted in ["Delete Slot 1?", "Delete", "Cancel"] {
            assert!(
                texts.iter().any(|text| text == wanted),
                "{wanted} in {texts:?}"
            );
        }
    }
}
