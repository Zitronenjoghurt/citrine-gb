use super::glyphs::{self, Glyph, Hint};
use super::pad_nav::{Command, Dir};
use crate::app::Citrine;
use crate::app::mobile::kit;
use crate::app::ui_state::keybinds::PadButton;
use crate::app::widgets::layout_editor::PadEdit;
use egui::{Align2, Rect, Ui, Vec2, pos2, vec2};

const ROW_ROOM: f32 = 56.0;
const COLUMN_ROOM: f32 = 140.0;

pub(super) fn command(pad: &mut PadEdit, command: Command) -> Option<bool> {
    match command {
        Command::Move(dir) => pad.steps += step(dir),
        Command::PagePrevious => pad.pick -= 1,
        Command::PageNext => pad.pick += 1,
        Command::TriggerLeft => pad.grow -= 1,
        Command::TriggerRight => pad.grow += 1,
        Command::FaceNorth => pad.reset = true,
        Command::Confirm => return Some(true),
        Command::Back => return Some(false),
        _ => {}
    }
    None
}

pub(super) fn keys(pad: &mut PadEdit, events: &[egui::Event]) -> bool {
    let mut done = false;
    for event in events {
        let egui::Event::Key {
            key, pressed: true, ..
        } = event
        else {
            continue;
        };
        match key {
            egui::Key::ArrowUp => pad.steps += step(Dir::Up),
            egui::Key::ArrowDown => pad.steps += step(Dir::Down),
            egui::Key::ArrowLeft => pad.steps += step(Dir::Left),
            egui::Key::ArrowRight => pad.steps += step(Dir::Right),
            egui::Key::Tab => pad.pick += 1,
            egui::Key::Minus => pad.grow -= 1,
            egui::Key::Plus | egui::Key::Equals => pad.grow += 1,
            egui::Key::Enter => done = true,
            _ => {}
        }
    }
    done
}

fn step(dir: Dir) -> Vec2 {
    match dir {
        Dir::Up => vec2(0.0, -1.0),
        Dir::Down => vec2(0.0, 1.0),
        Dir::Left => vec2(-1.0, 0.0),
        Dir::Right => vec2(1.0, 0.0),
    }
}

impl super::BigScreen {
    pub fn take_layout_pad(&mut self) -> PadEdit {
        std::mem::take(&mut self.layout_pad)
    }
}

impl Citrine {
    pub(in crate::app) fn edit_hints(&self, ui: &Ui, area: Rect, screen: Rect) {
        let hints = [
            Hint::new([Glyph::Arrows], "Move"),
            Hint::new(
                [
                    Glyph::Shoulder(PadButton::LeftTrigger),
                    Glyph::Shoulder(PadButton::RightTrigger),
                ],
                "Pick",
            ),
            Hint::new(
                [
                    Glyph::Shoulder(PadButton::LeftTrigger2),
                    Glyph::Shoulder(PadButton::RightTrigger2),
                ],
                "Size",
            ),
            Hint::new(
                [Glyph::Face(PadButton::North)],
                self.ui.layout_reset_button().label(),
            ),
            Hint::new(
                [Glyph::Face(self.confirm_button())],
                self.ui.layout_done_label(),
            ),
            Hint::new(
                [Glyph::Face(self.back_button())],
                crate::app::layout_edit::CANCEL,
            ),
        ];
        let (room, column) = hints_room(area, screen);
        let family = self.shown_family();
        let confirm = self.confirm_button();
        egui::Area::new(ui.id().with("edit_hints"))
            .order(egui::Order::Foreground)
            .pivot(Align2::CENTER_CENTER)
            .fixed_pos(room.center())
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(kit::card_fill(ui.visuals()))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(16, 8))
                    .show(ui, |ui| {
                        let layout = if column {
                            egui::Layout::top_down(egui::Align::Min)
                        } else {
                            egui::Layout::left_to_right(egui::Align::Center)
                        };
                        ui.with_layout(layout, |ui| {
                            ui.spacing_mut().item_spacing = vec2(20.0, 8.0);
                            for hint in &hints {
                                glyphs::hint(ui, hint, family, confirm);
                            }
                        });
                    });
            });
    }
}

fn hints_room(area: Rect, screen: Rect) -> (Rect, bool) {
    let below = Rect::from_min_max(pos2(area.left(), screen.bottom()), area.right_bottom());
    let above = Rect::from_min_max(area.left_top(), pos2(area.right(), screen.top()));
    let right = Rect::from_min_max(pos2(screen.right(), area.top()), area.right_bottom());
    let left = Rect::from_min_max(area.left_top(), pos2(screen.left(), area.bottom()));
    if below.height() >= ROW_ROOM {
        (below, false)
    } else if above.height() >= ROW_ROOM {
        (above, false)
    } else if right.width() >= COLUMN_ROOM && right.width() >= left.width() {
        (right, true)
    } else if left.width() >= COLUMN_ROOM {
        (left, true)
    } else {
        (area.with_min_y(area.bottom() - ROW_ROOM), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 720.0))
    }

    #[test]
    fn the_hints_sit_in_the_free_room_beside_the_screen() {
        let tall = Rect::from_min_size(pos2(240.0, 0.0), vec2(800.0, 720.0));
        assert!(hints_room(area(), tall).1);
        assert_eq!(hints_room(area(), tall).0.left(), tall.right());
        let pushed = tall.translate(vec2(240.0, 0.0));
        assert_eq!(hints_room(area(), pushed).0.right(), pushed.left());
        let short = Rect::from_min_size(pos2(400.0, 0.0), vec2(480.0, 432.0));
        let (room, column) = hints_room(area(), short);
        assert!(!column);
        assert_eq!(room.top(), short.bottom());
    }
}
