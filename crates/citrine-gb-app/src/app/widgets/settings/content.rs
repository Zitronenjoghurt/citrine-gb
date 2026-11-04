use crate::app::events::AppEventQueue;
use crate::app::tabs::Tab;
use crate::app::ui_state::keybinds::BindingCapture;
use crate::app::ui_state::settings::{
    self, ConsoleChoice, GBA_SCREEN_NOTE, GBC_SCREEN_NOTE, GbaScreen, LayoutMode, ScalingMode,
    Settings, SettingsTab, plays_on_both,
};
use crate::app::ui_state::ui_theme::UiTheme;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::section::{section, section_with_hint};
use crate::app::widgets::settings::cartridge::CartridgeState;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::palette::PaletteEditorState;
use crate::app::widgets::settings::{cartridge, controls, online, palette, your_data};
use crate::app::widgets::text::faint;
use crate::audio::{Audio, DEFAULT_LATENCY_MS, MAX_LATENCY_MS, MIN_LATENCY_MS};
use crate::autosave::{AutosaveInterval, DEFAULT_KEEP, MAX_KEEP};
use citrine_gb::console::ConsoleKind;
use citrine_gb::gb::GbModel;
use citrine_gb::gb::apu::CHANNEL_NAMES;
use citrine_gb::gb::ppu::types::color::ColorCorrection;
use citrine_gb::rom::header::RomCgbMode;
use egui::{Response, ScrollArea, Ui, Widget};

#[cfg(not(target_arch = "wasm32"))]
const UPDATES_NOTE: &str = "Citrine asks GitHub for the newest release when it starts and once a day. Updates install only when you choose to.";
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
const AUTOSAVE_NOTE: &str = "Citrine also autosaves when you close it or switch games, and after a crash. Autosaves show in the Saves tab, apart from your snapshots.";
#[cfg(target_arch = "wasm32")]
const AUTOSAVE_NOTE: &str = "Citrine also autosaves when you leave the page or switch games, and after a crash. Autosaves show in the Saves tab, apart from your snapshots.";
#[cfg(target_os = "android")]
const AUTOSAVE_NOTE: &str = "Citrine also autosaves when you leave the app or switch games, and after a crash. Autosaves show in the Saves tab, apart from your snapshots.";

pub struct SettingsContent<'a> {
    settings: &'a mut Settings,
    capture: &'a mut BindingCapture,
    palette_editor: &'a mut PaletteEditorState,
    audio: Option<&'a Audio>,
    game: Option<LoadedGame<'a>>,
    palettes: bool,
    console: ConsoleKind,
    channels: &'static [&'static str],
    #[cfg(feature = "gba")]
    gba_bios: Option<&'a str>,
    events: Option<&'a mut AppEventQueue>,
    online: Option<(&'a mut crate::online::Online, &'a mut online::JoinForm)>,
    your_data: Option<&'a mut your_data::YourDataUi>,
    cartridge: Option<CartridgeState<'a>>,
    top_space: f32,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    discord: Option<(
        Option<crate::discord::GameNow>,
        Option<egui::TextureId>,
        bool,
    )>,
}

#[derive(Clone, Copy)]
pub struct LoadedGame<'a> {
    pub rom_key: &'a str,
    pub title: &'a str,
    pub model: GbModel,
    pub cgb_mode: RomCgbMode,
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
        events: Option<&'a mut AppEventQueue>,
    ) -> Self {
        Self {
            settings,
            capture,
            palette_editor,
            audio,
            game,
            palettes: true,
            console: ConsoleKind::Gb,
            channels: &CHANNEL_NAMES,
            #[cfg(feature = "gba")]
            gba_bios: None,
            events,
            online: None,
            your_data: None,
            cartridge: None,
            top_space: 0.0,
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            discord: None,
        }
    }

    pub fn palettes(mut self, shown: bool) -> Self {
        self.palettes = shown;
        self
    }

    pub fn console(mut self, console: ConsoleKind) -> Self {
        self.console = console;
        self
    }

    pub fn channels(mut self, names: &'static [&'static str]) -> Self {
        self.channels = names;
        self
    }

    #[cfg(feature = "gba")]
    pub fn gba_bios(mut self, name: Option<&'a str>) -> Self {
        self.gba_bios = name;
        self
    }

    pub fn top_space(mut self, space: f32) -> Self {
        self.top_space = space;
        self
    }

    pub fn cartridge(mut self, state: CartridgeState<'a>) -> Self {
        self.cartridge = state.cart.any().then_some(state);
        self
    }

    pub fn your_data(mut self, state: &'a mut your_data::YourDataUi) -> Self {
        self.your_data = Some(state);
        self
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    pub fn discord(
        mut self,
        game: Option<crate::discord::GameNow>,
        screen: Option<egui::TextureId>,
        pictures_off: bool,
    ) -> Self {
        self.discord = Some((game, screen, pictures_off));
        self
    }

    pub fn online(
        mut self,
        online: &'a mut crate::online::Online,
        form: &'a mut online::JoinForm,
    ) -> Self {
        self.online = Some((online, form));
        self
    }
}

impl Widget for SettingsContent<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let margin = ui.visuals().clip_rect_margin;
        ui.scope(|ui| {
            ui.visuals_mut().clip_rect_margin = 0.0;
            ScrollArea::vertical()
                .id_salt("settings_content_scroll")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.visuals_mut().clip_rect_margin = margin;
                    ui.add_space(self.top_space);
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(ui.available_width().min(MAX_COLUMN_WIDTH));
                        match self.settings.current_tab {
                            SettingsTab::General => self.general(ui),
                            SettingsTab::Sound => self.sound(ui),
                            SettingsTab::Controls => self.controls(ui),
                            SettingsTab::Style => self.style(ui),
                            SettingsTab::Online => self.online_page(ui),
                            SettingsTab::YourData => self.your_data_page(ui),
                            SettingsTab::Developer => self.developer(ui),
                        }
                    })
                    .response
                })
                .inner
        })
        .inner
    }
}

impl SettingsContent<'_> {
    fn general(self, ui: &mut Ui) {
        let s = self.settings;
        let game = self.game;
        let video = self.console.video();
        let mut events = self.events;
        #[cfg(feature = "gba")]
        let gba_bios = self.gba_bios;
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        let discord = self.discord;
        let touch = s.layout.is_touch(ui.ctx());

        Form::show(ui, "general", |form| {
            let both = game.filter(|game| plays_on_both(game.cgb_mode));
            form.row_with_hint(
                "Console",
                "Automatic picks Game Boy Color for games made for it.",
                |ui| {
                    s.dirty |= GenericSelect::from_enum(&mut s.console, "enum_select_console")
                        .default_value(ConsoleChoice::default())
                        .ui(ui)
                        .changed();
                },
            );
            if let Some(game) = game
                && both.is_none()
                && s.console_for(game.rom_key, game.cgb_mode) != game.model
            {
                form.hint("The console changes the next time a game loads.");
            }
            if let Some(game) = both {
                form.row("This game", |ui| {
                    let mut choice = ConsoleChoice::of(game.model);
                    let default = ConsoleChoice::of(s.console.resolve(GbModel::Cgb));
                    let changed = GenericSelect::from_enum(&mut choice, "enum_select_game_console")
                        .filter(|choice| choice != ConsoleChoice::Auto)
                        .default_value(default)
                        .ui(ui)
                        .changed();
                    if changed && let Some(events) = events.as_deref_mut() {
                        events.play_on(choice.resolve(GbModel::Cgb));
                    }
                });
                let next = s.console_for(game.rom_key, game.cgb_mode);
                if next != game.model {
                    form.hint(format!(
                        "It starts on {} next time.",
                        settings::console_name(next)
                    ));
                }
            }

            #[cfg(feature = "gba")]
            form.row_with_hint("GBA BIOS", crate::app::gba_bios::BIOS_HINT, |ui| {
                use crate::app::gba_bios::BiosChoice;
                let mut choice = BiosChoice::current(gba_bios);
                let response = GenericSelect::new(
                    &mut choice,
                    BiosChoice::all(gba_bios),
                    "enum_select_gba_bios",
                )
                .default_value(BiosChoice::BuiltIn)
                .middle_ellipsis()
                .ui(ui);
                if let Some(name) = gba_bios {
                    response.clone().on_hover_text(name);
                }
                if response.changed()
                    && let Some(events) = events.as_deref_mut()
                {
                    choice.apply(events);
                }
            });

            form.row_with_hint("Layout", LAYOUT_HINT, |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.layout, "enum_select_layout")
                    .default_value(LayoutMode::default())
                    .ui(ui)
                    .changed();
            });
            if s.layout == LayoutMode::Desktop && LayoutMode::too_narrow_for_desktop(form.ctx()) {
                form.hint(
                    "This window is too narrow for the desktop layout, so it uses the touch layout.",
                );
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

            #[cfg(not(target_arch = "wasm32"))]
            form.row_with_hint("Check for updates", UPDATES_NOTE, |ui| {
                s.dirty |=
                    crate::app::widgets::check::checkbox(ui, &mut s.check_updates, "").changed();
            });

            form.row("Dev tools", |ui| {
                s.dirty |= crate::app::widgets::check::checkbox(ui, &mut s.dev_mode, "").changed();
            });
        });

        if let Some(state) = self.cartridge {
            ui.add_space(24.0);
            cartridge::show(ui, s, state, events);
        }

        ui.add_space(10.0);
        section_with_hint(
            ui,
            crate::icons::CLOCK_COUNTER_CLOCKWISE,
            "Autosave",
            AUTOSAVE_NOTE,
            |ui| {
                Form::show(ui, "general", |form| {
                    form.row_with_hint(
                        "Every",
                        "Minutes of play between autosaves. Paused time doesn't count.",
                        |ui| {
                            s.dirty |= GenericSelect::from_enum(
                                &mut s.autosave_interval,
                                "enum_select_autosave_interval",
                            )
                            .default_value(AutosaveInterval::default())
                            .ui(ui)
                            .changed();
                        },
                    );

                    form.row_with_hint(
                        "Keep",
                        "When there are more, the oldest autosave goes.",
                        |ui| {
                            let response = ResetSlider::new(&mut s.autosave_keep, 1..=MAX_KEEP)
                                .default_value(DEFAULT_KEEP)
                                .ui(ui);
                            if response.drag_stopped()
                                || (response.changed() && !response.dragged())
                            {
                                s.dirty = true;
                            }
                        },
                    );
                });
            },
        );

        ui.add_space(10.0);
        section(ui, crate::icons::CAMERA, "Capture", |ui| {
            Form::show(ui, "capture", |form| {
                form.row("Screenshot scale", |ui| {
                    s.dirty |= capture_scale(ui, &mut s.screenshot_scale, 1, video);
                });
                form.row("Recording scale", |ui| {
                    s.dirty |= capture_scale(ui, &mut s.recording_scale, 2, video);
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
        });
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        if let Some((now, screen, pictures_off)) = discord {
            ui.add_space(10.0);
            super::discord::section(ui, s, now.as_ref(), screen, pictures_off);
        }
        if crate::app::mobile::touch_square(ui) > 0.0 {
            return;
        }
        ui.separator();
        let where_kept = format!(
            "Citrine keeps settings in {}, beside your saves.",
            crate::storage::SettingsStore::default().location()
        );
        ui.horizontal(|ui| {
            ui.label(faint(format!("Citrine {}", env!("CARGO_PKG_VERSION"))));
            info_mark(ui, &where_kept);
        });
    }

    fn sound(self, ui: &mut Ui) {
        let s = self.settings;
        let names = self.channels;
        let hint = if names.len() == CHANNEL_NAMES.len() {
            CHANNELS_HINT
        } else {
            GBA_CHANNELS_HINT
        };

        Form::show(ui, "sound", |form| {
            form.row("Volume", |ui| {
                s.dirty |= ResetSlider::new(&mut s.volume, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_VOLUME)
                    .ui(ui)
                    .changed();
            });

            form.row("Mute", |ui| {
                s.dirty |= crate::app::widgets::check::checkbox(ui, &mut s.muted, "").changed();
            });

            form.row_with_hint(
                "Latency",
                "Lower reacts faster. Higher crackles less.",
                |ui| {
                    let response =
                        ResetSlider::new(&mut s.audio_queue_ms, MIN_LATENCY_MS..=MAX_LATENCY_MS)
                            .default_value(DEFAULT_LATENCY_MS)
                            .suffix(" ms")
                            .ui(ui);
                    if response.drag_stopped() || (response.changed() && !response.dragged()) {
                        s.dirty = true;
                    }
                },
            );

            form.row_with_hint("Channels", hint, |ui| {
                let columns = channel_columns(ui, names);
                let mut changed = false;
                egui::Grid::new("channels")
                    .num_columns(columns)
                    .show(ui, |ui| {
                        for (index, (name, enabled)) in names
                            .iter()
                            .zip(s.channels_for(names.len()).iter_mut())
                            .enumerate()
                        {
                            changed |=
                                crate::app::widgets::check::checkbox(ui, enabled, *name).changed();
                            if (index + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
                s.dirty |= changed;
            });

            #[cfg(not(target_arch = "wasm32"))]
            {
                form.separator();
                form.row_with_hint(
                    "Output device",
                    "Changing this reopens the sound device.",
                    |ui| device_picker(ui, s),
                );
            }
        });

        if let Some(audio) = self.audio {
            let phone = crate::app::mobile::touch_square(ui) > 0.0;
            ui.add_space(8.0);
            if audio.is_running() {
                if !phone {
                    ui.small(format!(
                        "Playing at {} Hz on {}.",
                        audio.get_sample_rate(),
                        audio.device_name().unwrap_or("the default device")
                    ));
                }
            } else {
                ui.small("Sound starts when you load a game or tap the screen.");
            }
        }
    }

    fn controls(self, ui: &mut Ui) {
        if controls::show(ui, self.settings, self.capture)
            && let Some(events) = self.events
        {
            events.open_tab(Tab::ControllerDebug);
        }
    }

    fn style(self, ui: &mut Ui) {
        if self.palettes {
            palette::show(ui, self.settings, self.palette_editor, self.game);
            ui.add_space(10.0);
            ui.separator();
        }

        let s = self.settings;
        let gba_screen = self.console == ConsoleKind::Gba;

        Form::show(ui, "style", |form| {
            form.row("Scaling", |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.scaling, "enum_select_scaling")
                    .default_value(ScalingMode::default())
                    .ui(ui)
                    .changed();
            });

            form.row_with_hint(
                "Matrix",
                "Draws the grid between pixels. It needs whole-number scaling.",
                |ui| {
                    s.dirty |=
                        crate::app::widgets::check::checkbox(ui, &mut s.matrix, "").changed();
                },
            );
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

            form.row_with_hint(
                "Ghosting",
                "Smears frames together, the way the original LCD did.",
                |ui| {
                    s.dirty |=
                        crate::app::widgets::check::checkbox(ui, &mut s.ghosting, "").changed();
                },
            );

            form.row("Strength", |ui| {
                s.dirty |= ResetSlider::new(&mut s.ghosting_strength, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_GHOSTING_STRENGTH)
                    .ui(ui)
                    .changed();
            });

            form.separator();

            if gba_screen {
                form.row_with_hint("GBA screen", GBA_SCREEN_NOTE, |ui| {
                    let mut screen = GbaScreen(s.color_correction);
                    s.dirty |= GenericSelect::new(&mut screen, GbaScreen::ALL, "gba_screen")
                        .default_value(GbaScreen(ColorCorrection::Screen))
                        .ui(ui)
                        .changed();
                    s.color_correction = screen.0;
                });
            } else {
                form.row_with_hint("GBC screen", GBC_SCREEN_NOTE, |ui| {
                    s.dirty |=
                        GenericSelect::from_enum(&mut s.color_correction, "enum_select_gbc_screen")
                            .default_value(ColorCorrection::Screen)
                            .ui(ui)
                            .changed();
                });
            }
        });
    }

    fn online_page(self, ui: &mut Ui) {
        let mut spare = online::JoinForm::default();
        let part = if self.settings.layout.is_touch(ui.ctx()) {
            online::Part::Relay
        } else {
            online::Part::All
        };
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            match self.online {
                Some((client, form)) => {
                    online::show_part(ui, self.settings, Some(client), form, part)
                }
                None => online::show_part(ui, self.settings, None, &mut spare, part),
            }
        });
    }

    fn your_data_page(self, ui: &mut Ui) {
        match self.your_data {
            Some(state) => your_data::show(ui, state),
            None => your_data::show(ui, &mut your_data::YourDataUi::default()),
        }
    }

    fn developer(self, ui: &mut Ui) {
        let s = self.settings;

        Form::show(ui, "developer", |form| {
            form.row("Randomized power-on RAM", |ui| {
                s.dirty |=
                    crate::app::widgets::check::checkbox(ui, &mut s.randomized_ram, "").changed();
            });
        });
    }
}

const MAX_COLUMN_WIDTH: f32 = 480.0;

fn channel_columns(ui: &Ui, names: &[&str]) -> usize {
    let spacing = ui.spacing();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let boxes: f32 = names
        .iter()
        .map(|name| {
            let text = ui
                .painter()
                .layout_no_wrap(name.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x;
            spacing.icon_width + spacing.icon_spacing + text
        })
        .sum();
    let gaps = spacing.item_spacing.x * (names.len() - 1) as f32;
    if boxes + gaps <= ui.available_width() {
        names.len()
    } else {
        2
    }
}

const CHANNELS_HINT: &str = "The Game Boy's four sound channels. Pulse 1 and 2 usually carry the tune, Wave the bass and Noise the drums.";

const GBA_CHANNELS_HINT: &str = "The Game Boy Advance's six sound channels. The first four are the Game Boy's. Direct Sound A and B play recorded sound, often the music and voices.";

const LAYOUT_HINT: &str = "Automatic uses the touch layout on small screens. To resize the touch layout, use UI size on the Controls page.";

fn capture_scale(
    ui: &mut Ui,
    scale: &mut usize,
    step: usize,
    video: citrine_gb::console::VideoSpec,
) -> bool {
    use crate::media::recording::{DEFAULT_SCALE, MAX_SCALE};
    let response = ResetSlider::new(scale, step..=MAX_SCALE)
        .step_by(step as f64)
        .default_value(DEFAULT_SCALE)
        .suffix("x")
        .ui(ui);
    let changed = response.drag_stopped() || (response.changed() && !response.dragged());
    response.on_hover_text(format!(
        "{} × {} pixels",
        video.width * *scale,
        video.height * *scale
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
                SettingsContent::new(&mut settings, &mut capture, &mut editor, None, None, None)
                    .ui(ui);
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

    #[test]
    fn selects_show_their_whole_value_in_the_desktop_dock() {
        let ctx = egui::Context::default();
        let mut settings = Settings {
            current_tab: SettingsTab::General,
            ..Settings::default()
        };
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let game = LoadedGame {
            rom_key: "key",
            title: "TOBUDX",
            model: GbModel::Cgb,
            cgb_mode: RomCgbMode::CgbAndGb,
            header: &[],
            own_colors: false,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut texts = Vec::new();
        let mut elided = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        SettingsContent::new(
                            &mut settings,
                            &mut capture,
                            &mut editor,
                            None,
                            Some(game),
                            None,
                        )
                        .ui(ui);
                    });
                });
            });
            let galleys = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley),
                    _ => None,
                })
                .collect::<Vec<_>>();
            texts = galleys
                .iter()
                .map(|galley| galley.text().trim().to_owned())
                .collect();
            elided = galleys
                .iter()
                .filter(|galley| galley.elided)
                .map(|galley| galley.text().trim().to_owned())
                .collect();
        }
        assert!(
            texts.iter().any(|text| text == "Game Boy Color"),
            "{texts:?}"
        );
        assert!(texts.iter().any(|text| text == "Automatic"), "{texts:?}");
        assert!(elided.is_empty(), "cut short: {elided:?}");
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
    fn a_state_line_sits_under_its_row() {
        let shapes = render(Settings {
            current_tab: SettingsTab::Style,
            matrix: true,
            scaling: ScalingMode::Fit,
            ..Settings::default()
        });
        let find = |prefix: &str| {
            shapes
                .iter()
                .find(|(_, text)| text.starts_with(prefix))
                .unwrap_or_else(|| panic!("no {prefix:?} on the page"))
                .0
        };
        let layout = find("Matrix");
        let hint = find("The matrix needs whole-number scaling");
        let next = shapes
            .iter()
            .map(|(rect, _)| *rect)
            .filter(|rect| rect.top() > layout.bottom() && rect.left() <= layout.left() + 1.0)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("a row after Matrix");
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

    #[test]
    fn explainers_stay_behind_their_marks() {
        let explainers = [
            AUTOSAVE_NOTE,
            LAYOUT_HINT,
            CHANNELS_HINT,
            "keeps settings in",
            "Minutes of play between autosaves",
            "Lower reacts faster",
            "Draws the grid between pixels",
            "The touch layout shows these",
            "Overwrite the starred snapshot slot",
            "Applies to every game",
        ];
        for tab in SettingsTab::iter_with_dev_mode(true) {
            for text in texts_of(tab) {
                assert!(
                    !explainers.iter().any(|explainer| text.contains(explainer)),
                    "{} shows {text:?} on the page",
                    tab.title()
                );
            }
        }
        let joined = render(joined_online());
        assert!(joined.iter().any(|(_, text)| text == "Relay ID"));
        for (_, text) in joined {
            assert!(
                !text.contains("Anyone with your sign-in key"),
                "Online shows {text:?} on the page"
            );
        }
    }

    fn joined_online() -> Settings {
        let mut settings = Settings {
            current_tab: SettingsTab::Online,
            ..Settings::default()
        };
        settings.online.remember(
            &crate::online::ServerAddress::parse("gb.lemon.industries").unwrap(),
            &citrine_gb_net::RelayId::random(),
            "pw",
            &citrine_gb_net::Account {
                online_id: citrine_gb_net::OnlineId::new(0x1234_5678).unwrap(),
                display_name: "Clara".into(),
                storage_used: 0,
                quota_bytes: 0,
                two_factor: false,
                codes_left: 0,
            },
            &Default::default(),
        );
        settings
    }

    #[test]
    fn hints_are_as_large_as_the_rows_they_explain() {
        let shapes = render(Settings {
            current_tab: SettingsTab::Style,
            matrix: true,
            scaling: ScalingMode::Fit,
            ..Settings::default()
        });
        let height = |prefix: &str| {
            let rows = shapes
                .iter()
                .filter(|(_, text)| text.starts_with(prefix))
                .map(|(rect, _)| rect.height())
                .collect::<Vec<_>>();
            assert!(!rows.is_empty(), "no {prefix:?} on the page");
            rows[0]
        };
        let label = height("Matrix");
        let hint = height("The matrix needs whole-number scaling");
        assert!(hint >= label - 0.5, "hint {hint} pt high, row {label} pt");
    }

    #[test]
    fn scrolled_rows_never_show_above_the_page() {
        let ctx = egui::Context::default();
        let mut settings = Settings {
            current_tab: SettingsTab::General,
            ..Settings::default()
        };
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        };
        let mut top = f32::NAN;
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.separator();
                top = ui.cursor().top();
                SettingsContent::new(&mut settings, &mut capture, &mut editor, None, None, None)
                    .ui(ui);
            });
        });
        let clips = output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::Text(_)))
            .map(|clipped| clipped.clip_rect.top())
            .collect::<Vec<_>>();
        assert!(!clips.is_empty());
        for clip in clips {
            assert!(
                clip >= top - 0.01,
                "rows show from {clip}, the page starts at {top}"
            );
        }
    }

    #[cfg(feature = "gba")]
    mod gba {
        use crate::app::files::tests::Opened;
        use crate::app::tabs::Tab;
        use crate::app::tests::{info_frame_out, text_rect};
        use crate::app::ui_state::settings::{LayoutMode, SettingsTab};
        use egui::Context;

        #[test]
        fn the_sound_page_lists_the_running_consoles_channels() {
            for gba in [false, true] {
                let mut opened = Opened::new(&format!("channels-{gba}"));
                opened.app.ui.settings.layout = LayoutMode::Desktop;
                if gba {
                    let file = opened.picked();
                    opened.app.handle_load_rom(file);
                }
                opened.app.ui.settings.current_tab = SettingsTab::Sound;
                opened.app.open_tab(Tab::Settings);
                let ctx = Context::default();
                info_frame_out(&mut opened.app, &ctx, 1280.0, 900.0);
                let output = info_frame_out(&mut opened.app, &ctx, 1280.0, 900.0);
                assert!(text_rect(&output, "Noise").is_some(), "gba {gba}");
                assert_eq!(
                    text_rect(&output, "Direct Sound A").is_some(),
                    gba,
                    "gba {gba}"
                );
            }
        }
    }
}
