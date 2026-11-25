use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::search::search;
use crate::app::ui_state::settings_tree::{Section, SettingsTab, Surface};
use crate::app::widgets::field::row_field;
use crate::icons;
use egui::scroll_area::ScrollSource;
use egui::{
    Align, Button, Key, Layout, RichText, ScrollArea, Sense, TextStyle, TextWrapMode, Ui,
    WidgetText,
};
use strum::IntoEnumIterator;

pub const NARROW_BELOW: f32 = 420.0;
const SECTION_GAP: f32 = 12.0;
const TITLE_GAP: f32 = 4.0;
const ROW_GAP: f32 = 2.0;
const NO_DRAG: ScrollSource = ScrollSource {
    scroll_bar: true,
    drag: false,
    mouse_wheel: true,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Page(SettingsTab),
    Row(RowId),
}

pub struct PageList<'a> {
    selected: Option<SettingsTab>,
    dev_mode: bool,
    query: &'a mut String,
    focus: bool,
    hits_here: bool,
}

impl<'a> PageList<'a> {
    pub fn new(selected: Option<SettingsTab>, dev_mode: bool, query: &'a mut String) -> Self {
        Self {
            selected,
            dev_mode,
            query,
            focus: false,
            hits_here: true,
        }
    }

    pub fn hits_elsewhere(mut self) -> Self {
        self.hits_here = false;
        self
    }

    pub fn focus(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Option<Pick> {
        let field = ui.add(
            row_field(ui, self.query)
                .hint_text(hint())
                .desired_width(ui.available_width()),
        );
        if self.focus {
            field.request_focus();
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.query.clear();
        }
        ui.add_space(SECTION_GAP);
        let query_on = !self.query.trim().is_empty();
        let searching = self.hits_here && query_on;
        let entered = query_on && field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let hits = if searching || entered {
            found(self.query, self.dev_mode)
        } else {
            Vec::new()
        };
        let first = hits.first().and_then(|(_, rows)| rows.first()).copied();
        let mut picked = first.filter(|_| entered).map(Pick::Row);
        ScrollArea::vertical()
            .id_salt("settings_page_list")
            .scroll_source(if query_on && !self.hits_here {
                ScrollSource::MOUSE_WHEEL
            } else {
                NO_DRAG
            })
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    let clicked = if searching {
                        hit_rows(ui, &hits).map(Pick::Row)
                    } else {
                        pages(ui, self.selected, self.dev_mode, !query_on).map(Pick::Page)
                    };
                    picked = picked.or(clicked);
                });
            });
        picked
    }
}

fn pages(
    ui: &mut Ui,
    selected: Option<SettingsTab>,
    dev_mode: bool,
    focusable: bool,
) -> Option<SettingsTab> {
    let sense = if focusable {
        Sense::click()
    } else {
        Sense::CLICK
    };
    let mut picked = None;
    for (index, (section, pages)) in sections(dev_mode).enumerate() {
        if index > 0 {
            ui.add_space(SECTION_GAP);
        }
        ui.label(RichText::new(section.title()).small().weak());
        ui.add_space(TITLE_GAP);
        for page in pages {
            let label = text(page);
            if ui
                .add(
                    Button::selectable(selected == Some(page), &label)
                        .truncate()
                        .sense(sense),
                )
                .on_hover_text(&label)
                .clicked()
            {
                picked = Some(page);
            }
            ui.add_space(ROW_GAP);
        }
    }
    picked
}

fn hit_rows(ui: &mut Ui, hits: &[(SettingsTab, Vec<RowId>)]) -> Option<RowId> {
    if hits.is_empty() {
        ui.label(RichText::new("No settings found").weak());
        return None;
    }
    let mut picked = None;
    for (index, (page, rows)) in hits.iter().enumerate() {
        if index > 0 {
            ui.add_space(SECTION_GAP);
        }
        ui.label(RichText::new(text(*page)).small().weak());
        ui.add_space(TITLE_GAP);
        for row in rows {
            let label = row.label(Surface::Desktop);
            if ui
                .add(Button::selectable(false, label).truncate())
                .on_hover_text(label)
                .clicked()
            {
                picked = Some(*row);
            }
            ui.add_space(ROW_GAP);
        }
    }
    picked
}

pub fn hits_pane(ui: &mut Ui, query: &str, dev_mode: bool) -> Option<RowId> {
    let hits = found(query, dev_mode);
    ScrollArea::vertical()
        .id_salt("settings_hits")
        .scroll_source(NO_DRAG)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_max_width(ui.available_width().min(super::content::MAX_COLUMN_WIDTH));
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    hit_rows(ui, &hits)
                })
                .inner
            })
            .inner
        })
        .inner
}

pub fn found(query: &str, dev_mode: bool) -> Vec<(SettingsTab, Vec<RowId>)> {
    by_page(search(query, Surface::Desktop, dev_mode))
}

pub fn by_page(
    hits: Vec<crate::app::ui_state::settings_tree::search::Hit>,
) -> Vec<(SettingsTab, Vec<RowId>)> {
    let mut pages: Vec<(SettingsTab, Vec<RowId>)> = Vec::new();
    for hit in hits {
        match pages.iter_mut().find(|(page, _)| *page == hit.page) {
            Some((_, rows)) => rows.push(hit.row),
            None => pages.push((hit.page, vec![hit.row])),
        }
    }
    pages
}

pub fn width(ui: &Ui, dev_mode: bool) -> f32 {
    let padding = 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x;
    sections(dev_mode)
        .flat_map(|(_, pages)| pages)
        .map(text)
        .chain([hint()])
        .map(|text| {
            WidgetText::from(text)
                .into_galley(
                    ui,
                    Some(TextWrapMode::Extend),
                    f32::INFINITY,
                    TextStyle::Button,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max)
        + padding
}

fn hint() -> String {
    format!("{} Search settings", icons::MAGNIFYING_GLASS)
}

pub fn text(page: SettingsTab) -> String {
    format!("{} {}", page.icon(), page.title())
}

fn sections(dev_mode: bool) -> impl Iterator<Item = (Section, Vec<SettingsTab>)> {
    Section::iter().filter_map(move |section| {
        let pages: Vec<SettingsTab> = section
            .pages()
            .iter()
            .copied()
            .filter(|page| page.available(dev_mode))
            .collect();
        (!pages.is_empty()).then_some((section, pages))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(dev_mode: bool) -> Vec<SettingsTab> {
        sections(dev_mode).flat_map(|(_, pages)| pages).collect()
    }

    #[test]
    fn the_list_shows_every_page_under_its_section_and_developer_only_in_dev_mode() {
        assert!(!listed(false).contains(&SettingsTab::Developer));
        assert_eq!(listed(true).last(), Some(&SettingsTab::Developer));
        assert_eq!(listed(true).first(), Some(&SettingsTab::Screen));
        assert_eq!(
            listed(true).contains(&SettingsTab::Ds),
            cfg!(feature = "nds")
        );
        for (section, pages) in sections(true) {
            for page in pages {
                assert_eq!(page.section(), section, "{page:?}");
            }
        }
    }

    #[test]
    fn the_list_names_its_sections_and_picks_a_page_on_click() {
        let ctx = egui::Context::default();
        let input = |pointer: Option<egui::Pos2>, pressed: bool| {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(220.0, 900.0),
                )),
                ..Default::default()
            };
            if let Some(pos) = pointer {
                input.events.push(egui::Event::PointerMoved(pos));
                input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
            input
        };
        let mut texts = Vec::new();
        let mut picked = None;
        let mut run = |ctx: &egui::Context, raw: egui::RawInput, texts: &mut Vec<_>| {
            let output = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut query = String::new();
                    if let Some(Pick::Page(page)) =
                        PageList::new(Some(SettingsTab::Screen), false, &mut query).show(ui)
                    {
                        picked = Some(page);
                    }
                });
            });
            *texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.rect.translate(text.pos.to_vec2()),
                        text.galley.text().to_owned(),
                    )),
                    _ => None,
                })
                .collect::<Vec<(egui::Rect, String)>>();
        };
        run(&ctx, input(None, false), &mut texts);
        for section in Section::iter() {
            assert!(
                texts.iter().any(|(_, text)| text == section.title()),
                "{section:?}: {texts:?}"
            );
        }
        let sound = texts
            .iter()
            .find(|(_, text)| *text == super::text(SettingsTab::Sound))
            .map(|(rect, _)| rect.center())
            .expect("a Sound row");
        run(&ctx, input(Some(sound), true), &mut texts);
        run(&ctx, input(Some(sound), false), &mut texts);
        assert_eq!(picked, Some(SettingsTab::Sound));
    }

    #[test]
    fn enter_in_the_search_opens_the_first_hit_even_when_hits_show_elsewhere() {
        for hits_here in [true, false] {
            let ctx = egui::Context::default();
            let mut query = "fast".to_owned();
            let mut picked = None;
            for frame in 0..3 {
                let mut raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(220.0, 900.0),
                    )),
                    ..Default::default()
                };
                if frame == 2 {
                    raw.events.push(egui::Event::Key {
                        key: Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    });
                }
                let _ = ctx.run(raw, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let mut list = PageList::new(Some(SettingsTab::Sound), false, &mut query)
                            .focus(frame == 0);
                        if !hits_here {
                            list = list.hits_elsewhere();
                        }
                        if let Some(pick) = list.show(ui) {
                            picked = Some(pick);
                        }
                    });
                });
            }
            let first = found("fast", false)[0].1[0];
            assert_eq!(picked, Some(Pick::Row(first)), "hits_here {hits_here}");
        }
    }
}
