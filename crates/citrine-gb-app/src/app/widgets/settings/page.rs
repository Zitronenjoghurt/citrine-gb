use crate::app::widgets::info_mark::info_mark;
use egui::{Align, Id, Layout, Rect, RichText, Separator, Ui, WidgetText};

pub const PAGE_EDGE: f32 = 12.0;
pub const SECTION_GAP: f32 = 24.0;
pub const HEADING_GAP: f32 = 6.0;
pub const NOTE_GAP: f32 = 4.0;
pub const PHONE_INDENT: f32 = 6.0;
pub const STACK_BELOW: f32 = 190.0;

fn active_key() -> Id {
    Id::new("settings_page_column")
}

pub fn column(ctx: &egui::Context) -> Option<Id> {
    ctx.data(|d| d.get_temp::<Option<Id>>(active_key()))
        .flatten()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Section,
    Heading,
    Row,
    Note,
}

#[cfg(test)]
thread_local! {
    static MARKS: std::cell::RefCell<Vec<(Mark, Rect)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub fn mark(kind: Mark, rect: Rect) {
    MARKS.with(|marks| marks.borrow_mut().push((kind, rect)));
}

#[cfg(not(test))]
pub fn mark(_kind: Mark, _rect: Rect) {}

#[cfg(test)]
fn take_marks() -> Vec<(Mark, Rect)> {
    MARKS.with(|marks| std::mem::take(&mut *marks.borrow_mut()))
}

#[derive(Clone, Copy)]
pub struct Heading<'a> {
    icon: &'a str,
    title: &'a str,
    hint: Option<&'a str>,
    aside: Option<&'a str>,
}

impl<'a> Heading<'a> {
    pub fn new(icon: &'a str, title: &'a str) -> Self {
        Self {
            icon,
            title,
            hint: None,
            aside: None,
        }
    }

    pub fn aside(mut self, aside: &'a str) -> Self {
        self.aside = Some(aside);
        self
    }

    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    fn show(self, ui: &mut Ui, phone: bool) {
        let top = ui.cursor().top();
        let mut lift = 0.0;
        if phone {
            let overhang = ui
                .scope(|ui| {
                    if self.hint.is_none() {
                        ui.spacing_mut().interact_size.y = 0.0;
                    }
                    crate::app::mobile::kit::section_title(ui, self.title, self.hint)
                })
                .inner;
            ui.add_space(-overhang);
            lift = overhang;
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.icon).size(15.0));
                ui.label(RichText::new(self.title).strong().size(15.0));
                if let Some(hint) = self.hint {
                    info_mark(ui, hint);
                }
                if let Some(aside) = self.aside {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(aside).weak());
                    });
                }
            });
            ui.add(Separator::default().spacing(HEADING_GAP));
        }
        mark(
            Mark::Heading,
            Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.min_rect().bottom() - lift),
        );
    }
}

pub struct Page<'u> {
    ui: &'u mut Ui,
    spacing: f32,
    phone: bool,
    sections: usize,
    outer: Option<Id>,
}

impl<'u> Page<'u> {
    pub fn show<R>(ui: &mut Ui, body: impl FnOnce(&mut Page) -> R) -> R {
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            let mut page = Page::begin(ui);
            let inner = body(&mut page);
            page.end();
            inner
        })
        .inner
    }

    pub fn begin(ui: &'u mut Ui) -> Self {
        let phone = crate::app::mobile::touch_square(ui) > 0.0;
        let spacing = ui.spacing().item_spacing.y;
        let outer = column(ui.ctx());
        let column_id = ui.id().with("settings_page");
        ui.data_mut(|d| d.insert_temp(active_key(), Some(column_id)));
        ui.spacing_mut().item_spacing.y = 0.0;
        if phone {
            ui.add_space(PAGE_EDGE);
        }
        Page {
            ui,
            spacing,
            phone,
            sections: 0,
            outer,
        }
    }

    pub fn end(self) {
        let bottom = if self.phone {
            (PAGE_EDGE + crate::app::mobile::kit::TITLE_GAP - crate::app::mobile::kit::SIDE)
                .max(0.0)
        } else {
            PAGE_EDGE
        };
        self.ui.add_space(bottom);
        self.ui.spacing_mut().item_spacing.y = self.spacing;
        let outer = self.outer;
        self.ui.data_mut(|d| d.insert_temp(active_key(), outer));
    }

    pub fn phone(&self) -> bool {
        self.phone
    }

    pub fn section<R>(&mut self, heading: Heading<'_>, body: impl FnOnce(&mut Ui) -> R) -> R {
        self.add(Some(heading), body)
    }

    pub fn plain<R>(&mut self, body: impl FnOnce(&mut Ui) -> R) -> R {
        self.add(None, body)
    }

    pub fn boxed<R>(&mut self, heading: Option<Heading<'_>>, body: impl FnOnce(&mut Ui) -> R) -> R {
        if !self.phone {
            return self.add(heading, body);
        }
        let spacing = self.spacing;
        self.add(heading, |ui| {
            crate::app::mobile::kit::card(ui, |ui| {
                ui.spacing_mut().item_spacing.y = spacing;
                crate::app::mobile::kit::padded(ui, body)
            })
        })
    }

    pub fn group<R>(&mut self, heading: Option<Heading<'_>>, body: impl FnOnce(&mut Ui) -> R) -> R {
        self.add(heading, body)
    }

    fn add<R>(&mut self, heading: Option<Heading<'_>>, body: impl FnOnce(&mut Ui) -> R) -> R {
        if self.sections > 0 {
            self.ui.add_space(SECTION_GAP);
        }
        self.sections += 1;
        let top = self.ui.cursor().top();
        if let Some(heading) = heading {
            heading.show(self.ui, self.phone);
            if self.phone {
                self.ui.add_space(HEADING_GAP);
            }
        }
        let spacing = self.spacing;
        let inner = self
            .ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = spacing;
                body(ui)
            })
            .inner;
        mark(
            Mark::Section,
            Rect::from_x_y_ranges(
                self.ui.max_rect().x_range(),
                top..=self.ui.min_rect().bottom(),
            ),
        );
        inner
    }

    pub fn note(&mut self, text: impl Into<WidgetText>) {
        self.ui.add_space(NOTE_GAP);
        note_line(self.ui, self.phone, text.into(), None);
    }

    pub fn footer(&mut self, text: impl Into<WidgetText>, hint: Option<&str>) {
        let phone = self.phone;
        self.add(None, |ui| note_line(ui, phone, text.into(), hint));
    }
}

fn note_line(ui: &mut Ui, phone: bool, text: WidgetText, hint: Option<&str>) {
    let indent = if phone { PHONE_INDENT } else { 0.0 };
    let top = ui.cursor().top();
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.add(egui::Label::new(note_text(text, phone)).wrap());
        if let Some(hint) = hint {
            info_mark(ui, hint);
        }
    });
    mark(
        Mark::Note,
        Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.min_rect().bottom()),
    );
}

fn note_text(text: WidgetText, phone: bool) -> WidgetText {
    let text = match text {
        WidgetText::Text(text) => RichText::new(text),
        WidgetText::RichText(text) => (*text).clone(),
        other => return other,
    };
    let text = text.weak();
    if phone { text.size(13.0) } else { text }.into()
}

#[cfg(test)]
pub mod tests {
    use super::{Mark, SECTION_GAP, take_marks};
    use egui::{Context, Rect};

    pub fn quiet(ctx: &Context) {
        ctx.options_mut(|o| o.max_passes = std::num::NonZeroUsize::MIN);
        ctx.style_mut(|s| s.interaction.selectable_labels = false);
    }

    pub type Laid = (Vec<(Mark, Rect)>, Vec<Rect>);

    pub fn after_pass(ctx: &Context) -> Laid {
        let controls = ctx.viewport(|v| {
            v.this_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .filter(|w| w.sense.senses_click() || w.sense.senses_drag())
                .map(|w| w.rect)
                .filter(|rect| rect.height() < 200.0 && rect.is_positive())
                .collect()
        });
        (take_marks(), controls)
    }

    pub fn assert_no_empty_cards(name: &str, marks: &[(Mark, Rect)]) {
        for (_, section) in marks.iter().filter(|(kind, _)| *kind == Mark::Section) {
            let tallest = marks
                .iter()
                .filter(|(kind, rect)| {
                    matches!(kind, Mark::Row | Mark::Note) && section.contains(rect.center())
                })
                .map(|(_, rect)| rect.height())
                .fold(0.0, f32::max);
            assert!(
                tallest >= 24.0,
                "{name}: the section at {section:?} has no row with content"
            );
        }
    }

    pub fn assert_unified(name: &str, marks: &[(Mark, Rect)], controls: &[Rect]) {
        let mut sections: Vec<Rect> = marks
            .iter()
            .filter(|(kind, _)| *kind == Mark::Section)
            .map(|(_, rect)| *rect)
            .collect();
        sections.sort_by(|a, b| a.top().total_cmp(&b.top()));
        assert!(!sections.is_empty(), "{name} draws no sections");
        let page = sections
            .iter()
            .fold(Rect::NOTHING, |page, section| page.union(*section));
        let holders: Vec<Rect> = marks
            .iter()
            .filter(|(kind, _)| *kind != Mark::Section)
            .map(|(_, rect)| *rect)
            .collect();
        for control in controls.iter().filter(|c| page.intersects(**c)) {
            assert!(
                holders
                    .iter()
                    .any(|holder| holder.contains(control.center())),
                "{name}: a control at {control:?} sits outside the shared rows"
            );
        }
        for pair in sections.windows(2) {
            let gap = pair[1].top() - pair[0].bottom();
            assert!(
                (gap - SECTION_GAP).abs() < 0.5,
                "{name}: {gap} pt between the sections at {:?} and {:?}",
                pair[0],
                pair[1]
            );
        }
        for (_, heading) in marks.iter().filter(|(kind, _)| *kind == Mark::Heading) {
            let below = marks
                .iter()
                .filter(|(kind, rect)| {
                    *kind != Mark::Heading
                        && *kind != Mark::Section
                        && rect.top() >= heading.bottom() - 0.5
                })
                .map(|(_, rect)| rect.top() - heading.bottom())
                .fold(f32::INFINITY, f32::min);
            assert!(
                below < SECTION_GAP / 2.0,
                "{name}: the heading at {heading:?} sits {below} pt above its rows"
            );
        }
    }
}
