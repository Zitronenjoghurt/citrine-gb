use crate::app::tabs::TabViewer;
use crate::app::ui_state::hotkeys::{HotkeyAction, Hotkeys};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::snapshots::{
    NameEdit, PendingLoad, SnapshotNews, editable_name, name_editor, quick_slot_message,
    relative_time, snapshot_message,
};
use crate::icons;
use crate::storage::{THUMB_HEIGHT, THUMB_WIDTH};

fn battery_section(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let status = viewer.emulator.save_status();
    ui.heading("Battery save");

    if !status.battery_backed {
        ui.small("This cartridge has no battery, so it cannot save on real hardware either.");
        ui.separator();
        return;
    }

    let mut renamed = None;
    Form::show(ui, "battery_save", |form| {
        form.row("Name", |ui| {
            match viewer.ui.snapshots.battery_rename_buffer() {
                Some(buffer) => match name_editor(ui, buffer, &status.name) {
                    NameEdit::Cancel => viewer.ui.snapshots.cancel_rename(),
                    NameEdit::Commit => renamed = viewer.ui.snapshots.take_battery_rename(),
                    NameEdit::Editing => {}
                },
                None => {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::app::mobile::compact_button(ui, icons::PENCIL_SIMPLE)
                            .on_hover_text("Rename this save")
                            .clicked()
                        {
                            viewer
                                .ui
                                .snapshots
                                .begin_battery_rename(status.name.clone());
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&status.name).strong())
                                    .truncate(),
                            );
                        });
                    });
                }
            }
        });

        form.text_row("Status", |ui| {
            match (status.stored, status.saved_at) {
                (true, Some(at)) => ui.label(format!("Saved {}", relative_time(at))),
                (true, None) => ui.label("Saved"),
                (false, _) => ui.label("Nothing saved yet"),
            };
        });

        form.text_row("Stored in", |ui| {
            ui.add(egui::Label::new(egui::RichText::new(&status.location).small()).truncate());
        });
    });

    if let Some(name) = renamed {
        let trimmed = name.trim().to_string();
        match viewer.emulator.rename_battery(&trimmed) {
            Ok(()) if trimmed.is_empty() => {
                viewer.events.notify("The save is back to its default name")
            }
            Ok(()) => viewer.events.notify(format!("Renamed to '{trimmed}'")),
            Err(err) => viewer
                .events
                .notify_error(format!("Rename failed: {err:?}")),
        }
    }

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(format!("{} Import save", icons::UPLOAD_SIMPLE))
            .on_hover_text(
                "Load a .sav, or a .srm with its .rtc, from another emulator or a backup",
            )
            .clicked()
        {
            crate::utils::file_loader::FileLoader::new()
                .title("Import save files")
                .add_filter("Save files", &["sav", "srm", "rtc"])
                .dispatch_many(viewer.files.sav_tx.clone());
        }

        ui.add_enabled_ui(status.stored, |ui| {
            if status.has_clock {
                ui.menu_button(format!("{} Export", icons::DOWNLOAD_SIMPLE), |ui| {
                    if ui
                        .button(".sav")
                        .on_hover_text("One file with the clock, for most emulators")
                        .clicked()
                    {
                        export_sav(viewer, &status.name);
                    }
                    if ui
                        .button(".srm and .rtc")
                        .on_hover_text("Two files, for SameBoy in RetroArch")
                        .clicked()
                        && let Some((ram, clock)) = viewer.emulator.export_rtc_files()
                    {
                        crate::utils::file_saver::FileSaver::new(&save_file_name(
                            &status.name,
                            "srm",
                        ))
                        .add_filter("Save files", &["srm"])
                        .dispatch_pair(
                            ram,
                            "rtc",
                            clock,
                            viewer.files.save_tx.clone(),
                        );
                    }
                });
            } else if ui
                .button(format!("{} Export .sav", icons::DOWNLOAD_SIMPLE))
                .on_hover_text("Save this game's data as a .sav file other emulators can read")
                .clicked()
            {
                export_sav(viewer, &status.name);
            }
        });
    });

    #[cfg(not(target_arch = "wasm32"))]
    if let Some(legacy) = &status.legacy_path {
        ui.add_space(4.0);
        ui.small(format!(
            "An older save file is still next to the ROM at {}.",
            legacy.display()
        ));
        if ui
            .button("Re-import old save")
            .on_hover_text("Overwrites the stored save with the file stored next to the ROM")
            .clicked()
        {
            if viewer.emulator.reimport_legacy_save() {
                viewer
                    .events
                    .notify("Imported the save file stored next to the ROM");
            } else {
                viewer
                    .events
                    .notify_error("Could not import the legacy save file");
            }
        }
    }

    ui.separator();
}

fn export_sav(viewer: &mut TabViewer, name: &str) {
    if let Some(data) = viewer.emulator.export_save_bytes() {
        crate::utils::file_saver::FileSaver::new(&save_file_name(name, "sav"))
            .add_filter("Save files", &["sav"])
            .dispatch(data, viewer.files.save_tx.clone());
    }
}

fn save_file_name(name: &str, extension: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    format!("{}.{extension}", stem.trim())
}

fn quick_keys_hint(touch: bool, hotkeys: &Hotkeys) -> Option<String> {
    if touch {
        return None;
    }
    let save = hotkeys.key_name(HotkeyAction::QuickSave);
    let load = hotkeys.key_name(HotkeyAction::QuickLoad);
    let star = icons::STAR;

    let mut parts = Vec::new();
    if let Some(key) = &save {
        parts.push(format!("{key} saves to {star}"));
    }
    if let Some(key) = load {
        let target = if save.is_some() { "it" } else { star };
        parts.push(format!("hold {key} to load {target}"));
    }
    let hint = parts.join(", ");
    let mut chars = hint.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

fn thumb_size(ui: &egui::Ui, title_height: f32) -> egui::Vec2 {
    let thumb = egui::vec2(THUMB_WIDTH as f32, THUMB_HEIGHT as f32);
    let side = crate::app::mobile::touch_square(ui);
    if side == 0.0 {
        return thumb;
    }
    let row = title_height
        + side
        + ui.text_style_height(&egui::TextStyle::Small)
        + 2.0 * ui.spacing().item_spacing.y;
    thumb * (row / thumb.y).max(1.0)
}

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    if !viewer.emulator.gb.cartridge.has_rom_loaded {
        ui.small("No ROM loaded");
        return;
    }
    let Some(rom_key) = viewer.emulator.rom_key().map(str::to_owned) else {
        ui.small("No ROM loaded");
        return;
    };

    battery_section(viewer, ui);
    ui.add_space(8.0);
    ui.heading("Snapshots");

    viewer
        .ui
        .snapshots
        .sync(ui.ctx(), &viewer.emulator.store, &rom_key);

    let quick_slot = viewer.ui.settings.quick_slot;
    let quick_keys = viewer.ui.settings.hotkeys.quick_keys();
    let mut action = None;
    let mut auto_action = None;

    let new_button = ui.button(format!("{} New snapshot", icons::PLUS));
    let new_key = viewer
        .ui
        .settings
        .hotkeys
        .key_name(HotkeyAction::QuickSaveNewSlot);
    let new_button = match new_key {
        Some(key) => new_button.on_hover_text(key),
        None => new_button,
    };
    if new_button.clicked() {
        action = Some(SlotAction::New);
    }
    let touch = crate::app::mobile_layout(viewer.ui.settings.layout, ui.ctx());
    if let Some(hint) = quick_keys_hint(touch, &viewer.ui.settings.hotkeys) {
        ui.add(egui::Label::new(egui::RichText::new(hint).small()).wrap());
    }
    ui.separator();

    if viewer.ui.snapshots.is_empty() {
        ui.small("No snapshots yet.");
    }

    let half = (ui.available_height() / 2.0).max(ui.spacing().interact_size.y * 3.0);
    egui::ScrollArea::vertical()
        .id_salt("snapshots")
        .max_height(half)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let slots: Vec<(usize, u64, String, egui::TextureId)> = viewer
                .ui
                .snapshots
                .slots()
                .iter()
                .map(|view| {
                    (
                        view.slot,
                        view.saved_at,
                        view.display_name(),
                        view.texture.id(),
                    )
                })
                .collect();

            for (index, (slot, saved_at, name, texture)) in slots.into_iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                let mut begin_rename = false;
                ui.horizontal(|ui| {
                    let size = thumb_size(ui, ui.spacing().interact_size.y);
                    ui.image(egui::load::SizedTexture::new(texture, size));

                    ui.vertical(|ui| {
                        match viewer.ui.snapshots.rename_buffer(slot) {
                            Some(buffer) => match name_editor(ui, buffer, &name) {
                                NameEdit::Cancel => viewer.ui.snapshots.cancel_rename(),
                                NameEdit::Commit => {
                                    if let Some((slot, text)) = viewer.ui.snapshots.take_rename() {
                                        action = Some(SlotAction::Rename(slot, text));
                                    }
                                }
                                NameEdit::Editing => {}
                            },
                            None => {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let mut rename = touch
                                            && crate::app::mobile::compact_button(
                                                ui,
                                                icons::PENCIL_SIMPLE,
                                            )
                                            .clicked();
                                        if !touch && slot == quick_slot {
                                            ui.colored_label(
                                                ui.visuals().warn_fg_color,
                                                icons::STAR,
                                            );
                                        }
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.add_space(4.0);
                                                rename |= editable_name(ui, &name).clicked();
                                            },
                                        );
                                        if rename {
                                            begin_rename = true;
                                        }
                                    },
                                );
                            }
                        }

                        ui.small(relative_time(saved_at));

                        ui.horizontal(|ui| {
                            if ui.button("Load").clicked() {
                                action = Some(SlotAction::Load(slot));
                            }
                            if ui.button("Overwrite").clicked() {
                                action = Some(SlotAction::Save(slot));
                            }
                            let side = crate::app::mobile::touch_square(ui);
                            let more = egui::Button::new(icons::DOTS_THREE)
                                .min_size(egui::vec2(side, side));
                            egui::containers::menu::MenuButton::from_button(more)
                                .ui(ui, |ui| {
                                    if !touch
                                        && ui
                                            .add_enabled(
                                                slot != quick_slot,
                                                egui::Button::new(format!(
                                                    "{} Use for {}",
                                                    icons::STAR,
                                                    quick_keys.as_deref().unwrap_or("quick save")
                                                )),
                                            )
                                            .clicked()
                                    {
                                        action = Some(SlotAction::MakeQuick(slot));
                                    }
                                    if !touch
                                        && ui
                                            .button(format!("{} Rename", icons::PENCIL_SIMPLE))
                                            .clicked()
                                    {
                                        begin_rename = true;
                                    }
                                    if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
                                        viewer.ui.snapshots.ask_delete(slot, name.clone());
                                    }
                                })
                                .0
                                .on_hover_text("More");
                        });
                    });
                });
                if begin_rename {
                    let current = if name == crate::storage::default_slot_name(slot) {
                        String::new()
                    } else {
                        name.clone()
                    };
                    viewer.ui.snapshots.begin_rename(slot, current);
                }
            }
        });

    autosave_section(viewer, ui, &mut auto_action);

    if let Some(question) = viewer.ui.snapshots.delete_question() {
        let choice =
            crate::app::widgets::confirm::confirm(ui.ctx(), "snapshot_delete", &question, "Delete");
        if let Some(slot) = viewer.ui.snapshots.answer_delete(choice) {
            action = Some(SlotAction::Delete(slot));
        }
    }

    if let Some(question) = viewer.ui.snapshots.delete_autosave_question() {
        let choice =
            crate::app::widgets::confirm::confirm(ui.ctx(), "autosave_delete", &question, "Delete");
        if let Some((id, label)) = viewer.ui.snapshots.answer_delete_autosave(choice) {
            auto_action = Some(AutoAction::Delete(id, label));
        }
    }

    if let Some(question) = viewer.ui.snapshots.load_autosave_question() {
        let choice =
            crate::app::widgets::confirm::confirm(ui.ctx(), "autosave_load", &question, "Load");
        match viewer.ui.snapshots.answer_load_autosave(choice) {
            Some((true, load)) => auto_action = Some(AutoAction::Load(load.id, load.label)),
            Some((false, load)) => viewer.emulator.running = load.was_running,
            None => {}
        }
    }

    if let Some(action) = action {
        apply(viewer, &rom_key, action, ui.ctx());
    }

    if let Some(action) = auto_action {
        apply_auto(viewer, &rom_key, action, ui.ctx());
    }
}

enum AutoAction {
    Load(usize, String),
    AskLoad(usize, String),
    Delete(usize, String),
}

fn autosave_section(viewer: &mut TabViewer, ui: &mut egui::Ui, action: &mut Option<AutoAction>) {
    ui.add_space(8.0);
    ui.heading("Autosaves");
    ui.separator();

    let rows: Vec<(usize, u64, egui::TextureId)> = viewer
        .ui
        .snapshots
        .autosaves()
        .iter()
        .take(viewer.ui.snapshots.shown_autosaves())
        .map(|view| (view.slot, view.saved_at, view.texture.id()))
        .collect();

    if rows.is_empty() {
        ui.small("No autosaves yet.");
    }

    egui::ScrollArea::vertical()
        .id_salt("autosaves")
        .auto_shrink([false, true])
        .show(ui, |ui| autosave_rows(viewer, ui, rows, action));
}

fn autosave_rows(
    viewer: &mut TabViewer,
    ui: &mut egui::Ui,
    rows: Vec<(usize, u64, egui::TextureId)>,
    action: &mut Option<AutoAction>,
) {
    for (index, (id, saved_at, texture)) in rows.into_iter().enumerate() {
        if index > 0 {
            ui.separator();
        }
        let label = relative_time(saved_at);
        ui.horizontal(|ui| {
            let size = thumb_size(ui, ui.text_style_height(&egui::TextStyle::Body));
            ui.image(egui::load::SizedTexture::new(texture, size));
            ui.vertical(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(&label).strong()).truncate());
                ui.small(crate::media::local_stamp(saved_at, "%b %-d, %H:%M"));
                ui.horizontal(|ui| {
                    if ui.button("Load").clicked() {
                        *action = Some(AutoAction::AskLoad(id, label.clone()));
                    }
                    if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
                        viewer.ui.snapshots.ask_delete_autosave(id, label.clone());
                    }
                });
            });
        });
    }

    if let Some(fold) = viewer.ui.snapshots.fold_label() {
        ui.separator();
        if ui.button(fold).clicked() {
            viewer.ui.snapshots.toggle_all_autosaves();
        }
    }
}

fn apply_auto(viewer: &mut TabViewer, rom_key: &str, action: AutoAction, ctx: &egui::Context) {
    let result = match action {
        AutoAction::AskLoad(id, label) if viewer.emulator.has_been_playing() => {
            viewer.ui.snapshots.ask_load_autosave(PendingLoad {
                id,
                label,
                was_running: viewer.emulator.running,
            });
            viewer.emulator.running = false;
            viewer.emulator.release_all();
            return;
        }
        AutoAction::AskLoad(id, label) | AutoAction::Load(id, label) => {
            match viewer.emulator.load_autosave(id) {
                Ok(true) => Ok(format!("Loaded the autosave from {label}")),
                Ok(false) => Err(format!("The autosave from {label} is gone")),
                Err(err) => Err(format!("Could not load the autosave: {err:?}")),
            }
        }
        AutoAction::Delete(id, label) => match viewer.emulator.delete_autosave(id) {
            Ok(()) => Ok(format!("Deleted the autosave from {label}")),
            Err(err) => Err(format!("Could not delete the autosave: {err:?}")),
        },
    };

    match result {
        Ok(message) => viewer.events.notify(message),
        Err(message) => viewer.events.notify_error(message),
    }
    viewer
        .ui
        .snapshots
        .reload(ctx, &viewer.emulator.store, rom_key);
}

enum SlotAction {
    New,
    Save(usize),
    Load(usize),
    Delete(usize),
    MakeQuick(usize),
    Rename(usize, String),
}

fn apply(viewer: &mut TabViewer, rom_key: &str, action: SlotAction, ctx: &egui::Context) {
    let name = |viewer: &TabViewer, slot| viewer.emulator.snapshot_name(slot);
    if let SlotAction::MakeQuick(slot) = action {
        viewer.ui.settings.quick_slot = slot;
        viewer.ui.settings.dirty = true;
        let keys = viewer.ui.settings.hotkeys.quick_keys();
        let message = quick_slot_message(keys.as_deref(), &name(viewer, slot));
        viewer.events.notify(message);
        return;
    }

    let result = match action {
        SlotAction::New => {
            let slot = viewer.emulator.store.next_free_slot(rom_key);
            viewer
                .emulator
                .save_snapshot(slot)
                .map(|_| snapshot_message(SnapshotNews::Saved, &name(viewer, slot)))
        }
        SlotAction::Save(slot) => viewer
            .emulator
            .save_snapshot(slot)
            .map(|_| snapshot_message(SnapshotNews::Overwrote, &name(viewer, slot))),
        SlotAction::Load(slot) => viewer.emulator.load_snapshot(slot).map(|loaded| {
            let news = if loaded {
                SnapshotNews::Loaded
            } else {
                SnapshotNews::Empty
            };
            snapshot_message(news, &name(viewer, slot))
        }),
        SlotAction::Delete(slot) => {
            let deleted = name(viewer, slot);
            viewer
                .emulator
                .delete_snapshot(slot)
                .map(|_| snapshot_message(SnapshotNews::Deleted, &deleted))
        }
        SlotAction::Rename(slot, name) => {
            let trimmed = name.trim().to_string();
            viewer.emulator.rename_snapshot(slot, &trimmed).map(|()| {
                snapshot_message(SnapshotNews::Renamed, &viewer.emulator.snapshot_name(slot))
            })
        }
        SlotAction::MakeQuick(_) => unreachable!("handled above"),
    };

    match result {
        Ok(message) => viewer.events.notify(message),
        Err(err) => viewer
            .events
            .notify_error(format!("Snapshot failed: {err:?}")),
    }
    // The set of snapshots changed, so drop the cached textures and read it back.
    viewer
        .ui
        .snapshots
        .reload(ctx, &viewer.emulator.store, rom_key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::hotkeys::Hotkey;
    use egui::Key;

    #[test]
    fn the_touch_layout_shows_no_keyboard_hint() {
        let hotkeys = Hotkeys::default();
        assert_eq!(quick_keys_hint(true, &hotkeys), None);
        let hint = quick_keys_hint(false, &hotkeys).unwrap();
        assert!(hint.starts_with("F8 saves to "), "got {hint}");
    }

    #[test]
    fn the_hint_names_the_keys_the_player_bound() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.unbind(Hotkey::plain(Key::F8));
        hotkeys.unbind(Hotkey::shift(Key::F8));
        hotkeys.unbind(Hotkey::plain(Key::F9));
        hotkeys.bind(Hotkey::plain(Key::Q), HotkeyAction::QuickSave);
        hotkeys.bind(Hotkey::shift(Key::Q), HotkeyAction::QuickSaveNewSlot);
        hotkeys.bind(Hotkey::plain(Key::L), HotkeyAction::QuickLoad);

        let hint = quick_keys_hint(false, &hotkeys).unwrap();
        assert!(!hint.contains("F8") && !hint.contains("F9"), "got {hint}");
        assert!(hint.starts_with("Q saves to "), "got {hint}");
        assert!(hint.contains("hold L to load it"), "got {hint}");
        assert_eq!(
            hint,
            format!("Q saves to {}, hold L to load it", icons::STAR)
        );

        let keys = hotkeys.quick_keys();
        assert_eq!(keys.as_deref(), Some("Q / L"));
        assert_eq!(
            quick_slot_message(keys.as_deref(), "Boss fight"),
            "Q / L now use 'Boss fight'"
        );
    }

    #[test]
    fn unbound_quick_keys_leave_no_key_names() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.unbind(Hotkey::plain(Key::F8));
        hotkeys.unbind(Hotkey::shift(Key::F8));
        hotkeys.unbind(Hotkey::plain(Key::F9));

        assert_eq!(quick_keys_hint(false, &hotkeys), None);
        assert_eq!(hotkeys.quick_keys(), None);
        assert_eq!(
            quick_slot_message(None, "Boss fight"),
            "Quick save now uses 'Boss fight'"
        );

        hotkeys.bind(Hotkey::plain(Key::L), HotkeyAction::QuickLoad);
        let hint = quick_keys_hint(false, &hotkeys).unwrap();
        assert!(hint.starts_with("Hold L to load "), "got {hint}");
    }
}
