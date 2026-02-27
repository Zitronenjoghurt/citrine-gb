use citrine_gb::gb::ppu::types::theme::DmgTheme;
use egui::{Response, Ui, Widget};

pub struct ThemeSelector<'a> {
    theme: &'a mut DmgTheme,
}

impl<'a> ThemeSelector<'a> {
    pub fn new(theme: &'a mut DmgTheme) -> Self {
        Self { theme }
    }
}

impl Widget for ThemeSelector<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        egui::ComboBox::from_id_salt("theme_selector")
            .selected_text(self.theme.to_string())
            .show_ui(ui, |ui| {
                for theme in DmgTheme::SELECTABLE {
                    ui.selectable_value(self.theme, *theme, theme.to_string());
                }
            })
            .response
    }
}
