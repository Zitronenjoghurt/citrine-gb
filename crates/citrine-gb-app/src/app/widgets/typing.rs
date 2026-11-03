use crate::app::widgets::emoji;
use egui::{Align, Color32, Layout, Response, Sense, TextStyle, Ui, vec2};
use std::time::Duration;

const DOTS: usize = 3;
const CYCLE: f64 = 1.2;
const FRAME: Duration = Duration::from_millis(40);
const STILL_CHECK: Duration = Duration::from_millis(500);

pub fn line(ui: &mut Ui, text: &str, style: TextStyle, color: Color32) -> Response {
    let height = ui.text_style_height(&style);
    let radius = (height * 0.1).max(1.5);
    let step = radius * 3.2;
    let width = step * (DOTS - 1) as f32 + radius * 2.0;
    let gap = radius * 1.5;
    let still = crate::utils::reduced_motion();
    let size = vec2(ui.available_width(), height);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let response = ui
            .scope(|ui| {
                ui.set_max_width((ui.available_width() - width - gap).max(0.0));
                emoji::line(ui, text, style.clone(), color)
            })
            .inner;
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        if ui.is_rect_visible(rect) {
            let time = ui.input(|i| i.time);
            let y = rect.bottom() - height * 0.3;
            for dot in 0..DOTS {
                let alpha = if still {
                    0.8
                } else {
                    let phase = (time / CYCLE - dot as f64 / DOTS as f64 / 2.0).rem_euclid(1.0);
                    let wave = (phase * std::f64::consts::TAU).sin().max(0.0) as f32;
                    0.3 + 0.7 * wave
                };
                let x = rect.left() + radius + step * dot as f32;
                ui.painter()
                    .circle_filled(egui::pos2(x, y), radius, color.gamma_multiply(alpha));
            }
        }
        ui.ctx()
            .request_repaint_after(if still { STILL_CHECK } else { FRAME });
        response
    })
    .inner
}

pub fn chat_line(names: &[&str]) -> Option<String> {
    match names {
        [] => None,
        [one] => Some(format!("{one} is typing")),
        [a, b] => Some(format!("{a} and {b} are typing")),
        _ => Some("Several people are typing".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_two_or_several_names() {
        assert_eq!(chat_line(&[]), None);
        assert_eq!(chat_line(&["Ana"]).unwrap(), "Ana is typing");
        assert_eq!(
            chat_line(&["Ana", "Ben"]).unwrap(),
            "Ana and Ben are typing"
        );
        assert_eq!(
            chat_line(&["Ana", "Ben", "Cem"]).unwrap(),
            "Several people are typing"
        );
    }

    #[test]
    fn the_line_is_no_taller_than_its_text_on_touch() {
        let ctx = egui::Context::default();
        crate::app::mobile::touch_spacing(&ctx, true);
        let mut height = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let color = ui.visuals().text_color();
                let top = ui.cursor().top();
                line(ui, "typing", TextStyle::Small, color);
                height = ui.cursor().top() - top - ui.spacing().item_spacing.y;
            });
        });
        let text = ctx.style().text_styles[&TextStyle::Small].size * 1.5;
        assert!(height <= text, "{height} over {text}");
    }
}
