use egui::{Id, TextStyle, TextWrapMode, Ui, WidgetText};

pub fn tab_bar<T: Copy + PartialEq>(ui: &mut Ui, current: &mut T, tabs: &[(T, String)]) {
    let available = ui.available_width();
    if row_width(ui, tabs) <= available {
        ui.horizontal(|ui| {
            for (tab, text) in tabs {
                ui.selectable_value(current, *tab, text.as_str());
            }
        });
        return;
    }

    let selected = tabs
        .iter()
        .find(|(tab, _)| tab == current)
        .map(|(_, text)| text.as_str())
        .unwrap_or_default();
    let id = Id::new("tab_bar").with(tabs.iter().map(|(_, text)| text).collect::<Vec<_>>());
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(available)
        .show_ui(ui, |ui| {
            for (tab, text) in tabs {
                ui.selectable_value(current, *tab, text.as_str());
            }
        });
}

fn row_width<T>(ui: &Ui, tabs: &[(T, String)]) -> f32 {
    let padding = 2.0 * ui.spacing().button_padding.x;
    let text: f32 = tabs
        .iter()
        .map(|(_, text)| {
            WidgetText::from(text.as_str())
                .into_galley(
                    ui,
                    Some(TextWrapMode::Extend),
                    f32::INFINITY,
                    TextStyle::Button,
                )
                .size()
                .x
                + padding
        })
        .sum();
    text + ui.spacing().item_spacing.x * tabs.len().saturating_sub(1) as f32
}
