use crate::app::ui_state::settings_tree::SettingsTab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::content::{SettingsContent, groups, label, rows, view};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use crate::emulator::rewind::{CLIP_LENGTH_NOTE, ClipLength};
use crate::media::recording::{DEFAULT_SCALE, MAX_SCALE};
use crate::media::render::RenderVideo;
use egui::{Ui, Widget};

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    #[cfg(not(target_arch = "wasm32"))]
    let mut events = content.events;
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
    let mut download = false;
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    let mut pick_folder = false;
    let view = view(s, SettingsTab::Capture);
    Page::show(ui, |page| {
        for group in groups(SettingsTab::Capture, view) {
            page.tree_group(group, None, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        match row {
                            RowId::ScreenshotSize => {
                                form.row(label(row), |ui| {
                                    s.dirty |= scale(ui, &mut s.screenshot_scale, 1);
                                });
                            }
                            RowId::RecordingSize => {
                                form.row(label(row), |ui| {
                                    s.dirty |= scale(ui, &mut s.recording_scale, 2);
                                });
                            }
                            RowId::RenderVideo => {
                                form.row(label(row), |ui| {
                                    s.dirty |= GenericSelect::from_enum(
                                        &mut s.render_video,
                                        "enum_select_render_video",
                                    )
                                    .default_value(RenderVideo::default())
                                    .ui(ui)
                                    .changed();
                                });
                            }
                            RowId::ClipLength => {
                                form.row_with_hint(label(row), CLIP_LENGTH_NOTE, |ui| {
                                    s.dirty |= GenericSelect::from_enum(
                                        &mut s.clip_length,
                                        "enum_select_clip_length",
                                    )
                                    .default_value(ClipLength::default())
                                    .ui(ui)
                                    .changed();
                                });
                            }
                            #[cfg(all(
                                not(target_arch = "wasm32"),
                                not(feature = "openh264-source")
                            ))]
                            RowId::VideoCodec => {
                                form.row(label(row), |ui| download |= codec_row(ui));
                                form.hint(format!(
                                    "Recording uses the {}",
                                    crate::media::recording::CODEC_NOTICE
                                ));
                            }
                            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
                            RowId::CaptureFolder => {
                                pick_folder |= folder_row(form, s, row);
                            }
                            _ => {}
                        }
                    }
                })
            });
        }
    });
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
    if download && let Some(events) = events.as_mut() {
        events.download_codec();
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    if pick_folder && let Some(events) = events.as_mut() {
        events.pick_capture_folder();
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn folder_row(
    form: &mut Form,
    s: &mut crate::app::ui_state::settings::Settings,
    row: RowId,
) -> bool {
    form.row(label(row), |ui| match s.capture_folder.as_deref() {
        Some(path) => {
            let name = std::path::Path::new(path).file_name().map_or_else(
                || path.to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
            ui.add(egui::Label::new(name).truncate())
                .on_hover_text(path);
        }
        None => {
            ui.label("Default");
        }
    });
    form.buttons(|ui| {
        let pick = ui.button("Choose").clicked();
        if ui
            .add_enabled(s.capture_folder.is_some(), egui::Button::new("Default"))
            .clicked()
        {
            s.capture_folder = None;
            s.dirty = true;
        }
        pick
    })
}

fn scale(ui: &mut Ui, scale: &mut usize, step: usize) -> bool {
    let response = ResetSlider::new(scale, step..=MAX_SCALE)
        .step_by(step as f64)
        .default_value(DEFAULT_SCALE)
        .suffix("x")
        .ui(ui);
    response.drag_stopped() || (response.changed() && !response.dragged())
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
fn codec_row(ui: &mut Ui) -> bool {
    use crate::media::recording::codec::{self, Status};
    let codec = codec::shared();
    let mut download = false;
    ui.horizontal(|ui| match codec.status() {
        Status::Ready => {
            ui.label("Downloaded");
            if ui.button("Remove").clicked() {
                let _ = codec.remove();
            }
        }
        Status::Missing => {
            download = ui.button("Download").clicked();
        }
        Status::Downloading => {
            ui.add(crate::app::widgets::progress::ProgressBar::new(
                "Downloading",
                codec
                    .progress()
                    .unwrap_or_else(crate::progress::Progress::unknown),
            ));
        }
        Status::Unsupported => {
            ui.label("Not available on this system");
        }
    });
    download
}
