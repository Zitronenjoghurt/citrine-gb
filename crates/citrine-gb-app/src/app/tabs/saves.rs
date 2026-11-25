use crate::app::tabs::TabViewer;
use crate::app::ui_state::hotkeys::{HotkeyAction, Hotkeys};
use crate::app::widgets::action::more_button;
use crate::app::widgets::cloud_mark::cloud_status;
use crate::app::widgets::scroll;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::snapshots::{
    AutosaveEntry, FailedStep, NameEdit, PendingLoad, SnapshotNews, content_end, failed_message,
    name_editor, named, quick_slot_message, snapshot_message, stamp_row_with,
};
use crate::emulator::SaveStatus;
use crate::icons;
use crate::online::cloud::LocalSave;
use crate::storage::list_stamp;
use citrine_gb::console::ConsoleKind;
use citrine_gb::error::GbError;

const LIST_END: f32 = 12.0;

fn choose_button(viewer: &mut TabViewer, ui: &mut egui::Ui, rom_key: &str, save: &LocalSave) {
    let Some(slot) = viewer
        .online
        .cloud_conflict(rom_key, save)
        .map(|c| c.slot.clone())
    else {
        return;
    };
    if ui
        .button(format!("{} Choose a copy", icons::CLOUD_WARNING))
        .clicked()
    {
        viewer.online.cloud_ask_again(rom_key, &slot);
    }
}

fn stamp_row(
    viewer: &mut TabViewer,
    ui: &mut egui::Ui,
    stamp: String,
    console: Option<ConsoleKind>,
    rom_key: &str,
    save: &LocalSave,
) -> crate::app::widgets::snapshots::StampRow {
    let state = viewer.online.cloud_state(rom_key, save);
    let conflict = viewer
        .online
        .cloud_conflict(rom_key, save)
        .map(|c| c.slot.clone());
    let row = stamp_row_with(ui, stamp, console, state, conflict.is_some());
    if row.chosen
        && let Some(slot) = conflict
    {
        viewer.online.cloud_ask_again(rom_key, &slot);
    }
    row
}

fn cloud_mark_for(viewer: &TabViewer, ui: &mut egui::Ui, rom_key: &str, save: &LocalSave) {
    if let Some(state) = viewer.online.cloud_state(rom_key, save) {
        cloud_status(ui, state);
    }
}

fn delete_confirm(
    viewer: &mut TabViewer,
    ctx: &egui::Context,
    id: &str,
    question: &str,
    rom_key: &str,
    save: &LocalSave,
) -> Option<bool> {
    if !viewer.online.cloud_has_copy(rom_key, save) {
        return crate::app::widgets::confirm::confirm(ctx, id, question, "Delete");
    }
    let mut cloud_too = *viewer.ui.snapshots.cloud_too();
    let choice = crate::app::widgets::confirm::confirm_with_option(
        ctx,
        id,
        question,
        "Delete",
        ("Also delete the cloud copy", &mut cloud_too),
    );
    *viewer.ui.snapshots.cloud_too() = cloud_too;
    choice
}

fn forget_in_cloud(viewer: &mut TabViewer, rom_key: &str, save: &LocalSave) {
    let cloud_too =
        !viewer.online.cloud_has_copy(rom_key, save) || *viewer.ui.snapshots.cloud_too();
    viewer.online.cloud_forget(rom_key, save, cloud_too);
}

fn other_batteries(viewer: &mut TabViewer, ui: &mut egui::Ui, rom_key: &str, times: &[u64]) {
    let others: Vec<(String, String, Option<u64>)> = viewer
        .ui
        .snapshots
        .other_batteries()
        .iter()
        .map(|b| {
            let name = b.name.clone().unwrap_or_else(|| "Battery save".to_owned());
            (b.token.clone(), name, b.saved_at)
        })
        .collect();
    if !others.is_empty() {
        ui.separator();
        ui.add_space(8.0);
        ui.heading("Other battery saves");
    }
    for (token, name, saved_at) in others {
        let save = LocalSave::OtherBattery(token.clone());
        let row = ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(&name).strong()).truncate());
                let stamp = saved_at.map(|at| list_stamp(at, times)).unwrap_or_default();
                stamp_row(viewer, ui, stamp, None, rom_key, &save);
                ui.horizontal(|ui| {
                    if ui.button("Use this").clicked() {
                        viewer
                            .ui
                            .snapshots
                            .ask_use_battery(token.clone(), name.clone());
                    }
                    if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
                        viewer
                            .ui
                            .snapshots
                            .ask_delete_battery(token.clone(), name.clone());
                    }
                });
            });
        });
        keep_asked_in_view(viewer, ui, &save, row.response.rect);
    }

    if let Some(question) = viewer.ui.snapshots.use_battery_question() {
        let choice =
            crate::app::widgets::confirm::confirm(ui.ctx(), "battery_use", &question, "Use this");
        if let Some((token, name)) = viewer.ui.snapshots.answer_use_battery(choice) {
            match viewer.emulator.use_battery(&token) {
                Ok(()) => viewer.events.notify("Switched battery saves"),
                Err(err) => viewer.events.notify_error(failed_message(
                    FailedStep::Switch,
                    &named(&name, "the battery save"),
                    &err,
                )),
            }
            viewer.ui.snapshots.invalidate();
        }
    }

    if let Some((token, question)) = viewer.ui.snapshots.delete_battery_question() {
        let save = LocalSave::OtherBattery(token);
        let choice = delete_confirm(
            viewer,
            ui.ctx(),
            "battery_delete",
            &question,
            rom_key,
            &save,
        );
        if let Some((token, name)) = viewer.ui.snapshots.answer_delete_battery(choice) {
            forget_in_cloud(viewer, rom_key, &save);
            match viewer
                .emulator
                .store
                .delete_save(rom_key, &LocalSave::OtherBattery(token))
            {
                Ok(()) => viewer.events.notify("Deleted the battery save"),
                Err(err) => viewer.events.notify_error(failed_message(
                    FailedStep::Delete,
                    &named(&name, "the battery save"),
                    &GbError::from(err),
                )),
            }
            viewer.ui.snapshots.invalidate();
        }
    }
}

fn battery_section(
    viewer: &mut TabViewer,
    ui: &mut egui::Ui,
    rom_key: &str,
    status: SaveStatus,
    times: &[u64],
) {
    ui.heading("Battery save");

    if !status.battery_backed {
        ui.small("This cartridge has no battery, so it cannot save on real hardware either.");
        ui.separator();
        return;
    }

    let touch = viewer.ui.settings.layout.is_touch(ui.ctx());
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

        let status_row = |ui: &mut egui::Ui| {
            match (status.stored, status.saved_at) {
                (true, Some(at)) => ui.label(format!("Saved {}", list_stamp(at, times))),
                (true, None) => ui.label("Saved"),
                (false, _) => ui.label("Nothing saved yet"),
            };
            if status.stored {
                cloud_mark_for(viewer, ui, rom_key, &LocalSave::Battery);
            }
        };
        form.text_row("Status", status_row);
        if !touch {
            form.text_row("Stored in", |ui| {
                ui.add(egui::Label::new(&status.location).truncate());
            });
        }
    });
    if touch {
        let text = egui::style::Spacing::default().interact_size.y;
        ui.add_space((crate::app::mobile::touch_square(ui) - text).max(0.0) / 2.0);
    }

    if let Some(name) = renamed {
        let trimmed = name.trim().to_string();
        match viewer.emulator.rename_battery(&trimmed) {
            Ok(()) if trimmed.is_empty() => {
                viewer.events.notify("The save is back to its default name")
            }
            Ok(()) => viewer.events.notify(format!("Renamed to {trimmed}")),
            Err(err) => viewer.events.notify_error(failed_message(
                FailedStep::Rename,
                &named(&status.name, "the battery save"),
                &err,
            )),
        }
    }

    ui.add_space(4.0);
    let kind = viewer.emulator.kind();
    let (import_hint, import_types): (&str, &[&str]) = if kind == ConsoleKind::Nds {
        (
            "Load a .sav, or a DeSmuME .dsv, from another emulator or a backup",
            &["sav", "dsv"],
        )
    } else if kind == ConsoleKind::Gba {
        (
            "Load a .sav from another emulator or a backup",
            &["sav", "srm"],
        )
    } else {
        (
            "Load a .sav, or a .srm with its .rtc, from another emulator or a backup",
            &["sav", "srm", "rtc"],
        )
    };
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(format!("{} Import save", icons::UPLOAD_SIMPLE))
            .on_hover_text(import_hint)
            .clicked()
        {
            crate::utils::file_loader::FileLoader::new()
                .title("Import save files")
                .add_filter("Save files", import_types)
                .dispatch_many(&viewer.files.spawner, viewer.files.sav_tx.clone());
        }

        ui.add_enabled_ui(status.stored, |ui| {
            if status.has_clock && matches!(kind, ConsoleKind::Gb | ConsoleKind::Gbc) {
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
                            &viewer.files.spawner,
                            ram,
                            "rtc",
                            clock,
                            viewer.files.save_tx.clone(),
                        );
                    }
                });
            } else if ui
                .button(format!("{} Export .sav", icons::DOWNLOAD_SIMPLE))
                .on_hover_text(if status.has_clock {
                    "Save this game's data and its clock as a .sav file mGBA can read"
                } else {
                    "Save this game's data as a .sav file other emulators can read"
                })
                .clicked()
            {
                export_sav(viewer, &status.name);
            }
        });
        choose_button(viewer, ui, rom_key, &LocalSave::Battery);
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

    other_batteries(viewer, ui, rom_key, times);

    ui.separator();
}

fn export_sav(viewer: &mut TabViewer, name: &str) {
    if let Some(data) = viewer.emulator.export_save_bytes() {
        crate::utils::file_saver::FileSaver::new(&save_file_name(name, "sav"))
            .add_filter("Save files", &["sav"])
            .dispatch(&viewer.files.spawner, data, viewer.files.save_tx.clone());
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

fn button_width(ui: &egui::Ui, text: &str) -> f32 {
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

fn row_thumb(ui: &egui::Ui, console: Option<ConsoleKind>) -> egui::Vec2 {
    let [width, height] = crate::storage::thumb_size(console);
    let thumb = egui::vec2(width as f32, height as f32);
    let side = crate::app::mobile::touch_square(ui);
    let thumb = if side == 0.0 {
        thumb
    } else {
        let row = ui.spacing().interact_size.y
            + side
            + ui.text_style_height(&egui::TextStyle::Small)
            + 2.0 * ui.spacing().item_spacing.y;
        thumb * (row / thumb.y).max(1.0)
    };
    let gap = ui.spacing().item_spacing.x;
    let more = button_width(ui, icons::DOTS_THREE).max(crate::app::mobile::touch_square(ui));
    let snapshot = button_width(ui, "Load") + button_width(ui, "Overwrite") + more + 2.0 * gap;
    let autosave =
        button_width(ui, "Load") + button_width(ui, &format!("{} Delete", icons::TRASH)) + gap;
    let room = ui.available_width() - gap - snapshot.max(autosave);
    thumb * (room / thumb.x).clamp(0.5, 1.0)
}

fn thumb_image(ui: &mut egui::Ui, texture: egui::TextureId, console: Option<ConsoleKind>) {
    let slot = row_thumb(ui, console);
    let [width, height] = crate::storage::thumb_size(console);
    let picture = egui::vec2(width as f32, height as f32);
    let fitted = picture * (slot.x / picture.x).min(slot.y / picture.y);
    ui.add_sized(
        slot,
        egui::Image::new(egui::load::SizedTexture::new(texture, fitted)),
    );
}

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    scroll::gutter(ui);
    if !viewer.emulator.has_game() {
        ui.small("No ROM loaded");
        return;
    }
    let Some(saves_key) = viewer.emulator.saves_key() else {
        ui.small("No ROM loaded");
        return;
    };

    viewer
        .ui
        .snapshots
        .sync(ui.ctx(), &viewer.emulator.store, &saves_key);

    let status = viewer.emulator.save_status();
    let times: Vec<u64> = viewer
        .ui
        .snapshots
        .page_times()
        .into_iter()
        .chain(status.saved_at.filter(|_| status.stored))
        .collect();
    battery_section(viewer, ui, &saves_key, status, &times);
    ui.add_space(8.0);
    ui.heading("Snapshots");
    if viewer.ui.snapshots.is_empty() {
        ui.weak("No snapshots yet.");
    }

    let quick_slot = viewer.ui.settings.quick_slot;
    let hardcore = viewer.emulator.hardcore();
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
    let touch = viewer.ui.settings.layout.is_touch(ui.ctx());
    if let Some(hint) = quick_keys_hint(touch, &viewer.ui.settings.hotkeys) {
        ui.add(egui::Label::new(egui::RichText::new(hint).small()).wrap());
    }
    if !viewer.ui.snapshots.is_empty() {
        ui.separator();
    }

    ui.vertical(|ui| {
        let slots: Vec<(usize, u64, String, egui::TextureId, Option<ConsoleKind>)> = viewer
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
                    view.console,
                )
            })
            .collect();

        for (index, (slot, saved_at, name, texture, console)) in slots.into_iter().enumerate() {
            let above = content_end(ui);
            if index > 0 {
                ui.separator();
            }
            let mut begin_rename = false;
            let mut name_rect = egui::Rect::NOTHING;
            let mut row = None;
            let card = ui.horizontal(|ui| {
                thumb_image(ui, texture, console);

                ui.vertical(|ui| {
                    match viewer.ui.snapshots.rename_buffer(slot) {
                        Some(buffer) => {
                            let edit = ui.scope(|ui| name_editor(ui, buffer, &name));
                            name_rect = edit.response.rect;
                            match edit.inner {
                                NameEdit::Cancel => viewer.ui.snapshots.cancel_rename(),
                                NameEdit::Commit => {
                                    if let Some((slot, text)) = viewer.ui.snapshots.take_rename() {
                                        action = Some(SlotAction::Rename(slot, text));
                                    }
                                }
                                NameEdit::Editing => {}
                            }
                        }
                        None if touch => {
                            name_rect = ui
                                .add(
                                    egui::Label::new(egui::RichText::new(&name).strong())
                                        .truncate(),
                                )
                                .rect;
                        }
                        None => {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if slot == quick_slot {
                                        ui.colored_label(ui.visuals().warn_fg_color, icons::STAR);
                                    }
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            name_rect = ui
                                                .add(
                                                    egui::Label::new(
                                                        egui::RichText::new(&name).strong(),
                                                    )
                                                    .truncate(),
                                                )
                                                .rect;
                                        },
                                    );
                                },
                            );
                        }
                    }

                    let save = LocalSave::Snapshot(slot);
                    row = Some(stamp_row(
                        viewer,
                        ui,
                        list_stamp(saved_at, &times),
                        console,
                        &saves_key,
                        &save,
                    ));

                    ui.horizontal(|ui| {
                        if crate::app::hardcore::load_button(ui, hardcore).clicked() {
                            action = Some(SlotAction::Load(slot));
                        }
                        if ui.button("Overwrite").clicked() {
                            action = Some(SlotAction::Save(slot));
                        }
                        if crate::app::mobile::compact_button(ui, icons::PENCIL_SIMPLE)
                            .on_hover_text("Rename")
                            .clicked()
                        {
                            begin_rename = true;
                        }
                        let side = crate::app::mobile::touch_square(ui);
                        let more = more_button(side);
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
                                if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
                                    viewer.ui.snapshots.ask_delete(slot, name.clone());
                                }
                            })
                            .0
                            .on_hover_text("More");
                    });
                });
            });
            if let Some(row) = row {
                row.show_tip(ui, card.response.rect, name_rect, above);
            }
            keep_asked_in_view(viewer, ui, &LocalSave::Snapshot(slot), card.response.rect);
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

    autosave_section(
        viewer,
        ui,
        &saves_key,
        &times,
        &mut auto_action,
        &mut action,
    );

    if let Some((slot, question)) = viewer.ui.snapshots.delete_question() {
        let save = LocalSave::Snapshot(slot);
        let choice = delete_confirm(
            viewer,
            ui.ctx(),
            "snapshot_delete",
            &question,
            &saves_key,
            &save,
        );
        if let Some(slot) = viewer.ui.snapshots.answer_delete(choice) {
            forget_in_cloud(viewer, &saves_key, &save);
            action = Some(SlotAction::Delete(slot));
        }
    }

    if let Some((id, question)) = viewer.ui.snapshots.delete_autosave_question() {
        let save = LocalSave::Autosave(id);
        let choice = delete_confirm(
            viewer,
            ui.ctx(),
            "autosave_delete",
            &question,
            &saves_key,
            &save,
        );
        if let Some((id, label)) = viewer.ui.snapshots.answer_delete_autosave(choice) {
            forget_in_cloud(viewer, &saves_key, &save);
            auto_action = Some(AutoAction::Delete(id, label));
        }
    }

    if let Some(question) = viewer.ui.snapshots.load_autosave_question() {
        let choice = crate::app::widgets::confirm::confirm_losing_progress(
            ui.ctx(),
            "autosave_load",
            &question,
            "Load",
        );
        match viewer.ui.snapshots.answer_load_autosave(choice) {
            Some((true, load)) => auto_action = Some(AutoAction::Load(load.id, load.label)),
            Some((false, load)) => viewer.emulator.running = load.was_running,
            None => {}
        }
    }

    if let Some(action) = action {
        apply(viewer, &saves_key, action, ui.ctx());
    }

    if let Some(action) = auto_action {
        apply_auto(viewer, &saves_key, action, ui.ctx());
    }
}

enum AutoAction {
    Load(usize, String),
    AskLoad(usize, String),
    Delete(usize, String),
}

struct AutosaveRow {
    slot: usize,
    saved_at: u64,
    title: String,
    texture: egui::TextureId,
    console: Option<ConsoleKind>,
    filed: bool,
}

fn autosave_section(
    viewer: &mut TabViewer,
    ui: &mut egui::Ui,
    rom_key: &str,
    times: &[u64],
    action: &mut Option<AutoAction>,
    slot_action: &mut Option<SlotAction>,
) {
    ui.separator();
    ui.add_space(8.0);
    ui.heading("Autosaves");

    let snapshots = &viewer.ui.snapshots;
    let rows: Vec<AutosaveRow> = snapshots
        .autosave_list()
        .into_iter()
        .map(|entry| {
            let (view, filed) = match entry {
                AutosaveEntry::Mine(index) => (&snapshots.autosaves()[index], false),
                AutosaveEntry::Filed(index) => (&snapshots.filed_autosaves()[index], true),
            };
            AutosaveRow {
                slot: view.slot,
                saved_at: view.saved_at,
                title: if filed {
                    view.display_name()
                } else {
                    "Autosave".to_owned()
                },
                texture: view.texture.id(),
                console: view.console,
                filed,
            }
        })
        .collect();
    if rows.is_empty() {
        ui.weak("No autosaves yet.");
    }

    for (index, row) in rows.into_iter().enumerate() {
        let above = content_end(ui);
        if index > 0 {
            ui.separator();
        }
        autosave_row(
            viewer,
            ui,
            rom_key,
            (row, above),
            times,
            action,
            slot_action,
        );
    }

    if let Some(fold) = viewer.ui.snapshots.fold_label() {
        ui.separator();
        if ui.button(fold).clicked() {
            viewer.ui.snapshots.toggle_all_autosaves();
        }
    }
    ui.add_space(LIST_END);
}

fn autosave_row(
    viewer: &mut TabViewer,
    ui: &mut egui::Ui,
    rom_key: &str,
    (row, above): (AutosaveRow, f32),
    times: &[u64],
    action: &mut Option<AutoAction>,
    slot_action: &mut Option<SlotAction>,
) {
    let save = if row.filed {
        LocalSave::Snapshot(row.slot)
    } else {
        LocalSave::Autosave(row.slot)
    };
    let mut name_rect = egui::Rect::NOTHING;
    let mut stamp = None;
    let shown = ui.horizontal(|ui| {
        thumb_image(ui, row.texture, row.console);
        ui.vertical(|ui| {
            name_rect = ui
                .add(egui::Label::new(egui::RichText::new(&row.title).strong()).truncate())
                .rect;
            stamp = Some(stamp_row(
                viewer,
                ui,
                list_stamp(row.saved_at, times),
                row.console,
                rom_key,
                &save,
            ));
            ui.horizontal(|ui| {
                let hardcore = viewer.emulator.hardcore();
                let load = crate::app::hardcore::load_button(ui, hardcore).clicked();
                let delete = ui.button(format!("{} Delete", icons::TRASH)).clicked();
                if row.filed {
                    if load {
                        *slot_action = Some(SlotAction::Load(row.slot));
                    }
                    if delete {
                        let spoken =
                            format!("{} from {}", row.title, list_stamp(row.saved_at, times));
                        viewer.ui.snapshots.ask_delete(row.slot, spoken);
                    }
                } else {
                    let label = list_stamp(row.saved_at, times);
                    if load {
                        *action = Some(AutoAction::AskLoad(row.slot, label.clone()));
                    }
                    if delete {
                        viewer.ui.snapshots.ask_delete_autosave(row.slot, label);
                    }
                }
            });
        });
    });
    if let Some(stamp) = stamp {
        stamp.show_tip(ui, shown.response.rect, name_rect, above);
    }
    keep_asked_in_view(viewer, ui, &save, shown.response.rect);
}

fn keep_asked_in_view(viewer: &TabViewer, ui: &egui::Ui, save: &LocalSave, row: egui::Rect) {
    let snapshots = &viewer.ui.snapshots;
    let asked = match save {
        LocalSave::Snapshot(slot) => snapshots
            .delete_question()
            .is_some_and(|(asked, _)| asked == *slot),
        LocalSave::Autosave(id) => snapshots
            .delete_autosave_question()
            .is_some_and(|(asked, _)| asked == *id),
        LocalSave::OtherBattery(token) => snapshots
            .delete_battery_question()
            .is_some_and(|(asked, _)| asked == *token),
        LocalSave::Battery => false,
    };
    if asked {
        crate::app::mobile::sheet::keep_above(ui, row);
        crate::app::widgets::confirm::keep_lit(ui, row);
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
                Err(err) => Err(failed_message(
                    FailedStep::Load,
                    &format!("the autosave from {label}"),
                    &err,
                )),
            }
        }
        AutoAction::Delete(id, label) => match viewer.emulator.delete_autosave(id) {
            Ok(()) => Ok(format!("Deleted the autosave from {label}")),
            Err(err) => Err(failed_message(
                FailedStep::Delete,
                &format!("the autosave from {label}"),
                &err,
            )),
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

    if let SlotAction::Load(slot) = action
        && crate::app::tabs::playing::group_partners(viewer.play, viewer.online).is_some()
    {
        viewer.ui.playing.unlink_load = Some(slot);
        return;
    }

    let failed = |viewer: &TabViewer, step, slot, err: GbError| {
        failed_message(step, &name(viewer, slot), &err)
    };
    let result = match action {
        SlotAction::New => {
            let slot = viewer.emulator.next_free_slot(rom_key);
            viewer
                .emulator
                .save_snapshot(slot)
                .map(|_| snapshot_message(SnapshotNews::Saved, &name(viewer, slot)))
                .map_err(|err| failed(viewer, FailedStep::Save, slot, err))
        }
        SlotAction::Save(slot) => viewer
            .emulator
            .save_snapshot(slot)
            .map(|_| snapshot_message(SnapshotNews::Overwrote, &name(viewer, slot)))
            .map_err(|err| failed(viewer, FailedStep::Save, slot, err)),
        SlotAction::Load(slot) => viewer
            .emulator
            .load_snapshot(slot)
            .map(|loaded| {
                let news = if loaded {
                    SnapshotNews::Loaded
                } else {
                    SnapshotNews::Empty
                };
                snapshot_message(news, &name(viewer, slot))
            })
            .map_err(|err| failed(viewer, FailedStep::Load, slot, err)),
        SlotAction::Delete(slot) => {
            let deleted = name(viewer, slot);
            viewer
                .emulator
                .delete_snapshot(slot)
                .map(|_| snapshot_message(SnapshotNews::Deleted, &deleted))
                .map_err(|err| failed_message(FailedStep::Delete, &deleted, &err))
        }
        SlotAction::Rename(slot, name) => {
            let trimmed = name.trim().to_string();
            viewer
                .emulator
                .rename_snapshot(slot, &trimmed)
                .map(|()| {
                    snapshot_message(SnapshotNews::Renamed, &viewer.emulator.snapshot_name(slot))
                })
                .map_err(|err| failed(viewer, FailedStep::Rename, slot, err))
        }
        SlotAction::MakeQuick(_) => unreachable!("handled above"),
    };

    match result {
        Ok(message) => viewer.events.notify(message),
        Err(message) => viewer.events.notify_error(message),
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
    fn a_list_shows_seconds_on_every_row_or_on_none() {
        let minute = 1_793_648_340;
        let shared = [minute + 50, minute + 5, minute - 300];
        for at in shared {
            assert_eq!(list_stamp(at, &shared).matches(':').count(), 2, "{at}");
        }
        let apart = [minute + 50, minute - 300, minute - 900];
        for at in apart {
            assert_eq!(list_stamp(at, &apart).matches(':').count(), 1, "{at}");
        }
        assert_eq!(
            list_stamp(minute, &shared),
            crate::media::local_stamp(minute, "%b %-d, %H:%M:%S")
        );
    }

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
        hotkeys.unbind(&Hotkey::plain(Key::F8));
        hotkeys.unbind(&Hotkey::shift(Key::F8));
        hotkeys.unbind(&Hotkey::plain(Key::F9));
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
        hotkeys.unbind(&Hotkey::plain(Key::F8));
        hotkeys.unbind(&Hotkey::shift(Key::F8));
        hotkeys.unbind(&Hotkey::plain(Key::F9));

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
