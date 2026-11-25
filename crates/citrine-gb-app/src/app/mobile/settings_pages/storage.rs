use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row};
use crate::app::widgets::progress::ProgressBar;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::app::widgets::settings::storage::{
    CLEAR, CLEAR_ALL, Request, StorageUi, counted_size, section_icon,
};
use crate::app::widgets::settings::your_data::{
    self as your_data_ui, DELETE, DELETE_SHORT, EXPORT, IMPORT, YourDataUi,
};
use crate::icons;
use crate::storage::kinds::{Kind, Section};
use egui::Ui;
use strum::IntoEnumIterator;

impl Citrine {
    pub(super) fn storage_page(&mut self, ui: &mut Ui) {
        let state = &mut self.ui.storage;
        let data = &mut self.ui.your_data;
        let mut request = None;
        let mut data_request = None;
        let mut page = Page::begin(ui);
        for section in Section::iter() {
            let total = state.total_text(section);
            page.section(
                Heading::new(section_icon(section), section.title())
                    .aside(&total)
                    .aside_inset(kit::TRAILING_VALUE_END),
                |ui| {
                    kit::list(ui, |list| {
                        for kind in state.rows(section) {
                            let counted = counted_size(state, data, kind);
                            if let Some(clear) = storage_row(list, state, kind, counted, true) {
                                request = Some(clear);
                            }
                        }
                        match section {
                            Section::Data => {
                                if let Some(asked) = data_rows(list, data) {
                                    data_request = Some(asked);
                                }
                            }
                            Section::Cache => {
                                let empty = state.total(Section::Cache) == 0;
                                let clear_all = Row::new(CLEAR_ALL)
                                    .icon(icons::BROOM)
                                    .no_chevron()
                                    .press(CLEAR_ALL);
                                if list
                                    .custom(|ui| ui.add_enabled_ui(!empty, |ui| clear_all.show(ui)))
                                    .inner
                                    .clicked()
                                {
                                    request = Some(Request::ClearAll);
                                }
                            }
                        }
                    })
                },
            );
            if section == Section::Data
                && let Some((label, progress)) = your_data_ui::load(data)
            {
                page.boxed(None, |ui| ui.add(ProgressBar::new(&label, progress)));
            }
        }
        page.end();
        if request.is_some() {
            state.request = request;
        }
        if data_request.is_some() {
            data.request = data_request;
        }
    }
}

fn data_rows(list: &mut kit::List<'_>, data: &YourDataUi) -> Option<your_data_ui::Request> {
    let busy = your_data_ui::loading(data);
    let empty = your_data_ui::nothing_stored(data);
    let mut request = None;
    let row = list.custom(|ui| {
        ui.add_enabled_ui(!busy && !empty, |ui| {
            Row::new(EXPORT)
                .icon(icons::DOWNLOAD_SIMPLE)
                .no_chevron()
                .press(EXPORT)
                .show(ui)
        })
        .inner
    });
    if row.clicked() {
        request = Some(your_data_ui::Request::Export);
    }
    let row = list.custom(|ui| {
        ui.add_enabled_ui(!busy, |ui| {
            Row::new(IMPORT)
                .icon(icons::UPLOAD_SIMPLE)
                .no_chevron()
                .press(IMPORT)
                .show(ui)
        })
        .inner
    });
    if row.clicked() {
        request = Some(your_data_ui::Request::Import);
    }
    let row = list.custom(|ui| {
        let row = ui
            .add_enabled_ui(!empty, |ui| {
                Row::new(DELETE)
                    .icon(icons::TRASH)
                    .danger()
                    .no_chevron()
                    .press(DELETE_SHORT)
                    .show(ui)
            })
            .inner;
        crate::app::widgets::settings::spotlight::mark(ui, DELETE, row.rect);
        row
    });
    if row.clicked() {
        request = Some(your_data_ui::Request::Delete);
    }
    request
}

pub(super) fn storage_row(
    list: &mut kit::List<'_>,
    state: &StorageUi,
    kind: Kind,
    counted: Option<String>,
    icon: bool,
) -> Option<Request> {
    let bytes = state.bytes(kind);
    let note = state
        .kept
        .filter(|(kept, _)| *kept == kind)
        .map(|(_, note)| note)
        .unwrap_or("");
    let mut row = Row::new(kind.name()).detail(note);
    row = match counted {
        Some(counted) => row.value(counted).fallback(state.size_text(kind)),
        None => row.value(state.size_text(kind)),
    };
    if icon {
        row = row.icon(kind.icon());
    }
    if let Some(hint) = kind.hint() {
        row = row.info(hint);
    }
    if kind.section() == Section::Data {
        list.row(row.trailing_space());
        return None;
    }
    let row = row.trailing(icons::BROOM);
    if bytes == 0 {
        list.row(row.trailing_off());
        return None;
    }
    list.row(row.press(CLEAR))
        .clicked()
        .then_some(Request::Clear(kind))
}
