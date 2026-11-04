use crate::app::widgets::disassembly::row_height;
use crate::app::widgets::field::row_field;
use crate::app::widgets::text::faint;
use crate::icons;
use citrine_gb::debug::breakpoints::{Access, Watch};
use citrine_gb::debug::code::{Address, BlockId};
use citrine_gb::debug::memory::MemoryBlock;
use egui::{
    Align2, Button, ComboBox, DragValue, Event, EventFilter, FontId, Id, Key, Pos2, Rect, Response,
    ScrollArea, Sense, Stroke, TextStyle, Ui, vec2,
};
use std::ops::Range;

pub const ACCESSES: [Access; 3] = [Access::Write, Access::Read, Access::Change];

pub fn access_icon(access: Access) -> &'static str {
    match access {
        Access::Write => icons::PENCIL_SIMPLE_LINE,
        Access::Read => icons::EYE,
        Access::Change => icons::SWAP,
    }
}

pub fn access_command(access: Access) -> &'static str {
    match access {
        Access::Write => "Stop when written",
        Access::Read => "Stop when read",
        Access::Change => "Stop when changed",
    }
}

pub fn span_label(from: Address, len: u32) -> String {
    if len <= 1 {
        from.to_string()
    } else {
        let last = from.offset_by(i64::from(len) - 1);
        format!("{from} to {last}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryAction {
    Poke(Address, u8),
    Watch(Watch),
    Unwatch(Watch),
    GoTo(String),
}

#[derive(Default)]
struct Seen {
    key: Option<(BlockId, u16, &'static str)>,
    cycles: u128,
    before: Vec<u8>,
    last: Vec<u8>,
}

#[derive(Default)]
pub struct MemoryViewState {
    block: Option<&'static str>,
    bank: u16,
    anchor: u32,
    end: u32,
    selected: bool,
    pending: Option<u8>,
    dragging: bool,
    scroll_to: Option<u32>,
    go_to: String,
    go_to_failed: bool,
    seen: Seen,
}

impl MemoryViewState {
    pub fn pick<'b>(&mut self, blocks: &'b [MemoryBlock]) -> Option<&'b MemoryBlock> {
        let found = self
            .block
            .and_then(|name| blocks.iter().find(|block| block.name == name));
        let block = match found {
            Some(block) => block,
            None => {
                let block = blocks.first()?;
                self.choose(block);
                block
            }
        };
        self.bank = self.bank.clamp(block.first_bank, block.last_bank());
        Some(block)
    }

    fn choose(&mut self, block: &MemoryBlock) {
        self.block = Some(block.name);
        self.bank = block.first_bank;
        self.selected = false;
        self.pending = None;
    }

    pub fn bank(&self) -> u16 {
        self.bank
    }

    pub fn show_at(&mut self, block: &MemoryBlock, at: Address, len: u32) {
        if self.block != Some(block.name) {
            self.choose(block);
        }
        self.bank = at.bank;
        self.anchor = at.offset;
        self.end = at.offset + len.max(1) - 1;
        self.selected = true;
        self.pending = None;
        self.scroll_to = Some(at.offset);
        self.go_to_failed = false;
    }

    pub fn go_to_failed(&mut self) {
        self.go_to_failed = true;
    }

    pub fn track(&mut self, block: &MemoryBlock, cycles: u128, bytes: &[u8]) {
        let key = Some((block.id, self.bank, block.name));
        let seen = &mut self.seen;
        if seen.key != key || seen.last.len() != bytes.len() {
            seen.key = key;
            seen.cycles = cycles;
            seen.before = bytes.to_vec();
            seen.last = bytes.to_vec();
        } else if seen.cycles != cycles {
            seen.cycles = cycles;
            std::mem::swap(&mut seen.before, &mut seen.last);
            seen.last.copy_from_slice(bytes);
        }
    }

    fn changed(&self, offset: usize, byte: u8) -> bool {
        self.seen
            .before
            .get(offset)
            .is_some_and(|&before| before != byte)
    }

    pub fn selection(&self) -> Option<Range<u32>> {
        self.selected
            .then(|| self.anchor.min(self.end)..self.anchor.max(self.end) + 1)
    }

    fn select(&mut self, offset: u32, extend: bool) {
        if !extend || !self.selected {
            self.anchor = offset;
        }
        self.end = offset;
        self.selected = true;
        self.pending = None;
    }
}

pub struct MemoryView<'a> {
    blocks: &'a [MemoryBlock],
    block: &'a MemoryBlock,
    bytes: &'a [u8],
    watches: &'a [Watch],
    watchable: bool,
    editable: bool,
    state: &'a mut MemoryViewState,
}

struct Layout {
    font: FontId,
    row_height: f32,
    char_width: f32,
    per_row: u32,
    hex: f32,
    text: f32,
}

const GROUP: u32 = 8;
const PAD: f32 = 4.0;

impl Layout {
    fn measure(ui: &Ui, address_chars: usize) -> Self {
        let font = TextStyle::Monospace.resolve(ui.style());
        let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
        let hex = PAD + (address_chars as f32 + 2.0) * char_width;
        let width = |per_row: u32| {
            let groups = per_row.div_ceil(GROUP) as f32;
            hex + (per_row as f32 * 3.0 + groups + 1.0 + per_row as f32) * char_width + PAD
        };
        let per_row = [16, 8]
            .into_iter()
            .find(|&per_row| ui.available_width() >= width(per_row))
            .unwrap_or(4);
        let groups = per_row.div_ceil(GROUP) as f32;
        let text = hex + (per_row as f32 * 3.0 + groups) * char_width;
        Self {
            row_height: row_height(ui),
            font,
            char_width,
            per_row,
            hex,
            text,
        }
    }

    fn hex_rect(&self, row: Rect, column: u32) -> Rect {
        let x = row.left()
            + self.hex
            + (column as f32 * 3.0 + (column / GROUP) as f32) * self.char_width;
        Rect::from_min_size(
            Pos2::new(x - self.char_width / 2.0, row.top()),
            vec2(self.char_width * 3.0, row.height()),
        )
    }

    fn text_rect(&self, row: Rect, column: u32) -> Rect {
        let x = row.left() + self.text + column as f32 * self.char_width;
        Rect::from_min_size(Pos2::new(x, row.top()), vec2(self.char_width, row.height()))
    }
}

impl<'a> MemoryView<'a> {
    pub fn new(
        blocks: &'a [MemoryBlock],
        block: &'a MemoryBlock,
        bytes: &'a [u8],
        state: &'a mut MemoryViewState,
    ) -> Self {
        Self {
            blocks,
            block,
            bytes,
            watches: &[],
            watchable: false,
            editable: false,
            state,
        }
    }

    pub fn watches(mut self, watches: &'a [Watch]) -> Self {
        self.watches = watches;
        self.watchable = true;
        self
    }

    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable && self.block.writable;
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Option<MemoryAction> {
        let mut action = None;
        self.toolbar(ui, &mut action);
        ui.separator();
        self.grid(ui, &mut action);
        action
    }

    fn toolbar(&mut self, ui: &mut Ui, action: &mut Option<MemoryAction>) {
        ui.horizontal(|ui| {
            let mut picked = None;
            ComboBox::from_id_salt("memory_block")
                .selected_text(self.block.name)
                .show_ui(ui, |ui| {
                    for block in self.blocks {
                        let on = block.name == self.block.name;
                        if ui.selectable_label(on, block.name).clicked() && !on {
                            picked = Some(*block);
                        }
                    }
                });
            if let Some(block) = picked {
                self.state.choose(&block);
            }
            if self.block.banks > 1 {
                let bank = ui.add(
                    DragValue::new(&mut self.state.bank)
                        .range(self.block.first_bank..=self.block.last_bank())
                        .prefix("Bank "),
                );
                if bank.changed() {
                    self.state.pending = None;
                }
            }
            let failed = self.state.go_to_failed;
            let mut field = row_field(ui, &mut self.state.go_to)
                .hint_text("Go to address")
                .desired_width(ui.available_width().min(120.0));
            if failed {
                field = field.text_color(ui.visuals().error_fg_color);
            }
            let response = ui.add(field);
            if response.changed() {
                self.state.go_to_failed = false;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                *action = Some(MemoryAction::GoTo(self.state.go_to.trim().to_string()));
            }
            if failed {
                response.on_hover_text("No memory at that address");
            }
        });
    }

    fn grid(&mut self, ui: &mut Ui, action: &mut Option<MemoryAction>) {
        let address_chars = Address::at(self.block.id, self.state.bank, 0)
            .to_string()
            .len();
        let layout = Layout::measure(ui, address_chars);
        let id = ui.id().with("memory_grid");
        let rows = (self.block.size.div_ceil(layout.per_row)) as usize;
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut scroll = ScrollArea::vertical()
            .id_salt("memory_rows")
            .auto_shrink(false);
        if let Some(offset) = self.state.scroll_to.take() {
            let row = (offset / layout.per_row) as f32;
            let top =
                row * layout.row_height - ui.available_height() / 2.0 + layout.row_height / 2.0;
            scroll = scroll.vertical_scroll_offset(top.max(0.0));
        }
        let mut shown = 0..0;
        scroll.show_rows(ui, layout.row_height, rows, |ui, range| {
            shown = range.clone();
            for row in range {
                self.row(ui, id, &layout, row as u32, action);
            }
        });
        if !ui.input(|i| i.pointer.primary_down()) {
            self.state.dragging = false;
        }
        if ui.memory(|memory| memory.has_focus(id)) {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
            if self.keys(ui, id, &layout, action) {
                let row = (self.state.end / layout.per_row) as usize;
                let inside = shown.start + 1..shown.end.saturating_sub(1);
                if !inside.contains(&row) {
                    self.state.scroll_to = Some(self.state.end);
                }
            }
        }
    }

    fn row(
        &mut self,
        ui: &mut Ui,
        id: Id,
        layout: &Layout,
        row: u32,
        action: &mut Option<MemoryAction>,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), layout.row_height),
            Sense::click_and_drag(),
        );
        let first = row * layout.per_row;
        let columns = layout.per_row.min(self.block.size.saturating_sub(first));
        let pointer = response.hover_pos();
        let under = pointer.and_then(|pos| {
            (0..columns).find(|&column| {
                layout.hex_rect(rect, column).contains(pos)
                    || layout.text_rect(rect, column).contains(pos)
            })
        });
        let shift = ui.input(|i| i.modifiers.shift);
        if let Some(column) = under {
            let offset = first + column;
            if response.drag_started() || response.clicked() {
                self.state.select(offset, shift);
                self.state.dragging = true;
                ui.memory_mut(|memory| memory.request_focus(id));
            } else if self.state.dragging && ui.input(|i| i.pointer.primary_down()) {
                self.state.end = offset;
            }
            if response.secondary_clicked()
                && !self
                    .state
                    .selection()
                    .is_some_and(|range| range.contains(&offset))
            {
                self.state.select(offset, false);
            }
        }
        let response = self.hints(response, under.map(|column| first + column));
        if self.watchable {
            response.context_menu(|ui| self.context_menu(ui, action));
        }

        if !ui.is_rect_visible(rect) {
            return;
        }
        self.paint(ui, rect, layout, row, columns, under);
    }

    fn hints(&self, response: Response, under: Option<u32>) -> Response {
        let selected = self
            .state
            .selection()
            .zip(under)
            .is_some_and(|(range, offset)| range.contains(&offset));
        if selected && self.block.writable && !self.editable {
            response.on_hover_text("Pause to edit")
        } else {
            response
        }
    }

    fn paint(
        &self,
        ui: &Ui,
        rect: Rect,
        layout: &Layout,
        row: u32,
        columns: u32,
        under: Option<u32>,
    ) {
        let visuals = ui.visuals();
        let painter = ui.painter();
        if row.is_multiple_of(2) {
            painter.rect_filled(rect, 0.0, visuals.faint_bg_color);
        }
        let first = row * layout.per_row;
        let y = rect.center().y;
        let at = Address::at(self.block.id, self.state.bank, first);
        painter.text(
            Pos2::new(rect.left() + PAD, y),
            Align2::LEFT_CENTER,
            at.to_string(),
            layout.font.clone(),
            visuals.weak_text_color(),
        );
        let selection = self.state.selection();
        let watch_colour = visuals.error_fg_color;
        for column in 0..columns {
            let offset = first + column;
            let Some(&byte) = self.bytes.get(offset as usize) else {
                break;
            };
            let hex = layout.hex_rect(rect, column);
            let text = layout.text_rect(rect, column);
            let selected = selection
                .as_ref()
                .is_some_and(|range| range.contains(&offset));
            if selected {
                painter.rect_filled(hex.shrink2(vec2(1.0, 1.0)), 2.0, visuals.selection.bg_fill);
                painter.rect_filled(text, 0.0, visuals.selection.bg_fill);
            } else if under == Some(column) {
                let fill = visuals.widgets.hovered.weak_bg_fill;
                painter.rect_filled(hex.shrink2(vec2(1.0, 1.0)), 2.0, fill);
                painter.rect_filled(text, 0.0, fill);
            }
            let colour = if selected {
                visuals.selection.stroke.color
            } else if self.state.changed(offset as usize, byte) {
                visuals.warn_fg_color
            } else if byte == 0 {
                visuals.weak_text_color()
            } else {
                visuals.text_color()
            };
            let cursor = selected && self.editable && offset == self.state.end;
            let digits = match self.state.pending {
                Some(high) if cursor => format!("{high:X}_"),
                _ => format!("{byte:02X}"),
            };
            painter.text(
                hex.center(),
                Align2::CENTER_CENTER,
                digits,
                layout.font.clone(),
                colour,
            );
            painter.text(
                text.center(),
                Align2::CENTER_CENTER,
                printable(byte),
                layout.font.clone(),
                colour,
            );
            let address = Address::at(self.block.id, self.state.bank, offset);
            if self.watches.iter().any(|watch| watch.covers(address)) {
                let line = |r: Rect| {
                    painter.hline(
                        r.x_range().shrink(1.0),
                        r.bottom() - 1.5,
                        Stroke::new(1.5_f32, watch_colour),
                    )
                };
                line(hex.shrink2(vec2(layout.char_width / 2.0, 0.0)));
                line(text);
            }
            if cursor && self.state.pending.is_none() {
                painter.hline(
                    hex.shrink2(vec2(layout.char_width / 2.0, 0.0)).x_range(),
                    hex.bottom() - 1.0,
                    Stroke::new(1.0_f32, colour),
                );
            }
        }
    }

    fn context_menu(&mut self, ui: &mut Ui, action: &mut Option<MemoryAction>) {
        let Some(range) = self.state.selection() else {
            return;
        };
        let from = Address::at(self.block.id, self.state.bank, range.start);
        let len = range.end - range.start;
        ui.label(faint(span_label(from, len)));
        for access in ACCESSES {
            let watch = Watch { from, len, access };
            let on = self.watches.contains(&watch);
            let label = format!("{}  {}", access_icon(access), access_command(access));
            if ui.add(Button::new(label).selected(on)).clicked() {
                *action = Some(if on {
                    MemoryAction::Unwatch(watch)
                } else {
                    MemoryAction::Watch(watch)
                });
                ui.close();
            }
        }
        let others: Vec<Watch> = self
            .watches
            .iter()
            .filter(|watch| watch.covers(from) && (watch.from != from || watch.len != len))
            .copied()
            .collect();
        if !others.is_empty() {
            ui.separator();
            for watch in others {
                let label = format!("{}  Remove {}", icons::X, span_label(watch.from, watch.len));
                if ui.button(label).clicked() {
                    *action = Some(MemoryAction::Unwatch(watch));
                    ui.close();
                }
            }
        }
    }

    fn keys(
        &mut self,
        ui: &mut Ui,
        id: Id,
        layout: &Layout,
        action: &mut Option<MemoryAction>,
    ) -> bool {
        let events = ui.input(|i| i.events.clone());
        let size = self.block.size;
        let mut moved = false;
        for event in events {
            match event {
                Event::Text(text) => {
                    for digit in text.chars().filter_map(|c| c.to_digit(16)) {
                        moved |= self.typed(digit as u8, action);
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let step = match key {
                        Key::ArrowLeft => -1,
                        Key::ArrowRight => 1,
                        Key::ArrowUp => -(layout.per_row as i64),
                        Key::ArrowDown => layout.per_row as i64,
                        Key::Escape => {
                            if self.state.pending.take().is_none() {
                                self.state.selected = false;
                                ui.memory_mut(|memory| memory.surrender_focus(id));
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    if self.state.selected {
                        let next = (i64::from(self.state.end) + step).clamp(0, i64::from(size) - 1);
                        self.state.select(next as u32, modifiers.shift);
                        moved = true;
                    }
                }
                _ => {}
            }
        }
        moved
    }

    fn typed(&mut self, digit: u8, action: &mut Option<MemoryAction>) -> bool {
        if !self.editable || !self.state.selected {
            return false;
        }
        let Some(high) = self.state.pending.take() else {
            self.state.pending = Some(digit);
            return false;
        };
        let offset = self.state.end;
        let at = Address::at(self.block.id, self.state.bank, offset);
        *action = Some(MemoryAction::Poke(at, (high << 4) | digit));
        if offset + 1 < self.block.size {
            self.state.select(offset + 1, false);
        }
        true
    }
}

fn printable(byte: u8) -> String {
    if (0x20..0x7F).contains(&byte) {
        char::from(byte).to_string()
    } else {
        "·".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widest_text(width: f32) -> f32 {
        let block = MemoryBlock {
            id: BlockId::Wram,
            name: "WRAM bank 0",
            start: 0xC000,
            size: 0x1000,
            first_bank: 0,
            banks: 1,
            writable: true,
        };
        let blocks = [block];
        let bytes: Vec<u8> = (0..0x1000).map(|offset| (offset % 256) as u8).collect();
        let mut state = MemoryViewState::default();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 600.0))),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                MemoryView::new(&blocks, &block, &bytes, &mut state).show(ui);
            });
        });
        output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some(text.pos.x + text.galley.size().x),
                _ => None,
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn rows_fit_narrow_and_wide_panels() {
        for width in [260.0, 300.0, 420.0, 640.0, 1000.0] {
            let widest = widest_text(width);
            assert!(
                widest <= width,
                "text reaches {widest} in a {width} wide panel"
            );
        }
    }
}
