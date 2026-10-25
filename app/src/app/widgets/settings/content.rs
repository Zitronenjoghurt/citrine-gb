use crate::app::ui_state::keybinds::BindingCapture;
use crate::app::ui_state::settings::{LayoutMode, ScalingMode, Settings, SettingsTab};
use crate::app::ui_state::ui_theme::UiTheme;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::palette::PaletteEditorState;
use crate::app::widgets::settings::{controls, palette};
use crate::audio::{Audio, DEFAULT_LATENCY_MS, MAX_LATENCY_MS, MIN_LATENCY_MS};
use egui::{Grid, Response, ScrollArea, Ui, Widget};

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
            .show(ui, |ui| {
                ui.vertical_centered(|ui| match self.settings.current_tab {
                    SettingsTab::General => self.general(ui),
                    SettingsTab::Sound => self.sound(ui),
                    SettingsTab::Controls => self.controls(ui),
                    SettingsTab::Style => self.style(ui),
                    SettingsTab::Developer => self.developer(ui),
                })
                .response
            })
            .inner
    }
}

impl SettingsContent<'_> {
    fn general(self, ui: &mut Ui) {
        ui.heading("General");
        ui.separator();

        let s = self.settings;

        Grid::new("settings_general_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Layout");
                s.dirty |= GenericSelect::from_enum(&mut s.layout, "enum_select_layout")
                    .default_value(LayoutMode::default())
                    .ui(ui)
                    .on_hover_text("Automatic uses the touch layout on small screens")
                    .changed();
                ui.end_row();

                ui.label("UI Scale");
                let response = ResetSlider::new(&mut s.ui_scale, 0.5..=5.0)
                    .step_by(0.1)
                    .default_value(Settings::DEFAULT_UI_SCALE)
                    .ui(ui);
                s.note_scale_drag(&response);
                ui.end_row();

                ui.label("UI Theme");
                s.dirty |= GenericSelect::from_enum(&mut s.ui_theme, "enum_select_ui_theme")
                    .default_value(UiTheme::default())
                    .ui(ui)
                    .changed();
                ui.end_row();

                ui.label("Developer Mode");
                s.dirty |= ui.checkbox(&mut s.dev_mode, "").changed();
                ui.end_row();
            });

        if s.layout == LayoutMode::Mobile {
            ui.add_space(6.0);
            ui.small(
                "The touch layout sizes itself to the device; use Controls → UI size to adjust it.",
            );
        }

        ui.add_space(10.0);
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
        ui.heading("Sound");
        ui.separator();

        let s = self.settings;

        Grid::new("settings_sound_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Volume");
                s.dirty |= ResetSlider::new(&mut s.volume, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_VOLUME)
                    .ui(ui)
                    .changed();
                ui.end_row();

                ui.label("Mute");
                s.dirty |= ui.checkbox(&mut s.muted, "").changed();
                ui.end_row();

                ui.label("Latency");
                let response = ui.add(
                    egui::Slider::new(&mut s.audio_latency_ms, MIN_LATENCY_MS..=MAX_LATENCY_MS)
                        .suffix(" ms"),
                );
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    s.dirty = true;
                }
                response.on_hover_text(
                    "Lower is more responsive, higher is more resistant to crackling",
                );
                if s.audio_latency_ms != DEFAULT_LATENCY_MS
                    && ui
                        .button(crate::icons::ARROW_COUNTER_CLOCKWISE)
                        .on_hover_text("Reset")
                        .clicked()
                {
                    s.audio_latency_ms = DEFAULT_LATENCY_MS;
                    s.dirty = true;
                }
                ui.end_row();

                ui.label("Channels");
                ui.horizontal(|ui| {
                    for (index, enabled) in s.channels.iter_mut().enumerate() {
                        s.dirty |= ui
                            .checkbox(enabled, format!("{}", index + 1))
                            .on_hover_text(CHANNEL_NAMES[index])
                            .changed();
                    }
                });
                ui.end_row();
            });

        #[cfg(not(target_arch = "wasm32"))]
        {
            ui.add_space(8.0);
            ui.separator();
            device_picker(ui, s);
        }

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
        ui.heading("Controls");
        ui.separator();
        controls::show(ui, self.settings, self.capture);
    }

    fn style(self, ui: &mut Ui) {
        ui.heading("GB Style");
        ui.separator();

        palette::show(ui, self.settings, self.palette_editor, self.game);

        ui.add_space(10.0);
        ui.separator();

        let s = self.settings;

        Grid::new("settings_style_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Scaling");
                s.dirty |= GenericSelect::from_enum(&mut s.scaling, "enum_select_scaling")
                    .default_value(ScalingMode::default())
                    .ui(ui)
                    .changed();
                ui.end_row();

                ui.label("Matrix");
                s.dirty |= ui
                    .checkbox(&mut s.matrix, "")
                    .on_hover_text("Draw the grid between pixels; forces integer scaling")
                    .changed();
                ui.end_row();

                ui.label("Edge Darkness");
                s.dirty |= ResetSlider::new(&mut s.matrix_edge_darkness, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_MATRIX_EDGE_DARKNESS)
                    .ui(ui)
                    .changed();
                ui.end_row();

                ui.label("Corner Darkness");
                s.dirty |= ResetSlider::new(&mut s.matrix_corner_darkness, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_MATRIX_CORNER_DARKNESS)
                    .ui(ui)
                    .changed();
                ui.end_row();

                ui.separator();
                ui.separator();
                ui.end_row();

                ui.label("Ghosting");
                s.dirty |= ui
                    .checkbox(&mut s.ghosting, "")
                    .on_hover_text("Smear frames together, the way the original LCD did")
                    .changed();
                ui.end_row();

                ui.label("Strength");
                s.dirty |= ResetSlider::new(&mut s.ghosting_strength, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_GHOSTING_STRENGTH)
                    .ui(ui)
                    .changed();
                ui.end_row();
            });

        if s.matrix && s.scaling != ScalingMode::Integer {
            ui.add_space(6.0);
            ui.small("The matrix needs whole-number scaling, so Citrine scales by whole numbers while it is on.");
        }
    }

    fn developer(self, ui: &mut Ui) {
        ui.heading("Developer");
        ui.separator();

        let s = self.settings;

        Grid::new("settings_developer_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Randomized power-on RAM");
                s.dirty |= ui.checkbox(&mut s.randomized_ram, "").changed();
                ui.end_row();
            });

        ui.add_space(10.0);
        ui.separator();
        ui.small(format!(
            "{} custom palette(s) saved.",
            s.palettes.entries().len()
        ));
    }
}

const CHANNEL_NAMES: [&str; 4] = ["Pulse with sweep", "Pulse", "Wave", "Noise"];

#[cfg(not(target_arch = "wasm32"))]
fn device_picker(ui: &mut Ui, s: &mut Settings) {
    ui.horizontal(|ui| {
        ui.label("Output device");

        let selected = s
            .audio_device
            .clone()
            .unwrap_or_else(|| "System default".to_string());

        egui::ComboBox::from_id_salt("audio_device_picker")
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
    });
    ui.small("Changing this reopens the sound device.");
}
