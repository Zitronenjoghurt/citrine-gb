use crate::app::widgets::reset_slider::{RESET_GAP, reset_button, reset_room};
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
    middle_ellipsis: bool,
}

#[allow(dead_code)]
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
            middle_ellipsis: false,
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

    pub fn middle_ellipsis(mut self) -> Self {
        self.middle_ellipsis = true;
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
            ui.spacing_mut().item_spacing.x = RESET_GAP;
            let mut width = ui.available_width();
            if self.default_value.is_some() || super::settings::form::keeps_reset_column(ui) {
                width -= reset_room(ui);
            }

            let combo = match self.label {
                Some(label) => egui::ComboBox::new(self.id, label),
                None => egui::ComboBox::from_id_salt(self.id),
            };
            let height = ui.spacing().interact_size.y;
            let spacing = ui.spacing();
            let room =
                width - 2.0 * spacing.button_padding.x - spacing.icon_width - spacing.icon_spacing;
            let font = egui::TextStyle::Button.resolve(ui.style());
            let ctx = ui.ctx().clone();
            let shown = |variant: T| {
                let text = variant.to_string();
                if !self.middle_ellipsis {
                    return text;
                }
                shorten_middle(&text, room, |text| {
                    ctx.fonts_mut(|fonts| {
                        fonts
                            .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                    })
                })
            };
            let variants = self
                .variants
                .into_iter()
                .filter(|&variant| self.filter.as_ref().is_none_or(|keep| keep(variant)))
                .collect::<Vec<_>>();
            let current = ctx.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(self.value.to_string(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            let chrome = ui.available_width() - room;
            if self.middle_ellipsis {
                super::settings::form::shrinking_select_need(ui, current + chrome);
                if let Some(default_value) = self.default_value {
                    let floor = ctx.fonts_mut(|fonts| {
                        fonts
                            .layout_no_wrap(
                                default_value.to_string(),
                                font.clone(),
                                egui::Color32::WHITE,
                            )
                            .size()
                            .x
                    });
                    super::settings::form::select_need(ui, floor + chrome);
                }
            } else {
                let floor = if super::settings::form::selects_shrink(ui) {
                    current.min((super::settings::form::SELECT_MIN - (width - room)).max(0.0))
                } else {
                    current
                };
                super::settings::form::cut_select_need(ui, current + chrome, floor + chrome);
            }
            let full = self.value.to_string();
            let selected = if self.middle_ellipsis {
                shown(*self.value)
            } else {
                shorten_end(&full, room, |text| {
                    ctx.fonts_mut(|fonts| {
                        fonts
                            .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                    })
                })
            };
            let shortened = selected != full || current > room;
            let mut response = ui
                .allocate_ui(egui::vec2(width, height), |ui| {
                    combo
                        .width(width)
                        .truncate()
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for &variant in &variants {
                                let full = variant.to_string();
                                let text = shown(variant);
                                let item = ui.selectable_value(self.value, variant, text.clone());
                                if text != full {
                                    item.on_hover_text(full);
                                }
                            }
                        })
                        .response
                })
                .inner;
            if shortened {
                response = response.on_hover_text(full);
            }

            if let Some(default_value) = self.default_value
                && reset_button(ui, self.value != &default_value, default_value).clicked()
            {
                *self.value = default_value;
            }

            if *self.value != old_value {
                response.mark_changed();
            }

            response
        })
        .inner
    }
}

const FIT_SLACK: f32 = 0.5;

pub(crate) fn shorten_end(text: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    let max = max + FIT_SLACK;
    if text.is_empty() || measure(text) <= max {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    (1..chars.len())
        .rev()
        .map(|keep| {
            let head: String = chars[..keep].iter().collect();
            format!("{}\u{2026}", head.trim_end())
        })
        .find(|short| measure(short) <= max)
        .unwrap_or_else(|| format!("{}\u{2026}", chars[0]))
}

fn shorten_middle(text: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(text) <= max {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let join = |head: &[char], tail: &[char]| {
        let head: String = head.iter().collect();
        let tail: String = tail.iter().collect();
        format!("{}…{}", head.trim_end(), tail.trim_start())
    };
    let extension = chars
        .iter()
        .rposition(|&c| c == '.')
        .filter(|&dot| dot > 1)
        .map_or(0, |dot| chars.len() - dot);
    let stem = chars.len() - extension;
    let finder_end = if extension > 0 { stem } else { 0 };
    let finder = (2..finder_end).rev().map(|keep| {
        let tail = (keep / 4).max(1);
        join(&chars[..keep - tail], &chars[stem - tail..])
    });
    let plain = (1..chars.len())
        .rev()
        .map(|keep| join(&chars[..keep.div_ceil(2)], &chars[chars.len() - keep / 2..]));
    finder
        .chain(plain)
        .find(|short| measure(short) <= max)
        .unwrap_or_else(|| join(&chars[..1], &[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(text: &str) -> f32 {
        text.chars().count() as f32 * 10.0
    }

    #[test]
    fn a_name_that_fits_stays_whole() {
        assert_eq!(shorten_middle("bios.bin", 80.0, measure), "bios.bin");
    }

    #[test]
    fn a_long_name_keeps_its_start_and_its_end() {
        let short = shorten_middle("gba_bios_from_my_own_console.bin", 120.0, measure);
        assert_eq!(short, "gba_bi…e.bin");
        assert!(measure(&short) <= 120.0);
    }

    #[test]
    fn a_shortened_name_keeps_a_letter_before_its_extension() {
        assert_eq!(
            shorten_middle("dmg_boot.bin", 110.0, measure),
            "dmg_b…t.bin"
        );
    }

    #[test]
    fn a_name_without_an_extension_splits_in_the_middle() {
        assert_eq!(shorten_middle("my_own_boot_rom", 70.0, measure), "my_…rom");
    }

    #[test]
    fn no_room_still_shows_the_first_letter() {
        assert_eq!(shorten_middle("gba_bios.bin", 5.0, measure), "g…");
    }

    #[test]
    fn a_value_that_fits_to_a_rounding_stays_whole() {
        let width = measure("Automatic");
        assert_eq!(shorten_end("Automatic", width - 0.01, measure), "Automatic");
    }

    #[test]
    fn a_value_that_fits_stays_whole() {
        assert_eq!(shorten_end("Live camera", 110.0, measure), "Live camera");
    }

    #[test]
    fn a_cut_value_ends_on_a_letter_before_the_ellipsis() {
        assert_eq!(shorten_end("Live camera", 70.0, measure), "Live c…");
        assert_eq!(shorten_end("Live camera", 60.0, measure), "Live…");
        assert_eq!(shorten_end("Live camera", 40.0, measure), "Liv…");
    }
}
