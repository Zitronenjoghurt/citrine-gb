use crate::app::tabs::TabViewer;
use crate::app::tabs::{Tab, Tool};
use crate::app::widgets::breakpoints::{BreakpointList, Entry, EntryAction};
use crate::app::widgets::debugger::{CodeView, DebuggerAction, DebuggerBar, Position};
use crate::app::widgets::disassembly::{self, CodeAction, DisassemblyView};
use crate::app::widgets::registers::RegisterTable;
use crate::icons;
use crate::utils::file_saver::FileSaver;
use citrine_gb::debug::Debug;
use citrine_gb::debug::print::{PrintLevel, PrintLine};
use egui::{
    Align, CentralPanel, CollapsingHeader, Color32, Frame, Id, Label, Layout, RichText, ScrollArea,
    TextEdit, TextStyle, TextWrapMode, TopBottomPanel, Ui, Widget, vec2,
};

const SIDE_BY_SIDE: f32 = 400.0;
const REGISTERS_WIDTH: f32 = 220.0;
const MIN_CODE_ROWS: f32 = 10.0;
const CODE_ROWS_BESIDE_LOG: f32 = 6.0;
const MAX_STOP_ROWS: f32 = 5.0;
const LOG_HEIGHT: f32 = 160.0;
const LOG_FLOOR_LINES: f32 = 4.0;

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    toolbar(viewer, ui);
    ui.separator();
    if ui.available_width() >= SIDE_BY_SIDE {
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            let code = ui.available_width() - REGISTERS_WIDTH - ui.spacing().item_spacing.x * 2.0;
            ui.allocate_ui_with_layout(vec2(code, height), Layout::top_down(Align::Min), |ui| {
                code_list(viewer, ui)
            });
            ui.separator();
            ui.vertical(|ui| {
                ScrollArea::vertical()
                    .id_salt("debugger_cpu")
                    .show(ui, |ui| {
                        cpu(viewer, ui, false);
                        stops(viewer, ui);
                        print_log(viewer, ui, false);
                    });
            });
        });
    } else {
        stacked(viewer, ui);
    }
}

fn stacked(viewer: &mut TabViewer, ui: &mut Ui) {
    let row = disassembly::row_height(ui);
    let header = ui.spacing().interact_size.y;
    let room = |ui: &Ui| (ui.available_height() - row * MIN_CODE_ROWS).max(0.0);
    let measured = Id::new("debugger_stops_height");
    let (stops_floor, stops_full) = if stop_count(viewer) == 0 {
        (0.0, 0.0)
    } else {
        let guess = (header + ui.spacing().item_spacing.y) * 3.0;
        ui.data(|data| data.get_temp::<(f32, f32)>(measured))
            .unwrap_or((guess, guess))
    };
    let log_floor = if has_log(viewer) {
        let gap = ui.spacing().item_spacing.y;
        let line = ui.text_style_height(&TextStyle::Monospace) + gap;
        header + gap * 3.0 + line * LOG_FLOOR_LINES + ui.spacing().interact_size.y * 2.0
    } else {
        0.0
    };
    let cpu_height = (room(ui) - stops_floor - log_floor).max(header);
    TopBottomPanel::bottom(ui.id().with("cpu"))
        .frame(Frame::NONE)
        .resizable(false)
        .max_height(cpu_height)
        .show_inside(ui, |ui| {
            let open = viewer.ui.registers.docked_cpu;
            let shown = CollapsingHeader::new(RichText::new("CPU").strong())
                .id_salt("docked_cpu")
                .open(Some(open))
                .show(ui, |ui| {
                    ui.set_max_height((cpu_height - header - ui.spacing().item_spacing.y).max(0.0));
                    ScrollArea::vertical().show(ui, |ui| cpu(viewer, ui, true));
                });
            if shown.header_response.clicked() {
                viewer.ui.registers.docked_cpu = !viewer.ui.registers.docked_cpu;
            }
        });
    if log_floor > 0.0 {
        let log_height =
            (ui.available_height() - row * CODE_ROWS_BESIDE_LOG - stops_floor).max(header);
        TopBottomPanel::bottom(ui.id().with("log"))
            .frame(Frame::NONE)
            .resizable(false)
            .max_height(log_height)
            .show_inside(ui, |ui| {
                ui.set_max_height(log_height);
                print_log(viewer, ui, true);
            });
    }
    let stops_height = stops_full
        .min(room(ui).min(row * MAX_STOP_ROWS))
        .max(stops_floor);
    TopBottomPanel::bottom(ui.id().with("stops"))
        .frame(Frame::NONE)
        .resizable(false)
        .show_separator_line(false)
        .exact_height(stops_height)
        .show_inside(ui, |ui| {
            let list = ScrollArea::vertical()
                .id_salt("docked_stops")
                .max_height(stops_height)
                .show(ui, |ui| stops(viewer, ui));
            let height = (list.inner, list.content_size.y);
            if ui.data(|data| data.get_temp::<(f32, f32)>(measured)) != Some(height) {
                ui.data_mut(|data| data.insert_temp(measured, height));
                ui.ctx().request_repaint();
            }
        });
    CentralPanel::default()
        .frame(Frame::NONE)
        .show_inside(ui, |ui| code_list(viewer, ui));
}

fn toolbar(viewer: &mut TabViewer, ui: &mut Ui) {
    let emulator = &mut *viewer.emulator;
    let running = emulator.is_running();
    let has_game = emulator.has_game();
    let machine = emulator.debug_mut();
    let frame_cycles = u128::from(machine.frame_cycles().max(1));
    let cycles = machine.cycles();
    let position = Position {
        cycles,
        frame: cycles / frame_cycles,
        line: machine.scanline(),
        unit: machine.features().cycle,
    };
    let analysis = machine.code_analysis();
    let instructions = machine.code_rows();
    let trace = machine.trace();
    let (tracing, traced) = (trace.is_on(), trace.len());
    let action = DebuggerBar::new(
        &viewer.ui.settings.hotkeys,
        &mut viewer.ui.time_ctrl,
        running,
        has_game,
        position,
        CodeView {
            follow_pc: &mut viewer.ui.settings.track_pc,
            analysis,
            instructions,
            tracing,
            traced,
        },
    )
    .show(ui);
    match action {
        Some(DebuggerAction::TogglePause) => emulator.toggle_running(),
        Some(DebuggerAction::Step(step)) => {
            emulator.step(step);
        }
        Some(DebuggerAction::ResetTo(cycles)) => emulator.reset_to(ui.ctx(), cycles),
        Some(action) => {
            let machine = emulator.debug_mut();
            match action {
                DebuggerAction::StaticAnalysis(on) => machine.set_code_analysis(on),
                DebuggerAction::Trace(on) => machine.trace().set_on(on),
                DebuggerAction::ClearTrace => machine.trace().clear(),
                DebuggerAction::ExportTrace => {
                    FileSaver::new("citrine_trace.txt")
                        .add_filter("Text", &["txt"])
                        .dispatch(
                            machine.trace().export().into_bytes(),
                            viewer.files.save_tx.clone(),
                        );
                }
                _ => {}
            }
        }
        None => {}
    }
}

fn cpu(viewer: &mut TabViewer, ui: &mut Ui, bare: bool) {
    let mut groups = Vec::new();
    let machine = viewer.emulator.debug();
    machine.registers(&mut groups);
    let cpu_groups = machine.features().cpu_groups;
    groups.retain(|group| cpu_groups.contains(&group.name));
    let table = RegisterTable::new(&groups, &mut viewer.ui.registers);
    if bare { table.bare() } else { table }.ui(ui);
}

#[derive(Clone, Default)]
struct LogFilter {
    search: String,
    hidden: [bool; LEVELS.len()],
}

impl LogFilter {
    fn shows(&self, line: &PrintLine) -> bool {
        let level = LEVELS.iter().position(|level| *level == line.level);
        let search = self.search.to_lowercase();
        !level.is_some_and(|index| self.hidden[index])
            && (search.is_empty() || line.text.to_lowercase().contains(&search))
    }
}

const LEVELS: [PrintLevel; 5] = [
    PrintLevel::Fatal,
    PrintLevel::Error,
    PrintLevel::Warn,
    PrintLevel::Info,
    PrintLevel::Debug,
];

fn print_log(viewer: &mut TabViewer, ui: &mut Ui, bounded: bool) {
    let Some(log) = viewer.emulator.debug_mut().print_log() else {
        return;
    };
    if log.lines().is_empty() {
        return;
    }
    ui.add_space(ui.spacing().item_spacing.y);
    let mut clear = false;
    let bottom = ui.max_rect().bottom();
    CollapsingHeader::new(RichText::new("Log").strong())
        .id_salt("debugger_log")
        .default_open(true)
        .show(ui, |ui| {
            if bounded {
                ui.set_max_height((bottom - ui.cursor().top()).max(0.0));
            }
            let id = Id::new("debugger_log_filter");
            let mut filter: LogFilter = ui.data(|data| data.get_temp(id)).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Clear").clicked() {
                        clear = true;
                    }
                    let filtered = filter.hidden.contains(&true);
                    ui.menu_button(icons::FUNNEL, |ui| {
                        for (index, level) in LEVELS.iter().enumerate() {
                            let mut shown = !filter.hidden[index];
                            if crate::app::widgets::check::checkbox(ui, &mut shown, level.name())
                                .changed()
                            {
                                filter.hidden[index] = !shown;
                            }
                        }
                    })
                    .response
                    .on_hover_text(if filtered {
                        "Levels, some hidden"
                    } else {
                        "Levels"
                    });
                    ui.add(
                        TextEdit::singleline(&mut filter.search)
                            .hint_text("Search")
                            .desired_width(ui.available_width()),
                    );
                });
            });
            let shown: Vec<_> = log
                .lines()
                .iter()
                .filter(|line| filter.shows(line))
                .collect();
            let gap = ui.spacing().item_spacing.y;
            let line_height = ui.text_style_height(&TextStyle::Monospace) + gap;
            let floor = line_height * LOG_FLOOR_LINES;
            let wanted = (line_height * shown.len() as f32).min(LOG_HEIGHT);
            let fits = wanted.min(floor).min(ui.available_height());
            ui.set_min_height(fits);
            let small = TextStyle::Small.resolve(ui.style());
            let level_width = LEVELS
                .iter()
                .map(|level| {
                    ui.fonts_mut(|fonts| {
                        fonts
                            .layout_no_wrap(level.name().to_owned(), small.clone(), Color32::WHITE)
                            .size()
                            .x
                    })
                })
                .fold(0.0, f32::max)
                .ceil();
            ScrollArea::vertical()
                .id_salt("debugger_log_lines")
                .max_height(LOG_HEIGHT)
                .min_scrolled_height(fits)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    let row = ui.text_style_height(&TextStyle::Monospace);
                    for line in shown {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().interact_size.y = 0.0;
                            ui.allocate_ui_with_layout(
                                vec2(level_width, row),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.set_width(level_width);
                                    ui.label(level_text(ui, line.level));
                                },
                            );
                            ui.add(
                                Label::new(RichText::new(&line.text).monospace())
                                    .wrap_mode(TextWrapMode::Wrap),
                            );
                        });
                    }
                });
            ui.data_mut(|data| data.insert_temp(id, filter));
        });
    if clear {
        log.clear();
    }
}

fn has_log(viewer: &mut TabViewer) -> bool {
    viewer
        .emulator
        .debug_mut()
        .print_log()
        .is_some_and(|log| !log.lines().is_empty())
}

fn level_text(ui: &Ui, level: PrintLevel) -> RichText {
    let text = RichText::new(level.name()).small();
    match level {
        PrintLevel::Fatal | PrintLevel::Error => text.color(ui.visuals().error_fg_color),
        PrintLevel::Warn => text.color(ui.visuals().warn_fg_color),
        PrintLevel::Info => text,
        PrintLevel::Debug => text.weak(),
    }
}

fn stop_count(viewer: &mut TabViewer) -> usize {
    let breakpoints = viewer.emulator.debug_mut().breakpoints();
    breakpoints.code().len() + breakpoints.watches().len()
}

fn stops(viewer: &mut TabViewer, ui: &mut Ui) -> f32 {
    let top = ui.cursor().top();
    let running = viewer.emulator.is_running();
    let gb = viewer.emulator.debug_mut();
    let breakpoints = gb.breakpoints();
    let entries: Vec<Entry> = breakpoints
        .code()
        .into_iter()
        .map(Entry::Code)
        .chain(breakpoints.watches().iter().copied().map(Entry::Watch))
        .collect();
    if entries.is_empty() {
        return 0.0;
    }
    let last = if running {
        None
    } else {
        breakpoints.last_stop()
    };
    ui.add_space(ui.spacing().item_spacing.y);
    let list = BreakpointList::new(&entries, last).show(ui);
    match list.action {
        Some(EntryAction::Remove(Entry::Code(at))) => {
            gb.breakpoints().remove(&at);
        }
        Some(EntryAction::Remove(Entry::Watch(watch))) => {
            gb.breakpoints().unwatch(watch);
        }
        Some(EntryAction::Show(Entry::Watch(watch))) => {
            let mut blocks = Vec::new();
            gb.memory_blocks(&mut blocks);
            if let Some(block) = blocks.iter().find(|block| block.contains(watch.from)) {
                viewer.ui.memory.show_at(block, watch.from, watch.len);
                viewer.events.open_tab(Tab::Tool(Tool::Memory));
            }
        }
        Some(EntryAction::Show(Entry::Code(_))) | None => {}
    }
    list.top_rows_bottom - top
}

fn code_list(viewer: &mut TabViewer, ui: &mut Ui) {
    let colours = viewer.ui.settings.ui_theme.code_colours();
    let machine = viewer.emulator.debug_mut();
    let action = DisassemblyView::new(machine, viewer.ui.settings.track_pc, colours).show(ui);
    match action {
        Some(CodeAction::ToggleBreakpoint(at)) => machine.breakpoints().toggle(at),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: PrintLevel, text: &str) -> PrintLine {
        PrintLine {
            level,
            text: text.into(),
        }
    }

    #[test]
    fn the_log_filter_hides_levels_and_matches_text_in_any_case() {
        let mut filter = LogFilter::default();
        assert!(filter.shows(&line(PrintLevel::Debug, "Pulse 1 on")));

        filter.hidden[4] = true;
        assert!(!filter.shows(&line(PrintLevel::Debug, "Pulse 1 on")));
        assert!(filter.shows(&line(PrintLevel::Warn, "Noise on")));

        filter.search = "NOISE".into();
        assert!(filter.shows(&line(PrintLevel::Warn, "Noise on")));
        assert!(!filter.shows(&line(PrintLevel::Error, "No sound data")));
    }
}
