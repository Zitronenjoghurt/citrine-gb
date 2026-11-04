use crate::app::ui_state::hotkeys::{HotkeyAction, Hotkeys};
use crate::app::widgets::disassembly::instruction_count;
use crate::app::widgets::shortcut::shortcut_button;
use crate::icons;
use citrine_gb::debug::step::Step;
use egui::{DragValue, Label, RichText, TextWrapMode, Ui};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct TimeControlState {
    pub step_cycles: u32,
    pub start_cycles: u32,
}

impl Default for TimeControlState {
    fn default() -> Self {
        Self {
            step_cycles: 1,
            start_cycles: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebuggerAction {
    TogglePause,
    Step(Step),
    ResetTo(u32),
    StaticAnalysis(bool),
    Trace(bool),
    ClearTrace,
    ExportTrace,
}

const STEPS: [(HotkeyAction, Step); 5] = [
    (HotkeyAction::StepInstruction, Step::Instruction),
    (HotkeyAction::StepOver, Step::Over),
    (HotkeyAction::StepOut, Step::Out),
    (HotkeyAction::StepLine, Step::Line),
    (HotkeyAction::StepFrame, Step::Frame),
];

const TOOL_GAP: f32 = 2.0;

pub struct Position {
    pub cycles: u128,
    pub frame: u128,
    pub line: u16,
    pub unit: &'static str,
}

pub struct CodeView<'a> {
    pub follow_pc: &'a mut bool,
    pub analysis: bool,
    pub instructions: usize,
    pub tracing: bool,
    pub traced: usize,
}

pub struct DebuggerBar<'a> {
    hotkeys: &'a Hotkeys,
    state: &'a mut TimeControlState,
    running: bool,
    loaded: bool,
    position: Position,
    code: CodeView<'a>,
}

impl<'a> DebuggerBar<'a> {
    pub fn new(
        hotkeys: &'a Hotkeys,
        state: &'a mut TimeControlState,
        running: bool,
        loaded: bool,
        position: Position,
        code: CodeView<'a>,
    ) -> Self {
        Self {
            hotkeys,
            state,
            running,
            loaded,
            position,
            code,
        }
    }

    pub fn show(mut self, ui: &mut Ui) -> Option<DebuggerAction> {
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = TOOL_GAP;
            let pause = HotkeyAction::TogglePause;
            if shortcut_button(ui, self.hotkeys, pause, self.running, self.loaded).clicked() {
                action = Some(DebuggerAction::TogglePause);
            }
            for (hotkey, step) in STEPS {
                if shortcut_button(ui, self.hotkeys, hotkey, false, self.loaded).clicked() {
                    action = Some(DebuggerAction::Step(step));
                }
            }
            ui.menu_button(icons::DOTS_THREE, |ui| {
                more_menu(
                    ui,
                    self.position.unit,
                    self.loaded,
                    self.state,
                    &mut self.code,
                    &mut action,
                );
            })
            .response
            .on_hover_text("More");
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for part in position_parts(&self.position) {
                let part = RichText::new(part).small().weak();
                ui.add(Label::new(part).wrap_mode(TextWrapMode::Extend));
            }
        });
        action
    }
}

fn more_menu(
    ui: &mut Ui,
    unit: &str,
    loaded: bool,
    state: &mut TimeControlState,
    code: &mut CodeView,
    action: &mut Option<DebuggerAction>,
) {
    ui.add_enabled_ui(loaded, |ui| run_rows(ui, unit, state, action));
    ui.separator();
    crate::app::widgets::check::checkbox(ui, code.follow_pc, "Follow PC");
    ui.horizontal(|ui| {
        let mut analysis = code.analysis;
        if crate::app::widgets::check::checkbox(ui, &mut analysis, "Static analysis").changed() {
            *action = Some(DebuggerAction::StaticAnalysis(analysis));
        }
        let count = RichText::new(instruction_count(code.instructions)).weak();
        ui.add(Label::new(count).wrap_mode(TextWrapMode::Extend));
    });
    ui.separator();
    trace_rows(ui, code, action);
}

fn trace_rows(ui: &mut Ui, code: &CodeView, action: &mut Option<DebuggerAction>) {
    ui.horizontal(|ui| {
        let mut tracing = code.tracing;
        if ui
            .checkbox(&mut tracing, "Trace")
            .on_hover_text("Keep the last 30,000 instructions with their registers")
            .changed()
        {
            *action = Some(DebuggerAction::Trace(tracing));
        }
        let count = RichText::new(traced_count(code.traced)).weak();
        ui.add(Label::new(count).wrap_mode(TextWrapMode::Extend));
    });
    ui.add_enabled_ui(code.traced > 0, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Export trace").clicked() {
                *action = Some(DebuggerAction::ExportTrace);
            }
            if ui.button("Clear").clicked() {
                *action = Some(DebuggerAction::ClearTrace);
            }
        });
    });
}

fn traced_count(lines: usize) -> String {
    if lines == 1 {
        "1 line".to_owned()
    } else {
        format!("{} lines", grouped(lines as u128))
    }
}

fn run_rows(
    ui: &mut Ui,
    unit: &str,
    state: &mut TimeControlState,
    action: &mut Option<DebuggerAction>,
) {
    ui.horizontal(|ui| {
        ui.add(cycles_field(&mut state.step_cycles, 1, unit));
        if ui
            .button("Run")
            .on_hover_text(format!("Run this many {unit}s"))
            .clicked()
        {
            *action = Some(DebuggerAction::Step(Step::Cycles(state.step_cycles)));
        }
    });
    ui.horizontal(|ui| {
        ui.add(cycles_field(&mut state.start_cycles, 0, unit));
        if ui
            .button("Reset and run")
            .on_hover_text(format!("Reset the game, then run this many {unit}s"))
            .clicked()
        {
            *action = Some(DebuggerAction::ResetTo(state.start_cycles));
        }
    });
}

fn cycles_field<'a>(value: &'a mut u32, min: u32, unit: &str) -> DragValue<'a> {
    let suffix = cycles_suffix(*value, unit);
    DragValue::new(value)
        .range(min..=100_000_000)
        .speed(10.0)
        .suffix(suffix)
}

fn cycles_suffix(value: u32, unit: &str) -> String {
    if value == 1 {
        format!(" {unit}")
    } else {
        format!(" {unit}s")
    }
}

fn position_parts(position: &Position) -> [String; 2] {
    [
        format!("{} {}s, ", grouped(position.cycles), position.unit),
        format!("frame {}, line {}", grouped(position.frame), position.line),
    ]
}

fn grouped(value: u128) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_position_names_its_unit_frame_and_line() {
        let position = Position {
            cycles: 1_234_567,
            frame: 70,
            line: 45,
            unit: "M-cycle",
        };
        assert_eq!(
            position_parts(&position),
            ["1,234,567 M-cycles, ", "frame 70, line 45"]
        );
    }

    #[test]
    fn a_trace_counts_its_lines() {
        assert_eq!(traced_count(1), "1 line");
        assert_eq!(traced_count(30_000), "30,000 lines");
    }

    #[test]
    fn one_m_cycle_is_singular() {
        assert_eq!(cycles_suffix(1, "M-cycle"), " M-cycle");
        assert_eq!(cycles_suffix(0, "M-cycle"), " M-cycles");
        assert_eq!(cycles_suffix(70, "cycle"), " cycles");
    }
}
