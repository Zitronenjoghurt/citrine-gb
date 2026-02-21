use crate::app::panels::PanelKind;
use strum::IntoEnumIterator;

pub struct PanelMenu<'a> {
    icon: &'static str,
    slot: &'a mut Option<PanelKind>,
}

impl<'a> PanelMenu<'a> {
    pub fn new(icon: &'static str, slot: &'a mut Option<PanelKind>) -> Self {
        Self { icon, slot }
    }
}

impl egui::Widget for PanelMenu<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.menu_button(self.icon, |ui| {
            if ui.selectable_label(self.slot.is_none(), "None").clicked() {
                *self.slot = None;
                ui.close_kind(egui::UiKind::Menu);
            }
            for kind in PanelKind::iter() {
                if ui
                    .selectable_label(*self.slot == Some(kind), kind.label())
                    .clicked()
                {
                    *self.slot = Some(kind);
                    ui.close_kind(egui::UiKind::Menu);
                }
            }
        })
        .response
    }
}
