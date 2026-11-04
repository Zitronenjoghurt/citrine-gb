use crate::app::ui_state::ui_theme::CodeColours;
use crate::icons;
use citrine_gb::debug::Debug;
use citrine_gb::debug::code::{Address, Line};
use citrine_gb::debug::disassembly::Confidence;
use egui::{Align2, Color32, FontId, Pos2, Rect, ScrollArea, Sense, Stroke, TextStyle, Ui, vec2};

const GUTTER: f32 = 16.0;
const GAP: f32 = 8.0;
const MARK_RADIUS: f32 = 3.5;
const MARK_SLOT: f32 = 14.0;

pub enum CodeAction {
    ToggleBreakpoint(Address),
}

pub struct DisassemblyView<'a> {
    gb: &'a mut dyn Debug,
    track_pc: bool,
    colours: CodeColours,
}

struct Columns {
    font: FontId,
    row_height: f32,
    char_width: f32,
    address: f32,
    mnemonic: f32,
}

impl Columns {
    fn measure(ui: &Ui, lines: &[Line]) -> Self {
        let font = TextStyle::Monospace.resolve(ui.style());
        let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
        let widest = |texts: &mut dyn Iterator<Item = usize>, least: usize| {
            texts.max().unwrap_or(0).max(least) as f32 * char_width
        };
        let address = widest(
            &mut lines.iter().map(|line| line.at.to_string().len()),
            "00:0000".len(),
        );
        let mnemonic = widest(&mut lines.iter().map(|line| line.mnemonic.len()), 4);
        Self {
            row_height: row_height(ui),
            font,
            char_width,
            address,
            mnemonic,
        }
    }
}

pub fn row_height(ui: &Ui) -> f32 {
    ui.text_style_height(&TextStyle::Monospace) + 4.0
}

impl<'a> DisassemblyView<'a> {
    pub fn new(gb: &'a mut dyn Debug, track_pc: bool, colours: CodeColours) -> Self {
        Self {
            gb,
            track_pc,
            colours,
        }
    }

    pub fn show(self, ui: &mut Ui) -> Option<CodeAction> {
        let mut action = None;
        let rows = self.gb.code_rows();
        let pc = self.gb.pc();
        let row_height = Columns::measure(ui, &[]).row_height;
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut scroll = ScrollArea::vertical().auto_shrink(false);
        if self.track_pc
            && let Some(row) = pc.and_then(|pc| self.gb.code_row_of(pc))
        {
            let top = row as f32 * row_height - ui.available_height() / 2.0 + row_height / 2.0;
            scroll = scroll.vertical_scroll_offset(top.max(0.0));
        }

        let mut lines = Vec::new();
        scroll.show_rows(ui, row_height, rows, |ui, range| {
            let first = range.start;
            self.gb.disassemble(range, &mut lines);
            let breakpoints = self.gb.breakpoints();
            let columns = Columns::measure(ui, &lines);
            for (offset, line) in lines.iter().enumerate() {
                let index = first + offset;
                let is_pc = pc.is_some_and(|pc| line.at <= pc && pc < line.end());
                let row = Row {
                    line,
                    index,
                    is_pc,
                    is_breakpoint: breakpoints.contains(&line.at),
                    columns: &columns,
                    colours: &self.colours,
                };
                if row.show(ui) {
                    action = Some(CodeAction::ToggleBreakpoint(line.at));
                }
            }
        });
        action
    }
}

struct Row<'a> {
    line: &'a Line,
    index: usize,
    is_pc: bool,
    is_breakpoint: bool,
    columns: &'a Columns,
    colours: &'a CodeColours,
}

impl Row<'_> {
    fn show(&self, ui: &mut Ui) -> bool {
        let line = self.line;
        let columns = self.columns;
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), columns.row_height),
            Sense::click(),
        );
        let gutter = Rect::from_min_size(rect.min, vec2(GUTTER, rect.height()));
        let on_gutter = ui.rect_contains_pointer(gutter);

        response.context_menu(|ui| context_menu(ui, line));

        if !ui.is_rect_visible(rect) {
            return on_gutter && response.clicked();
        }

        let visuals = ui.visuals();
        let fill = if self.is_pc {
            visuals.selection.bg_fill
        } else if response.hovered() && !on_gutter {
            visuals.widgets.hovered.weak_bg_fill
        } else if self.index.is_multiple_of(2) {
            visuals.faint_bg_color
        } else {
            Color32::TRANSPARENT
        };
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, fill);

        let dot = Pos2::new(rect.left() + GUTTER / 2.0, rect.center().y);
        if self.is_breakpoint {
            painter.circle_filled(dot, 4.0, visuals.error_fg_color);
        } else if on_gutter {
            painter.circle_stroke(dot, 4.0, Stroke::new(1.0_f32, visuals.error_fg_color));
        }

        let y = rect.center().y;
        let mut x = rect.left() + GUTTER;
        let text_painter = painter.with_clip_rect(
            rect.with_max_x(rect.right() - MARK_SLOT)
                .intersect(painter.clip_rect()),
        );
        let text = |x: f32, text: &str, colour: Color32| {
            text_painter
                .text(
                    Pos2::new(x, y),
                    Align2::LEFT_CENTER,
                    text,
                    columns.font.clone(),
                    colour,
                )
                .width()
        };
        text(x, &line.at.to_string(), visuals.weak_text_color());
        x += columns.address + GAP / 2.0;

        let ram_slot = Rect::from_center_size(
            Pos2::new(x + columns.char_width, y),
            vec2(columns.char_width * 2.0, rect.height()),
        );
        if line.at.is_ram() {
            painter.text(
                ram_slot.center(),
                Align2::CENTER_CENTER,
                icons::MEMORY,
                FontId::proportional(columns.font.size),
                visuals.weak_text_color(),
            );
            ui.interact(ram_slot, ui.id().with(("ram", line.at)), Sense::hover())
                .on_hover_text("Code in RAM, read again each frame");
        }
        x = ram_slot.right() + GAP / 2.0;

        text(x, line.mnemonic, self.colours.class(line.class));
        x += columns.mnemonic + GAP;

        let mut pieces = Vec::new();
        for (position, operand) in line.operands.iter().enumerate() {
            if position > 0 {
                pieces.push((", ", visuals.weak_text_color()));
            }
            pieces.push((operand.text.as_str(), self.colours.operand(operand.kind)));
        }
        let room = ((rect.right() - MARK_SLOT - x) / columns.char_width).floor() as usize;
        let total: usize = pieces.iter().map(|(piece, _)| piece.chars().count()).sum();
        let cut = total > room;
        let mut left = if cut { room.saturating_sub(1) } else { total };
        for (piece, colour) in &pieces {
            if left == 0 {
                break;
            }
            let shown: String = piece.chars().take(left).collect();
            left -= shown.chars().count();
            x += text(x, &shown, *colour);
        }
        if cut {
            text(x, "…", visuals.weak_text_color());
            let operands: String = pieces.iter().map(|(piece, _)| *piece).collect();
            response
                .clone()
                .on_hover_text(format!("{} {operands}", line.mnemonic));
        }

        let mark = Pos2::new(rect.right() - MARK_SLOT / 2.0, y);
        let colour = visuals.weak_text_color();
        match line.confidence {
            Confidence::Fetched => {
                painter.circle_filled(mark, MARK_RADIUS, colour);
            }
            Confidence::Unconditional => {
                painter.circle_stroke(mark, MARK_RADIUS, Stroke::new(1.5_f32, colour));
            }
            Confidence::Conditional => {
                painter.circle_stroke(mark, MARK_RADIUS - 1.0, Stroke::new(1.0_f32, colour));
            }
        }
        let mark_rect = Rect::from_center_size(mark, vec2(MARK_SLOT, rect.height()));
        ui.interact(
            mark_rect,
            ui.id().with(("confidence", line.at)),
            Sense::hover(),
        )
        .on_hover_text(confidence_text(line.confidence));

        on_gutter && response.clicked()
    }
}

fn context_menu(ui: &mut Ui, line: &Line) {
    let bytes: Vec<String> = line.bytes().iter().map(|b| format!("{b:02X}")).collect();
    ui.label(format!("{}  {}", line.at, bytes.join(" ")));
    if line.cycles > 0 {
        ui.small(format!("{} machine cycles", line.cycles));
    }
    ui.small(confidence_text(line.confidence));
    if let Some(note) = &line.note {
        ui.separator();
        ui.label(note);
    }
}

fn confidence_text(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Fetched => "Run by the CPU",
        Confidence::Unconditional => "Reached from run code without a condition",
        Confidence::Conditional => "Reached only through a conditional jump",
    }
}

pub fn instruction_count(rows: usize) -> String {
    if rows == 1 {
        "1 instruction".to_string()
    } else {
        format!("{rows} instructions")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_names_its_unit() {
        assert_eq!(instruction_count(0), "0 instructions");
        assert_eq!(instruction_count(1), "1 instruction");
        assert_eq!(instruction_count(2048), "2048 instructions");
    }
}
