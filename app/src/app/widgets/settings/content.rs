use crate::app::ui_state::keybinds::BindingCapture;
use crate::app::ui_state::settings::{
    ConsoleChoice, LayoutMode, ScalingMode, Settings, SettingsTab,
};
use crate::app::ui_state::ui_theme::UiTheme;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::palette::PaletteEditorState;
use crate::app::widgets::settings::{controls, palette};
use crate::audio::{Audio, DEFAULT_LATENCY_MS, MAX_LATENCY_MS, MIN_LATENCY_MS};
use crate::autosave::{AutosaveInterval, DEFAULT_KEEP, MAX_KEEP};
use citrine_gb::gb::GbModel;
use citrine_gb::gb::ppu::types::color::ColorCorrection;
use egui::{Response, ScrollArea, Ui, Widget};

#[cfg(not(target_arch = "wasm32"))]
const AUTOSAVE_NOTE: &str = "Citrine also autosaves when you close it or switch games, and after a crash. Autosaves show in the Saves tab, apart from your snapshots.";
#[cfg(target_arch = "wasm32")]
const AUTOSAVE_NOTE: &str = "Citrine also autosaves when you leave the page or switch games, and after a crash. Autosaves show in the Saves tab, apart from your snapshots.";

pub struct SettingsContent<'a> {
    settings: &'a mut Settings,
    capture: &'a mut BindingCapture,
    palette_editor: &'a mut PaletteEditorState,
    audio: Option<&'a Audio>,
    game: Option<LoadedGame<'a>>,
}

#[derive(Clone, Copy)]
pub struct LoadedGame<'a> {
    pub rom_key: &'a str,
    pub title: &'a str,
    pub model: GbModel,
    pub game_model: GbModel,
    pub header: &'a [u8],
    pub own_colors: bool,
}

impl<'a> SettingsContent<'a> {
    pub fn new(
        settings: &'a mut Settings,
        capture: &'a mut BindingCapture,
        palette_editor: &'a mut PaletteEditorState,
        audio: Option<&'a Audio>,
        game: Option<LoadedGame<'a>>,
    ) -> Self {
        Self {
            settings,
            capture,
            palette_editor,
            audio,
            game,
        }
    }
}

impl Widget for SettingsContent<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(ui.available_width().min(MAX_COLUMN_WIDTH));
                    match self.settings.current_tab {
                        SettingsTab::General => self.general(ui),
                        SettingsTab::Sound => self.sound(ui),
                        SettingsTab::Controls => self.controls(ui),
                        SettingsTab::Style => self.style(ui),
                        SettingsTab::Developer => self.developer(ui),
                    }
                })
                .response
            })
            .inner
    }
}

impl SettingsContent<'_> {
    fn general(self, ui: &mut Ui) {
        let s = self.settings;
        let game = self.game;
        let touch = crate::app::mobile_layout(s.layout, ui.ctx());

        Form::show(ui, "general", |form| {
            form.row("Console", |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.console, "enum_select_console")
                    .default_value(ConsoleChoice::default())
                    .ui(ui)
                    .on_hover_text("Automatic picks Game Boy Color for games made for it")
                    .changed();
            });
            if let Some(game) = game
                && s.console.resolve(game.game_model) != game.model
            {
                form.hint("The console changes the next time a game loads.");
            }

            form.row("Layout", |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.layout, "enum_select_layout")
                    .default_value(LayoutMode::default())
                    .ui(ui)
                    .on_hover_text("Automatic uses the touch layout on small screens")
                    .changed();
            });
            if s.layout == LayoutMode::Desktop && crate::app::desktop_too_narrow(form.ctx()) {
                form.hint(
                    "This window is too narrow for the desktop layout, so it uses the touch layout.",
                );
            }
            if s.layout == LayoutMode::Mobile {
                form.hint(format!(
                    "The touch layout sizes itself to the device; use Controls {} UI size to adjust it.",
                    crate::icons::ARROW_RIGHT
                ));
            }

            if !touch {
                form.row("UI scale", |ui| {
                    let response = ResetSlider::new(&mut s.ui_scale, 0.5..=5.0)
                        .step_by(0.1)
                        .default_value(Settings::DEFAULT_UI_SCALE)
                        .ui(ui);
                    s.note_scale_drag(&response);
                });
            }

            form.row("UI theme", |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.ui_theme, "enum_select_ui_theme")
                    .default_value(UiTheme::default())
                    .ui(ui)
                    .changed();
            });

            form.row("Developer mode", |ui| {
                s.dirty |= ui.checkbox(&mut s.dev_mode, "").changed();
            });

            #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
            {
                form.row("Video codec", codec_row);
                form.hint(format!(
                    "Recording uses the {}",
                    crate::media::recording::CODEC_NOTICE
                ));
            }
        });

        ui.add_space(10.0);
        controls::section(
            ui,
            crate::icons::CLOCK_COUNTER_CLOCKWISE,
            "Autosave",
            |ui| {
                Form::show(ui, "general", |form| {
                    form.row("Every", |ui| {
                        s.dirty |= GenericSelect::from_enum(
                            &mut s.autosave_interval,
                            "enum_select_autosave_interval",
                        )
                        .default_value(AutosaveInterval::default())
                        .ui(ui)
                        .on_hover_text(
                            "Minutes of play between autosaves; paused time doesn't count",
                        )
                        .changed();
                    });

                    form.row("Keep", |ui| {
                        let response = ResetSlider::new(&mut s.autosave_keep, 1..=MAX_KEEP)
                            .default_value(DEFAULT_KEEP)
                            .ui(ui);
                        if response.drag_stopped() || (response.changed() && !response.dragged()) {
                            s.dirty = true;
                        }
                        response.on_hover_text("When there are more, the oldest autosave goes");
                    });
                });
                ui.add_space(4.0);
                ui.small(AUTOSAVE_NOTE);
            },
        );

        ui.add_space(10.0);
        controls::section(ui, crate::icons::CAMERA, "Capture", |ui| {
            Form::show(ui, "capture", |form| {
                form.row("Screenshot scale", |ui| {
                    s.dirty |= capture_scale(ui, &mut s.screenshot_scale, 1);
                });
                form.row("Recording scale", |ui| {
                    s.dirty |= capture_scale(ui, &mut s.recording_scale, 2);
                });
            });
        });
        ui.separator();
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "Citrine keeps settings in {}, beside your saves.",
                    crate::storage::SettingsStore::default().location()
                ))
                .small()
                .weak(),
            )
            .wrap(),
        );
    }

    fn sound(self, ui: &mut Ui) {
        let s = self.settings;

        Form::show(ui, "sound", |form| {
            form.row("Volume", |ui| {
                s.dirty |= ResetSlider::new(&mut s.volume, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_VOLUME)
                    .ui(ui)
                    .changed();
            });

            form.row("Mute", |ui| {
                s.dirty |= ui.checkbox(&mut s.muted, "").changed();
            });

            form.row("Latency", |ui| {
                let response =
                    ResetSlider::new(&mut s.audio_queue_ms, MIN_LATENCY_MS..=MAX_LATENCY_MS)
                        .default_value(DEFAULT_LATENCY_MS)
                        .suffix(" ms")
                        .ui(ui);
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    s.dirty = true;
                }
                response.on_hover_text(
                    "Lower is more responsive, higher is more resistant to crackling",
                );
            });

            form.row("Channels", |ui| {
                for (index, enabled) in s.channels.iter_mut().enumerate() {
                    s.dirty |= ui
                        .checkbox(enabled, format!("{}", index + 1))
                        .on_hover_text(CHANNEL_NAMES[index])
                        .changed();
                }
            });

            #[cfg(not(target_arch = "wasm32"))]
            {
                form.separator();
                form.row("Output device", |ui| device_picker(ui, s));
                form.hint("Changing this reopens the sound device.");
            }
        });

        if let Some(audio) = self.audio {
            ui.add_space(8.0);
            if audio.is_running() {
                ui.small(format!(
                    "Playing at {} Hz on {}.",
                    audio.get_sample_rate(),
                    audio.device_name().unwrap_or("the default device")
                ));
            } else {
                ui.small("Sound starts when you load a game or tap the screen.");
            }
        }
    }

    fn controls(self, ui: &mut Ui) {
        controls::show(ui, self.settings, self.capture);
    }

    fn style(self, ui: &mut Ui) {
        palette::show(ui, self.settings, self.palette_editor, self.game);

        ui.add_space(10.0);
        ui.separator();

        let s = self.settings;

        Form::show(ui, "style", |form| {
            form.row("Scaling", |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.scaling, "enum_select_scaling")
                    .default_value(ScalingMode::default())
                    .ui(ui)
                    .changed();
            });

            form.row("Matrix", |ui| {
                s.dirty |= ui
                    .checkbox(&mut s.matrix, "")
                    .on_hover_text("Draw the grid between pixels; forces integer scaling")
                    .changed();
            });
            if s.matrix && s.scaling != ScalingMode::Integer {
                form.hint(
                    "The matrix needs whole-number scaling, so Citrine scales by whole numbers while it is on.",
                );
            }

            form.row("Edge darkness", |ui| {
                s.dirty |= ResetSlider::new(&mut s.matrix_edge_darkness, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_MATRIX_EDGE_DARKNESS)
                    .ui(ui)
                    .changed();
            });

            form.row("Corner darkness", |ui| {
                s.dirty |= ResetSlider::new(&mut s.matrix_corner_darkness, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_MATRIX_CORNER_DARKNESS)
                    .ui(ui)
                    .changed();
            });

            form.separator();

            form.row("Ghosting", |ui| {
                s.dirty |= ui
                    .checkbox(&mut s.ghosting, "")
                    .on_hover_text("Smear frames together, the way the original LCD did")
                    .changed();
            });

            form.row("Strength", |ui| {
                s.dirty |= ResetSlider::new(&mut s.ghosting_strength, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_GHOSTING_STRENGTH)
                    .ui(ui)
                    .changed();
            });

            form.separator();

            form.row("GBC screen", |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.color_correction, "enum_select_gbc_screen")
                    .default_value(ColorCorrection::Screen)
                    .ui(ui)
                    .on_hover_text("How Game Boy Color colours look. Like a GBC softens them the way its screen did.")
                    .changed();
            });
        });
    }

    fn developer(self, ui: &mut Ui) {
        let s = self.settings;

        Form::show(ui, "developer", |form| {
            form.row("Randomized power-on RAM", |ui| {
                s.dirty |= ui.checkbox(&mut s.randomized_ram, "").changed();
            });
        });

        ui.add_space(10.0);
        ui.separator();
        ui.small(format!(
            "{} custom palette(s) saved.",
            s.palettes.entries().len()
        ));
    }
}

const MAX_COLUMN_WIDTH: f32 = 480.0;

const CHANNEL_NAMES: [&str; 4] = ["Pulse with sweep", "Pulse", "Wave", "Noise"];

fn capture_scale(ui: &mut Ui, scale: &mut usize, step: usize) -> bool {
    use crate::media::recording::{DEFAULT_SCALE, MAX_SCALE, SOURCE_HEIGHT, SOURCE_WIDTH};
    let response = ResetSlider::new(scale, step..=MAX_SCALE)
        .step_by(step as f64)
        .default_value(DEFAULT_SCALE)
        .suffix("x")
        .ui(ui);
    let changed = response.drag_stopped() || (response.changed() && !response.dragged());
    response.on_hover_text(format!(
        "{} × {} pixels",
        SOURCE_WIDTH * *scale,
        SOURCE_HEIGHT * *scale
    ));
    changed
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
fn codec_row(ui: &mut Ui) {
    use crate::media::recording::codec::{self, Status};
    let codec = codec::shared();
    ui.horizontal(|ui| match codec.status() {
        Status::Ready => {
            ui.label("Downloaded");
            if ui.button("Remove").clicked() {
                let _ = codec.remove();
            }
        }
        Status::Missing => {
            if ui.button("Download").clicked() {
                codec.download(codec::from_cisco());
            }
        }
        Status::Downloading => {
            ui.spinner();
            ui.label("Downloading");
        }
        Status::Unsupported => {
            ui.label("Not available on this system");
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn device_picker(ui: &mut Ui, s: &mut Settings) {
    let selected = s
        .audio_device
        .clone()
        .unwrap_or_else(|| "System default".to_string());

    egui::ComboBox::from_id_salt("audio_device_picker")
        .width(ui.available_width())
        .truncate()
        .selected_text(selected)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(s.audio_device.is_none(), "System default")
                .clicked()
            {
                s.audio_device = None;
                s.dirty = true;
            }
            for name in Audio::output_devices() {
                let chosen = s.audio_device.as_deref() == Some(name.as_str());
                if ui.selectable_label(chosen, &name).clicked() {
                    s.audio_device = Some(name);
                    s.dirty = true;
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(mut settings: Settings) -> Vec<(egui::Rect, String)> {
        let ctx = egui::Context::default();
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 4000.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                SettingsContent::new(&mut settings, &mut capture, &mut editor, None, None).ui(ui);
            });
        });
        output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.rect.translate(text.pos.to_vec2()),
                    text.galley.text().trim().to_owned(),
                )),
                _ => None,
            })
            .collect()
    }

    fn texts_of(tab: SettingsTab) -> Vec<String> {
        render(Settings {
            current_tab: tab,
            ..Settings::default()
        })
        .into_iter()
        .map(|(_, text)| text)
        .collect()
    }

    #[test]
    fn the_layout_hint_sits_under_the_layout_row() {
        let shapes = render(Settings {
            current_tab: SettingsTab::General,
            layout: LayoutMode::Mobile,
            ..Settings::default()
        });
        let find = |prefix: &str| {
            shapes
                .iter()
                .find(|(_, text)| text.starts_with(prefix))
                .unwrap_or_else(|| panic!("no {prefix:?} on the page"))
                .0
        };
        let layout = find("Layout");
        let hint = find("The touch layout sizes itself");
        let next = shapes
            .iter()
            .map(|(rect, _)| *rect)
            .filter(|rect| rect.top() > layout.bottom() && rect.left() <= layout.left() + 1.0)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("a row after Layout");
        assert!(
            hint.top() >= layout.bottom(),
            "hint {hint:?} above its row {layout:?}"
        );
        assert!(
            hint.bottom() <= next.top(),
            "hint {hint:?} not before the next row {next:?}"
        );
        assert!(
            hint.left() > layout.right(),
            "hint {hint:?} not under the control column"
        );
        assert!(
            hint.top() - layout.bottom() < next.top() - hint.bottom(),
            "hint {hint:?} is closer to the next row {next:?} than to its own {layout:?}"
        );
    }

    #[test]
    fn no_page_repeats_its_own_name() {
        for tab in SettingsTab::iter_with_dev_mode(true) {
            let texts = texts_of(tab);
            assert!(!texts.is_empty(), "{} rendered nothing", tab.title());
            assert!(
                !texts.iter().any(|text| text == tab.title()),
                "{} shows its own name",
                tab.title()
            );
        }
    }
}
