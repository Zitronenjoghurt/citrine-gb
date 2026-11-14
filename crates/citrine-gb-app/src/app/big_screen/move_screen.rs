use super::glyphs::{self, Glyph, Hint};
use super::pad_nav::{Command, Dir};
use crate::app::Citrine;
use crate::app::mobile::kit;
use crate::app::ui_state::keybinds::PadButton;
use egui::{Align2, Rect, Ui, Vec2, pos2, vec2};

const ROW_ROOM: f32 = 56.0;
const COLUMN_ROOM: f32 = 140.0;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ScreenMove {
    pub steps: Vec2,
    pub centre: bool,
}

impl ScreenMove {
    pub(super) fn command(&mut self, command: Command) -> bool {
        match command {
            Command::Move(dir) => self.steps += step(dir),
            Command::FaceNorth => self.centre = true,
            Command::Confirm | Command::Back => return true,
            _ => {}
        }
        false
    }

    pub(super) fn keys(&mut self, events: &[egui::Event]) -> bool {
        let mut done = false;
        for event in events {
            let egui::Event::Key {
                key, pressed: true, ..
            } = event
            else {
                continue;
            };
            match key {
                egui::Key::ArrowUp => self.steps += step(Dir::Up),
                egui::Key::ArrowDown => self.steps += step(Dir::Down),
                egui::Key::ArrowLeft => self.steps += step(Dir::Left),
                egui::Key::ArrowRight => self.steps += step(Dir::Right),
                egui::Key::Enter => done = true,
                _ => {}
            }
        }
        done
    }
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
    pub fn take_screen_move(&mut self) -> ScreenMove {
        std::mem::take(&mut self.screen_move)
    }
}

impl Citrine {
    pub(in crate::app) fn moving_screen(&self) -> bool {
        self.big.moving
    }

    pub(in crate::app) fn start_moving_screen(&mut self) {
        self.big.moving = true;
        self.big.screen_move = ScreenMove::default();
        self.mobile.nav.show_game();
    }

    pub(in crate::app) fn stop_moving_screen(&mut self) {
        self.big.moving = false;
    }

    pub(in crate::app) fn move_hints(&self, ui: &Ui, area: Rect, screen: Rect) {
        let hints = [
            Hint::new([Glyph::Arrows], "Move"),
            Hint::new([Glyph::Face(PadButton::North)], "Centre"),
            Hint::new([Glyph::Face(self.back_button())], "Done"),
        ];
        let (room, column) = hints_room(area, screen);
        let family = self.shown_family();
        let confirm = self.confirm_button();
        egui::Area::new(ui.id().with("move_hints"))
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
