use crate::app::mobile::kit;
use egui::{Rect, Response, Sense, TextStyle, TextWrapMode, Ui, WidgetText, vec2};

const SWITCH_GAP: f32 = 12.0;

pub fn menu_row(ui: &mut Ui, text: impl Into<WidgetText>, switch: Option<bool>) -> Response {
    let padding = ui.spacing().button_padding;
    let galley = text.into().into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    let text_height = galley.size().y;
    let switch_size = switch.map(|_| vec2(text_height * 1.7, text_height));
    let content =
        galley.size().x + 2.0 * padding.x + switch_size.map_or(0.0, |size| SWITCH_GAP + size.x);
    let height = (text_height + 2.0 * padding.y).max(ui.spacing().interact_size.y);
    let width = if ui.is_sizing_pass() || ui.layout().is_horizontal() {
        content
    } else {
        ui.available_width().max(content)
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let text = galley.text().to_owned();
    response.widget_info(|| match switch {
        Some(on) => {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), on, &text)
        }
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &text),
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.style().interact(&response);
    if response.hovered() || response.has_focus() || response.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let text_pos = egui::pos2(rect.left() + padding.x, rect.center().y - text_height / 2.0);
    ui.painter().galley(text_pos, galley, visuals.text_color());
    if let (Some(on), Some(size)) = (switch, switch_size) {
        let t = ui.ctx().animate_bool_responsive(response.id, on);
        let right = egui::pos2(rect.right() - padding.x, rect.center().y);
        let switch_rect = Rect::from_min_max(
            right - vec2(size.x, size.y / 2.0),
            right + vec2(0.0, size.y / 2.0),
        );
        kit::paint_switch(ui, switch_rect, t);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::ui_theme::{BRAND, BRAND_DARK};
    use egui::{Pos2, Shape, epaint::ClippedShape};

    fn shapes(dark: bool, add: impl Fn(&mut Ui)) -> Vec<ClippedShape> {
        let ctx = egui::Context::default();
        crate::app::ui_state::ui_theme::UiTheme::Light.apply(&ctx);
        if dark {
            crate::app::ui_state::ui_theme::UiTheme::Dark.apply(&ctx);
        }
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            ..Default::default()
        };
        let mut out = Vec::new();
        for _ in 0..30 {
            out = ctx
                .run(input.clone(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| add(ui));
                })
                .shapes;
        }
        out
    }

    #[test]
    fn a_switch_that_is_on_fills_with_the_brand_colour() {
        for (dark, wanted) in [(false, BRAND), (true, BRAND_DARK)] {
            let shapes = shapes(dark, |ui| {
                menu_row(ui, "Watch Lena", Some(true));
            });
            let filled = shapes
                .iter()
                .any(|clipped| matches!(&clipped.shape, Shape::Rect(rect) if rect.fill == wanted));
            assert!(filled, "dark {dark}: no brand fill");
        }
    }
}
