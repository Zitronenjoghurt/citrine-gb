use crate::app::widgets::snapshots::relative_time;
use crate::app::widgets::text::faint;
use crate::online::cloud::{Choice, Conflict, ConflictSide};
use crate::storage::thumb_size_of;
use egui::{ColorImage, Context, RichText, TextureHandle, TextureOptions, Ui};

pub enum Answer {
    Choose(Choice),
    NotNow,
}

fn thumb(ctx: &Context, key: String, side: &ConflictSide) -> Option<TextureHandle> {
    let pixels = side.thumb.as_ref()?;
    let size = thumb_size_of(pixels)?;
    let id = egui::Id::new(("cloud_thumb", &key));
    if let Some(texture) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(texture);
    }
    let image = ColorImage::from_rgba_unmultiplied(size, pixels);
    let texture = ctx.load_texture(key, image, TextureOptions::NEAREST);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

fn card(
    ui: &mut Ui,
    title: &str,
    side: Option<&ConflictSide>,
    texture: Option<TextureHandle>,
    button: &str,
    enabled: bool,
) -> bool {
    let mut pressed = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(150.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(title).strong());
            if let Some(texture) = &texture {
                ui.image(egui::load::SizedTexture::new(
                    texture.id(),
                    texture.size_vec2(),
                ));
            }
            match side {
                Some(side) => {
                    if !side.device.is_empty() {
                        ui.label(RichText::new(&side.device).small());
                    }
                    ui.label(faint(format!("Saved {}", relative_time(side.saved_at))));
                }
                None => {
                    ui.label(faint("Not on this device"));
                }
            }
            ui.add_space(4.0);
            pressed = ui
                .add_enabled(
                    enabled,
                    egui::Button::new(button).min_size(egui::vec2(120.0, 32.0)),
                )
                .clicked();
        });
    });
    pressed
}

pub fn show(ctx: &Context, conflict: &Conflict) -> Option<Answer> {
    let mut answer = None;
    let key = format!("{}-{}", conflict.rom, conflict.slot);
    let here_thumb = conflict
        .here
        .as_ref()
        .and_then(|side| thumb(ctx, format!("{key}-here"), side));
    let cloud_thumb = thumb(
        ctx,
        format!("{key}-cloud-{}", conflict.cloud.saved_at),
        &conflict.cloud,
    );
    let modal = egui::Modal::new(egui::Id::new("cloud_conflict")).show(ctx, |ui| {
        ui.set_max_width(340.0);
        ui.heading(&conflict.name);
        let text = match &conflict.here {
            Some(_) => "This save changed here and in the cloud. Which one do you want?",
            None => "The cloud has a newer copy of this save.",
        };
        ui.label(text);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if conflict.here.is_some()
                && card(
                    ui,
                    "This device",
                    conflict.here.as_ref(),
                    here_thumb,
                    "Keep this device's",
                    true,
                )
            {
                answer = Some(Answer::Choose(Choice::KeepHere));
            }
            if card(
                ui,
                "Cloud",
                Some(&conflict.cloud),
                cloud_thumb,
                "Keep the cloud's",
                conflict.ready(),
            ) {
                answer = Some(Answer::Choose(Choice::KeepCloud));
            }
        });
        if !conflict.ready() {
            ui.add(super::progress::ProgressBar::new(
                "Getting the cloud's copy",
                crate::progress::Progress::unknown(),
            ));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if conflict.here.is_some() {
                let both = ui
                    .add_enabled(
                        conflict.ready(),
                        egui::Button::new("Keep both").min_size(egui::vec2(80.0, 32.0)),
                    )
                    .on_hover_text(if conflict.slot.starts_with("battery") {
                        "The cloud's copy becomes a second battery save you can switch to"
                    } else {
                        "The cloud's copy goes into a new slot"
                    });
                if both.clicked() {
                    answer = Some(Answer::Choose(Choice::KeepBoth));
                }
            }
            if ui
                .add(egui::Button::new("Not now").min_size(egui::vec2(80.0, 32.0)))
                .clicked()
            {
                answer = Some(Answer::NotNow);
            }
        });
    });
    if modal.should_close() && answer.is_none() {
        answer = Some(Answer::NotNow);
    }
    answer
}
