use crate::app::widgets::action::icon_button;
use crate::app::widgets::memory::{access_icon, span_label};
use crate::icons;
use citrine_gb::debug::breakpoints::{Access, Watch};
use citrine_gb::debug::code::Address;
use citrine_gb::debug::step::Stop;
use egui::{
    Align, Align2, CollapsingHeader, FontId, Id, Layout, Pos2, Rect, RichText, Sense, TextStyle,
    Ui, vec2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Code(Address),
    Watch(Watch),
}

impl Entry {
    fn label(&self) -> String {
        match self {
            Entry::Code(at) => at.to_string(),
            Entry::Watch(watch) => span_label(watch.from, watch.len),
        }
    }

    fn hint(&self) -> &'static str {
        match self {
            Entry::Code(_) => "Stops when this code runs",
            Entry::Watch(watch) => match watch.access {
                Access::Write => "Stops when written",
                Access::Read => "Stops when read",
                Access::Change => "Stops when the value changes",
            },
        }
    }

    pub fn stopped(&self, stop: Option<Stop>) -> bool {
        match (self, stop) {
            (Entry::Code(at), Some(Stop::Breakpoint(hit))) => *at == hit,
            (Entry::Watch(watch), Some(Stop::Watchpoint { at, access, .. })) => {
                watch.access == access && watch.covers(at)
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryAction {
    Remove(Entry),
    Show(Entry),
}

const TOP_ROWS: usize = 2;

pub struct ListOutput {
    pub action: Option<EntryAction>,
    pub top_rows_bottom: f32,
}

pub struct BreakpointList<'a> {
    entries: &'a [Entry],
    last: Option<Stop>,
}

impl<'a> BreakpointList<'a> {
    pub fn new(entries: &'a [Entry], last: Option<Stop>) -> Self {
        Self { entries, last }
    }

    pub fn show(self, ui: &mut Ui) -> ListOutput {
        let mut action = None;
        let mut top_rows = None;
        let seen = Id::new("breakpoint_list_stop");
        let fresh = ui.data(|data| data.get_temp::<Option<Stop>>(seen)) != Some(self.last);
        let shown = CollapsingHeader::new(RichText::new("Breakpoints").strong())
            .id_salt("breakpoint_list")
            .default_open(true)
            .show(ui, |ui| {
                for (i, entry) in self.entries.iter().enumerate() {
                    if let Some(picked) = self.row(ui, entry, fresh) {
                        action = Some(picked);
                    }
                    if i < TOP_ROWS {
                        top_rows = Some(ui.min_rect().bottom());
                    }
                }
            });
        ui.data_mut(|data| data.insert_temp(seen, self.last));
        ListOutput {
            action,
            top_rows_bottom: top_rows.unwrap_or(shown.header_response.rect.bottom()),
        }
    }

    fn row(&self, ui: &mut Ui, entry: &Entry, fresh: bool) -> Option<EntryAction> {
        let side = ui.spacing().interact_size.y;
        let font = TextStyle::Monospace.resolve(ui.style());
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), side), Sense::click());
        let stopped = entry.stopped(self.last);
        if stopped && fresh {
            response.scroll_to_me(None);
        }
        let visuals = ui.visuals();
        let fill = if stopped {
            Some(visuals.selection.bg_fill)
        } else if response.hovered() {
            Some(visuals.widgets.hovered.weak_bg_fill)
        } else {
            None
        };
        let painter = ui.painter();
        if let Some(fill) = fill {
            painter.rect_filled(rect, 2.0, fill);
        }
        let mark = Pos2::new(rect.left() + side / 2.0, rect.center().y);
        let red = visuals.error_fg_color;
        match entry {
            Entry::Code(_) => {
                painter.circle_filled(mark, 4.0, red);
            }
            Entry::Watch(watch) => {
                painter.text(
                    mark,
                    Align2::CENTER_CENTER,
                    access_icon(watch.access),
                    FontId::proportional(font.size),
                    red,
                );
            }
        }
        let ink = if stopped {
            visuals.selection.stroke.color
        } else {
            visuals.text_color()
        };
        painter
            .with_clip_rect(rect.with_max_x(rect.right() - side))
            .text(
                Pos2::new(rect.left() + side, rect.center().y),
                Align2::LEFT_CENTER,
                entry.label(),
                font,
                ink,
            );
        let mut action = None;
        let remove =
            Rect::from_min_size(Pos2::new(rect.right() - side, rect.top()), vec2(side, side));
        let removed = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(remove)
                    .layout(Layout::right_to_left(Align::Center)),
                |ui| ui.add(icon_button(icons::X, side).frame(false)),
            )
            .inner
            .on_hover_text("Remove");
        if removed.clicked() {
            action = Some(EntryAction::Remove(*entry));
        } else if response.clicked() && matches!(entry, Entry::Watch(_)) {
            action = Some(EntryAction::Show(*entry));
        }
        let hint = if stopped {
            format!("{}. Stopped here last.", entry.hint())
        } else {
            entry.hint().to_string()
        };
        response.on_hover_text(hint);
        action
    }
}
