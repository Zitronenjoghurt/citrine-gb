use egui::{Id, ScrollArea, TextStyle, TextWrapMode, Ui, WidgetText};

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
        .height(ui.ctx().content_rect().height())
        .show_ui(ui, |ui| {
            for (tab, text) in tabs {
                ui.selectable_value(current, *tab, text.as_str());
            }
        });
}

pub fn side_tabs<T: Copy + PartialEq>(
    ui: &mut Ui,
    current: &mut T,
    tabs: &[(T, String)],
    spacing: f32,
) {
    ScrollArea::vertical()
        .id_salt("side_tabs_scroll")
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                for (tab, text) in tabs {
                    ui.selectable_value(current, *tab, text.as_str());
                    ui.add_space(spacing);
                }
            });
        });
}

pub fn side_tabs_width<T>(ui: &Ui, tabs: &[(T, String)]) -> f32 {
    tab_widths(ui, tabs).fold(0.0, f32::max)
}

fn row_width<T>(ui: &Ui, tabs: &[(T, String)]) -> f32 {
    let text: f32 = tab_widths(ui, tabs).sum();
    text + ui.spacing().item_spacing.x * tabs.len().saturating_sub(1) as f32
}

fn tab_widths<'a, T>(ui: &'a Ui, tabs: &'a [(T, String)]) -> impl Iterator<Item = f32> + 'a {
    let padding = 2.0 * ui.spacing().button_padding.x;
    tabs.iter().map(move |(_, text)| {
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
}
