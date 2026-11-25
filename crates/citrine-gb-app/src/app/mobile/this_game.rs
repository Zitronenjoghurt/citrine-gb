use super::kit::{self, Row, SwitchRow};
use super::settings_pages::Sheet;
use super::settings_pages::cartridge::motion_row;
use crate::app::tabs::Tab;
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::settings::{ConsoleChoice, Settings, console_name};
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::ui_theme::own_value;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::app::widgets::settings::scope;
use crate::app::widgets::settings::this_game::{
    CHEATS, GAME_CLOCK, PLAY_ON, PRINTER, TITLE, ThisGame,
};
use crate::app::{Citrine, report};
use crate::emulator::gimmicks::clock::offset_label;
use crate::emulator::gimmicks::settings::{PRINTER_HINT, PRINTER_LINKED};
use crate::icons;
use egui::epaint::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, FontId, Rect, RichText, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};

const CAMERA_VIEW: &str = "Camera view";

#[derive(Default)]
struct Picked {
    sheet: Option<Sheet>,
    level: bool,
    cheats: bool,
    report: bool,
    find: Option<RowId>,
    drop: Option<RowId>,
    all: bool,
}

impl Citrine {
    fn this_game(&self) -> Option<ThisGame> {
        ThisGame::of(&self.emulator, self.play.session().is_some(), &self.senses)
    }

    pub(super) fn this_game_title(&self) -> String {
        self.this_game()
            .map_or_else(|| TITLE.to_owned(), |game| game.heading().to_owned())
    }

    pub(super) fn this_game_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let Some(game) = self.this_game() else {
            let empty = ThisGame::empty_text(&self.emulator);
            kit::place(ui, "this_game_page", |ui| {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(empty).weak()));
            });
            return;
        };
        let s = &mut self.ui.settings;
        let picked = kit::place(ui, "this_game_page", |ui| rows(ui, &game, s));
        if let Some(row) = picked.drop {
            self.ui.settings.drop_row(row, Surface::Phone);
        }
        if let Some(row) = picked.find {
            self.ui.settings.set_scope(Scope::ThisGame);
            self.find_setting(row, true);
        }
        if picked.all {
            self.ui.settings.set_scope(Scope::ThisGame);
            let tab = scope::first_tab(&self.ui.settings, Surface::Phone);
            self.open_settings(tab, true);
        }
        if picked.level {
            self.centre_tilt();
        }
        if picked.cheats {
            self.mobile.open(Tab::Cheats);
        }
        if picked.report {
            self.save_game_report();
        }
        self.open_sheet(picked.sheet);
        self.settings_sheet(&ctx);
    }
}

fn rows(ui: &mut Ui, game: &ThisGame, s: &mut Settings) -> Picked {
    let mut picked = Picked::default();
    let mut page = Page::begin(ui);
    page.section(Heading::new(icons::GEAR_FINE, scope::ONLY_HERE), |ui| {
        kit::list(ui, |list| {
            if let Some(model) = game.play_on() {
                let detail = game
                    .starts_on(s)
                    .map(|next| format!("Starts on {} next time", console_name(next)))
                    .unwrap_or_default();
                if list
                    .row(
                        Row::new(PLAY_ON)
                            .value(ConsoleChoice::of(model).to_string())
                            .detail(&detail),
                    )
                    .clicked()
                {
                    picked.sheet = Some(Sheet::GameConsole);
                }
            }
            if game.cart.clock
                && list
                    .row(
                        Row::new(GAME_CLOCK)
                            .value(offset_label(s.gimmicks.clock_offset(&game.rom_key))),
                    )
                    .clicked()
            {
                picked.sheet = Some(Sheet::Clock);
            }
            if game.cart.motion() {
                picked.level = motion_row(list, game.cart, game.motion);
            }
            if game.camera_view(s) && list.row(Row::new(CAMERA_VIEW)).clicked() {
                picked.sheet = Some(Sheet::Camera);
            }
            if game.cart.printer {
                let mut on = s.gimmicks.printer(&game.rom_key);
                let detail = (!game.printer_free).then_some(PRINTER_LINKED);
                if list.switch(
                    SwitchRow::new(PRINTER, &mut on)
                        .info(PRINTER_HINT)
                        .detail(detail)
                        .enabled(game.printer_free),
                ) {
                    s.gimmicks.set_printer(&game.rom_key, on);
                    s.dirty = true;
                }
            }
            picked.cheats = list
                .row(Row::new(CHEATS).value(game.cheats_value(s)))
                .clicked();
            picked.report = list
                .row(
                    Row::new(report::SAVE_GAME_REPORT)
                        .icon(report::SAVE_GAME_REPORT_ICON)
                        .no_chevron(),
                )
                .clicked();
        })
    });
    if let Some(line) = game.match_line() {
        page.note(line);
    }
    changed(&mut page, game, s, &mut picked);
    page.end();
    picked
}

fn changed(page: &mut Page, game: &ThisGame, s: &Settings, picked: &mut Picked) {
    let console_row = game.play_on().is_some();
    let rows: Vec<(RowId, String)> = scope::changed_rows(s, Surface::Phone, s.dev_mode)
        .into_iter()
        .filter(|row| !(console_row && *row == RowId::RunGamesOn))
        .filter_map(|row| Some((row, scope::value_text(row, s, Surface::Phone)?)))
        .collect();
    page.section(
        Heading::new(scope::MARK, scope::changed_heading(&rows)),
        |ui| {
            kit::list(ui, |list| {
                for (row, value) in &rows {
                    match changed_row(list, row.label(Surface::Phone), value) {
                        Tap::Open => picked.find = Some(*row),
                        Tap::Drop => picked.drop = Some(*row),
                        Tap::None => {}
                    }
                }
                picked.all = list
                    .row(Row::new(scope::ALL_FOR_GAME).icon(icons::GEAR))
                    .clicked();
            })
        },
    );
}

enum Tap {
    None,
    Open,
    Drop,
}

fn changed_row(list: &mut kit::List<'_>, label: &str, value: &str) -> Tap {
    list.custom(|ui| {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), kit::ROW_HEIGHT), Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
        let visuals = ui.visuals().clone();
        let cy = rect.center().y;
        ui.painter().text(
            pos2(rect.left() + 12.0, cy),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(16.0),
            visuals.strong_text_color(),
        );
        let x = Rect::from_center_size(
            pos2(rect.right() - 12.0 - kit::ROW_HEIGHT / 4.0, cy),
            vec2(kit::ROW_HEIGHT / 2.0 + 12.0, kit::ROW_HEIGHT),
        );
        ui.painter().text(
            x.center(),
            Align2::CENTER_CENTER,
            icons::X,
            FontId::proportional(17.0),
            kit::value_color(&visuals),
        );
        let drop = ui
            .interact(x, ui.id().with(("changed_drop", label)), Sense::click())
            .on_hover_text(scope::USE_ALL);
        drop.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, scope::USE_ALL));
        let shown = one_line_job(
            value,
            (x.left() - 14.0 - rect.center().x).max(0.0),
            &visuals,
        );
        let galley = ui.painter().layout_job(shown);
        ui.painter().galley(
            pos2(
                x.left() - 14.0 - galley.size().x,
                cy - galley.size().y / 2.0,
            ),
            galley,
            own_value(&visuals),
        );
        if drop.clicked() {
            Tap::Drop
        } else if response.clicked() {
            Tap::Open
        } else {
            Tap::None
        }
    })
}

fn one_line_job(text: &str, width: f32, visuals: &egui::Visuals) -> LayoutJob {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat::simple(FontId::proportional(15.0), own_value(visuals)),
    );
    job.wrap = TextWrapping::truncate_at_width(width);
    job
}
