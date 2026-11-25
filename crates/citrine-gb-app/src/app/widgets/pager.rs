use crate::app::ui_state::ui_theme::{brand, ink_on};
use crate::icons;
use egui::{Align2, FontId, Response, Sense, Ui, Widget, vec2};

const DESK_SIDE: f32 = 28.0;
const DESK_HEIGHT: f32 = 26.0;
const DESK_RADIUS: f32 = 4.0;
const TOUCH_SIDE: f32 = 40.0;
const GAP: f32 = 4.0;
const TOUCH_GAP: f32 = 6.0;
const EDGE: f32 = 16.0;
const DIMMED: f32 = 0.55;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Page(usize),
    Gap,
}

pub fn marks(page: usize, pages: usize) -> Vec<Mark> {
    let last = pages.saturating_sub(1);
    let page = page.min(last);
    let low = page.saturating_sub(1).min(last.saturating_sub(2));
    let high = (page + 1).max(2).min(last);
    let mut shown: Vec<usize> = std::iter::once(0)
        .chain(low..=high)
        .chain(std::iter::once(last))
        .collect();
    shown.sort_unstable();
    shown.dedup();
    let mut marks = Vec::new();
    for (index, &at) in shown.iter().enumerate() {
        if let Some(&before) = index.checked_sub(1).and_then(|back| shown.get(back)) {
            match at - before {
                2 => marks.push(Mark::Page(before + 1)),
                skipped if skipped > 2 => marks.push(Mark::Gap),
                _ => {}
            }
        }
        marks.push(Mark::Page(at));
    }
    marks
}

pub struct Pager<'a> {
    page: &'a mut usize,
    pages: usize,
    touch: bool,
}

impl<'a> Pager<'a> {
    pub fn new(page: &'a mut usize, pages: usize) -> Self {
        Self {
            page,
            pages,
            touch: false,
        }
    }

    pub fn touch(mut self) -> Self {
        self.touch = true;
        self
    }
}

enum Cell {
    Back,
    Page(usize),
    Gap,
    Next,
}

impl Widget for Pager<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (side, height, gap, radius, size) = if self.touch {
            (TOUCH_SIDE, TOUCH_SIDE, TOUCH_GAP, TOUCH_SIDE / 2.0, 15.0)
        } else {
            (DESK_SIDE, DESK_HEIGHT, GAP, DESK_RADIUS, 14.0)
        };
        let mut cells = vec![Cell::Back];
        cells.extend(
            marks(*self.page, self.pages)
                .into_iter()
                .map(|mark| match mark {
                    Mark::Page(page) => Cell::Page(page),
                    Mark::Gap => Cell::Gap,
                }),
        );
        cells.push(Cell::Next);
        let font = FontId::proportional(size);
        let widths: Vec<f32> = cells
            .iter()
            .map(|cell| match cell {
                Cell::Page(page) => {
                    let digits = ui
                        .painter()
                        .layout_no_wrap((page + 1).to_string(), font.clone(), egui::Color32::BLACK)
                        .size()
                        .x;
                    side.max(digits + 12.0)
                }
                Cell::Gap => side * 0.6,
                Cell::Back | Cell::Next => side,
            })
            .collect();
        let row = widths.iter().sum::<f32>() + gap * (cells.len() - 1) as f32;
        let room = ui.available_width();
        let (whole, mut response) =
            ui.allocate_exact_size(vec2(room, height + 2.0 * EDGE), Sense::hover());
        let mut x = whole.center().x - row / 2.0;
        let visuals = ui.visuals().clone();
        let fill = brand(&visuals);
        for (index, (cell, width)) in cells.iter().zip(widths).enumerate() {
            let rect =
                egui::Rect::from_min_size(egui::pos2(x, whole.top() + EDGE), vec2(width, height));
            x += width + gap;
            let (label, target, enabled) = match cell {
                Cell::Back => (
                    icons::CARET_LEFT.to_owned(),
                    self.page.checked_sub(1),
                    *self.page > 0,
                ),
                Cell::Next => (
                    icons::CARET_RIGHT.to_owned(),
                    Some(*self.page + 1),
                    *self.page + 1 < self.pages,
                ),
                Cell::Page(page) => ((page + 1).to_string(), Some(*page), true),
                Cell::Gap => ("…".to_owned(), None, false),
            };
            let current = matches!(cell, Cell::Page(page) if *page == *self.page);
            let id = ui.id().with(("pager_cell", index));
            let sense = if enabled && !current {
                Sense::click()
            } else if matches!(cell, Cell::Gap) {
                Sense::hover()
            } else if current || ui.memory(|memory| memory.has_focus(id)) {
                Sense::focusable_noninteractive()
            } else {
                Sense::hover()
            };
            let cell_response = ui.interact(rect, id, sense);
            crate::app::big_screen::ring::mark(
                &cell_response,
                crate::app::big_screen::ring::Mark::Ring { rect, radius },
            );
            let name = match cell {
                Cell::Back => "Previous page".to_owned(),
                Cell::Next => "Next page".to_owned(),
                Cell::Page(page) => format!("Page {}", page + 1),
                Cell::Gap => String::new(),
            };
            if !matches!(cell, Cell::Gap) {
                cell_response.widget_info(|| {
                    let mut info =
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name);
                    info.selected = Some(current);
                    info
                });
            }
            let (back, ink) = if current {
                (fill, ink_on(fill))
            } else if matches!(cell, Cell::Gap) {
                (egui::Color32::TRANSPARENT, visuals.weak_text_color())
            } else if !enabled {
                (visuals.widgets.inactive.weak_bg_fill, dimmed_ink(&visuals))
            } else if cell_response.hovered() {
                (
                    visuals.widgets.hovered.weak_bg_fill,
                    visuals.strong_text_color(),
                )
            } else {
                (visuals.widgets.inactive.weak_bg_fill, visuals.text_color())
            };
            if ui.is_rect_visible(rect) {
                ui.painter().rect_filled(rect, radius, back);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    font.clone(),
                    ink,
                );
            }
            if cell_response.clicked()
                && let Some(target) = target
            {
                *self.page = target;
                response.mark_changed();
            }
        }
        response
    }
}

fn dimmed_ink(visuals: &egui::Visuals) -> egui::Color32 {
    visuals
        .widgets
        .inactive
        .weak_bg_fill
        .lerp_to_gamma(visuals.weak_text_color(), DIMMED)
}

#[cfg(test)]
mod tests {
    use super::{Mark, dimmed_ink, marks};
    use crate::app::ui_state::ui_theme::{UiTheme, contrast};
    use strum::IntoEnumIterator;

    fn shown(page: usize, pages: usize) -> String {
        marks(page, pages)
            .into_iter()
            .map(|mark| match mark {
                Mark::Page(page) => (page + 1).to_string(),
                Mark::Gap => "…".to_owned(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_pager_shows_the_first_pages_and_the_last() {
        assert_eq!(shown(0, 13), "1 2 3 … 13");
        assert_eq!(shown(1, 13), "1 2 3 … 13");
    }

    #[test]
    fn the_pager_keeps_the_pages_beside_the_current_one() {
        assert_eq!(shown(5, 13), "1 … 5 6 7 … 13");
        assert_eq!(shown(12, 13), "1 … 11 12 13");
        assert_eq!(shown(3, 13), "1 2 3 4 5 … 13");
    }

    #[test]
    fn a_short_library_lists_every_page() {
        assert_eq!(shown(0, 1), "1");
        assert_eq!(shown(1, 2), "1 2");
        assert_eq!(shown(0, 4), "1 2 3 4");
        assert_eq!(shown(9, 4), "1 2 3 4");
    }

    #[test]
    fn a_disabled_arrow_shows_a_dimmed_chevron_in_every_theme() {
        for theme in UiTheme::iter() {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let visuals = ctx.style().visuals.clone();
            let ground = visuals.widgets.inactive.weak_bg_fill;
            let dimmed = contrast(dimmed_ink(&visuals), ground);
            assert!(
                dimmed >= 1.5,
                "{theme} disabled chevron fades out: {dimmed}"
            );
            assert!(
                dimmed < contrast(visuals.text_color(), ground),
                "{theme} disabled chevron looks enabled"
            );
        }
    }
}
