use crate::app::big_screen::scroll::Following;
use crate::app::events::AppEventQueue;
use crate::app::tabs::Tab;
use crate::app::ui_state::keybinds::BindingCapture;
use crate::app::ui_state::settings::{
    self, ConsoleChoice, GBA_SCREEN_NOTE, GBC_SCREEN_NOTE, GbaScreen, LayoutMode, SLOWDOWN_LABEL,
    ScalingMode, Settings, SettingsTab, plays_on_both,
};
use crate::app::ui_state::touch::TouchSettings;
use crate::app::ui_state::ui_theme::UiTheme;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::cartridge::CartridgeState;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::app::widgets::settings::palette::PaletteEditorState;
use crate::app::widgets::settings::{cartridge, controls, ds_screens, online, palette, your_data};
use crate::audio::pacer::SlowSound;
use crate::audio::{Audio, DEFAULT_LATENCY_MS, MAX_LATENCY_MS, MIN_LATENCY_MS};
use crate::autosave::{AutosaveInterval, DEFAULT_KEEP, MAX_KEEP};
use crate::emulator::fast_forward::FastForwardSpeed;
use crate::emulator::load::ScreenMatch;
use crate::emulator::rewind::{REWIND_NOTE, RewindLength};
use citrine_gb::console::ConsoleKind;
use citrine_gb::gb::GbModel;
use citrine_gb::gb::apu::CHANNEL_NAMES;
use citrine_gb::gb::ppu::types::color::ColorCorrection;
use citrine_gb::rom::header::RomCgbMode;
use egui::{Response, ScrollArea, Ui, Widget};

const COVERS_NOTE: &str = "Citrine downloads each game's box art from libretro's thumbnail collection on GitHub the first time you play it, and keeps it on this device.";
#[cfg(not(target_arch = "wasm32"))]
const PROGRESS_NOTE: &str = "Shows under each game in the library how many of its RetroAchievements you've earned, while you're signed in to RetroAchievements.";
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
    #[cfg(feature = "nds")]
    ds: Option<super::ds::DsState<'a>>,
    events: Option<&'a mut AppEventQueue>,
    online: Option<(&'a mut crate::online::Online, &'a mut online::JoinForm)>,
    your_data: Option<&'a mut your_data::YourDataUi>,
    cartridge: Option<CartridgeState<'a>>,
    screen_match: Option<ScreenMatch>,
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
            #[cfg(feature = "nds")]
            ds: None,
            events,
            online: None,
            your_data: None,
            cartridge: None,
            screen_match: None,
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

    #[cfg(feature = "nds")]
    pub fn ds(mut self, state: super::ds::DsState<'a>) -> Self {
        self.ds = Some(state);
        self
    }

    pub fn screen_match(mut self, status: Option<ScreenMatch>) -> Self {
        self.screen_match = status;
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
                .show_following(ui, |ui| {
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
        let has_game = self.cartridge.is_some();
        #[cfg(feature = "gba")]
        let gba_bios = self.gba_bios;
        #[cfg(feature = "nds")]
        let ds = self.ds;
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        let discord = self.discord;
        let touch = s.layout.is_touch(ui.ctx());

        Page::show(ui, |page| {
            page.plain(|ui| Form::show(ui, "general", |form| {
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

            if touch {
                form.row("UI size", |ui| {
                    let response = ResetSlider::new(
                        &mut s.touch.ui_scale,
                        TouchSettings::MIN_UI_SCALE..=TouchSettings::MAX_UI_SCALE,
                    )
                    .step_by(0.05)
                    .default_value(1.0)
                    .ui(ui);
                    s.note_scale_drag(&response);
                });
            } else {
                form.row("UI scale", |ui| {
                    let response = ResetSlider::new(&mut s.ui_scale, 0.5..=5.0)
                        .step_by(0.1)
                        .default_value(Settings::DEFAULT_UI_SCALE)
                        .ui(ui);
                    s.note_scale_drag(&response);
                });
            }

            form.row("Theme", |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.ui_theme, "enum_select_ui_theme")
                    .default_value(UiTheme::default())
                    .ui(ui)
                    .changed();
            });

            #[cfg(not(target_arch = "wasm32"))]
            {
                s.dirty |=
                    form.toggle_with_hint("Check for updates", UPDATES_NOTE, &mut s.check_updates);
            }

            s.dirty |= form.toggle_with_hint("Game covers", COVERS_NOTE, &mut s.covers);

            #[cfg(not(target_arch = "wasm32"))]
            {
                s.dirty |= form.toggle_with_hint(
                    "Achievement progress",
                    PROGRESS_NOTE,
                    &mut s.achievements.in_library,
                );
            }

            #[cfg(feature = "nds")]
            if citrine_gb::nds::machine::JIT_AVAILABLE {
                s.dirty |= form.toggle_with_hint(settings::JIT_LABEL, settings::JIT_HINT, &mut s.ds_jit);
                form.buttons(|ui| {
                    if ui
                        .button(format!("{} {}", crate::icons::TRASH, settings::CLEAR_JIT_LABEL))
                        .clicked()
                        && let Some(events) = events.as_deref_mut()
                    {
                        events.clear_jit_cache();
                    }
                });
            }

            s.dirty |= form.toggle("Dev tools", &mut s.dev_mode);
        }));

            if let Some(state) = self.cartridge {
                cartridge::show(page, s, state, &mut events);
            }

            #[cfg(feature = "nds")]
            if let Some(state) = ds {
                super::ds::show(page, s, state, &mut events);
                super::ds::walker(page, s);
            }

            page.section(
                Heading::new(crate::icons::CLOCK_COUNTER_CLOCKWISE, "Autosave").hint(AUTOSAVE_NOTE),
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

            page.section(
                Heading::new(crate::icons::FAST_FORWARD, "Fast forward"),
                |ui| {
                    Form::show(ui, "fast_forward", |form| {
                        form.row("Speed", |ui| {
                            s.dirty |= GenericSelect::from_enum(
                                &mut s.fast_forward_speed,
                                "enum_select_fast_forward_speed",
                            )
                            .default_value(FastForwardSpeed::default())
                            .ui(ui)
                            .changed();
                        });
                    });
                },
            );

            page.section(
                Heading::new(crate::icons::REWIND, "Rewind").hint(REWIND_NOTE),
                |ui| {
                    Form::show(ui, "rewind", |form| {
                        form.row("How far back", |ui| {
                            s.dirty |=
                                GenericSelect::from_enum(&mut s.rewind, "enum_select_rewind")
                                    .default_value(RewindLength::default())
                                    .ui(ui)
                                    .changed();
                        });
                    });
                },
            );

            page.section(Heading::new(crate::icons::CAMERA, "Capture"), |ui| {
                Form::show(ui, "capture", |form| {
                    form.row("Screenshot size", |ui| {
                        s.dirty |= capture_scale(ui, &mut s.screenshot_scale, 1, video);
                    });
                    form.row("Recording size", |ui| {
                        s.dirty |= capture_scale(ui, &mut s.recording_scale, 2, video);
                    });

                    #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
                    {
                        let mut download = false;
                        form.row("Video codec", |ui| download = codec_row(ui));
                        if download && let Some(events) = &mut events {
                            events.download_codec();
                        }
                        form.hint(format!(
                            "Recording uses the {}",
                            crate::media::recording::CODEC_NOTICE
                        ));
                    }
                });
            });
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            if let Some((now, screen, pictures_off)) = discord {
                super::discord::section(page, s, now.as_ref(), screen, pictures_off);
            }
            if has_game {
                page.section(
                    Heading::new(crate::icons::BUG, "Game report")
                        .hint(crate::app::report::GAME_REPORT_NOTE),
                    |ui| {
                        Form::show(ui, "game_report", |form| {
                            form.custom(|ui| {
                                let button = egui::Button::new(format!(
                                    "{} {}",
                                    crate::app::report::SAVE_GAME_REPORT_ICON,
                                    crate::app::report::SAVE_GAME_REPORT
                                ))
                                .min_size(egui::vec2(ui.available_width(), 32.0));
                                if ui.add(button).clicked()
                                    && let Some(events) = events.as_mut()
                                {
                                    events.save_game_report();
                                }
                            });
                        })
                    },
                );
            }
            if page.phone() {
                return;
            }
            let where_kept = format!(
                "Citrine keeps settings in {}, beside your saves.",
                crate::storage::SettingsStore::default().location()
            );
            page.footer(
                format!("Citrine {}", env!("CARGO_PKG_VERSION")),
                Some(&where_kept),
            );
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

        Page::show(ui, |page| {
            page.plain(|ui| {
                Form::show(ui, "sound", |form| {
                    form.row("Volume", |ui| {
                        s.dirty |= ResetSlider::new(&mut s.volume, 0.0..=1.0)
                            .step_by(0.01)
                            .default_value(Settings::DEFAULT_VOLUME)
                            .ui(ui)
                            .changed();
                    });

                    s.dirty |= form.toggle("Mute", &mut s.muted);
                })
            });
            page.plain(|ui| {
                Form::show(ui, "sound-output", |form| {
                    form.row_with_hint(
                        "Latency",
                        "Lower reacts faster. Higher crackles less.",
                        |ui| {
                            let response = ResetSlider::new(
                                &mut s.audio_queue_ms,
                                MIN_LATENCY_MS..=MAX_LATENCY_MS,
                            )
                            .default_value(DEFAULT_LATENCY_MS)
                            .suffix(" ms")
                            .ui(ui);
                            if response.drag_stopped()
                                || (response.changed() && !response.dragged())
                            {
                                s.dirty = true;
                            }
                        },
                    );

                    form.row(SLOWDOWN_LABEL, |ui| {
                        s.dirty |=
                            GenericSelect::from_enum(&mut s.slow_sound, "enum_select_slow_sound")
                                .default_value(SlowSound::default())
                                .ui(ui)
                                .changed();
                    });

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
                                        crate::app::widgets::check::checkbox(ui, enabled, *name)
                                            .changed();
                                    if (index + 1) % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        s.dirty |= changed;
                    });

                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        form.row_with_hint(
                            "Output device",
                            "Changing this reopens the sound device.",
                            |ui| device_picker(ui, s),
                        );
                    }
                })
            });

            if let Some(audio) = self.audio {
                if audio.is_running() {
                    if !page.phone() {
                        page.note(format!(
                            "Playing at {} Hz on {}.",
                            audio.get_sample_rate(),
                            audio.device_name().unwrap_or("the default device")
                        ));
                    }
                } else {
                    page.note("Sound starts when you load a game or tap the screen.");
                }
            }
        });
    }

    fn controls(self, ui: &mut Ui) {
        if controls::show(ui, self.settings, self.capture)
            && let Some(events) = self.events
        {
            events.open_tab(Tab::ControllerDebug);
        }
    }

    fn style(self, ui: &mut Ui) {
        let palettes = self.palettes;
        let game = self.game;
        let editor = self.palette_editor;
        let s = self.settings;
        let gba_screen = self.console == ConsoleKind::Gba;
        let ds = self.console == ConsoleKind::Nds;
        let screen_match = self.screen_match;

        Page::show(ui, |page| {
            if palettes {
                palette::show(page, s, editor, game);
            }
            if ds {
                ds_screens::section(page, s);
            }
            page.section(Heading::new(crate::icons::MONITOR, "Screen"), |ui| {
                Form::show(ui, "style", |form| {
                    form.row("Scaling", |ui| {
                        s.dirty |= GenericSelect::from_enum(&mut s.scaling, "enum_select_scaling")
                            .default_value(ScalingMode::default())
                            .ui(ui)
                            .changed();
                    });

                    if s.scaling != ScalingMode::Integer {
                        s.dirty |= form.toggle_with_hint(
                            "Same-size pixels",
                            settings::SAME_SIZE_PIXELS_NOTE,
                            &mut s.same_size_pixels,
                        );
                    }

                    s.dirty |=
                        form.toggle_with_hint(settings::GRID_LABEL, settings::GRID_NOTE, &mut s.matrix);
                    if s.matrix && s.scaling != ScalingMode::Integer && !s.same_size_pixels {
                        form.hint(format!(
                            "The game shows at Integer scaling while the LCD grid is on. Turn on Same-size pixels to keep {} scaling.",
                            s.scaling
                        ));
                    }

                    if s.matrix {
                        form.row("Line darkness", |ui| {
                            s.dirty |= ResetSlider::new(&mut s.matrix_edge_darkness, 0.0..=1.0)
                                .step_by(0.01)
                                .default_value(Settings::DEFAULT_MATRIX_EDGE_DARKNESS)
                                .ui(ui)
                                .changed();
                        });

                        form.row("Crossing darkness", |ui| {
                            s.dirty |= ResetSlider::new(&mut s.matrix_corner_darkness, 0.0..=1.0)
                                .step_by(0.01)
                                .default_value(Settings::DEFAULT_MATRIX_CORNER_DARKNESS)
                                .ui(ui)
                                .changed();
                        });
                    }

                    s.dirty |= form.toggle_with_hint(
                        "Ghosting",
                        settings::GHOSTING_NOTE,
                        &mut s.ghosting,
                    );

                    if s.ghosting {
                        form.row("Strength", |ui| {
                            s.dirty |= ResetSlider::new(&mut s.ghosting_strength, 0.0..=1.0)
                                .step_by(0.01)
                                .default_value(Settings::DEFAULT_GHOSTING_STRENGTH)
                                .ui(ui)
                                .changed();
                        });
                    }

                    if gba_screen {
                        form.row_with_hint("GBA screen", GBA_SCREEN_NOTE, |ui| {
                            let mut screen = GbaScreen(s.color_correction);
                            s.dirty |= GenericSelect::new(&mut screen, GbaScreen::ALL, "gba_screen")
                                .default_value(GbaScreen(ColorCorrection::Screen))
                                .ui(ui)
                                .changed();
                            s.color_correction = screen.0;
                        });
                    } else if !ds {
                        form.row_with_hint("GBC screen", GBC_SCREEN_NOTE, |ui| {
                            s.dirty |= GenericSelect::from_enum(
                                &mut s.color_correction,
                                "enum_select_gbc_screen",
                            )
                            .default_value(ColorCorrection::Screen)
                            .ui(ui)
                            .changed();
                        });
                    }

                    s.dirty |= form.toggle_with_hint(
                        settings::MATCH_SCREEN_LABEL,
                        settings::MATCH_SCREEN_NOTE,
                        &mut s.match_screen,
                    );
                    if let Some(line) = settings::match_screen_line(screen_match) {
                        form.hint(line);
                    }
                    s.dirty |= form.toggle("Show HUDs", &mut s.hud.shown);
                    form.row_with_hint("HUD card", crate::app::hud::editor::CARD_HINT, |ui| {
                        let mut card = (s.hud.opacity() * 100.0).round();
                        let changed = ui
                            .add(
                                ResetSlider::new(&mut card, 0.0..=100.0)
                                    .step_by(5.0)
                                    .suffix("%")
                                    .default_value((crate::app::hud::DEFAULT_OPACITY * 100.0).round()),
                            )
                            .changed();
                        if changed {
                            s.hud.opacity = card / 100.0;
                            s.dirty = true;
                        }
                    });
                });
            });
        });
    }

    fn online_page(self, ui: &mut Ui) {
        let mut spare = online::JoinForm::default();
        match self.online {
            Some((client, form)) => online::show(ui, self.settings, Some(client), form),
            None => online::show(ui, self.settings, None, &mut spare),
        }
    }

    fn your_data_page(self, ui: &mut Ui) {
        match self.your_data {
            Some(state) => your_data::show(ui, state),
            None => your_data::show(ui, &mut your_data::YourDataUi::default()),
        }
    }

    fn developer(self, ui: &mut Ui) {
        let s = self.settings;

        Page::show(ui, |page| {
            page.boxed(None, |ui| {
                Form::show(ui, "developer", |form| {
                    s.dirty |= form.toggle("Randomized power-on RAM", &mut s.randomized_ram);
                });
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

const LAYOUT_HINT: &str = "Automatic uses the touch layout on small screens.";

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

#[cfg(not(target_arch = "wasm32"))]
fn device_picker(ui: &mut Ui, s: &mut Settings) {
    let selected = s
        .audio_device
        .clone()
        .unwrap_or_else(|| "System default".to_string());
    let spacing = ui.spacing();
    let chrome = 2.0 * spacing.button_padding.x + spacing.icon_width + spacing.icon_spacing;
    let default = egui::WidgetText::from("System default")
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        )
        .size()
        .x;
    super::form::need(ui, default + chrome);

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
    use crate::app::ui_state::ds_screens::Arrangement;

    fn render(settings: Settings) -> Vec<(egui::Rect, String)> {
        render_on(settings, ConsoleKind::Gb)
    }

    fn render_on(settings: Settings, console: ConsoleKind) -> Vec<(egui::Rect, String)> {
        render_shapes(settings, console, false)
    }

    fn render_shapes(
        mut settings: Settings,
        console: ConsoleKind,
        ds: bool,
    ) -> Vec<(egui::Rect, String)> {
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
                let content = SettingsContent::new(
                    &mut settings,
                    &mut capture,
                    &mut editor,
                    None,
                    None,
                    None,
                )
                .console(console);
                #[cfg(feature = "nds")]
                let content = if ds {
                    content.ds(super::super::ds::DsState {
                        account: None,
                        files: [None; 3],
                    })
                } else {
                    content
                };
                #[cfg(not(feature = "nds"))]
                let _ = ds;
                content.ui(ui);
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

    #[cfg(feature = "nds")]
    #[test]
    fn clear_jit_cache_sits_under_the_jit_switch_and_nowhere_else() {
        if !citrine_gb::nds::machine::JIT_AVAILABLE {
            return;
        }
        let texts = render_shapes(
            Settings {
                current_tab: SettingsTab::General,
                ..Settings::default()
            },
            ConsoleKind::Nds,
            true,
        );
        let top = |wanted: &str| {
            texts
                .iter()
                .find(|(_, text)| text == wanted)
                .map(|(rect, _)| rect.top())
                .unwrap_or_else(|| panic!("{wanted} missing: {texts:?}"))
        };
        let clear: Vec<_> = texts
            .iter()
            .filter(|(_, text)| text.ends_with(settings::CLEAR_JIT_LABEL))
            .collect();
        assert_eq!(clear.len(), 1, "{clear:?}");
        let clear = clear[0].0.top();
        assert!(top(settings::JIT_LABEL) < clear);
        assert!(clear < top("Dev tools"));
        assert!(clear < top("Nintendo DS"));
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
        let layout = find("LCD grid");
        let hint = find("The game shows at Integer scaling");
        let next = shapes
            .iter()
            .map(|(rect, _)| *rect)
            .filter(|rect| rect.top() > layout.bottom() && rect.left() <= layout.left() + 1.0)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("a row after LCD grid");
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
            "Draws thin lines between",
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
                !text.contains("Anyone with your relay ID"),
                "Online shows {text:?} on the page"
            );
        }
    }

    #[test]
    fn the_app_calls_it_relay_id_everywhere() {
        let old_names = [format!("sign-in {}", "key"), format!("sign in {}", "key")];
        let says_old_name = |text: &str| {
            let text = text.to_lowercase();
            old_names.iter().any(|old| text.contains(old.as_str()))
        };
        let mut pages: Vec<_> = SettingsTab::iter_with_dev_mode(true)
            .flat_map(texts_of)
            .collect();
        pages.extend(render(joined_online()).into_iter().map(|(_, text)| text));
        for text in pages {
            assert!(!says_old_name(&text), "Settings shows {text:?}");
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut folders = vec![
            crates.join("citrine-gb-app/src"),
            crates.join("citrine-gb-server/src"),
            crates.join("citrine-gb-net/src"),
        ];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    assert!(
                        !says_old_name(&source),
                        "{} says the old name",
                        path.display()
                    );
                }
            }
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
        let label = height("LCD grid");
        let hint = height("The game shows at Integer scaling");
        assert!(hint >= label - 0.5, "hint {hint} pt high, row {label} pt");
    }

    fn laid_out(settings: Settings, width: f32) -> super::super::page::tests::Laid {
        laid_out_on(settings, width, ConsoleKind::Gb)
    }

    fn laid_out_on(
        mut settings: Settings,
        width: f32,
        console: ConsoleKind,
    ) -> super::super::page::tests::Laid {
        use super::super::page::tests::{after_pass, quiet};
        let ctx = egui::Context::default();
        quiet(&ctx);
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut laid = Default::default();
        for _ in 0..3 {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                        None,
                    )
                    .console(console)
                    .ui(ui);
                });
            });
            laid = after_pass(&ctx);
        }
        laid
    }

    #[test]
    fn every_desktop_page_draws_its_rows_with_the_shared_widgets() {
        use super::super::page::tests::assert_unified;
        let page = |tab: Option<SettingsTab>| match tab {
            Some(tab) => Settings {
                current_tab: tab,
                dev_mode: true,
                matrix: true,
                ghosting: true,
                ..Settings::default()
            },
            None => joined_online(),
        };
        let tabs = SettingsTab::iter_with_dev_mode(true)
            .map(Some)
            .chain([None]);
        for tab in tabs {
            for width in [217.0, 800.0] {
                let (marks, controls) = laid_out(page(tab), width);
                let name = match tab {
                    Some(tab) => format!("{tab:?} at {width}"),
                    None => format!("Online signed in at {width}"),
                };
                assert_unified(&name, &marks, &controls);
            }
        }
        for width in [217.0, 800.0] {
            let (marks, controls) = laid_out_on(ds_style(false), width, ConsoleKind::Nds);
            assert_unified(&format!("DS screens at {width}"), &marks, &controls);
        }
    }

    fn ds_style(separate_tabs: bool) -> Settings {
        let mut settings = Settings {
            current_tab: SettingsTab::Style,
            ..Settings::default()
        };
        settings.ds_screens.desktop.arrangement = Arrangement::BigAndSmall;
        settings.ds_screens.separate_tabs = separate_tabs;
        settings
    }

    #[test]
    fn the_style_page_shows_ds_screen_rows_only_for_a_ds_game() {
        let texts = |settings: Settings, console| -> Vec<String> {
            render_on(settings, console)
                .into_iter()
                .map(|(_, text)| text)
                .collect()
        };
        let gb = texts(ds_style(false), ConsoleKind::Gb);
        assert!(!gb.iter().any(|text| text == "DS screens"), "{gb:?}");
        assert!(gb.iter().any(|text| text == "GBC screen"), "{gb:?}");
        let ds = texts(ds_style(false), ConsoleKind::Nds);
        for gone in ["GBC screen", "GBA screen"] {
            assert!(!ds.iter().any(|text| text == gone), "{gone}: {ds:?}");
        }
        for row in [
            "DS screens",
            "Layout",
            "Swap screens",
            "Only the top screen",
            "Separate tabs",
            "Small screen corner",
            "Small screen size",
            "Gap, in DS pixels",
            "Rotation",
        ] {
            assert!(ds.iter().any(|text| text == row), "{row}: {ds:?}");
        }
        let tabs = texts(ds_style(true), ConsoleKind::Nds);
        for gone in [
            "Layout",
            "Swap screens",
            "Small screen corner",
            "Gap, in DS pixels",
        ] {
            assert!(!tabs.iter().any(|text| text == gone), "{gone}: {tabs:?}");
        }
        assert!(tabs.iter().any(|text| text == "Rotation"), "{tabs:?}");
    }

    fn cut_short(mut settings: Settings, width: f32) -> Vec<String> {
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut cut = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                        None,
                    )
                    .ui(ui);
                });
            });
            cut = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) if text.galley.elided => {
                        Some(text.galley.text().trim().to_owned())
                    }
                    _ => None,
                })
                .collect();
        }
        cut
    }

    fn desktop_pages() -> impl Iterator<Item = (String, Settings)> {
        SettingsTab::iter_with_dev_mode(true)
            .map(|tab| {
                (
                    format!("{tab:?}"),
                    Settings {
                        current_tab: tab,
                        dev_mode: true,
                        matrix: true,
                        ghosting: true,
                        ..Settings::default()
                    },
                )
            })
            .chain([("Online signed in".to_owned(), joined_online())])
    }

    fn stacked_rows(marks: &super::super::page::tests::Laid) -> usize {
        let (marks, controls) = marks;
        marks
            .iter()
            .filter(|(kind, _)| *kind == super::super::page::Mark::Row)
            .filter(|(_, row)| {
                controls.iter().any(|control| {
                    row.height() < 60.0
                        && row.contains(control.center())
                        && control.left() < row.left() + 20.0
                        && control.top() > row.top() + 12.0
                })
            })
            .count()
    }

    const LONG_VALUES: [&str; 3] = ["cgb1-", "Integer (sharpest)", "Beside the screen"];

    #[test]
    fn the_default_dock_keeps_each_label_beside_its_value() {
        for ((name, settings), (_, again)) in desktop_pages().zip(desktop_pages()) {
            let laid = laid_out(settings, 217.0);
            assert_eq!(stacked_rows(&laid), 0, "{name} stacks a row at 217 pt");
            let cut = cut_short(again, 217.0);
            assert!(
                cut.iter()
                    .all(|text| LONG_VALUES.iter().any(|long| text.starts_with(long))),
                "{name} cuts short at 217 pt: {cut:?}"
            );
        }
    }

    #[test]
    fn a_slider_with_room_keeps_its_label_on_one_line() {
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        let mut settings = Settings::default();
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(217.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut lines = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                        None,
                    )
                    .ui(ui);
                });
            });
            lines = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().starts_with("Screenshot") => {
                        Some(text.galley.rows.len())
                    }
                    _ => None,
                })
                .collect();
        }
        assert_eq!(lines, [1]);
    }

    #[test]
    fn a_really_narrow_dock_stacks_its_rows() {
        let width = super::super::page::STACK_BELOW - 10.0;
        let settings = Settings {
            current_tab: SettingsTab::General,
            ..Settings::default()
        };
        let laid = laid_out(settings, width);
        assert!(stacked_rows(&laid) > 3, "General at {width} pt");
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

    #[test]
    fn general_settles_without_discards_in_the_desktop_dock() {
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
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut reasons = Vec::new();
        for _ in 0..5 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        let content = SettingsContent::new(
                            &mut settings,
                            &mut capture,
                            &mut editor,
                            None,
                            None,
                            None,
                        );
                        #[cfg(feature = "nds")]
                        let content = content.ds(super::super::ds::DsState {
                            account: None,
                            files: [None; 3],
                        });
                        content.ui(ui);
                    });
                });
            });
            reasons = output.platform_output.request_discard_reasons;
        }
        assert!(reasons.is_empty(), "{reasons:?}");
    }
}
