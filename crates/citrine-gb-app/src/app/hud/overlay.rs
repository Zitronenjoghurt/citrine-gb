use super::cells::Facts;
use super::paint::{self, Look};
use super::{HudSettings, HudState, INSET, SIZES, TRACKER, editor};
use egui::{CursorIcon, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

const SNAP: f32 = 8.0;
const HANDLE: f32 = 12.0;
const LIFT_AFTER: f64 = 0.45;
const LIFT_SLOP: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub id: u32,
    pub resize: bool,
    pub lifted: bool,
    start: Rect,
    size: f32,
    moved: Vec2,
    zoom: f32,
    guides: [Option<f32>; 2],
}

impl Drag {
    fn new(id: u32, resize: bool, start: Rect, size: f32) -> Drag {
        Drag {
            id,
            resize,
            lifted: false,
            start,
            size,
            moved: Vec2::ZERO,
            zoom: 1.0,
            guides: [None, None],
        }
    }

    fn changed(&self) -> bool {
        self.moved.length() > 0.5 || (self.zoom - 1.0).abs() > 0.01
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Outcome {
    pub changed: bool,
    pub remove: Option<u32>,
    pub edit_sheet: Option<u32>,
}

pub struct Place {
    pub area: Rect,
    pub picture: Option<Rect>,
    pub screens: Vec<Rect>,
    pub touch: bool,
    pub editing: bool,
    pub keep_below: Option<f32>,
}

pub fn snap(rect: Rect, place: &Place) -> (Rect, [Option<f32>; 2]) {
    let mut xs = vec![place.area.left() + INSET, place.area.right() - INSET];
    let mut ys = vec![place.area.top() + INSET, place.area.bottom() - INSET];
    let mut centres = Vec::new();
    if let Some(picture) = place.picture {
        xs.extend([picture.left() + INSET, picture.right() - INSET]);
        ys.extend([picture.top() + INSET, picture.bottom() - INSET]);
        centres.push(picture.center());
    }
    centres.extend(place.screens.iter().map(|screen| screen.center()));
    let near = |sides: [f32; 2], middle: f32, edges: &[f32], centres: &[f32]| {
        let to_edges = edges
            .iter()
            .flat_map(|&line| sides.iter().map(move |&side| (line - side, line)));
        let to_centres = centres.iter().map(|&line| (line - middle, line));
        to_edges
            .chain(to_centres)
            .filter(|(gap, _)| gap.abs() <= SNAP)
            .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
    };
    let centres_x: Vec<f32> = centres.iter().map(|centre| centre.x).collect();
    let centres_y: Vec<f32> = centres.iter().map(|centre| centre.y).collect();
    let x = near(
        [rect.left(), rect.right()],
        rect.center().x,
        &xs,
        &centres_x,
    );
    let y = near(
        [rect.top(), rect.bottom()],
        rect.center().y,
        &ys,
        &centres_y,
    );
    let shift = vec2(x.map_or(0.0, |(gap, _)| gap), y.map_or(0.0, |(gap, _)| gap));
    (
        rect.translate(shift),
        [x.map(|(_, line)| line), y.map(|(_, line)| line)],
    )
}

fn inside(rect: Rect, area: Rect) -> Rect {
    let x = rect
        .left()
        .min(area.right() - rect.width())
        .max(area.left());
    let y = rect
        .top()
        .min(area.bottom() - rect.height())
        .max(area.top());
    Rect::from_min_size(pos2(x, y), rect.size())
}

pub fn show(
    ui: &mut Ui,
    place: &Place,
    facts: &Facts,
    trackers: &[(String, String)],
    settings: &mut HudSettings,
    state: &mut HudState,
) -> Outcome {
    let mut outcome = Outcome::default();
    state.placed.clear();
    if !settings.visible(!trackers.is_empty()) {
        state.drag = None;
        return outcome;
    }
    if place.editing {
        state.drag = None;
        state.editor = None;
    }
    let mut ids: Vec<u32> = settings.huds.iter().map(|hud| hud.id).collect();
    if trackers.is_empty() {
        if state.drag.is_some_and(|drag| drag.id == TRACKER) {
            state.drag = None;
        }
    } else {
        outcome.changed |= settings.tracker.is_none();
        settings.tracker();
        ids.push(TRACKER);
    }
    let look = Look::of(ui.visuals(), settings.opacity());
    let painter = ui.painter().with_clip_rect(place.area);
    let mut taken = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(hud) = settings.get(id) else {
            continue;
        };
        let laid = if id == TRACKER {
            paint::lay_lines(&painter, hud, trackers, look, place.area.size())
        } else {
            paint::lay(&painter, hud, facts, look, place.area.size())
        };
        let dragging = state.drag.filter(|drag| drag.id == id);
        let rect = hud.rect(place.area, laid.size);
        let rect = match place.keep_below {
            Some(line) if place.editing && rect.top() < line => {
                rect.translate(vec2(0.0, line - rect.top()))
            }
            _ => rect,
        };
        let rect = if dragging.is_some() {
            rect
        } else {
            let rect = hud.clear_of(rect, &taken, place.area);
            taken.push(rect);
            rect
        };
        if let Some(drag) = dragging {
            for (axis, line) in drag.guides.iter().enumerate() {
                if let Some(line) = *line {
                    guide(&painter, place, axis == 0, line);
                }
            }
        }
        paint::paint(&painter, rect.min, &laid, facts, look);
        state.placed.push((id, rect));
        if place.editing {
            continue;
        }
        let key = Id::new(("hud", id));
        if place.touch {
            outcome = merge(outcome, touch(ui, place, settings, state, id, rect));
            continue;
        }
        let response = ui
            .interact(rect, key, Sense::click_and_drag())
            .on_hover_cursor(if dragging.is_some() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        let corner = Rect::from_min_max(rect.max - Vec2::splat(HANDLE), rect.max);
        let handle = ui
            .interact(corner, key.with("size"), Sense::drag())
            .on_hover_cursor(CursorIcon::ResizeNwSe);
        if response.hovered() || handle.hovered() || dragging.is_some_and(|drag| drag.resize) {
            grip(&painter, corner, look);
        }
        for (response, resize) in [(&response, false), (&handle, true)] {
            if response.drag_started() {
                let size = settings.get(id).map_or(1.0, |hud| hud.scale());
                state.drag = Some(Drag::new(id, resize, rect, size));
            }
            if response.dragged()
                && let Some(drag) = state.drag.as_mut().filter(|drag| drag.id == id)
            {
                drag.moved += response.drag_delta();
                follow(settings, place, drag);
            }
            if response.drag_stopped() && state.drag.is_some_and(|drag| drag.id == id) {
                state.drag = None;
                outcome.changed = true;
            }
        }
        if response.secondary_clicked() && id != TRACKER {
            state.editor = Some(id);
        }
        let mut open = state.editor == Some(id);
        if open {
            let HudSettings { huds, opacity, .. } = &mut *settings;
            if let Some(hud) = huds.iter_mut().find(|hud| hud.id == id) {
                let shown = editor::popup(ui, &response, hud, opacity, facts, &mut open);
                outcome.changed |= shown.changed;
                if shown.remove {
                    outcome.remove = Some(id);
                    open = false;
                }
            }
            if !open {
                state.editor = None;
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                }
            }
        }
    }
    outcome
}

fn merge(a: Outcome, b: Outcome) -> Outcome {
    Outcome {
        changed: a.changed || b.changed,
        remove: a.remove.or(b.remove),
        edit_sheet: a.edit_sheet.or(b.edit_sheet),
    }
}

fn follow(settings: &mut HudSettings, place: &Place, drag: &mut Drag) {
    let Some(hud) = settings.get_mut(drag.id) else {
        return;
    };
    if drag.resize || (drag.zoom - 1.0).abs() > 0.001 {
        let grow = if drag.resize {
            (drag.start.width() + drag.moved.x) / drag.start.width().max(1.0)
        } else {
            drag.zoom
        };
        let size = ((drag.size * grow) * 20.0).round() / 20.0;
        hud.size = size.clamp(*SIZES.start(), *SIZES.end());
        let ratio = hud.size / drag.size;
        let rect = Rect::from_min_size(
            drag.start.min + if drag.resize { Vec2::ZERO } else { drag.moved },
            drag.start.size() * ratio,
        );
        drag.guides = [None, None];
        hud.place(place.area, inside(rect, place.area));
        return;
    }
    let (rect, guides) = snap(drag.start.translate(drag.moved), place);
    drag.guides = guides;
    hud.place(place.area, inside(rect, place.area));
}

fn touch(
    ui: &Ui,
    place: &Place,
    settings: &mut HudSettings,
    state: &mut HudState,
    id: u32,
    rect: Rect,
) -> Outcome {
    let mut outcome = Outcome::default();
    let (down, origin, at, held, zoom, delta) = ui.input(|i| {
        let pointer = &i.pointer;
        (
            pointer.primary_down() || i.any_touches(),
            pointer.press_origin(),
            pointer.interact_pos(),
            pointer.press_start_time().map(|start| i.time - start),
            i.zoom_delta(),
            pointer.delta(),
        )
    });
    match state.drag.filter(|drag| drag.id == id) {
        Some(mut drag) if down => {
            drag.moved += delta;
            drag.zoom *= zoom;
            drag.lifted = true;
            follow(settings, place, &mut drag);
            state.drag = Some(drag);
            ui.ctx().request_repaint();
        }
        Some(drag) => {
            state.drag = None;
            if drag.changed() {
                outcome.changed = true;
            } else if id != TRACKER {
                outcome.edit_sheet = Some(id);
            }
        }
        None if down && state.drag.is_none() => {
            let lift = origin.is_some_and(|origin| rect.contains(origin))
                && at
                    .zip(origin)
                    .is_some_and(|(at, origin)| at.distance(origin) <= LIFT_SLOP);
            if lift {
                if held.is_some_and(|held| held >= LIFT_AFTER) {
                    let size = settings.get(id).map_or(1.0, |hud| hud.scale());
                    let mut drag = Drag::new(id, false, rect, size);
                    drag.lifted = true;
                    state.drag = Some(drag);
                } else {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(60));
                }
            }
        }
        None => {}
    }
    if state.drag.is_some_and(|drag| drag.id == id) || state.editor == Some(id) {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            8.0,
            Stroke::new(2.0_f32, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Outside,
        );
    }
    outcome
}

fn guide(painter: &egui::Painter, place: &Place, vertical: bool, line: f32) {
    let bounds = place.picture.unwrap_or(place.area);
    let stroke = Stroke::new(
        1.0_f32,
        painter.ctx().style().visuals.selection.stroke.color,
    );
    let (from, to) = if vertical {
        (pos2(line, bounds.top()), pos2(line, bounds.bottom()))
    } else {
        (pos2(bounds.left(), line), pos2(bounds.right(), line))
    };
    painter.line_segment([from, to], stroke);
}

fn grip(painter: &egui::Painter, corner: Rect, look: Look) {
    let stroke = Stroke::new(1.2_f32, look.text());
    let tip: Pos2 = corner.max - vec2(3.0, 3.0);
    for step in [4.0, 8.0] {
        painter.line_segment([tip - vec2(step, 0.0), tip - vec2(0.0, step)], stroke);
    }
}
