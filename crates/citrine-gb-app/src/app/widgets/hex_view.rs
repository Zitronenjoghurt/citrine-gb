use egui::text::LayoutJob;
use egui::{Color32, FontId, Id, Sense, TextFormat, TextStyle, Ui, vec2};

const WIDE_ROW: usize = 16;
const NARROW_ROW: usize = 8;

pub struct HexView<'a> {
    bytes: &'a [u8],
    id: Id,
    jump: Option<usize>,
}

impl<'a> HexView<'a> {
    pub fn new(id: impl std::hash::Hash, bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            id: Id::new(id),
            jump: None,
        }
    }

    pub fn jump(mut self, offset: Option<usize>) -> Self {
        self.jump = offset;
        self
    }

    pub fn show(self, ui: &mut Ui) {
        let font = TextStyle::Monospace.resolve(ui.style());
        let glyph = ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
        let per_row = row_width(ui.available_width(), glyph);
        let height = ui.text_style_height(&TextStyle::Monospace) + 4.0;
        let rows = self.bytes.len().div_ceil(per_row).max(1);
        let mut area = egui::ScrollArea::both().id_salt(self.id).auto_shrink(false);
        if let Some(offset) = self.jump {
            area = area.vertical_scroll_offset((offset / per_row) as f32 * height);
        }
        let colors = Colors {
            text: ui.visuals().strong_text_color(),
            weak: ui.visuals().weak_text_color(),
            ink: ui.visuals().text_color(),
        };
        area.show_rows(ui, height, rows, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in range {
                let start = row * per_row;
                let end = (start + per_row).min(self.bytes.len());
                let job = line(start, &self.bytes[start..end], per_row, &font, &colors);
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                let (rect, _) = ui.allocate_exact_size(
                    vec2(galley.size().x.max(ui.available_width()), height),
                    Sense::hover(),
                );
                ui.painter().galley(
                    rect.left_center() - vec2(0.0, galley.size().y / 2.0),
                    galley,
                    colors.text,
                );
            }
        });
    }
}

struct Colors {
    text: Color32,
    weak: Color32,
    ink: Color32,
}

fn row_width(available: f32, glyph: f32) -> usize {
    let wide = (10 + WIDE_ROW * 3 + 2 + WIDE_ROW) as f32 * glyph;
    if available >= wide {
        WIDE_ROW
    } else {
        NARROW_ROW
    }
}

fn line(offset: usize, bytes: &[u8], per_row: usize, font: &FontId, colors: &Colors) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut push = |text: &str, color: Color32| {
        job.append(text, 0.0, TextFormat::simple(font.clone(), color));
    };
    push(&format!("{offset:08X}  "), colors.weak);
    for index in 0..per_row {
        let gap = if index == 0 {
            ""
        } else if index == 8 {
            "  "
        } else {
            " "
        };
        push(gap, colors.text);
        match bytes.get(index) {
            Some(0) => push("00", colors.weak),
            Some(byte) => push(&format!("{byte:02X}"), colors.text),
            None => push("  ", colors.text),
        }
    }
    push("  ", colors.text);
    push(&ascii(bytes), colors.ink);
    job
}

pub fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte.is_ascii_graphic() || byte == b' ' {
                byte as char
            } else {
                '·'
            }
        })
        .collect()
}

pub fn parse_offset(text: &str) -> Option<usize> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    usize::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_bytes_show_as_text() {
        assert_eq!(ascii(b"BMD0\xFF\xFE\x00 a"), "BMD0··· a");
    }

    #[test]
    fn offsets_read_as_hex_with_or_without_prefix() {
        assert_eq!(parse_offset("1F0"), Some(0x1F0));
        assert_eq!(parse_offset(" 0x10 "), Some(0x10));
        assert_eq!(parse_offset("zz"), None);
    }

    #[test]
    fn narrow_places_get_eight_bytes_a_row() {
        assert_eq!(row_width(2000.0, 8.0), WIDE_ROW);
        assert_eq!(row_width(300.0, 8.0), NARROW_ROW);
    }

    #[test]
    fn a_short_last_row_keeps_its_columns() {
        let font = FontId::monospace(12.0);
        let colors = Colors {
            text: Color32::WHITE,
            weak: Color32::GRAY,
            ink: Color32::WHITE,
        };
        let job = line(0x10, &[0x42, 0x00], 8, &font, &colors);
        assert_eq!(job.text, "00000010  42 00                    B·");
    }
}
