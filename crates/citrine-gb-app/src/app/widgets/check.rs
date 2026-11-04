use crate::app::ui_state::ui_theme;
use egui::{Response, Sense, Stroke, TextStyle, Ui, Widget, WidgetText, pos2, vec2};

pub struct Check<'a> {
    on: &'a mut bool,
    text: WidgetText,
}

impl<'a> Check<'a> {
    pub fn new(on: &'a mut bool, text: impl Into<WidgetText>) -> Self {
        Self {
            on,
            text: text.into(),
        }
    }
}

pub fn checkbox(ui: &mut Ui, on: &mut bool, text: impl Into<WidgetText>) -> Response {
    ui.add(Check::new(on, text))
}

impl Widget for Check<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let spacing = ui.spacing();
        let icon_width = spacing.icon_width;
        let icon_spacing = spacing.icon_spacing;
        let interact_height = spacing.interact_size.y;
        let galley = (!self.text.is_empty()).then(|| {
            self.text.into_galley(
                ui,
                None,
                ui.available_width() - icon_width - icon_spacing,
                TextStyle::Button,
            )
        });
        let mut size = vec2(icon_width, 0.0);
        if let Some(galley) = &galley {
            size.x += icon_spacing + galley.size().x;
            size.y = galley.size().y;
        }
        let size = size.max(egui::Vec2::splat(interact_height));
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
        if response.clicked() {
            *self.on = !*self.on;
            response.mark_changed();
        }
        let on = *self.on;
        let label = galley
            .as_ref()
            .map(|g| g.text().to_owned())
            .unwrap_or_default();
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), on, &label)
        });
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let visuals = ui.style().interact(&response);
        let (small, big) = ui.spacing().icon_rectangles(rect);
        let (fill, stroke) = if on {
            (ui_theme::brand(ui.visuals()), Stroke::NONE)
        } else {
            (visuals.bg_fill, visuals.bg_stroke)
        };
        ui.painter().rect(
            big.expand(visuals.expansion),
            visuals.corner_radius,
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        if on {
            ui.painter().line(
                vec![
                    pos2(small.left(), small.center().y),
                    pos2(small.center().x, small.bottom()),
                    pos2(small.right(), small.top()),
                ],
                Stroke::new(2.0_f32, ui_theme::ink_on(fill)),
            );
        }
        if let Some(galley) = galley {
            let pos = pos2(
                big.right() + icon_spacing,
                rect.center().y - galley.size().y / 2.0,
            );
            ui.painter().galley(pos, galley, visuals.text_color());
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::ui_theme::{BRAND, BRAND_DARK, ON_BRAND, UiTheme};
    use egui::{Pos2, Rect, Shape};

    fn shapes(theme: UiTheme, on: bool) -> Vec<Shape> {
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
            ..Default::default()
        };
        let mut on = on;
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                checkbox(ui, &mut on, "Sync settings");
            });
        })
        .shapes
        .into_iter()
        .map(|clipped| clipped.shape)
        .collect()
    }

    #[test]
    fn a_ticked_box_fills_with_the_brand_colour_and_its_tick_reads() {
        for (theme, wanted) in [(UiTheme::Light, BRAND), (UiTheme::Dark, BRAND_DARK)] {
            let shapes = shapes(theme, true);
            assert!(
                shapes
                    .iter()
                    .any(|s| matches!(s, Shape::Rect(r) if r.fill == wanted)),
                "{theme}: no brand box"
            );
            assert!(
                shapes
                    .iter()
                    .any(|s| matches!(s, Shape::Path(p) if p.stroke.color == egui::epaint::ColorMode::Solid(ON_BRAND))),
                "{theme}: no dark tick"
            );
        }
    }

    #[test]
    fn an_empty_box_keeps_the_brand_colour_out() {
        for theme in [UiTheme::Light, UiTheme::Dark] {
            let shapes = shapes(theme, false);
            assert!(
                !shapes.iter().any(
                    |s| matches!(s, Shape::Rect(r) if r.fill == BRAND || r.fill == BRAND_DARK)
                ),
                "{theme}"
            );
        }
    }
}
