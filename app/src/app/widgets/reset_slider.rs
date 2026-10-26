use crate::icons;
use egui::emath::Numeric;
use egui::{Response, TextStyle, TextWrapMode, Ui, Widget, WidgetText, vec2};
use std::fmt::Display;

pub struct ResetSlider<'a, V: Numeric> {
    value: &'a mut V,
    range: std::ops::RangeInclusive<V>,
    default: V,
    step: Option<f64>,
    suffix: Option<&'static str>,
}

impl<'a, V: Numeric> ResetSlider<'a, V> {
    pub fn new(value: &'a mut V, range: std::ops::RangeInclusive<V>) -> Self {
        let default = *value;
        Self {
            value,
            range,
            default,
            step: None,
            suffix: None,
        }
    }

    pub fn default_value(mut self, default: V) -> Self {
        self.default = default;
        self
    }

    pub fn step_by(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    pub fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = Some(suffix);
        self
    }
}

impl<V: Numeric> Widget for ResetSlider<'_, V> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = RESET_GAP;

            let decimals = self
                .step
                .map_or(0, |step| (-step.log10()).ceil().max(0.0) as usize);
            let suffix = self.suffix.unwrap_or_default();
            let widest = format!("{:.*}{suffix}", decimals, self.range.end().to_f64());
            let value_width = WidgetText::from(widest)
                .into_galley(
                    ui,
                    Some(TextWrapMode::Extend),
                    f32::INFINITY,
                    ui.style().drag_value_text_style.clone(),
                )
                .size()
                .x
                + 2.0 * ui.spacing().button_padding.x;

            let reset_width = reset_width(ui);
            ui.spacing_mut().slider_width =
                (ui.available_width() - value_width - reset_width - 2.0 * RESET_GAP)
                    .max(ui.spacing().interact_size.y);
            ui.spacing_mut().interact_size.x = value_width;

            let mut slider = egui::Slider::new(self.value, self.range)
                .fixed_decimals(decimals)
                .suffix(suffix);
            if let Some(step) = self.step {
                slider = slider.step_by(step);
            }
            let mut response = slider.ui(ui);

            let is_default = self.value.to_f64() == self.default.to_f64();
            if reset_button(ui, !is_default, self.default.to_f64()).clicked() {
                *self.value = self.default;
                response.mark_changed();
            }

            response
        })
        .inner
    }
}

pub const RESET_GAP: f32 = 4.0;

pub fn reset_width(ui: &Ui) -> f32 {
    let icon = WidgetText::from(icons::ARROW_COUNTER_CLOCKWISE).into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    (icon.size().x + 2.0 * ui.spacing().button_padding.x).max(crate::app::mobile::touch_square(ui))
}

pub fn reset_button(ui: &mut Ui, enabled: bool, default: impl Display) -> Response {
    let width = reset_width(ui);
    let button = crate::app::mobile::compact(ui, egui::Button::new(icons::ARROW_COUNTER_CLOCKWISE))
        .min_size(vec2(width, 0.0));
    ui.add_enabled(enabled, button)
        .on_hover_text(format!("Reset to {default}"))
}
