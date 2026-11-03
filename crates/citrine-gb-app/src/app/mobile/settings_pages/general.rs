use super::{Sheet, pick, pick_from, settled, slide};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::Amount;
use crate::app::ui_state::settings::{self, ConsoleChoice, LayoutMode, plays_on_both};
use crate::autosave::{DEFAULT_KEEP, MAX_KEEP};
use crate::media::recording::{MAX_SCALE, SOURCE_HEIGHT, SOURCE_WIDTH};
use citrine_gb::gb::GbModel;
use egui::{Context, Ui};

const LAYOUT_NOTE: &str = "Automatic uses the touch layout on small screens.";
const AUTOSAVE_NOTE: &str = "Minutes of play between autosaves. Paused time doesn't count. Citrine also autosaves when you leave or switch games, and after a crash.";

struct Game {
    rom_key: String,
    both: bool,
    model: GbModel,
    mode: citrine_gb::rom::header::RomCgbMode,
}

fn scale_label(scale: usize) -> String {
    format!(
        "{scale}x, {} × {}",
        SOURCE_WIDTH * scale,
        SOURCE_HEIGHT * scale
    )
}

fn count(value: f64) -> String {
    format!("{value:.0}")
}

impl Citrine {
    fn general_game(&self) -> Option<Game> {
        let rom_key = self.emulator.rom_key()?.to_owned();
        let mode = self.emulator.gb.cartridge.header.cgb_mode;
        Some(Game {
            rom_key,
            both: plays_on_both(mode),
            model: self.emulator.console(),
            mode,
        })
    }

    pub(super) fn general_page(&mut self, ui: &mut Ui) {
        let game = self.general_game();
        let narrow = LayoutMode::too_narrow_for_desktop(ui.ctx());
        let s = &mut self.ui.settings;
        let mut open = None;
        let mut dirty = false;
        ui.add_space(8.0);
        kit::list(ui, |list| {
            let later = game
                .as_ref()
                .filter(|game| !game.both)
                .filter(|game| s.console_for(&game.rom_key, game.mode) != game.model)
                .map(|_| "Changes when the next game loads");
            if list
                .row(
                    Row::new("Console")
                        .value(s.console.to_string())
                        .detail(later.unwrap_or_default()),
                )
                .clicked()
            {
                open = Some(Sheet::Console);
            }
            if let Some(game) = game.as_ref().filter(|game| game.both) {
                let next = s.console_for(&game.rom_key, game.mode);
                let detail = if next != game.model {
                    format!("Starts on {} next time", settings::console_name(next))
                } else {
                    String::new()
                };
                if list
                    .row(
                        Row::new("This game's console")
                            .value(ConsoleChoice::of(game.model).to_string())
                            .detail(&detail),
                    )
                    .clicked()
                {
                    open = Some(Sheet::GameConsole);
                }
            }
            let too_narrow = if s.layout == LayoutMode::Desktop && narrow {
                "Too narrow here, so it uses touch"
            } else {
                ""
            };
            if list
                .row(
                    Row::new("Layout")
                        .value(s.layout.to_string())
                        .detail(too_narrow),
                )
                .clicked()
            {
                open = Some(Sheet::Layout);
            }
            if list
                .row(Row::new("Theme").value(s.ui_theme.to_string()))
                .clicked()
            {
                open = Some(Sheet::Theme);
            }
            let ui_size = crate::app::mobile::sheet::percent(s.touch.ui_scale.into());
            if list.row(Row::new("UI size").value(ui_size)).clicked() {
                open = Some(Sheet::UiSize);
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                dirty |= list.switch(
                    SwitchRow::new("Check for updates", &mut s.check_updates)
                        .detail(Some("Asks GitHub when Citrine starts and once a day")),
                );
            }
            dirty |= list.switch(SwitchRow::new("Dev tools", &mut s.dev_mode));
        });
        kit::section_label(ui, "Autosave");
        kit::list(ui, |list| {
            if list
                .row(Row::new("Every").value(s.autosave_interval.to_string()))
                .clicked()
            {
                open = Some(Sheet::AutosaveEvery);
            }
            if list
                .row(Row::new("Keep").value(match s.autosave_keep {
                    1 => "1 save".to_owned(),
                    n => format!("{n} saves"),
                }))
                .clicked()
            {
                open = Some(Sheet::AutosaveKeep);
            }
        });
        kit::section_label(ui, "Capture");
        kit::list(ui, |list| {
            if list
                .row(Row::new("Screenshot size").value(format!("{}x", s.screenshot_scale)))
                .clicked()
            {
                open = Some(Sheet::ScreenshotScale);
            }
            if list
                .row(Row::new("Recording size").value(format!("{}x", s.recording_scale)))
                .clicked()
            {
                open = Some(Sheet::RecordingScale);
            }
        });
        s.dirty |= dirty;
        self.open_sheet(open);
    }

    pub(super) fn general_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let game = self.general_game();
        let s = &mut self.ui.settings;
        let (changed, done) = match open {
            Sheet::Console => pick(
                ctx,
                "Console",
                Some("Automatic picks Game Boy Color for games made for it."),
                &mut s.console,
            ),
            Sheet::GameConsole => {
                let Some(game) = game else {
                    return true;
                };
                let mut choice = ConsoleChoice::of(game.model);
                let options = [ConsoleChoice::GameBoy, ConsoleChoice::GameBoyColor];
                let (changed, done) = pick_from(
                    ctx,
                    "This game's console",
                    Some("Used for this game instead of Console."),
                    options,
                    &mut choice,
                    |c| c.to_string(),
                );
                if changed {
                    self.events.play_on(choice.resolve(GbModel::Cgb));
                }
                return done;
            }
            Sheet::Layout => pick(ctx, "Layout", Some(LAYOUT_NOTE), &mut s.layout),
            Sheet::Theme => pick(ctx, "Theme", None, &mut s.ui_theme),
            Sheet::AutosaveEvery => pick(
                ctx,
                "Autosave every",
                Some(AUTOSAVE_NOTE),
                &mut s.autosave_interval,
            ),
            Sheet::AutosaveKeep => {
                let shown = slide(
                    ctx,
                    "Autosaves to keep",
                    Some("When there are more, the oldest autosave goes."),
                    Amount {
                        value: &mut s.autosave_keep,
                        range: 1..=MAX_KEEP,
                        step: 1.0,
                        default: DEFAULT_KEEP,
                        text: count,
                    },
                );
                (settled(&shown.inner), shown.closed)
            }
            Sheet::ScreenshotScale => pick_from(
                ctx,
                "Screenshot size",
                None,
                1..=MAX_SCALE,
                &mut s.screenshot_scale,
                scale_label,
            ),
            _ => pick_from(
                ctx,
                "Recording size",
                None,
                (2..=MAX_SCALE).step_by(2),
                &mut s.recording_scale,
                scale_label,
            ),
        };
        s.dirty |= changed;
        done
    }
}
