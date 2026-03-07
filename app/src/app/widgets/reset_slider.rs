use crate::icons;
use egui::emath::Numeric;
use egui::{Response, Ui, Widget};

pub struct ResetSlider<'a, V: Numeric> {
    value: &'a mut V,
    range: std::ops::RangeInclusive<V>,
    default: V,
    step: Option<f64>,
}

impl<'a, V: Numeric> ResetSlider<'a, V> {
    pub fn new(value: &'a mut V, range: std::ops::RangeInclusive<V>) -> Self {
        let default = *value;
        Self {
            value,
            range,
            default,
            step: None,
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
}

impl<V: Numeric> Widget for ResetSlider<'_, V> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            let mut slider = egui::Slider::new(self.value, self.range);
            if let Some(step) = self.step {
                slider = slider.step_by(step);
            }
            let mut response = slider.ui(ui);

            let is_default = self.value.to_f64() == self.default.to_f64();
            if ui
                .add_enabled(
                    !is_default,
                    egui::Button::new(icons::ARROW_COUNTER_CLOCKWISE).small(),
                )
                .on_hover_text(format!("Reset to {}", self.default.to_f64()))
                .clicked()
            {
                *self.value = self.default;
                response.mark_changed();
            }

            response
        })
        .inner
    }
}
