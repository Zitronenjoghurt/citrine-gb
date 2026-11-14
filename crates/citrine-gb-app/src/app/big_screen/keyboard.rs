use super::glyphs::{self, Family, Glyph};
use super::pad_nav::{Command, Dir};
use crate::app::ui_state::keybinds::PadButton;
use egui::{Align2, CornerRadius, FontId, Id, Order, Rect, Sense, Stroke, Ui, pos2, vec2};

const LETTERS: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm,.-"];
const SYMBOLS: [&str; 4] = [
    "!@#$%&*()?",
    "\u{e0}\u{e1}\u{e2}\u{e4}\u{e9}\u{e8}\u{ea}\u{eb}\u{ed}\u{f1}",
    "\u{f3}\u{f6}\u{fa}\u{fc}\u{df}\u{e7}/:;\"",
    "_+=<>[]~^\u{20ac}",
];
const COLUMNS: usize = 10;
const ROWS: usize = 5;
const SIDE: usize = COLUMNS;
const GAP: f32 = 6.0;
const FOOT: f32 = 48.0;
const SCREEN_SHARE: f32 = 0.36;

struct Size {
    key: f32,
    side: f32,
    width: f32,
    height: f32,
}

impl Size {
    fn of(screen: Rect) -> Self {
        let frame = 24.0 + GAP * (ROWS - 1) as f32;
        let tall = (screen.height() * SCREEN_SHARE - frame) / ROWS as f32;
        let key = (screen.width() * 0.6 / 13.0).min(tall).clamp(28.0, 44.0);
        let side = (key * 2.6).max(114.0);
        Self {
            key,
            side,
            width: key * COLUMNS as f32 + GAP * COLUMNS as f32 + side + 24.0,
            height: key * ROWS as f32 + frame,
        }
    }
}

pub fn room(screen: Rect) -> f32 {
    Size::of(screen).height + FOOT + 8.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Delete,
    Space,
    Done,
    Cursor,
    Shift,
    Symbols,
}

impl Key {
    fn label(self, shift: bool) -> String {
        match self {
            Key::Char(c) if shift => c.to_uppercase().collect(),
            Key::Char(c) => c.to_string(),
            Key::Delete => "Delete".into(),
            Key::Space => "Space".into(),
            Key::Done => "Done".into(),
            Key::Cursor => "Cursor".into(),
            Key::Shift => "Shift".into(),
            Key::Symbols => "Symbols".into(),
        }
    }

    fn glyphs(self) -> &'static [Glyph] {
        match self {
            Key::Delete => &[Glyph::Face(PadButton::West)],
            Key::Space => &[Glyph::Face(PadButton::North)],
            Key::Done => &[Glyph::Start],
            Key::Cursor => &[
                Glyph::Shoulder(PadButton::LeftTrigger),
                Glyph::Shoulder(PadButton::RightTrigger),
            ],
            Key::Shift => &[Glyph::Shoulder(PadButton::LeftTrigger2)],
            Key::Symbols => &[Glyph::Shoulder(PadButton::RightTrigger2)],
            Key::Char(_) => &[],
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PadKeyboard {
    row: usize,
    col: usize,
    shift: bool,
    symbols: bool,
}

#[derive(Debug, PartialEq)]
pub enum Typed {
    Events(Vec<egui::Event>),
    Hide,
}

impl PadKeyboard {
    pub fn key_at(&self, row: usize, col: usize) -> Option<Key> {
        let rows = if self.symbols { SYMBOLS } else { LETTERS };
        match (row, col) {
            (4, 0..=4) => Some(Key::Shift),
            (4, 5..COLUMNS) => Some(Key::Symbols),
            (0, SIDE) => Some(Key::Delete),
            (1, SIDE) => Some(Key::Space),
            (2, SIDE) => Some(Key::Cursor),
            (3, SIDE) => Some(Key::Done),
            (0..4, 0..COLUMNS) => rows[row].chars().nth(col).map(Key::Char),
            _ => None,
        }
    }

    pub fn focused(&self) -> Option<Key> {
        self.key_at(self.row, self.col)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn command(&mut self, command: Command) -> Typed {
        let events = match command {
            Command::Move(dir) => {
                self.step(dir);
                Vec::new()
            }
            Command::Confirm => match self.focused() {
                Some(key) => self.press(key),
                None => Vec::new(),
            },
            Command::Back => return Typed::Hide,
            Command::FaceWest => self.press(Key::Delete),
            Command::FaceNorth => self.press(Key::Space),
            Command::PagePrevious => key_tap(egui::Key::ArrowLeft),
            Command::PageNext => key_tap(egui::Key::ArrowRight),
            Command::TriggerLeft => self.press(Key::Shift),
            Command::TriggerRight => self.press(Key::Symbols),
            Command::Start => self.press(Key::Done),
            Command::Menu => Vec::new(),
        };
        Typed::Events(events)
    }

    fn press(&mut self, key: Key) -> Vec<egui::Event> {
        match key {
            Key::Char(_) => vec![egui::Event::Text(key.label(self.shift))],
            Key::Delete => key_tap(egui::Key::Backspace),
            Key::Space => vec![egui::Event::Text(" ".into())],
            Key::Done => key_tap(egui::Key::Enter),
            Key::Cursor => Vec::new(),
            Key::Shift => {
                self.shift = !self.shift;
                Vec::new()
            }
            Key::Symbols => {
                self.symbols = !self.symbols;
                Vec::new()
            }
        }
    }

    fn step(&mut self, dir: Dir) {
        let (row, col) = (self.row, self.col);
        let (row, col) = match dir {
            Dir::Up => (row.saturating_sub(1), col),
            Dir::Down => ((row + 1).min(ROWS - 1), col.min(COLUMNS - 1)),
            Dir::Left if row == 4 && col >= 5 => (row, 0),
            Dir::Left => (row, col.saturating_sub(1)),
            Dir::Right if row == 4 => (row, if col < 5 { 5 } else { col }),
            Dir::Right => (row, (col + 1).min(SIDE)),
        };
        if self.key_at(row, col).is_some() {
            self.row = row;
            self.col = col;
        }
    }

    pub fn show(&self, ctx: &egui::Context, family: Family, confirm: PadButton) {
        let screen = ctx.content_rect();
        let Size {
            key,
            side,
            width,
            height,
        } = Size::of(screen);
        let gap = GAP;
        let bottom = screen.bottom() - FOOT;
        let origin = pos2(screen.center().x - width / 2.0, bottom - height);
        egui::Area::new(Id::new("pad_keyboard"))
            .order(Order::Foreground)
            .fixed_pos(origin)
            .interactable(false)
            .show(ctx, |ui| {
                let (frame, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                let visuals = ui.visuals().clone();
                ui.painter().rect(
                    frame,
                    CornerRadius::same(14),
                    crate::app::mobile::kit::card_fill(&visuals),
                    crate::app::mobile::kit::hairline(&visuals),
                    egui::StrokeKind::Inside,
                );
                let inner = frame.shrink(12.0);
                for row in 0..ROWS {
                    for col in 0..=SIDE {
                        let Some(found) = self.key_at(row, col) else {
                            continue;
                        };
                        if row == 4 && col != 0 && col != 5 {
                            continue;
                        }
                        let rect = key_rect(inner, row, col, key, gap, side);
                        self.paint_key(
                            ui,
                            rect,
                            found,
                            (row, col) == (self.row, self.col),
                            family,
                            confirm,
                        );
                    }
                }
            });
    }

    fn paint_key(
        &self,
        ui: &mut Ui,
        rect: Rect,
        key: Key,
        focused: bool,
        family: Family,
        confirm: PadButton,
    ) {
        let visuals = ui.visuals().clone();
        let lit =
            matches!(key, Key::Shift if self.shift) || matches!(key, Key::Symbols if self.symbols);
        let fill = if key == Key::Done {
            visuals.selection.bg_fill
        } else if lit {
            visuals.selection.bg_fill.gamma_multiply(0.5)
        } else if key == Key::Cursor {
            egui::Color32::TRANSPARENT
        } else {
            visuals.widgets.inactive.weak_bg_fill
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
        if focused {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                CornerRadius::same(10),
                Stroke::new(
                    3.0_f32,
                    crate::app::ui_state::ui_theme::brand_mark(&visuals),
                ),
                egui::StrokeKind::Outside,
            );
        }
        let ink = if key == Key::Done {
            crate::app::ui_state::ui_theme::ink_on(visuals.selection.bg_fill)
        } else {
            visuals.strong_text_color()
        };
        let glyphs: Vec<Glyph> = key
            .glyphs()
            .iter()
            .copied()
            .filter(|glyph| glyphs::shown(family, *glyph, confirm))
            .collect();
        if glyphs.is_empty() {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                key.label(self.shift),
                FontId::proportional((rect.height() * 0.42).max(13.0)),
                ink,
            );
            return;
        }
        let mut cell = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(8.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        cell.spacing_mut().item_spacing.x = 4.0;
        for glyph in glyphs {
            glyphs::paint(&mut cell, family, glyph, confirm, 18.0);
        }
        cell.add_space(4.0);
        cell.label(egui::RichText::new(key.label(false)).size(14.0).color(ink));
    }
}

fn key_rect(inner: Rect, row: usize, col: usize, key: f32, gap: f32, side: f32) -> Rect {
    let top = inner.top() + row as f32 * (key + gap);
    if row == 4 {
        let half = (key * COLUMNS as f32 + gap * (COLUMNS - 1) as f32 - gap) / 2.0;
        let left = inner.left() + if col == 0 { 0.0 } else { half + gap };
        return Rect::from_min_size(pos2(left, top), vec2(half, key));
    }
    if col == SIDE {
        let left = inner.left() + COLUMNS as f32 * (key + gap) + 6.0;
        return Rect::from_min_size(pos2(left, top), vec2(side, key));
    }
    Rect::from_min_size(
        pos2(inner.left() + col as f32 * (key + gap), top),
        vec2(key, key),
    )
}

fn key_tap(key: egui::Key) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(keyboard: &mut PadKeyboard, command: Command) -> Vec<egui::Event> {
        match keyboard.command(command) {
            Typed::Events(events) => events,
            Typed::Hide => panic!("the keyboard hid"),
        }
    }

    #[test]
    fn the_d_pad_walks_the_keys_and_a_types_the_focused_one() {
        let mut keyboard = PadKeyboard::default();
        typed(&mut keyboard, Command::Move(Dir::Down));
        typed(&mut keyboard, Command::Move(Dir::Right));
        assert_eq!(keyboard.focused(), Some(Key::Char('w')));
        assert_eq!(
            typed(&mut keyboard, Command::Confirm),
            vec![egui::Event::Text("w".into())]
        );
        typed(&mut keyboard, Command::TriggerLeft);
        assert_eq!(
            typed(&mut keyboard, Command::Confirm),
            vec![egui::Event::Text("W".into())]
        );
    }

    #[test]
    fn the_side_keys_and_the_bottom_row_are_reachable() {
        let mut keyboard = PadKeyboard::default();
        for _ in 0..12 {
            typed(&mut keyboard, Command::Move(Dir::Right));
        }
        assert_eq!(keyboard.focused(), Some(Key::Delete));
        for _ in 0..6 {
            typed(&mut keyboard, Command::Move(Dir::Down));
        }
        assert_eq!(keyboard.focused(), Some(Key::Symbols));
        typed(&mut keyboard, Command::Move(Dir::Left));
        assert_eq!(keyboard.focused(), Some(Key::Shift));
    }

    #[test]
    fn symbols_hold_the_at_sign_and_accents() {
        let mut keyboard = PadKeyboard::default();
        typed(&mut keyboard, Command::TriggerRight);
        typed(&mut keyboard, Command::Move(Dir::Right));
        assert_eq!(keyboard.focused(), Some(Key::Char('@')));
        typed(&mut keyboard, Command::Move(Dir::Down));
        typed(&mut keyboard, Command::Move(Dir::Right));
        typed(&mut keyboard, Command::Move(Dir::Right));
        typed(&mut keyboard, Command::Move(Dir::Right));
        assert_eq!(keyboard.focused(), Some(Key::Char('\u{e9}')));
    }

    #[test]
    fn the_buttons_delete_space_move_the_cursor_and_finish() {
        let mut keyboard = PadKeyboard::default();
        let tap = |key| key_tap(key);
        assert_eq!(
            typed(&mut keyboard, Command::FaceWest),
            tap(egui::Key::Backspace)
        );
        assert_eq!(
            typed(&mut keyboard, Command::FaceNorth),
            vec![egui::Event::Text(" ".into())]
        );
        assert_eq!(
            typed(&mut keyboard, Command::PagePrevious),
            tap(egui::Key::ArrowLeft)
        );
        assert_eq!(typed(&mut keyboard, Command::Start), tap(egui::Key::Enter));
        assert_eq!(keyboard.command(Command::Back), Typed::Hide);
    }
}
