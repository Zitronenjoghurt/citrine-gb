use egui::{Rect, Response, Sense, TextStyle, TextWrapMode, Ui, WidgetText, vec2};

const SWITCH_GAP: f32 = 12.0;

pub fn menu_row(ui: &mut Ui, text: impl Into<WidgetText>, switch: Option<bool>) -> Response {
    menu_row_with(ui, text, None, switch)
}

pub fn menu_row_width(ui: &Ui, text: impl Into<WidgetText>) -> f32 {
    let galley = text.into().into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    galley.size().x + 2.0 * ui.spacing().button_padding.x
}

pub fn menu_row_with(
    ui: &mut Ui,
    text: impl Into<WidgetText>,
    end: Option<&str>,
    switch: Option<bool>,
) -> Response {
    let padding = ui.spacing().button_padding;
    let galley = text.into().into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    let end = end.map(|end| {
        WidgetText::from(egui::RichText::new(end).weak()).into_galley(
            ui,
            Some(TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Button,
        )
    });
    let end_width = end.as_ref().map_or(0.0, |end| SWITCH_GAP + end.size().x);
    let text_height = galley.size().y;
    let touch = crate::app::mobile::touch_square(ui) > 0.0;
    let switch_size = switch.map(|_| {
        if touch {
            vec2(text_height * 1.7, text_height)
        } else {
            egui::Vec2::splat(ui.spacing().icon_width)
        }
    });
    let content = galley.size().x
        + 2.0 * padding.x
        + end_width
        + switch_size.map_or(0.0, |size| SWITCH_GAP + size.x);
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
    if crate::app::big_screen::ring::fill(&response) {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, focus_fill(ui.visuals()));
    } else if response.hovered() || response.has_focus() || response.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }
    let text_pos = egui::pos2(rect.left() + padding.x, rect.center().y - text_height / 2.0);
    ui.painter().galley(text_pos, galley, visuals.text_color());
    if let Some(end) = end {
        let right = rect.right() - padding.x - switch_size.map_or(0.0, |size| size.x + SWITCH_GAP);
        let at = egui::pos2(right - end.size().x, rect.center().y - end.size().y / 2.0);
        ui.painter().galley(at, end, visuals.text_color());
    }
    if let (Some(on), Some(size)) = (switch, switch_size) {
        let t = ui.ctx().animate_bool_responsive(response.id, on);
        let right = egui::pos2(rect.right() - padding.x, rect.center().y);
        let switch_rect = Rect::from_min_max(
            right - vec2(size.x, size.y / 2.0),
            right + vec2(0.0, size.y / 2.0),
        );
        if !crate::app::mobile::switch(ui, switch_rect, t) {
            super::check::paint_box(ui, switch_rect, &response, on);
        }
    }
    response
}

fn focus_fill(visuals: &egui::Visuals) -> egui::Color32 {
    crate::app::big_screen::ring::fill_color(visuals, visuals.window_fill)
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

    fn has_knob(shapes: &[ClippedShape]) -> bool {
        shapes
            .iter()
            .any(|clipped| matches!(&clipped.shape, Shape::Circle(_)))
    }

    #[test]
    fn a_desktop_menu_ticks_a_box_instead_of_the_phone_switch() {
        for dark in [false, true] {
            let shapes = shapes(dark, |ui| {
                menu_row(ui, "Watch Lena", Some(false));
            });
            assert!(!has_knob(&shapes), "dark {dark}: a phone switch on desktop");
            assert!(
                shapes.iter().any(|clipped| matches!(
                    &clipped.shape,
                    Shape::Rect(rect) if rect.stroke.width >= 1.0
                        && (rect.rect.width() - rect.rect.height()).abs() < 0.5
                )),
                "dark {dark}: no empty box"
            );
        }
    }

    #[test]
    fn a_touch_menu_keeps_the_switch() {
        let shapes = shapes(false, |ui| {
            ui.spacing_mut().interact_size.y = 48.0;
            menu_row(ui, "Watch Lena", Some(true));
        });
        assert!(has_knob(&shapes));
    }
}
