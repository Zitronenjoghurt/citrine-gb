use crate::icons;
use egui::{Ui, Widget};
use std::fmt::Display;

pub struct GenericSelect<'a, T, V>
where
    T: PartialEq + Copy + Display,
    V: IntoIterator<Item = T>,
{
    value: &'a mut T,
    variants: V,
    label: Option<&'a str>,
    id: &'a str,
    default_value: Option<T>,
    filter: Option<Box<dyn Fn(T) -> bool + 'a>>,
}

impl<'a, T, V> GenericSelect<'a, T, V>
where
    T: PartialEq + Copy + Display,
    V: IntoIterator<Item = T>,
{
    pub fn new(value: &'a mut T, variants: V, id: &'a str) -> Self {
        Self {
            value,
            variants,
            label: None,
            id,
            default_value: None,
            filter: None,
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

    pub fn filter(mut self, predicate: impl Fn(T) -> bool + 'a) -> Self {
        self.filter = Some(Box::new(predicate));
        self
    }
}

impl<'a, T> GenericSelect<'a, T, <T as strum::IntoEnumIterator>::Iterator>
where
    T: strum::IntoEnumIterator + PartialEq + Copy + Display,
{
    pub fn from_enum(value: &'a mut T, id: &'a str) -> Self {
        Self::new(value, T::iter(), id)
    }
}

impl<T, V> Widget for GenericSelect<'_, T, V>
where
    T: PartialEq + Copy + Display,
    V: IntoIterator<Item = T>,
{
    fn ui(self, ui: &mut Ui) -> egui::Response {
        let old_value = *self.value;

        ui.horizontal(|ui| {
            let mut response = egui::ComboBox::new(self.id, self.label.unwrap_or_default())
                .selected_text(self.value.to_string())
                .show_ui(ui, |ui| {
                    for variant in self.variants {
                        if let Some(ref filter_fn) = self.filter
                            && !filter_fn(variant)
                        {
                            continue;
                        }
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
