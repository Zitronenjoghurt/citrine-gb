use crate::icons;
use egui::{Ui, Widget};
use std::fmt::Display;
use strum::IntoEnumIterator;

pub struct EnumSelect<'a, T>
where
    T: IntoEnumIterator + PartialEq + Copy + Display,
{
    value: &'a mut T,
    label: Option<&'a str>,
    id: &'a str,
    default_value: Option<T>,
}

impl<'a, T> EnumSelect<'a, T>
where
    T: IntoEnumIterator + PartialEq + Copy + Display,
{
    pub fn new(value: &'a mut T, id: &'a str) -> Self {
        Self {
            value,
            label: None,
            id,
            default_value: None,
        }
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    pub fn default_value(mut self, default_value: T) -> Self {
        self.default_value = Some(default_value);
        self
    }
}

impl<T> Widget for EnumSelect<'_, T>
where
    T: IntoEnumIterator + PartialEq + Copy + Display,
{
    fn ui(self, ui: &mut Ui) -> egui::Response {
        let old_value = *self.value;

        ui.horizontal(|ui| {
            let mut response = egui::ComboBox::new(self.id, self.label.unwrap_or_default())
                .selected_text(self.value.to_string())
                .show_ui(ui, |ui| {
                    for variant in T::iter() {
                        ui.selectable_value(self.value, variant, variant.to_string());
                    }
                })
                .response;

            if let Some(default_value) = self.default_value {
                let is_default = self.value == &default_value;
                if ui
                    .add_enabled(
                        !is_default,
                        egui::Button::new(icons::ARROW_COUNTER_CLOCKWISE).small(),
                    )
                    .on_hover_text(format!("Reset to {}", default_value))
                    .clicked()
                {
                    *self.value = default_value;
                }
            }

            if *self.value != old_value {
                response.mark_changed();
            }

            response
        })
        .inner
    }
}
