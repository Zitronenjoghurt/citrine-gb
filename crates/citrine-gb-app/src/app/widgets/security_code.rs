use crate::app::mobile::touch_square;
use crate::app::widgets::confirm::PHONE_NOTE;
use crate::app::widgets::info_mark::{info_mark_above, info_mark_ink_width, info_mark_over};
use egui::{FontFamily, FontId, Rect, RichText, TextStyle, Ui};

pub const TITLE: &str = "Security code";
const DIGITS_MOST: f32 = 32.0;
const TOUCH_TEXT: f32 = 16.0;
const MARGIN: f32 = 16.0;

pub struct SecurityCode<'a> {
    pub title: &'a str,
    pub hint: &'a str,
    pub lines: &'a [String],
    pub compare: Option<&'a str>,
}

impl SecurityCode<'_> {
    pub fn show(&self, ui: &mut Ui) {
        self.draw(ui, true, None);
    }

    pub fn show_last(&self, ui: &mut Ui) {
        self.draw(ui, false, None);
    }

    pub fn show_under(&self, ui: &mut Ui, cover: Rect) -> f32 {
        self.draw(ui, true, Some(cover))
    }

    fn draw(&self, ui: &mut Ui, gap_after: bool, cover: Option<Rect>) -> f32 {
        let touch = touch_square(ui) > 0.0;
        let galley = ui.painter().layout_no_wrap(
            self.title.to_owned(),
            TextStyle::Body.resolve(ui.style()),
            ui.visuals().weak_text_color(),
        );
        let used = galley.size().x + info_mark_ink_width(ui);
        let centered = ui.layout().horizontal_align() == egui::Align::Center;
        let row = ui.horizontal(|ui| {
            if centered {
                ui.add_space(((ui.available_width() - used) / 2.0).max(0.0));
            }
            let title = ui.label(RichText::new(self.title).weak());
            let short = match cover {
                Some(cover) => info_mark_over(ui, self.hint, cover).1,
                None => {
                    info_mark_above(ui, self.hint, ui.min_rect().top());
                    0.0
                }
            };
            (title.rect.top(), short)
        });
        let (title_top, short) = row.inner;
        let inset = (title_top - row.response.rect.top()).max(0.0);
        let size = self.digit_size(ui, touch);
        for line in self.lines {
            ui.label(
                RichText::new(line)
                    .family(FontFamily::Monospace)
                    .size(size)
                    .strong(),
            );
        }
        if let Some(compare) = self.compare {
            let size = if touch {
                PHONE_NOTE
            } else {
                TextStyle::Body.resolve(ui.style()).size
            };
            ui.label(RichText::new(compare).size(size));
            if gap_after {
                ui.add_space(inset);
            }
        } else if gap_after {
            ui.add_space((inset - below_baseline(ui, size)).max(0.0));
        }
        short
    }

    fn digit_size(&self, ui: &Ui, touch: bool) -> f32 {
        let heading = TextStyle::Heading.resolve(ui.style()).size;
        let least = if touch {
            heading.max(TOUCH_TEXT)
        } else {
            heading
        };
        let widest = self
            .lines
            .iter()
            .map(|line| {
                ui.painter()
                    .layout_no_wrap(
                        line.clone(),
                        FontId::monospace(DIGITS_MOST),
                        ui.visuals().text_color(),
                    )
                    .size()
                    .x
            })
            .fold(0.0, f32::max);
        if widest <= 0.0 {
            return DIGITS_MOST;
        }
        (DIGITS_MOST * (ui.available_width() - 2.0 * MARGIN) / widest)
            .min(DIGITS_MOST)
            .max(least)
    }
}

fn below_baseline(ui: &Ui, size: f32) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        "0".to_owned(),
        FontId::monospace(size),
        egui::Color32::PLACEHOLDER,
    );
    galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map_or(0.0, |glyph| (glyph.line_height - glyph.pos.y).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Drawn {
        text: String,
        size: f32,
        width: f32,
        family: FontFamily,
    }

    fn drawn(width: f32, lines: &[String], compare: Option<&str>) -> Vec<Drawn> {
        drawn_in(&egui::Context::default(), width, lines, compare)
    }

    fn drawn_in(
        ctx: &egui::Context,
        width: f32,
        lines: &[String],
        compare: Option<&str>,
    ) -> Vec<Drawn> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 600.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                SecurityCode {
                    title: TITLE,
                    hint: "Compare it",
                    lines,
                    compare,
                }
                .show(ui);
            });
        });
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(Drawn {
                    text: text.galley.text().to_owned(),
                    size: text.galley.job.sections[0].format.font_id.size,
                    width: text.galley.size().x,
                    family: text.galley.job.sections[0].format.font_id.family.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    fn find<'a>(drawn: &'a [Drawn], text: &str) -> &'a Drawn {
        drawn
            .iter()
            .find(|d| d.text == text)
            .unwrap_or_else(|| panic!("{text} is drawn"))
    }

    #[test]
    fn a_short_code_reads_large_and_its_compare_line_reads_as_body_text() {
        let style = egui::Style::default();
        let heading = TextStyle::Heading.resolve(&style).size;
        let body = TextStyle::Body.resolve(&style).size;
        let lines = ["48392".to_owned()];
        let drawn = drawn(
            300.0,
            &lines,
            Some("Allow if Pixel 8 shows the same digits"),
        );
        assert_eq!(find(&drawn, "48392").size, DIGITS_MOST);
        assert!(DIGITS_MOST >= heading);
        assert!(find(&drawn, "Allow if Pixel 8 shows the same digits").size >= body);
    }

    #[test]
    fn the_digits_stay_monospace_under_a_font_override() {
        let ctx = egui::Context::default();
        ctx.style_mut(|style| style.override_font_id = Some(FontId::proportional(14.0)));
        let lines = ["48392".to_owned()];
        let drawn = drawn_in(&ctx, 300.0, &lines, None);
        assert_eq!(find(&drawn, "48392").family, FontFamily::Monospace);
    }

    #[test]
    fn a_long_code_shrinks_to_fit_but_stays_at_heading_size_or_more() {
        let heading = TextStyle::Heading.resolve(&egui::Style::default()).size;
        let lines = [
            "88194 38371 57551 00612".to_owned(),
            "24650 06303 55842 86865".to_owned(),
        ];
        let drawn = drawn(320.0, &lines, None);
        for line in &lines {
            let shown = find(&drawn, line);
            assert!(shown.size >= heading, "{} at {}", line, shown.size);
            assert!(
                shown.width <= 320.0 - 2.0 * MARGIN,
                "{} is {} wide, past its margin",
                line,
                shown.width
            );
        }
    }
}
