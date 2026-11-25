use crate::app::events::AppEventQueue;
use crate::app::report::{GAME_REPORT_NOTE, SAVE_GAME_REPORT, SAVE_GAME_REPORT_ICON};
use crate::app::tabs::Tab;
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::settings::{
    ConsoleChoice, Settings, console_name, match_screen_line, plays_on_both,
};
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::ui_theme::own_value;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::settings::cartridge::{self, CartridgeState};
use crate::app::widgets::settings::content::MAX_COLUMN_WIDTH;
use crate::app::widgets::settings::form::{self, Form};
use crate::app::widgets::settings::page::{Heading, Page};
use crate::app::widgets::settings::scope;
use crate::emulator::Emulator;
use crate::emulator::gimmicks::camera::Feed;
use crate::emulator::gimmicks::{CameraSource, Cart, Motion};
use crate::emulator::load::ScreenMatch;
use crate::icons;
use crate::senses::Senses;
use citrine_gb::gb::GbModel;
use citrine_gb::rom::header::RomCgbMode;
use egui::{Align, Label, Layout, Response, RichText, ScrollArea, Ui, Widget};

pub(crate) const TITLE: &str = "This game";
pub(crate) const NO_GAME: &str = "Load a game to see its settings";
pub(crate) const SESSION_GAME: &str = "This game's settings open when you play on your own";
pub(crate) const PLAY_ON: &str = "Console";
pub(crate) const GAME_CLOCK: &str = "Game clock";
pub(crate) const PRINTER: &str = "Game Boy Printer";
pub(crate) const CHEATS: &str = "Cheats";

#[derive(Clone)]
pub(crate) struct ThisGame {
    pub rom_key: String,
    pub title: String,
    pub gb: Option<GbGame>,
    pub session: bool,
    pub cart: Cart,
    pub motion: Motion,
    pub printer_free: bool,
    pub live_camera: bool,
    pub camera_error: Option<String>,
    pub screen_match: Option<ScreenMatch>,
}

#[derive(Clone)]
pub(crate) struct GbGame {
    pub model: GbModel,
    pub cgb_mode: RomCgbMode,
}

impl ThisGame {
    pub(crate) fn of(emulator: &Emulator, session: bool, senses: &Senses) -> Option<Self> {
        if !emulator.has_game() || emulator.shows_session_frame() {
            return None;
        }
        let rom_key = emulator.rom_key()?.to_owned();
        let gb = emulator.as_gb().map(|gb| GbGame {
            model: emulator.console(),
            cgb_mode: gb.cartridge.header.cgb_mode,
        });
        Some(Self {
            rom_key,
            title: emulator.game_name().to_owned(),
            gb,
            session,
            cart: emulator.cart(),
            motion: emulator.gimmicks.motion,
            printer_free: emulator.printer_free(),
            live_camera: senses.has_live_camera(),
            camera_error: senses.camera_error().map(str::to_owned),
            screen_match: emulator.screen_match(),
        })
    }

    pub(crate) fn empty_text(emulator: &Emulator) -> &'static str {
        if emulator.shows_session_frame() {
            SESSION_GAME
        } else {
            NO_GAME
        }
    }

    pub(crate) fn heading(&self) -> &str {
        if self.title.is_empty() {
            TITLE
        } else {
            &self.title
        }
    }

    pub(crate) fn play_on(&self) -> Option<GbModel> {
        self.gb
            .as_ref()
            .filter(|gb| !self.session && plays_on_both(gb.cgb_mode))
            .map(|gb| gb.model)
    }

    pub(crate) fn starts_on(&self, settings: &Settings) -> Option<GbModel> {
        let gb = self.gb.as_ref()?;
        self.play_on()?;
        let next = settings.console_for(&self.rom_key, gb.cgb_mode);
        (next != gb.model).then_some(next)
    }

    pub(crate) fn camera_view(&self, settings: &Settings) -> bool {
        self.cart.camera
            && self.live_camera
            && self.camera_error.is_none()
            && settings.gimmicks.camera == CameraSource::Camera
    }

    pub(crate) fn match_line(&self) -> Option<String> {
        match_screen_line(self.screen_match)
    }

    pub(crate) fn cheats_value(&self, settings: &Settings) -> String {
        let saved = settings.cheats.list(&self.rom_key);
        match saved.iter().filter(|cheat| cheat.on).count() {
            0 if saved.is_empty() => "None".to_owned(),
            0 => "Off".to_owned(),
            on => format!("{on} on"),
        }
    }

    pub(crate) fn cartridge(&self) -> CartridgeState<'_> {
        CartridgeState {
            rom_key: &self.rom_key,
            cart: self.cart,
            motion: self.motion,
            printer_free: self.printer_free,
            touch: false,
        }
    }
}

pub(crate) struct ThisGameSheet<'a> {
    game: Option<&'a ThisGame>,
    settings: &'a mut Settings,
    events: Option<&'a mut AppEventQueue>,
    feed: Option<(&'a Feed, u64)>,
    empty: &'static str,
}

impl<'a> ThisGameSheet<'a> {
    pub(crate) fn new(
        game: Option<&'a ThisGame>,
        settings: &'a mut Settings,
        events: Option<&'a mut AppEventQueue>,
    ) -> Self {
        Self {
            game,
            settings,
            events,
            feed: None,
            empty: NO_GAME,
        }
    }

    pub(crate) fn empty(mut self, text: &'static str) -> Self {
        self.empty = text;
        self
    }

    pub(crate) fn feed(mut self, feed: Option<(&'a Feed, u64)>) -> Self {
        self.feed = feed;
        self
    }
}

impl Widget for ThisGameSheet<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Some(game) = self.game else {
            return ui
                .vertical_centered(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new(self.empty).weak());
                })
                .response;
        };
        let (settings, events, feed) = (self.settings, self.events, self.feed);
        ScrollArea::vertical()
            .id_salt("this_game_scroll")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(ui.available_width().min(MAX_COLUMN_WIDTH));
                    form::shared_labels(ui, |ui| {
                        Page::show(ui, |page| rows(page, game, settings, events, feed))
                    });
                })
                .response
            })
            .inner
    }
}

fn rows(
    page: &mut Page,
    game: &ThisGame,
    s: &mut Settings,
    mut events: Option<&mut AppEventQueue>,
    feed: Option<(&Feed, u64)>,
) {
    let state = game.cartridge();
    let mut cheats = false;
    let mut report = false;
    page.section(Heading::new(icons::GEAR_FINE, game.heading()), |ui| {
        Form::show(ui, "this_game", |form| {
            if let Some(model) = game.play_on() {
                form.row(PLAY_ON, |ui| {
                    let mut choice = ConsoleChoice::of(model);
                    let default = ConsoleChoice::of(s.global_console().resolve(GbModel::Cgb));
                    let changed =
                        GenericSelect::from_enum(&mut choice, "enum_select_this_game_console")
                            .filter(|choice| choice != ConsoleChoice::Auto)
                            .default_value(default)
                            .ui(ui)
                            .changed();
                    if changed && let Some(events) = events.as_deref_mut() {
                        events.play_on(choice.resolve(GbModel::Cgb));
                    }
                });
                if let Some(next) = game.starts_on(s) {
                    form.hint(format!("It starts on {} next time.", console_name(next)));
                }
            }
            if game.cart.clock {
                cartridge::clock_row(form, s, &state, GAME_CLOCK);
            }
            if game.cart.motion() {
                cartridge::motion_row(form, &state, &mut events);
            }
            if game.camera_view(s) {
                form.full(|ui| cartridge::camera_frame(ui, s, feed));
            }
            if game.cart.printer {
                cartridge::printer_row(form, s, &state, PRINTER);
            }
            form.row(CHEATS, |ui| {
                let value = format!("{}  {}", game.cheats_value(s), icons::CARET_RIGHT);
                cheats = ui.button(value).clicked();
            });
            form.buttons(|ui| {
                let label = format!("{SAVE_GAME_REPORT_ICON} {SAVE_GAME_REPORT}");
                report = ui.button(label).on_hover_text(GAME_REPORT_NOTE).clicked();
            });
        });
    });
    if let Some(line) = game.match_line() {
        page.note(line);
    }
    let all = changed(page, game, s);
    if let Some(events) = events {
        if cheats {
            events.open_tab(Tab::Cheats);
        }
        if report {
            events.save_game_report();
        }
        if all {
            s.set_scope(Scope::ThisGame);
            events.open_settings(scope::first_tab(s, Surface::Desktop));
        }
    }
}

fn changed(page: &mut Page, game: &ThisGame, s: &mut Settings) -> bool {
    let console_row = game.play_on().is_some();
    let rows: Vec<(RowId, String)> = scope::changed_rows(s, Surface::Desktop, s.dev_mode)
        .into_iter()
        .filter(|row| !(console_row && *row == RowId::RunGamesOn))
        .filter_map(|row| {
            let value = scope::value_text(row, s, Surface::Desktop)?;
            Some((row, value))
        })
        .collect();
    let mut dropped = None;
    let mut all = false;
    page.section(
        Heading::new(scope::MARK, scope::changed_heading(&rows)),
        |ui| {
            Form::show(ui, "this_game_changed", |form| {
                for (row, value) in &rows {
                    if changed_row(form, row.label(Surface::Desktop), value) {
                        dropped = Some(*row);
                    }
                }
            });
            if !rows.is_empty() {
                ui.add_space(super::page::SECTION_GAP);
            }
            Form::show(ui, "this_game_all", |form| {
                form.buttons(|ui| {
                    all = ui
                        .button(format!("{} {}", icons::GEAR, scope::ALL_FOR_GAME))
                        .clicked();
                });
            })
        },
    );
    if let Some(row) = dropped {
        s.drop_row(row, Surface::Desktop);
    }
    all
}

fn changed_row(form: &mut Form, label: &str, value: &str) -> bool {
    form.row(label, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clicked = ui
                .add(egui::Button::new(icons::X).frame(false))
                .on_hover_text(scope::USE_ALL)
                .clicked();
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(Label::new(RichText::new(value).color(own_value(ui.visuals()))).truncate());
            });
            clicked
        })
        .inner
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Citrine;
    use crate::app::report::SAVE_GAME_REPORT;
    use crate::app::tests::{load_game, phone_app};

    fn facts(app: &Citrine, session: bool) -> Option<ThisGame> {
        ThisGame::of(&app.emulator, session, &app.senses)
    }

    fn gb_rom(cgb_flag: u8) -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0143] = cgb_flag;
        rom
    }

    fn sheet_texts(game: Option<&ThisGame>) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 2000.0),
            )),
            ..Default::default()
        };
        let mut texts = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add(ThisGameSheet::new(game, &mut settings, None));
                });
            });
            texts = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
        }
        texts
    }

    fn text_rect(game: &ThisGame, settings: &mut Settings, wanted: &str) -> egui::Rect {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 2000.0),
            )),
            ..Default::default()
        };
        let mut found = None;
        for _ in 0..4 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add(ThisGameSheet::new(Some(game), settings, None));
                });
            });
            found = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == wanted => {
                        Some(text.visual_bounding_rect())
                    }
                    _ => None,
                });
        }
        found.unwrap_or_else(|| panic!("no {wanted}"))
    }

    #[test]
    fn changed_values_start_at_the_settings_pages_value_column() {
        let mut app = phone_app("this-game-changed-column");
        load_game(&mut app, &gb_rom(0x00), None);
        let game = facts(&app, false).expect("a game");
        let key = app.emulator.rom_key().expect("key").to_owned();
        let mut settings = Settings::default();
        settings.play_game(Some(&key));
        settings.ghosting = true;
        settings.set_own(&key, "ghosting_strength", serde_json::json!(0.8));
        let label = text_rect(&game, &mut settings, "Ghosting strength");
        let value = text_rect(&game, &mut settings, "80%");
        assert!(
            value.left() - label.left() >= form::DESKTOP_LABELS,
            "value {} pt after the label's start, label {} pt wide",
            value.left() - label.left(),
            label.width()
        );
    }

    fn shows(texts: &[String], wanted: &str) -> bool {
        texts.iter().any(|text| text.contains(wanted))
    }

    #[test]
    fn this_game_offers_play_on_only_for_games_that_run_on_both() {
        let mut app = phone_app("this-game-play-on");
        for (flag, offered) in [(0x00, false), (0x80, true), (0xC0, false)] {
            load_game(&mut app, &gb_rom(flag), None);
            let game = facts(&app, false).expect("a game");
            assert_eq!(game.play_on().is_some(), offered, "cgb flag {flag:#04x}");
            assert_eq!(
                shows(&sheet_texts(Some(&game)), PLAY_ON),
                offered,
                "cgb flag {flag:#04x}"
            );
        }
        let in_session = facts(&app, true).expect("a game");
        assert!(in_session.play_on().is_none());
        assert!(!shows(&sheet_texts(Some(&in_session)), PLAY_ON));
    }

    #[test]
    fn this_game_offers_the_game_report_for_any_loaded_game() {
        let mut app = phone_app("this-game-report");
        assert!(facts(&app, false).is_none());
        let empty = sheet_texts(None);
        assert!(shows(&empty, NO_GAME));
        assert!(!shows(&empty, SAVE_GAME_REPORT));
        for flag in [0x00, 0x80, 0xC0] {
            load_game(&mut app, &gb_rom(flag), None);
            let game = facts(&app, false).expect("a game");
            assert!(
                shows(&sheet_texts(Some(&game)), SAVE_GAME_REPORT),
                "cgb flag {flag:#04x}"
            );
            let in_session = facts(&app, true).expect("a game");
            assert!(shows(&sheet_texts(Some(&in_session)), SAVE_GAME_REPORT));
        }
        #[cfg(feature = "gba")]
        {
            let mut rom = vec![0u8; 0x400];
            rom[..4].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
            app.emulator.put_gba(&rom, "this-game-gba");
            let game = facts(&app, false).expect("a gba game");
            assert!(game.gb.is_none());
            assert!(shows(&sheet_texts(Some(&game)), SAVE_GAME_REPORT));
        }
    }

    #[cfg(all(feature = "gba", feature = "nds"))]
    fn take_seat(app: &mut Citrine, pair: &mut crate::play::tests::Pair, host: bool) {
        let side = if host {
            &mut pair.host
        } else {
            &mut pair.guest
        };
        std::mem::swap(&mut *app.play, &mut side.play);
        std::mem::swap(&mut app.online, &mut side.online);
    }

    #[cfg(all(feature = "gba", feature = "nds"))]
    #[test]
    fn game_settings_wait_for_solo_play_in_a_ds_session_after_a_gba_game() {
        use crate::play::MyGame;
        use crate::play::tests::{Pair, empty, nickname, one_ds_pair_at_a_time};
        use citrine_gb::console::ConsoleKind;
        let _one = one_ds_pair_at_a_time();
        for (host, solo) in [(true, true), (false, true), (false, false)] {
            let mut app = phone_app(&format!("this-game-ds-after-gba-{host}-{solo}"));
            app.ui.settings.check_updates = false;
            if solo {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("fixtures/gba/save-sram.gba");
                app.handle_load_rom_data(std::fs::read(path).expect("the GBA fixture"));
                app.emulator.running = true;
                assert!(facts(&app, false).is_some());
            }
            let mut pair = Pair::new(true, 0, 0, empty());
            let session = pair.invite(nickname());
            pair.accept(session, MyGame::Own(nickname()));
            pair.play_frames(60);
            pair.hand_over_at_real_time();
            take_seat(&mut app, &mut pair, host);
            let ctx = egui::Context::default();
            let mut frame = eframe::Frame::_new_kittest();
            for step in 0..60 {
                if step >= 6 && app.emulator.played_kind() == ConsoleKind::Nds {
                    break;
                }
                if host {
                    pair.run_guest_alone(1);
                } else {
                    pair.run_host_alone(1);
                }
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(412.0, 900.0),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    eframe::App::update(&mut app, ctx, &mut frame);
                });
            }
            let kind = app.emulator.played_kind();
            let game = facts(&app, app.play.session().is_some());
            let empty_text = ThisGame::empty_text(&app.emulator);
            take_seat(&mut app, &mut pair, host);
            let seat = format!("host {host}, solo game {solo}");
            assert_eq!(kind, ConsoleKind::Nds, "{seat}");
            assert!(game.is_none(), "{seat}: shows the solo game's settings");
            assert_eq!(empty_text, SESSION_GAME, "{seat}");
        }
    }
}
