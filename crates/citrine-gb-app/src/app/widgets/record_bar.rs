pub const FRAME_TIMES: &str = "Performance data";
pub const FRAME_TIMES_HINT: &str = "Record performance data";
use crate::icons;
use egui::{RichText, Ui, vec2};

pub const PHONE_HEIGHT: f32 = 48.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordBarFacts {
    pub time: String,
    pub start_over: bool,
    pub frame_times: Option<bool>,
    pub phone: bool,
}

impl RecordBarFacts {
    pub fn of(
        emulator: &crate::emulator::Emulator,
        ui: &crate::app::ui_state::UiState,
        phone: bool,
    ) -> Option<Self> {
        emulator.is_recording().then(|| Self {
            time: emulator.recording_label(),
            start_over: emulator.is_taping() && !emulator.hardcore(),
            frame_times: (emulator.is_recording() && ui.settings.record_frame_times)
                .then(|| ui.frame_log.is_recording()),
            phone,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordBarAction {
    StartOver,
    Stop,
    FrameTimes(bool),
}

pub fn record_bar(ui: &mut Ui, facts: &RecordBarFacts) -> Option<RecordBarAction> {
    let height = if facts.phone {
        PHONE_HEIGHT
    } else {
        ui.spacing().interact_size.y
    };
    let side = if facts.phone {
        crate::app::mobile::touch_square(ui)
    } else {
        ui.spacing().interact_size.y
    };
    let (_, action) = egui::Sides::new().height(height).show(
        ui,
        |ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(dot.center(), 5.0, ui.visuals().error_fg_color);
            ui.label(RichText::new(&facts.time).monospace());
        },
        |ui| {
            let button = |text: String| egui::Button::new(text).min_size(vec2(0.0, side));
            let mut action = None;
            if ui.add(button(format!("{} Stop", icons::STOP))).clicked() {
                action = Some(RecordBarAction::Stop);
            }
            if facts.start_over
                && ui
                    .add(button(format!("{} Start over", icons::SKIP_BACK)))
                    .clicked()
            {
                action = Some(RecordBarAction::StartOver);
            }
            if let Some(on) = facts.frame_times
                && ui
                    .add(button(format!("{} {FRAME_TIMES}", icons::CHART_LINE)).selected(on))
                    .on_hover_text(FRAME_TIMES_HINT)
                    .clicked()
            {
                action = Some(RecordBarAction::FrameTimes(!on));
            }
            action
        },
    );
    action
}
