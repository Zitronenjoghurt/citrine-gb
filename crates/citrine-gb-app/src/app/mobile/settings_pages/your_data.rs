use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row};
use crate::app::widgets::settings::your_data::{
    DELETE, EXPORT, Exporting, IMPORT, Request, count_text, size_text,
};
use crate::icons;
use egui::Ui;

impl Citrine {
    pub(super) fn your_data_page(&mut self, ui: &mut Ui) {
        let state = &mut self.ui.your_data;
        ui.add_space(8.0);
        let rows: Vec<(String, String)> = state
            .rows
            .iter()
            .map(|(kind, tally)| {
                let count = match kind.units {
                    ("", "") => kind.noun(tally.count),
                    ("changed", _) => format!("{} changed", kind.noun(tally.count)),
                    _ => count_text(kind, tally.count),
                };
                (count, size_text(tally))
            })
            .collect();
        kit::list(ui, |list| {
            for ((kind, _), (count, size)) in state.rows.iter().zip(&rows) {
                list.row(
                    Row::new(kind.name)
                        .icon(kind.icon)
                        .detail(size)
                        .value(count.clone())
                        .no_chevron(),
                );
            }
        });
        ui.add_space(12.0);
        let busy = state.exporting == Exporting::Busy;
        let export = if busy { "Exporting…" } else { EXPORT };
        let mut request = None;
        kit::list(ui, |list| {
            let row = list.custom(|ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    Row::new(export)
                        .icon(icons::DOWNLOAD_SIMPLE)
                        .no_chevron()
                        .show(ui)
                })
                .inner
            });
            if row.clicked() {
                request = Some(Request::Export);
            }
            if list
                .row(Row::new(IMPORT).icon(icons::UPLOAD_SIMPLE).no_chevron())
                .clicked()
            {
                request = Some(Request::Import);
            }
            if list
                .row(
                    Row::new(&format!("{DELETE} everything"))
                        .icon(icons::TRASH)
                        .danger()
                        .no_chevron(),
                )
                .clicked()
            {
                request = Some(Request::Delete);
            }
        });
        if request.is_some() {
            state.request = request;
        }
    }
}
