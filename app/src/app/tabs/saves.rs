use crate::app::tabs::TabViewer;
use crate::app::widgets::snapshots::relative_time;
use crate::icons;
use crate::storage::{THUMB_HEIGHT, THUMB_WIDTH};

fn battery_section(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let status = viewer.emulator.save_status();
    ui.heading("Battery Save");

    if !status.battery_backed {
        ui.small("This cartridge has no battery, so it cannot save on real hardware either.");
        ui.separator();
        return;
    }

    egui::Grid::new("battery_save_grid")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Status");
            match (status.stored, status.saved_at) {
                (true, Some(at)) => ui.label(format!("Saved {}", relative_time(at))),
                (true, None) => ui.label("Saved"),
                (false, _) => ui.label("Nothing saved yet"),
            };
            ui.end_row();

            ui.label("Stored in");
            ui.add(egui::Label::new(egui::RichText::new(&status.location).small()).wrap());
            ui.end_row();
        });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .button(format!("{} Import .sav", icons::UPLOAD_SIMPLE))
            .on_hover_text("Load a save file from another emulator or a backup")
            .clicked()
        {
            crate::utils::file_loader::FileLoader::new()
                .title("Import save file")
                .add_filter("Save files", &["sav"])
                .dispatch(viewer.files.sav_tx.clone());
        }

        ui.add_enabled_ui(status.stored, |ui| {
            if ui
                .button(format!("{} Export .sav", icons::DOWNLOAD_SIMPLE))
                .on_hover_text("Save this game's data as a .sav file other emulators can read")
                .clicked()
                && let Some(data) = viewer.emulator.export_save_bytes()
            {
                let title = viewer.emulator.gb.cartridge.header.title.trim();
                let name = if title.is_empty() { "save" } else { title };
                crate::utils::file_saver::FileSaver::new(&format!("{name}.sav"))
                    .add_filter("Save files", &["sav"])
                    .dispatch(data, viewer.files.save_tx.clone());
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
                    .notify("Imported the save file next stored to the ROM");
            } else {
                viewer
                    .events
                    .notify_error("Could not import the legacy save file");
            }
        }
    }

    ui.separator();
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
    let mut action = None;

    ui.horizontal(|ui| {
        if ui.button(format!("{} New Snapshot", icons::PLUS)).clicked() {
            action = Some(SlotAction::New);
        }
        ui.separator();
        ui.small(format!(
            "F8 overwrites slot  {}{quick_slot}, Shift+F8 makes a new one, holding F9 loads slot \
             {}{quick_slot}",
            icons::STAR,
            icons::STAR
        ));
    });
    ui.separator();

    if viewer.ui.snapshots.is_empty() {
        ui.small("No snapshots yet.");
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for view in viewer.ui.snapshots.slots() {
            let slot = view.slot;
            ui.horizontal(|ui| {
                ui.image(egui::load::SizedTexture::new(
                    view.texture.id(),
                    egui::vec2(THUMB_WIDTH as f32, THUMB_HEIGHT as f32),
                ));

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(format!("Slot {slot}"));
                        if slot == quick_slot {
                            ui.colored_label(ui.visuals().warn_fg_color, icons::STAR);
                        }
                    });
                    ui.small(relative_time(view.saved_at));

                    ui.horizontal(|ui| {
                        if ui.button("Load").clicked() {
                            action = Some(SlotAction::Load(slot));
                        }
                        if ui.button("Overwrite").clicked() {
                            action = Some(SlotAction::Save(slot));
                        }
                        if ui.button("Delete").clicked() {
                            action = Some(SlotAction::Delete(slot));
                        }
                        ui.add_enabled_ui(slot != quick_slot, |ui| {
                            if ui
                                .button(icons::STAR)
                                .on_hover_text("Use this slot for F8 / F9")
                                .clicked()
                            {
                                action = Some(SlotAction::MakeQuick(slot));
                            }
                        });
                    });
                });
            });
            ui.separator();
        }
    });

    if let Some(action) = action {
        apply(viewer, &rom_key, action, ui.ctx());
    }
}

enum SlotAction {
    New,
    Save(usize),
    Load(usize),
    Delete(usize),
    MakeQuick(usize),
}

fn apply(viewer: &mut TabViewer, rom_key: &str, action: SlotAction, ctx: &egui::Context) {
    if let SlotAction::MakeQuick(slot) = action {
        viewer.ui.settings.quick_slot = slot;
        viewer.ui.settings.dirty = true;
        viewer.events.notify(format!("F8 / F9 now use slot {slot}"));
        return;
    }

    let result = match action {
        SlotAction::New => {
            let slot = viewer.emulator.store.next_free_slot(rom_key);
            viewer
                .emulator
                .save_snapshot(slot)
                .map(|_| format!("Saved snapshot to slot {slot}"))
        }
        SlotAction::Save(slot) => viewer
            .emulator
            .save_snapshot(slot)
            .map(|_| format!("Overwrote slot {slot}")),
        SlotAction::Load(slot) => viewer.emulator.load_snapshot(slot).map(|loaded| {
            if loaded {
                format!("Loaded snapshot from slot {slot}")
            } else {
                format!("Slot {slot} is empty")
            }
        }),
        SlotAction::Delete(slot) => viewer
            .emulator
            .delete_snapshot(slot)
            .map(|_| format!("Deleted slot {slot}")),
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
