use crate::app::ui_state::keybinds::GbButton;
use crate::app::ui_state::touch::TouchSettings;
use citrine_gb::gb::joypad::JoypadState;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

const DPAD_DEAD_ZONE: f32 = 0.22;

const DPAD_ARM: f32 = 1.0 / 3.0;

const FACE_RADIUS: f32 = 0.23;

const FACE_SPREAD: f32 = 1.15;

#[derive(Debug, Clone, PartialEq)]
pub struct TouchLayout {
    dpad: Rect,
    face: Vec<(GbButton, Pos2, f32)>,
    pills: Vec<(GbButton, Rect)>,
    face_slop: f32,
}

impl TouchLayout {
    fn bar_padding(width: f32) -> f32 {
        (width * 0.03).clamp(4.0, 24.0)
    }

    fn pill_height(width: f32) -> f32 {
        (width * 0.11).clamp(22.0, 54.0)
    }

    pub fn bar_height(width: f32, settings: &TouchSettings) -> f32 {
        let pad = Self::bar_padding(width);
        let dpad = (width - pad * 2.0) * 0.42 * settings.clamped_scale();
        pad * 3.0 + dpad + Self::pill_height(width)
    }

    pub fn bar(area: Rect, settings: &TouchSettings) -> Self {
        let scale = settings.clamped_scale();
        let pad = Self::bar_padding(area.width());
        let usable = area.shrink(pad);

        let pill_height = Self::pill_height(area.width()).min(usable.height() * 0.4);
        let upper = Rect::from_min_max(
            usable.min,
            pos2(usable.max.x, usable.max.y - pill_height - pad),
        );

        let dpad_size = (upper.width() * 0.42 * scale).min(upper.height());
        let face_radius = dpad_size * FACE_RADIUS;

        let dpad_centre = pos2(upper.left() + dpad_size / 2.0, upper.center().y);
        let face_centre = pos2(
            upper.right() - face_radius * (1.0 + FACE_SPREAD),
            upper.center().y,
        );

        let pill_width = (usable.width() * 0.26).clamp(60.0, 160.0);
        let pill_gap = pill_width * 0.2;
        let pill_y = usable.max.y - pill_height / 2.0;
        let select_centre = pos2(
            usable.center().x - pill_width / 2.0 - pill_gap / 2.0,
            pill_y,
        );
        let start_centre = pos2(
            usable.center().x + pill_width / 2.0 + pill_gap / 2.0,
            pill_y,
        );

        Self::assemble(
            dpad_centre,
            dpad_size,
            face_centre,
            face_radius,
            [
                (GbButton::Select, select_centre),
                (GbButton::Start, start_centre),
            ],
            vec2(pill_width, pill_height),
            settings.mirrored,
            area,
        )
    }

    pub fn columns(left: Rect, right: Rect, settings: &TouchSettings) -> Self {
        let scale = settings.clamped_scale();
        let pad = (left.width() * 0.06).clamp(4.0, 20.0);
        let dpad_area = left.shrink(pad);
        let face_area = right.shrink(pad);

        let pill_height = (dpad_area.height() * 0.12).clamp(20.0, 48.0);
        let column_height = dpad_area.height() - pill_height - pad;

        let dpad_size = (dpad_area.width() * 0.92 * scale).min(column_height);
        let face_radius = dpad_size * FACE_RADIUS;

        let upper_y = dpad_area.top() + column_height / 2.0;
        let dpad_centre = pos2(dpad_area.center().x, upper_y);
        let face_centre = pos2(face_area.center().x, upper_y);

        let pill_width = (dpad_area.width() * 0.8).clamp(56.0, 150.0);
        let pill_y = dpad_area.max.y - pill_height / 2.0;

        Self::assemble(
            dpad_centre,
            dpad_size,
            face_centre,
            face_radius,
            [
                (GbButton::Select, pos2(dpad_area.center().x, pill_y)),
                (GbButton::Start, pos2(face_area.center().x, pill_y)),
            ],
            vec2(pill_width, pill_height),
            settings.mirrored,
            left.union(right),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn assemble(
        dpad_centre: Pos2,
        dpad_size: f32,
        face_centre: Pos2,
        face_radius: f32,
        pills: [(GbButton, Pos2); 2],
        pill_size: Vec2,
        mirrored: bool,
        bounds: Rect,
    ) -> Self {
        let flip = |p: Pos2| {
            if mirrored {
                pos2(bounds.left() + bounds.right() - p.x, p.y)
            } else {
                p
            }
        };

        let dpad_centre = flip(dpad_centre);
        let face_centre = flip(face_centre);

        let spread = face_radius * FACE_SPREAD;
        let (a_offset, b_offset) = if mirrored {
            (vec2(-spread, -spread), vec2(spread, spread))
        } else {
            (vec2(spread, -spread), vec2(-spread, spread))
        };

        Self {
            dpad: Rect::from_center_size(dpad_centre, Vec2::splat(dpad_size)),
            face: vec![
                (GbButton::A, face_centre + a_offset, face_radius),
                (GbButton::B, face_centre + b_offset, face_radius),
            ],
            pills: pills
                .into_iter()
                .map(|(button, centre)| (button, Rect::from_center_size(flip(centre), pill_size)))
                .collect(),
            face_slop: face_radius * 0.35,
        }
    }

    pub fn hit(&self, pos: Pos2) -> JoypadState {
        let mut state = JoypadState::empty();

        if self.dpad.contains(pos) {
            state |= dpad_direction(self.dpad, pos);
        }

        for (button, centre, radius) in &self.face {
            if centre.distance(pos) <= radius + self.face_slop {
                state |= button.joypad();
            }
        }

        for (button, rect) in &self.pills {
            if rect.contains(pos) {
                state |= button.joypad();
            }
        }

        state
    }

    pub fn bounds(&self) -> Rect {
        let mut rect = self.dpad;
        for (_, centre, radius) in &self.face {
            rect = rect.union(Rect::from_center_size(*centre, Vec2::splat(radius * 2.0)));
        }
        for (_, pill) in &self.pills {
            rect = rect.union(*pill);
        }
        rect
    }

    pub fn paint(&self, ui: &Ui, held: JoypadState, settings: &TouchSettings) {
        let alpha = settings.clamped_opacity();
        let visuals = ui.visuals();
        let painter = ui.painter();

        let idle = visuals.widgets.inactive.bg_fill.gamma_multiply(alpha);
        let down = visuals.widgets.active.bg_fill.gamma_multiply(alpha);
        let edge = Stroke::new(
            1.0_f32,
            visuals
                .widgets
                .inactive
                .bg_stroke
                .color
                .gamma_multiply(alpha),
        );
        let ink = visuals.strong_text_color().gamma_multiply(alpha);

        self.paint_dpad(ui, held, idle, down, edge, ink);

        for (button, centre, radius) in &self.face {
            let pressed = held.contains(button.joypad());
            painter.circle(*centre, *radius, if pressed { down } else { idle }, edge);
            painter.text(
                *centre,
                egui::Align2::CENTER_CENTER,
                button.to_string(),
                FontId::proportional(radius * 0.9),
                ink,
            );
        }

        for (button, rect) in &self.pills {
            let pressed = held.contains(button.joypad());
            painter.rect(
                *rect,
                rect.height() / 2.0,
                if pressed { down } else { idle },
                edge,
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                button.to_string().to_uppercase(),
                FontId::proportional((rect.height() * 0.42).max(9.0)),
                ink,
            );
        }
    }

    fn paint_dpad(
        &self,
        ui: &Ui,
        held: JoypadState,
        idle: Color32,
        down: Color32,
        edge: Stroke,
        ink: Color32,
    ) {
        let painter = ui.painter();
        let arm = self.dpad.width() * DPAD_ARM;
        let round = arm * 0.28;
        let centre = self.dpad.center();

        let horizontal = Rect::from_center_size(centre, vec2(self.dpad.width(), arm));
        let vertical = Rect::from_center_size(centre, vec2(arm, self.dpad.height()));

        painter.rect(horizontal, round, idle, edge, egui::StrokeKind::Inside);
        painter.rect(vertical, round, idle, edge, egui::StrokeKind::Inside);

        for button in GbButton::DPAD {
            if held.contains(button.joypad()) {
                painter.rect_filled(self.arm_rect(button), round, down);
            }
            painter.add(egui::Shape::convex_polygon(
                self.arrow(button),
                ink,
                Stroke::NONE,
            ));
        }
    }

    fn arm_rect(&self, button: GbButton) -> Rect {
        let arm = self.dpad.width() * DPAD_ARM;
        let c = self.dpad.center();
        let half = arm / 2.0;
        match button {
            GbButton::Up => Rect::from_min_max(
                pos2(c.x - half, self.dpad.top()),
                pos2(c.x + half, c.y + half),
            ),
            GbButton::Down => Rect::from_min_max(
                pos2(c.x - half, c.y - half),
                pos2(c.x + half, self.dpad.bottom()),
            ),
            GbButton::Left => Rect::from_min_max(
                pos2(self.dpad.left(), c.y - half),
                pos2(c.x + half, c.y + half),
            ),
            GbButton::Right => Rect::from_min_max(
                pos2(c.x - half, c.y - half),
                pos2(self.dpad.right(), c.y + half),
            ),
            _ => self.dpad,
        }
    }

    fn arrow(&self, button: GbButton) -> Vec<Pos2> {
        let size = self.dpad.width() * 0.09;
        let reach = self.dpad.width() * 0.34;
        let c = self.dpad.center();
        let (tip, back) = match button {
            GbButton::Up => (pos2(c.x, c.y - reach), vec2(0.0, size)),
            GbButton::Down => (pos2(c.x, c.y + reach), vec2(0.0, -size)),
            GbButton::Left => (pos2(c.x - reach, c.y), vec2(size, 0.0)),
            GbButton::Right => (pos2(c.x + reach, c.y), vec2(-size, 0.0)),
            _ => (c, Vec2::ZERO),
        };
        let side = vec2(back.y, back.x);
        vec![tip, tip + back + side, tip + back - side]
    }
}

fn dpad_direction(dpad: Rect, pos: Pos2) -> JoypadState {
    let offset = pos - dpad.center();
    let radius = dpad.width() / 2.0;
    if radius <= 0.0 || offset.length() / radius < DPAD_DEAD_ZONE {
        return JoypadState::empty();
    }

    let sector = (offset.y.atan2(offset.x) / std::f32::consts::FRAC_PI_4).round() as i32;
    match sector.rem_euclid(8) {
        0 => JoypadState::RIGHT,
        1 => JoypadState::RIGHT | JoypadState::DOWN,
        2 => JoypadState::DOWN,
        3 => JoypadState::DOWN | JoypadState::LEFT,
        4 => JoypadState::LEFT,
        5 => JoypadState::LEFT | JoypadState::UP,
        6 => JoypadState::UP,
        _ => JoypadState::UP | JoypadState::RIGHT,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contact {
    Finger(u64),
    Pointer,
}

#[derive(Default)]
pub struct TouchInput {
    contacts: Vec<(Contact, JoypadState)>,
    previous: JoypadState,
}

impl TouchInput {
    pub fn update(&mut self, ui: &Ui, layout: &TouchLayout) -> JoypadState {
        let ctx = ui.ctx();
        let (events, any_touches, pointer) = ctx.input(|i| {
            (
                i.events.clone(),
                i.any_touches(),
                i.pointer.interact_pos().filter(|_| i.pointer.any_down()),
            )
        });

        self.forget_stale_fingers(any_touches);

        for event in &events {
            let egui::Event::Touch { id, phase, pos, .. } = event else {
                continue;
            };
            let contact = Contact::Finger(id.0);
            match phase {
                egui::TouchPhase::Start | egui::TouchPhase::Move => {
                    self.set(contact, layout.hit(*pos));
                }
                egui::TouchPhase::End | egui::TouchPhase::Cancel => self.clear(contact),
            }
        }

        if any_touches {
            self.clear(Contact::Pointer);
        } else {
            match pointer {
                Some(pos) => self.set(Contact::Pointer, layout.hit(pos)),
                None => self.clear(Contact::Pointer),
            }
        }

        self.combined()
    }

    fn combined(&self) -> JoypadState {
        self.contacts
            .iter()
            .fold(JoypadState::empty(), |acc, (_, state)| acc | *state)
    }

    pub fn transitions(&mut self, held: JoypadState) -> (JoypadState, JoypadState) {
        let pressed = held & !self.previous;
        let released = self.previous & !held;
        self.previous = held;
        (pressed, released)
    }

    #[must_use = "the buttons that were held still need releasing"]
    pub fn reset(&mut self) -> JoypadState {
        self.contacts.clear();
        std::mem::replace(&mut self.previous, JoypadState::empty())
    }

    fn forget_stale_fingers(&mut self, any_touches: bool) {
        if !any_touches {
            self.contacts
                .retain(|(contact, _)| !matches!(contact, Contact::Finger(_)));
        }
    }

    fn set(&mut self, contact: Contact, state: JoypadState) {
        match self.contacts.iter_mut().find(|(c, _)| *c == contact) {
            Some(entry) => entry.1 = state,
            None => self.contacts.push((contact, state)),
        }
    }

    fn clear(&mut self, contact: Contact) {
        self.contacts.retain(|(c, _)| *c != contact);
    }
}

pub fn claim(ui: &mut Ui, rect: Rect) {
    ui.allocate_rect(rect, Sense::click_and_drag());
}

pub fn vibrate(_duration_ms: u32) {
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().vibrate_with_duration(_duration_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> TouchLayout {
        TouchLayout::bar(
            Rect::from_min_size(Pos2::ZERO, vec2(400.0, 260.0)),
            &TouchSettings::default(),
        )
    }

    #[track_caller]
    fn assert_buttons(actual: JoypadState, expected: JoypadState, context: &str) {
        assert_eq!(
            actual.bits(),
            expected.bits(),
            "{context}: expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn the_dpad_reads_eight_directions() {
        let layout = layout();
        let c = layout.dpad.center();
        let reach = layout.dpad.width() * 0.4;

        assert_buttons(layout.hit(pos2(c.x, c.y - reach)), JoypadState::UP, "up");
        assert_buttons(
            layout.hit(pos2(c.x, c.y + reach)),
            JoypadState::DOWN,
            "down",
        );
        assert_buttons(
            layout.hit(pos2(c.x - reach, c.y)),
            JoypadState::LEFT,
            "left",
        );
        assert_buttons(
            layout.hit(pos2(c.x + reach, c.y)),
            JoypadState::RIGHT,
            "right",
        );

        let diagonal = reach * 0.72;
        assert_buttons(
            layout.hit(pos2(c.x + diagonal, c.y - diagonal)),
            JoypadState::UP | JoypadState::RIGHT,
            "diagonals matter for most games",
        );
        assert_buttons(
            layout.hit(pos2(c.x - diagonal, c.y + diagonal)),
            JoypadState::DOWN | JoypadState::LEFT,
            "down-left",
        );
    }

    #[test]
    fn the_middle_of_the_dpad_presses_nothing() {
        let layout = layout();
        assert_buttons(
            layout.hit(layout.dpad.center()),
            JoypadState::empty(),
            "dead zone",
        );
    }

    #[test]
    fn face_and_menu_buttons_are_hit() {
        let layout = layout();
        for (button, centre, _) in &layout.face {
            assert_buttons(layout.hit(*centre), button.joypad(), &button.to_string());
        }
        for (button, rect) in &layout.pills {
            assert_buttons(
                layout.hit(rect.center()),
                button.joypad(),
                &button.to_string(),
            );
        }
    }

    #[test]
    fn empty_space_presses_nothing() {
        let layout = layout();
        assert_buttons(
            layout.hit(pos2(-50.0, -50.0)),
            JoypadState::empty(),
            "outside the controls",
        );
    }

    #[test]
    fn a_and_b_do_not_overlap() {
        let layout = layout();
        let (_, a, _) = layout.face[0];
        let (_, b, _) = layout.face[1];
        assert_buttons(layout.hit(a), JoypadState::A, "A is unambiguous");
        assert_buttons(layout.hit(b), JoypadState::B, "B is unambiguous");
    }

    #[test]
    fn every_button_stays_inside_the_control_area() {
        let area = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 260.0));
        for settings in [
            TouchSettings::default(),
            TouchSettings {
                mirrored: true,
                ..Default::default()
            },
            TouchSettings {
                scale: TouchSettings::MAX_SCALE,
                ..Default::default()
            },
        ] {
            let layout = TouchLayout::bar(area, &settings);
            let bounds = layout.bounds();
            assert!(
                area.contains_rect(bounds),
                "controls spilled out of {area:?}: {bounds:?} (scale {}, mirrored {})",
                settings.scale,
                settings.mirrored
            );
        }
    }

    #[test]
    fn mirroring_swaps_the_sides() {
        let area = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 260.0));
        let normal = TouchLayout::bar(area, &TouchSettings::default());
        let mirrored = TouchLayout::bar(
            area,
            &TouchSettings {
                mirrored: true,
                ..Default::default()
            },
        );

        assert!(
            normal.dpad.center().x < area.center().x,
            "d-pad starts left"
        );
        assert!(
            mirrored.dpad.center().x > area.center().x,
            "and moves right when mirrored"
        );
    }

    #[test]
    fn several_fingers_hold_several_buttons() {
        let layout = layout();
        let mut input = TouchInput::default();
        let c = layout.dpad.center();
        let (_, a_centre, _) = layout.face[0];

        input.set(Contact::Finger(1), layout.hit(pos2(c.x - 40.0, c.y)));
        input.set(Contact::Finger(2), layout.hit(a_centre));

        assert_buttons(
            input.combined(),
            JoypadState::LEFT | JoypadState::A,
            "two fingers, two buttons",
        );

        input.clear(Contact::Finger(1));
        assert_buttons(
            input.combined(),
            JoypadState::A,
            "lifting one finger keeps the other",
        );
    }

    #[test]
    fn only_transitions_are_reported() {
        let mut input = TouchInput::default();

        let (pressed, released) = input.transitions(JoypadState::A);
        assert_buttons(pressed, JoypadState::A, "first press");
        assert_buttons(released, JoypadState::empty(), "nothing let go yet");

        let (pressed, released) = input.transitions(JoypadState::A);
        assert_buttons(
            pressed,
            JoypadState::empty(),
            "still held is not a new press",
        );
        assert_buttons(
            released,
            JoypadState::empty(),
            "still held is not a release",
        );

        let (pressed, released) = input.transitions(JoypadState::B);
        assert_buttons(pressed, JoypadState::B, "the new button");
        assert_buttons(released, JoypadState::A, "and the one let go");
    }

    #[test]
    fn a_finger_that_never_lifts_does_not_stay_stuck() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.set(Contact::Finger(1), layout.hit(layout.face[0].1));
        assert_buttons(input.combined(), JoypadState::A, "held while touching");

        input.forget_stale_fingers(false);

        assert_buttons(
            input.combined(),
            JoypadState::empty(),
            "released once nothing is touching the screen",
        );
    }

    #[test]
    fn a_finger_still_down_is_kept() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.set(Contact::Finger(1), layout.hit(layout.face[0].1));

        input.forget_stale_fingers(true);

        assert_buttons(input.combined(), JoypadState::A, "still touching");
    }

    #[test]
    fn a_button_held_elsewhere_is_left_alone() {
        let mut input = TouchInput::default();
        let (pressed, released) = input.transitions(JoypadState::empty());
        assert_buttons(pressed, JoypadState::empty(), "nothing pressed");
        assert_buttons(released, JoypadState::empty(), "and nothing released");
    }

    #[test]
    fn resetting_reports_what_it_was_holding() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.set(Contact::Finger(1), layout.hit(layout.face[0].1));
        let held = input.combined();
        input.transitions(held);

        let stuck = input.reset();

        assert!(input.contacts.is_empty());
        assert_buttons(stuck, JoypadState::A, "so the caller can release it");
        assert_buttons(input.previous, JoypadState::empty(), "after reset");
    }

    #[test]
    fn the_columns_layout_keeps_the_dpad_and_buttons_apart() {
        let left = Rect::from_min_size(Pos2::ZERO, vec2(180.0, 320.0));
        let right = Rect::from_min_size(pos2(600.0, 0.0), vec2(180.0, 320.0));
        let layout = TouchLayout::columns(left, right, &TouchSettings::default());

        assert!(left.contains(layout.dpad.center()));
        for (_, centre, _) in &layout.face {
            assert!(right.expand(40.0).contains(*centre));
        }
    }
}
