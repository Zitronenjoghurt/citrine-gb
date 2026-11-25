pub const CLOSE_HINT: &str = "Close the replay";
pub const WENT_ANOTHER_WAY: &str = "The game went another way than in the recording here";
use super::replay_graph::{replay_graph, seek_span_id};
use crate::app::widgets::action::icon_button;
use crate::emulator::Emulator;
use crate::emulator::tape_play::TapeFacts;
use crate::frame_log::TapeTimes;
use crate::icons;
use crate::media::playback::clock;
use egui::style::HandleShape;
use egui::{Align, Layout, Rangef, Rect, RichText, Ui, vec2};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayBarFacts {
    pub tape: TapeFacts,
    pub phone: bool,
}

impl ReplayBarFacts {
    pub fn of(emulator: &Emulator, phone: bool) -> Option<Self> {
        emulator.tape_facts().map(|tape| Self { tape, phone })
    }

    pub fn at(&self) -> u64 {
        self.tape.seeking.unwrap_or(self.tape.played)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayBarAction {
    Toggle,
    Seek(u64),
    Clip,
    Close,
}

pub fn replay_bar(
    ui: &mut Ui,
    facts: &ReplayBarFacts,
    times: Option<&TapeTimes>,
) -> Option<ReplayBarAction> {
    ui.vertical(|ui| {
        let graph = times.and_then(|times| {
            let tape = facts.tape;
            replay_graph(
                ui,
                times,
                tape.frames,
                facts.at(),
                tape.frame_secs,
                facts.phone,
            )
        });
        bar_row(ui, facts).or(graph.map(ReplayBarAction::Seek))
    })
    .inner
}

fn bar_row(ui: &mut Ui, facts: &ReplayBarFacts) -> Option<ReplayBarAction> {
    let tape = facts.tape;
    let (height, side) = if facts.phone {
        let side = crate::app::mobile::touch_square(ui);
        (side.max(ui.spacing().interact_size.y), side)
    } else {
        let side = ui.spacing().interact_size.y;
        (side, side)
    };
    let width = ui.available_width();
    let at = facts.at();
    let mut action = None;
    ui.allocate_ui_with_layout(
        vec2(width, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_size(vec2(width, height));
            let (icon, hint) = if tape.running {
                (icons::PAUSE, "Pause")
            } else {
                (icons::PLAY, "Play")
            };
            if ui
                .add(icon_button(icon, side))
                .on_hover_text(hint)
                .clicked()
            {
                action = Some(ReplayBarAction::Toggle);
            }
            ui.label(RichText::new(clock(at as f64 * tape.frame_secs)).monospace());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(icon_button(icons::X, side))
                    .on_hover_text(CLOSE_HINT)
                    .clicked()
                {
                    action = Some(ReplayBarAction::Close);
                }
                if ui
                    .add(icon_button(icons::SCISSORS, side))
                    .on_hover_text(crate::app::widgets::clip_panel::TITLE)
                    .clicked()
                {
                    action = Some(ReplayBarAction::Clip);
                }
                ui.label(RichText::new(clock(tape.frames as f64 * tape.frame_secs)).monospace());
                if tape.diverged {
                    ui.label(RichText::new(icons::WARNING).color(ui.visuals().warn_fg_color))
                        .on_hover_text(WENT_ANOTHER_WAY);
                }
                let mut to = at;
                ui.spacing_mut().slider_width = ui.available_width().max(0.0);
                let seek = ui.add(
                    egui::Slider::new(&mut to, 1..=tape.frames.max(1))
                        .show_value(false)
                        .trailing_fill(true),
                );
                let span = seek_span(seek.rect, ui.visuals().handle_shape);
                ui.data_mut(|d| d.insert_temp(seek_span_id(), span));
                if seek.changed() {
                    action = Some(ReplayBarAction::Seek(to));
                }
            });
        },
    );
    action
}

fn seek_span(rect: Rect, handle: HandleShape) -> Rangef {
    let radius = rect.height() / 2.5;
    let thumb = match handle {
        HandleShape::Circle => radius,
        HandleShape::Rect { aspect_ratio } => radius * aspect_ratio,
    };
    rect.x_range().shrink(thumb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seek_span_is_where_the_slider_handle_travels() {
        let rect = Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(500.0, 25.0));
        let span = seek_span(rect, HandleShape::Rect { aspect_ratio: 0.75 });
        assert_eq!(span, Rangef::new(107.5, 492.5));
        let span = seek_span(rect, HandleShape::Circle);
        assert_eq!(span, Rangef::new(110.0, 490.0));
    }

    #[test]
    fn a_click_on_the_graph_seeks_where_the_slider_handle_would_be() {
        let ctx = egui::Context::default();
        let mut value = 1_u64;
        let mut seek_rect = Rect::NOTHING;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().slider_width = 400.0;
                seek_rect = ui
                    .add(egui::Slider::new(&mut value, 1..=1001).show_value(false))
                    .rect;
            });
        });
        let span = seek_span(seek_rect, ctx.style().visuals.handle_shape);
        let x = span.min + span.span() * 0.25;
        let raw = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(egui::pos2(x, seek_rect.center().y)),
                egui::Event::PointerButton {
                    pos: egui::pos2(x, seek_rect.center().y),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().slider_width = 400.0;
                ui.add(egui::Slider::new(&mut value, 1..=1001).show_value(false));
            });
        });
        assert!(value.abs_diff(251) <= 1, "{value}");
    }
}
