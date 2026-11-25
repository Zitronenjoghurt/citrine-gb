use crate::app::hud::SIZES;
use crate::app::ui_state::screen_layouts::{Item, fit_inside};
use crate::app::ui_state::ui_theme;
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

pub const HANDLE_REACH: f32 = 24.0;
const HANDLE_RADIUS: f32 = 7.0;
const SNAP: f32 = 8.0;
const DASH: f32 = 5.0;
const SCREEN_DIM: u8 = 70;
const PAD_STEP: f32 = 16.0;
const PAD_GROW: f32 = 1.1;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PadEdit {
    pub on: bool,
    pub steps: Vec2,
    pub grow: i32,
    pub pick: i32,
    pub reset: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    Layout(Item),
    Hud(u32),
}

impl Target {
    fn is_screen(self) -> bool {
        matches!(self, Self::Layout(item) if item.is_screen())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditItem {
    pub target: Target,
    pub rect: Rect,
    pub smallest: f32,
    pub largest: f32,
}

impl EditItem {
    pub fn new(item: Item, rect: Rect, default: Vec2) -> Self {
        Self {
            target: Target::Layout(item),
            rect,
            smallest: item.smallest(default),
            largest: item.largest(default),
        }
    }

    pub fn hud(id: u32, rect: Rect, size: f32) -> Self {
        let unit = rect.width() / size.max(0.01);
        Self {
            target: Target::Hud(id),
            rect,
            smallest: unit * SIZES.start(),
            largest: unit * SIZES.end(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Grip {
    Move,
    Corner(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    target: Target,
    grip: Grip,
    from: Rect,
    start: Pos2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Guide {
    Across(f32),
    Down(f32),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct LayoutEditor {
    pub selected: Option<Target>,
    drag: Option<Drag>,
    guides: Vec<Guide>,
}

fn corners(rect: Rect) -> [Pos2; 4] {
    [
        rect.left_top(),
        rect.right_top(),
        rect.left_bottom(),
        rect.right_bottom(),
    ]
}

impl LayoutEditor {
    pub fn dragging(&self) -> Option<Target> {
        self.drag.map(|d| d.target)
    }

    fn grab(&self, items: &[EditItem], pos: Pos2) -> Option<(Target, Grip, Rect)> {
        if let Some(selected) = self.selected
            && let Some(item) = items.iter().find(|i| i.target == selected)
            && let Some(corner) = corners(item.rect)
                .iter()
                .position(|c| c.distance(pos) <= HANDLE_REACH)
        {
            return Some((selected, Grip::Corner(corner), item.rect));
        }
        items
            .iter()
            .rev()
            .find(|i| i.rect.contains(pos))
            .map(|i| (i.target, Grip::Move, i.rect))
    }

    pub fn show(&mut self, ui: &mut Ui, room: Rect, items: &[EditItem]) -> Option<(Target, Rect)> {
        let response = ui.interact(room, ui.id().with("layout_editor"), Sense::click_and_drag());
        let origin = ui.input(|i| i.pointer.press_origin());
        let at = ui.input(|i| i.pointer.interact_pos());

        if response.clicked() {
            self.selected = response
                .interact_pointer_pos()
                .and_then(|pos| self.grab(items, pos))
                .map(|(target, ..)| target);
        }
        if response.drag_started()
            && let Some(start) = origin
        {
            self.drag = self.grab(items, start).map(|(target, grip, from)| Drag {
                target,
                grip,
                from,
                start,
            });
            if let Some(drag) = self.drag {
                self.selected = Some(drag.target);
            }
        }
        let mut placed = None;
        if let (Some(drag), Some(pos)) = (self.drag, at)
            && response.dragged()
            && let Some(item) = items.iter().find(|i| i.target == drag.target)
        {
            let others: Vec<Rect> = items
                .iter()
                .filter(|i| i.target != drag.target)
                .map(|i| i.rect)
                .collect();
            let rect = match drag.grip {
                Grip::Move => self.moved(drag.from, pos - drag.start, room, &others),
                Grip::Corner(corner) => {
                    self.guides.clear();
                    resized(drag.from, corner, pos, room, item)
                }
            };
            placed = Some((drag.target, rect));
        }
        if response.drag_stopped() || !response.is_pointer_button_down_on() && !response.dragged() {
            self.drag = None;
            self.guides.clear();
        }
        placed
    }

    pub fn pad(&mut self, room: Rect, items: &[EditItem], pad: PadEdit) -> Option<(Target, Rect)> {
        if !pad.on || items.is_empty() {
            return None;
        }
        let at = self
            .selected
            .and_then(|selected| items.iter().position(|i| i.target == selected));
        let index = match at {
            Some(at) => (at as i32 + pad.pick).rem_euclid(items.len() as i32) as usize,
            None => 0,
        };
        let item = &items[index];
        self.selected = Some(item.target);
        if pad.steps == Vec2::ZERO && pad.grow == 0 {
            return None;
        }
        let width = (item.rect.width() * PAD_GROW.powi(pad.grow))
            .clamp(
                item.smallest.min(item.rect.width()),
                item.largest.max(item.rect.width()),
            )
            .min(room.width())
            .min(room.height() * item.rect.width() / item.rect.height().max(1.0));
        let size = item.rect.size() * (width / item.rect.width().max(1.0));
        let rect = Rect::from_center_size(item.rect.center() + pad.steps * PAD_STEP, size);
        Some((item.target, fit_inside(rect, room)))
    }

    fn moved(&mut self, from: Rect, delta: Vec2, room: Rect, others: &[Rect]) -> Rect {
        let rect = fit_inside(from.translate(delta), room);
        let xs = std::iter::once([room.left(), room.center().x, room.right()])
            .chain(others.iter().map(|r| [r.left(), r.center().x, r.right()]))
            .flatten()
            .collect::<Vec<_>>();
        let ys = std::iter::once([room.top(), room.center().y, room.bottom()])
            .chain(others.iter().map(|r| [r.top(), r.center().y, r.bottom()]))
            .flatten()
            .collect::<Vec<_>>();
        let snap = |own: [f32; 3], lines: &[f32]| {
            own.iter()
                .flat_map(|o| lines.iter().map(move |l| (l - o, *l)))
                .filter(|(gap, _)| gap.abs() <= SNAP)
                .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
        };
        self.guides.clear();
        let mut shift = Vec2::ZERO;
        if let Some((gap, line)) = snap([rect.left(), rect.center().x, rect.right()], &xs) {
            shift.x = gap;
            self.guides.push(Guide::Down(line));
        }
        if let Some((gap, line)) = snap([rect.top(), rect.center().y, rect.bottom()], &ys) {
            shift.y = gap;
            self.guides.push(Guide::Across(line));
        }
        fit_inside(rect.translate(shift), room)
    }

    pub fn paint(&self, ui: &Ui, room: Rect, items: &[EditItem]) {
        let visuals = ui.visuals();
        let brand = ui_theme::brand(visuals);
        let painter = ui.painter();
        for item in items.iter().filter(|i| i.target.is_screen()) {
            painter.rect_filled(item.rect, 0.0, Color32::from_black_alpha(SCREEN_DIM));
        }
        let (picked, others): (Vec<&EditItem>, Vec<&EditItem>) = items
            .iter()
            .partition(|item| self.selected == Some(item.target));
        for item in others {
            dashed_rect(painter, item.rect.shrink(0.75), Stroke::new(1.5_f32, brand));
        }
        for item in picked {
            painter.rect_stroke(
                item.rect,
                0.0,
                Stroke::new(2.0_f32, brand),
                egui::StrokeKind::Inside,
            );
            for corner in corners(item.rect) {
                painter.circle(
                    corner,
                    HANDLE_RADIUS,
                    brand,
                    Stroke::new(2.0_f32, visuals.panel_fill),
                );
            }
        }
        for guide in &self.guides {
            let points = match *guide {
                Guide::Down(x) => [pos2(x, room.top()), pos2(x, room.bottom())],
                Guide::Across(y) => [pos2(room.left(), y), pos2(room.right(), y)],
            };
            painter.extend(Shape::dashed_line(
                &points,
                Stroke::new(1.5_f32, brand),
                DASH,
                DASH,
            ));
        }
    }
}

fn dashed_rect(painter: &egui::Painter, rect: Rect, stroke: Stroke) {
    let points = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.extend(Shape::dashed_line(&points, stroke, DASH, DASH));
}

fn resized(from: Rect, corner: usize, pos: Pos2, room: Rect, item: &EditItem) -> Rect {
    let anchor = corners(from)[3 - corner];
    let aspect = from.height() / from.width().max(1.0);
    let towards = vec2(
        if corner % 2 == 1 { 1.0 } else { -1.0 },
        if corner >= 2 { 1.0 } else { -1.0 },
    );
    let reach = (pos - anchor) * towards;
    let wanted = reach.x.max(reach.y / aspect);
    let room_x = if towards.x > 0.0 {
        room.right() - anchor.x
    } else {
        anchor.x - room.left()
    };
    let room_y = if towards.y > 0.0 {
        room.bottom() - anchor.y
    } else {
        anchor.y - room.top()
    };
    let most = item.largest.min(room_x).min(room_y / aspect);
    let width = wanted.clamp(item.smallest.min(most), most);
    let other = anchor + towards * vec2(width, width * aspect);
    Rect::from_two_pos(anchor, other)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: Rect = Rect {
        min: pos2(0.0, 0.0),
        max: pos2(400.0, 600.0),
    };

    fn screen(rect: Rect) -> EditItem {
        EditItem::new(Item::Screen, rect, rect.size())
    }

    #[test]
    fn a_corner_drag_keeps_the_shape_and_holds_the_opposite_corner() {
        let from = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 150.0));
        let item = screen(from);
        let rect = resized(from, 3, pos2(340.0, 260.0), ROOM, &item);
        assert_eq!(rect.min, from.min);
        assert!((rect.height() / rect.width() - 0.75).abs() < 0.001);
        assert!((rect.width() - 240.0).abs() < 0.01, "{rect:?}");
    }

    #[test]
    fn a_corner_drag_stops_at_the_room_and_at_the_smallest_size() {
        let from = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 150.0));
        let item = screen(from);
        let big = resized(from, 3, pos2(900.0, 900.0), ROOM, &item);
        assert!(ROOM.contains_rect(big), "{big:?}");
        assert_eq!(big.right(), ROOM.right());
        let small = resized(from, 0, pos2(299.0, 249.0), ROOM, &item);
        assert!((small.width() - item.smallest).abs() < 0.01, "{small:?}");
        assert_eq!(small.max, from.max);
    }

    #[test]
    fn a_move_snaps_to_the_room_centre_and_to_another_items_edge() {
        let mut editor = LayoutEditor::default();
        let from = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        let other = Rect::from_min_size(pos2(300.0, 400.0), vec2(50.0, 50.0));
        let rect = editor.moved(from, vec2(146.0, 403.0), ROOM, &[other]);
        assert_eq!(rect.center().x, ROOM.center().x);
        assert_eq!(rect.top(), other.top());
        assert_eq!(editor.guides.len(), 2);
    }

    #[test]
    fn a_move_never_leaves_the_room() {
        let mut editor = LayoutEditor::default();
        let from = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        let rect = editor.moved(from, vec2(-500.0, 900.0), ROOM, &[]);
        assert!(ROOM.contains_rect(rect), "{rect:?}");
    }

    #[test]
    fn a_press_on_the_selected_items_corner_grabs_the_corner_before_the_item_on_top() {
        let screen_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 150.0));
        let dpad = Rect::from_min_size(pos2(180.0, 130.0), vec2(80.0, 80.0));
        let items = [
            screen(screen_rect),
            EditItem::new(Item::Dpad, dpad, dpad.size()),
        ];
        let mut editor = LayoutEditor::default();
        assert_eq!(
            editor.grab(&items, pos2(200.0, 150.0)).map(|g| g.0),
            Some(Target::Layout(Item::Dpad))
        );
        editor.selected = Some(Target::Layout(Item::Screen));
        assert_eq!(
            editor.grab(&items, pos2(200.0, 150.0)).map(|g| (g.0, g.1)),
            Some((Target::Layout(Item::Screen), Grip::Corner(3)))
        );
    }

    #[test]
    fn a_hud_resizes_between_its_smallest_and_largest_size() {
        let from = Rect::from_min_size(pos2(10.0, 10.0), vec2(120.0, 30.0));
        let item = EditItem::hud(7, from, 1.5);
        assert!((item.smallest - 60.0).abs() < 0.01);
        assert!((item.largest - 160.0).abs() < 0.01);
        let big = resized(from, 3, pos2(390.0, 590.0), ROOM, &item);
        assert!((big.width() - 160.0).abs() < 0.01, "{big:?}");
        let small = resized(from, 3, pos2(11.0, 11.0), ROOM, &item);
        assert!((small.width() - 60.0).abs() < 0.01, "{small:?}");
    }

    #[test]
    fn the_pad_picks_the_first_item_then_cycles_moves_and_grows_it() {
        let a = Rect::from_min_size(pos2(100.0, 100.0), vec2(100.0, 75.0));
        let b = Rect::from_min_size(pos2(250.0, 300.0), vec2(40.0, 40.0));
        let items = [screen(a), EditItem::new(Item::Dpad, b, b.size())];
        let mut editor = LayoutEditor::default();
        let on = PadEdit {
            on: true,
            ..Default::default()
        };
        assert_eq!(editor.pad(ROOM, &items, on), None);
        assert_eq!(editor.selected, Some(Target::Layout(Item::Screen)));
        let back = PadEdit { pick: -1, ..on };
        assert_eq!(editor.pad(ROOM, &items, back), None);
        assert_eq!(editor.selected, Some(Target::Layout(Item::Dpad)));
        let step = PadEdit {
            steps: vec2(1.0, 0.0),
            ..on
        };
        let (target, moved) = editor.pad(ROOM, &items, step).expect("a move");
        assert_eq!(target, Target::Layout(Item::Dpad));
        assert_eq!(moved, b.translate(vec2(PAD_STEP, 0.0)));
        let grow = PadEdit { grow: 1, ..on };
        let (_, grown) = editor.pad(ROOM, &items, grow).expect("a resize");
        assert!((grown.width() - 44.0).abs() < 0.01, "{grown:?}");
        assert_eq!(grown.center(), b.center());
        let off = PadEdit {
            grow: 1,
            ..Default::default()
        };
        assert_eq!(editor.pad(ROOM, &items, off), None);
    }
}
