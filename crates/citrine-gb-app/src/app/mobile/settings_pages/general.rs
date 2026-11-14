use super::{Sheet, pick, pick_from, settled, slide};
use crate::app::big_screen;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::Amount;
use crate::app::ui_state::settings::{self, ConsoleChoice, LayoutMode, plays_on_both};
use crate::app::widgets::settings::page::{Heading, Page};
use crate::app::{Citrine, report};
use crate::autosave::{DEFAULT_KEEP, MAX_KEEP};
use crate::emulator::rewind::REWIND_NOTE;
use crate::icons;
use crate::media::recording::MAX_SCALE;
use citrine_gb::console::VideoSpec;
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

fn scale_label(scale: usize, video: VideoSpec) -> String {
    format!(
        "{scale}x, {} × {}",
        video.width * scale,
        video.height * scale
    )
}

fn count(value: f64) -> String {
    format!("{value:.0}")
}

impl Citrine {
    fn general_game(&self) -> Option<Game> {
        let rom_key = self.emulator.rom_key()?.to_owned();
        let mode = self.emulator.as_gb()?.cartridge.header.cgb_mode;
        Some(Game {
            rom_key,
            both: plays_on_both(mode),
            model: self.emulator.console(),
            mode,
        })
    }

    pub(super) fn general_page(&mut self, ui: &mut Ui) {
        let game = self.general_game();
        #[cfg(feature = "gba")]
        let gba_bios = self.emulator.gba_bios_name().map(str::to_owned);
        let narrow = LayoutMode::too_narrow_for_desktop(ui.ctx());
        let has_game = self.emulator.has_game();
        let mut save_report = false;
        #[cfg(feature = "nds")]
        let mut clear_jit = false;
        let s = &mut self.ui.settings;
        let mut open = None;
        let mut dirty = false;
        let mut big = None;
        let mut page = Page::begin(ui);
        page.plain(|ui| {
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
                #[cfg(feature = "gba")]
                if list
                    .row(Row::new("GBA BIOS").value(
                        crate::app::gba_bios::BiosChoice::current(gba_bios.as_deref()).to_string(),
                    ))
                    .clicked()
                {
                    open = Some(Sheet::GbaBios);
                }
                let too_narrow = if s.layout == LayoutMode::Desktop && narrow {
                    "Too narrow here, so it uses touch"
                } else {
                    ""
                };
                let layout = list.row(
                    Row::new("Layout")
                        .value(s.layout.to_string())
                        .detail(too_narrow)
                        .stepper(),
                );
                if layout.clicked() {
                    open = Some(Sheet::Layout);
                }
                if let Some(step) = big_screen::take_step(&layout) {
                    s.layout = big_screen::cycle(s.layout, step);
                    dirty = true;
                }
                if !crate::platform::is_tv() {
                    let mut on = s.big_screen.on;
                    if list.switch(SwitchRow::new("Big screen", &mut on)) {
                        big = Some(on);
                    }
                }
                let theme = list.row(Row::new("Theme").value(s.ui_theme.to_string()).stepper());
                if theme.clicked() {
                    open = Some(Sheet::Theme);
                }
                if let Some(step) = big_screen::take_step(&theme) {
                    s.ui_theme = big_screen::cycle(s.ui_theme, step);
                    dirty = true;
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
                dirty |= list.switch(
                    SwitchRow::new("Game covers", &mut s.covers)
                        .detail(Some("Box art from libretro's collection on GitHub")),
                );
                #[cfg(not(target_arch = "wasm32"))]
                {
                    dirty |= list.switch(
                        SwitchRow::new("Achievement progress", &mut s.achievements.in_library)
                            .detail(Some("RetroAchievements earned, under each game")),
                    );
                }
                #[cfg(feature = "nds")]
                if citrine_gb::nds::machine::JIT_AVAILABLE {
                    dirty |= list.switch(
                        SwitchRow::new(settings::JIT_LABEL, &mut s.ds_jit).info(settings::JIT_HINT),
                    );
                    clear_jit = list
                        .row(
                            Row::new(settings::CLEAR_JIT_LABEL)
                                .icon(icons::TRASH)
                                .no_chevron(),
                        )
                        .clicked();
                }
                dirty |= list.switch(SwitchRow::new("Dev tools", &mut s.dev_mode));
            })
        });
        if let Some(sheet) = self.cartridge_rows(&mut page) {
            open = Some(sheet);
        }
        #[cfg(feature = "nds")]
        if let Some(sheet) = self.ds_rows(&mut page) {
            open = Some(sheet);
        }
        let s = &mut self.ui.settings;
        page.section(
            Heading::new(icons::CLOCK_COUNTER_CLOCKWISE, "Autosave"),
            |ui| {
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
                })
            },
        );
        page.section(Heading::new(icons::FAST_FORWARD, "Fast forward"), |ui| {
            kit::list(ui, |list| {
                if list
                    .row(Row::new("Speed").value(s.fast_forward_speed.to_string()))
                    .clicked()
                {
                    open = Some(Sheet::FastForward);
                }
            })
        });
        page.section(
            Heading::new(icons::REWIND, "Rewind").hint(REWIND_NOTE),
            |ui| {
                kit::list(ui, |list| {
                    if list
                        .row(Row::new("How far back").value(s.rewind.to_string()))
                        .clicked()
                    {
                        open = Some(Sheet::Rewind);
                    }
                })
            },
        );
        page.section(Heading::new(icons::CAMERA, "Capture"), |ui| {
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
            })
        });
        if has_game {
            page.section(
                Heading::new(icons::BUG, "Game report").hint(report::GAME_REPORT_NOTE),
                |ui| {
                    kit::list(ui, |list| {
                        save_report = list
                            .row(
                                Row::new(report::SAVE_GAME_REPORT)
                                    .icon(report::SAVE_GAME_REPORT_ICON)
                                    .no_chevron(),
                            )
                            .clicked();
                    })
                },
            );
        }
        page.end();
        s.dirty |= dirty;
        self.open_sheet(open);
        if let Some(on) = big {
            self.set_big_screen(on);
        }
        #[cfg(feature = "nds")]
        if clear_jit {
            self.clear_jit_cache();
        }
        if save_report {
            self.save_game_report();
        }
    }

    pub(super) fn general_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let game = self.general_game();
        let video = self.emulator.video();
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
            #[cfg(feature = "gba")]
            Sheet::GbaBios => {
                use crate::app::gba_bios::BiosChoice;
                let name = self.emulator.gba_bios_name();
                let mut choice = BiosChoice::current(name);
                let (changed, done) = pick_from(
                    ctx,
                    "GBA BIOS",
                    Some(crate::app::gba_bios::BIOS_HINT),
                    BiosChoice::all(name),
                    &mut choice,
                    |choice| choice.to_string(),
                );
                if changed {
                    choice.apply(&mut self.events);
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
            Sheet::FastForward => pick(ctx, "Fast forward speed", None, &mut s.fast_forward_speed),
            Sheet::Rewind => pick(ctx, "Rewind", Some(REWIND_NOTE), &mut s.rewind),
            Sheet::ScreenshotScale => pick_from(
                ctx,
                "Screenshot size",
                None,
                1..=MAX_SCALE,
                &mut s.screenshot_scale,
                |scale| scale_label(scale, video),
            ),
            _ => pick_from(
                ctx,
                "Recording size",
                None,
                (2..=MAX_SCALE).step_by(2),
                &mut s.recording_scale,
                |scale| scale_label(scale, video),
            ),
        };
        s.dirty |= changed;
        done
    }
}
