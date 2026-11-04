use crate::app::ui_state::keybinds::GbButton;
use crate::app::ui_state::touch::TouchSettings;
use citrine_gb::console::Keys;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

mod dpad;

const DPAD_ARM: f32 = 1.0 / 3.0;

const DPAD_SHARE: f32 = 0.42;

const DPAD_START_MARGIN: f32 = 14.0;

const EDGE_CLEAR: f32 = 24.0;

const FACE_RADIUS: f32 = 0.22;

const FACE_SPREAD: f32 = 1.15;

const FACE_REACH: f32 = 1.6;

const BOTH_RADIUS: f32 = 0.5;

const SHOULDER_MIN_HEIGHT: f32 = 44.0;

const SHOULDER_WIDTH: f32 = 0.7;

#[derive(Debug, Clone, PartialEq)]
pub struct TouchLayout {
    dpad: Rect,
    dpad_start: Rect,
    face: Vec<(GbButton, Pos2, f32)>,
    both: (Pos2, f32),
    pills: Vec<(Keys, &'static str, Rect)>,
}

impl TouchLayout {
    fn bar_padding(width: f32) -> f32 {
        (width * 0.03).clamp(4.0, 24.0)
    }

    fn edge_clear(width: f32) -> f32 {
        EDGE_CLEAR.min(width * 0.1)
    }

    fn dpad_size(width: f32, scale: f32) -> f32 {
        let room = (width - Self::edge_clear(width) * 2.0) / 2.0;
        (width * DPAD_SHARE * scale).min(room)
    }

    fn pill_height(width: f32) -> f32 {
        (width * 0.11).clamp(22.0, 54.0)
    }

    fn shoulder_height(pill_height: f32) -> f32 {
        pill_height.max(SHOULDER_MIN_HEIGHT)
    }

    pub fn bar_height(width: f32, settings: &TouchSettings, shoulders: bool) -> f32 {
        let pad = Self::bar_padding(width);
        let dpad = Self::dpad_size(width, settings.clamped_scale());
        let pill = Self::pill_height(width);
        let shoulder = if shoulders {
            Self::shoulder_height(pill) + pad
        } else {
            0.0
        };
        pad * 3.0 + dpad + pill + shoulder
    }

    pub fn bar(area: Rect, settings: &TouchSettings, shoulders: bool) -> Self {
        let scale = settings.clamped_scale();
        let pad = Self::bar_padding(area.width());
        let usable = area.shrink2(vec2(Self::edge_clear(area.width()), pad));

        let pill_height = Self::pill_height(area.width()).min(usable.height() * 0.4);
        let shoulder_height = if shoulders {
            Self::shoulder_height(pill_height).min(usable.height() * 0.25)
        } else {
            0.0
        };
        let upper_top = if shoulders {
            usable.top() + shoulder_height + pad
        } else {
            usable.top()
        };
        let upper = Rect::from_min_max(
            pos2(usable.min.x, upper_top),
            pos2(usable.max.x, usable.max.y - pill_height - pad),
        );

        let dpad_size = Self::dpad_size(area.width(), scale).min(upper.height());
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

        let mut layout = Self::assemble(
            dpad_centre,
            dpad_size,
            face_centre,
            face_radius,
            [(Keys::SELECT, select_centre), (Keys::START, start_centre)],
            vec2(pill_width, pill_height),
            settings.mirrored,
            area,
        );
        if shoulders {
            let size = vec2(
                (dpad_size * SHOULDER_WIDTH).max(pill_width),
                shoulder_height,
            );
            layout.add_shoulders(usable.left(), usable.right(), usable.top(), size);
        }
        layout
    }

    fn add_shoulders(&mut self, left: f32, right: f32, top: f32, size: Vec2) {
        self.pills
            .push((Keys::L, "L", Rect::from_min_size(pos2(left, top), size)));
        self.pills.push((
            Keys::R,
            "R",
            Rect::from_min_size(pos2(right - size.x, top), size),
        ));
    }

    pub fn columns(left: Rect, right: Rect, settings: &TouchSettings, shoulders: bool) -> Self {
        let scale = settings.clamped_scale();
        let pad = (left.width() * 0.06).clamp(4.0, 20.0);
        let side = EDGE_CLEAR.max(pad).min(left.width() * 0.2);
        let dpad_area = left.shrink2(vec2(side, pad));
        let face_area = right.shrink2(vec2(side, pad));

        let pill_height = (dpad_area.height() * 0.12).clamp(20.0, 48.0);
        let shoulder_height = if shoulders {
            Self::shoulder_height(pill_height).min(dpad_area.height() * 0.2)
        } else {
            0.0
        };
        let shoulder_top = dpad_area.top().max(face_area.top());
        let column_top = if shoulders {
            shoulder_top + shoulder_height + pad
        } else {
            dpad_area.top()
        };
        let column_height = dpad_area.bottom() - column_top - pill_height - pad;

        let dpad_size = (dpad_area.width() * 0.92 * scale)
            .min(dpad_area.width() + side)
            .min(column_height);
        let face_radius = dpad_size * FACE_RADIUS;

        let upper_y = column_top + column_height / 2.0;
        let dpad_centre = pos2(dpad_area.center().x, upper_y);
        let face_centre = pos2(face_area.center().x, upper_y);

        let pill_width = (dpad_area.width() * 0.8).clamp(56.0, 150.0);
        let pill_y = dpad_area.max.y - pill_height / 2.0;

        let mut layout = Self::assemble(
            dpad_centre,
            dpad_size,
            face_centre,
            face_radius,
            [
                (Keys::SELECT, pos2(dpad_area.center().x, pill_y)),
                (Keys::START, pos2(face_area.center().x, pill_y)),
            ],
            vec2(pill_width, pill_height),
            settings.mirrored,
            left.union(right),
        );
        if shoulders {
            let width = pill_width.min(dpad_area.width());
            let size = vec2(width, shoulder_height);
            layout.add_shoulders(
                dpad_area.center().x - width / 2.0,
                face_area.center().x + width / 2.0,
                shoulder_top,
                size,
            );
        }
        layout
    }

    #[allow(clippy::too_many_arguments)]
    fn assemble(
        dpad_centre: Pos2,
        dpad_size: f32,
        face_centre: Pos2,
        face_radius: f32,
        pills: [(Keys, Pos2); 2],
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

        let dpad = Rect::from_center_size(dpad_centre, Vec2::splat(dpad_size));
        Self {
            dpad,
            dpad_start: dpad
                .expand(DPAD_START_MARGIN)
                .intersect(bounds.shrink2(vec2(Self::edge_clear(bounds.width()), 0.0))),
            face: vec![
                (GbButton::A, face_centre + a_offset, face_radius),
                (GbButton::B, face_centre + b_offset, face_radius),
            ],
            both: (face_centre, face_radius * BOTH_RADIUS),
            pills: pills
                .into_iter()
                .map(|(keys, centre)| {
                    let label = if keys == Keys::SELECT {
                        "SELECT"
                    } else {
                        "START"
                    };
                    (keys, label, Rect::from_center_size(flip(centre), pill_size))
                })
                .collect(),
        }
    }

    pub fn starts_dpad(&self, pos: Pos2) -> bool {
        self.dpad_start.contains(pos) && self.dpad.distance_to_pos(pos) <= self.nearest_button(pos)
    }

    pub fn steer(&self, pos: Pos2, held: Keys) -> Keys {
        dpad::direction(pos - self.dpad.center(), self.dpad.width() / 2.0, held)
    }

    pub fn buttons(&self, pos: Pos2) -> Keys {
        let (centre, radius) = self.both;
        if centre.distance(pos) <= radius {
            return Keys::A | Keys::B;
        }

        let face = self.face.iter().filter_map(|(button, centre, radius)| {
            let gap = centre.distance(pos) - radius;
            (gap <= radius * (FACE_REACH - 1.0)).then_some((button, gap.max(0.0)))
        });
        let pills = self
            .pills
            .iter()
            .filter(|(_, _, rect)| rect.contains(pos))
            .map(|(keys, _, _)| (*keys, 0.0));

        face.map(|(button, gap)| (button.key(), gap))
            .chain(pills)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(Keys::empty(), |(keys, _)| keys)
    }

    fn nearest_button(&self, pos: Pos2) -> f32 {
        let face = self
            .face
            .iter()
            .map(|(_, centre, radius)| (centre.distance(pos) - radius).max(0.0));
        let pills = self
            .pills
            .iter()
            .map(|(_, _, rect)| rect.distance_to_pos(pos));
        face.chain(pills).fold(f32::INFINITY, f32::min)
    }

    pub fn reach(&self) -> Rect {
        self.bounds().union(self.dpad_start)
    }

    pub fn bounds(&self) -> Rect {
        let mut rect = self.dpad;
        for (_, centre, radius) in &self.face {
            rect = rect.union(Rect::from_center_size(*centre, Vec2::splat(radius * 2.0)));
        }
        for (_, _, pill) in &self.pills {
            rect = rect.union(*pill);
        }
        rect
    }

    pub fn paint(&self, ui: &Ui, held: Keys, settings: &TouchSettings) {
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
            let pressed = held.contains(button.key());
            painter.circle(*centre, *radius, if pressed { down } else { idle }, edge);
            painter.text(
                *centre,
                egui::Align2::CENTER_CENTER,
                button.to_string(),
                FontId::proportional(radius * 0.9),
                ink,
            );
        }

        for (keys, label, rect) in &self.pills {
            let pressed = held.contains(*keys);
            painter.rect(
                *rect,
                rect.height() / 2.0,
                if pressed { down } else { idle },
                edge,
                egui::StrokeKind::Inside,
            );
            let font = FontId::proportional(Self::pill_font_size(painter, *rect, label));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                font,
                ink,
            );
        }
    }

    fn pill_font_size(painter: &egui::Painter, rect: Rect, label: &str) -> f32 {
        let size = (rect.height() * 0.42).max(9.0);
        let room = rect.width() - rect.height() * 0.5;
        let width = painter
            .layout_no_wrap(label.to_owned(), FontId::proportional(size), Color32::WHITE)
            .size()
            .x;
        if width > room {
            (size * room / width).max(6.0)
        } else {
            size
        }
    }

    fn paint_dpad(
        &self,
        ui: &Ui,
        held: Keys,
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
            if held.contains(button.key()) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contact {
    Finger(u64),
    Pointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transitions {
    pub pressed: Keys,
    pub released: Keys,
    pub tick: bool,
}

#[derive(Default)]
pub struct TouchInput {
    contacts: Vec<(Contact, Keys)>,
    dpad_owner: Option<Contact>,
    previous: Keys,
}

impl TouchInput {
    pub fn update(&mut self, ui: &Ui, layout: &TouchLayout) -> Keys {
        let ctx = ui.ctx();
        let (events, any_touches, pointer) = ctx.input(|i| {
            (
                i.events.clone(),
                i.any_touches(),
                i.pointer
                    .interact_pos()
                    .filter(|_| i.pointer.any_down() && !i.has_touch_screen()),
            )
        });

        self.forget_stale_fingers(any_touches);
        if any_touches {
            self.lift(Contact::Pointer);
        }

        for event in &events {
            let egui::Event::Touch { id, phase, pos, .. } = event else {
                continue;
            };
            let contact = Contact::Finger(id.0);
            match phase {
                egui::TouchPhase::Start => self.press(contact, *pos, layout),
                egui::TouchPhase::Move => self.slide(contact, *pos, layout),
                egui::TouchPhase::End | egui::TouchPhase::Cancel => self.lift(contact),
            }
        }

        match pointer.filter(|_| !any_touches) {
            Some(pos) => self.slide(Contact::Pointer, pos, layout),
            None => self.lift(Contact::Pointer),
        }

        self.combined()
    }

    fn press(&mut self, contact: Contact, pos: Pos2, layout: &TouchLayout) {
        self.lift(contact);
        let state = if self.dpad_owner.is_none() && layout.starts_dpad(pos) {
            self.dpad_owner = Some(contact);
            layout.steer(pos, Keys::empty())
        } else {
            layout.buttons(pos)
        };
        self.set(contact, state);
    }

    fn slide(&mut self, contact: Contact, pos: Pos2, layout: &TouchLayout) {
        let Some(held) = self.held_by(contact) else {
            self.press(contact, pos, layout);
            return;
        };
        let state = if self.dpad_owner == Some(contact) {
            layout.steer(pos, held)
        } else {
            layout.buttons(pos)
        };
        self.set(contact, state);
    }

    fn lift(&mut self, contact: Contact) {
        self.clear(contact);
        if self.dpad_owner == Some(contact) {
            self.dpad_owner = None;
        }
    }

    fn held_by(&self, contact: Contact) -> Option<Keys> {
        self.contacts
            .iter()
            .find(|(c, _)| *c == contact)
            .map(|(_, state)| *state)
    }

    fn combined(&self) -> Keys {
        self.contacts
            .iter()
            .fold(Keys::empty(), |acc, (_, state)| acc | *state)
    }

    pub fn transitions(&mut self, held: Keys) -> Transitions {
        let pressed = held & !self.previous;
        let released = self.previous & !held;
        let steer = held & dpad::ALL;
        let turned = !steer.is_empty() && steer != self.previous & dpad::ALL;
        self.previous = held;
        Transitions {
            pressed,
            released,
            tick: !pressed.is_empty() || turned,
        }
    }

    #[must_use = "the buttons that were held still need releasing"]
    pub fn reset(&mut self) -> Keys {
        self.contacts.clear();
        self.dpad_owner = None;
        std::mem::replace(&mut self.previous, Keys::empty())
    }

    fn forget_stale_fingers(&mut self, any_touches: bool) {
        if !any_touches {
            self.contacts
                .retain(|(contact, _)| !matches!(contact, Contact::Finger(_)));
            if matches!(self.dpad_owner, Some(Contact::Finger(_))) {
                self.dpad_owner = None;
            }
        }
    }

    fn set(&mut self, contact: Contact, state: Keys) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn phone(width: f32) -> Rect {
        let height = TouchLayout::bar_height(width, &TouchSettings::default(), false) * 1.15;
        Rect::from_min_size(pos2(0.0, 600.0), vec2(width, height))
    }

    fn layout() -> TouchLayout {
        TouchLayout::bar(phone(412.0), &TouchSettings::default(), false)
    }

    #[track_caller]
    fn assert_buttons(actual: Keys, expected: Keys, context: &str) {
        assert_eq!(
            actual.bits(),
            expected.bits(),
            "{context}: expected {expected:?}, got {actual:?}"
        );
    }

    fn arm(layout: &TouchLayout, degrees: f32, reach: f32) -> Pos2 {
        let r = degrees.to_radians();
        layout.dpad.center() + vec2(r.cos(), r.sin()) * layout.dpad.width() / 2.0 * reach
    }

    #[test]
    fn the_dpad_reads_eight_directions() {
        let layout = layout();
        let mut input = TouchInput::default();
        for (degrees, expected) in [
            (270.0, Keys::UP),
            (90.0, Keys::DOWN),
            (180.0, Keys::LEFT),
            (0.0, Keys::RIGHT),
            (315.0, Keys::UP | Keys::RIGHT),
            (135.0, Keys::DOWN | Keys::LEFT),
        ] {
            input.press(Contact::Finger(1), arm(&layout, degrees, 0.7), &layout);
            assert_buttons(input.combined(), expected, &format!("{degrees} degrees"));
            input.lift(Contact::Finger(1));
        }
    }

    #[test]
    fn the_dpad_is_42_percent_of_the_width_in_portrait() {
        for (width, size) in [(412.0, 173.04), (360.0, 151.2)] {
            let layout = TouchLayout::bar(phone(width), &TouchSettings::default(), false);
            assert!(
                (layout.dpad.width() - size).abs() < 0.1,
                "{width}: d-pad {} wide",
                layout.dpad.width()
            );
        }
    }

    #[test]
    fn every_control_stays_24_px_clear_of_the_side_edges() {
        for width in [360.0, 412.0] {
            for mirrored in [false, true] {
                let area = phone(width);
                let settings = TouchSettings {
                    mirrored,
                    ..Default::default()
                };
                let layout = TouchLayout::bar(area, &settings, false);
                let reach = layout.reach();
                assert!(
                    reach.left() >= area.left() + 24.0 - 0.01
                        && reach.right() <= area.right() - 24.0 + 0.01,
                    "{width} (mirrored {mirrored}): {reach:?}"
                );
            }
        }
    }

    #[test]
    fn the_middle_of_the_dpad_presses_nothing() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), layout.dpad.center(), &layout);
        assert_buttons(input.combined(), Keys::empty(), "dead zone");
    }

    #[test]
    fn a_press_just_past_the_drawing_starts_the_dpad() {
        let layout = layout();
        let c = layout.dpad.center();
        let mut input = TouchInput::default();

        input.press(
            Contact::Finger(1),
            pos2(c.x, layout.dpad.top() - 12.0),
            &layout,
        );
        assert_buttons(input.combined(), Keys::UP, "12 px above the drawing");
        input.lift(Contact::Finger(1));

        input.press(
            Contact::Finger(1),
            pos2(c.x, layout.dpad.top() - 16.0),
            &layout,
        );
        assert_buttons(input.combined(), Keys::empty(), "16 px above it");
        assert_eq!(input.dpad_owner, None);
    }

    #[test]
    fn a_finger_past_the_drawn_edge_keeps_its_direction() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), arm(&layout, 0.0, 0.7), &layout);
        input.slide(Contact::Finger(1), arm(&layout, 5.0, 1.3), &layout);
        assert_buttons(input.combined(), Keys::RIGHT, "past the arm's tip");
        input.slide(Contact::Finger(1), arm(&layout, 5.0, 1.6), &layout);
        assert_buttons(input.combined(), Keys::empty(), "out of the ring");
        input.slide(Contact::Finger(1), arm(&layout, 5.0, 1.0), &layout);
        assert_buttons(input.combined(), Keys::RIGHT, "back in");
    }

    #[test]
    fn a_finger_from_the_dpad_never_presses_another_button() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), arm(&layout, 90.0, 0.7), &layout);
        for (_, button, rect) in &layout.pills {
            input.slide(Contact::Finger(1), rect.center(), &layout);
            assert!(
                (input.combined() & !dpad::ALL).is_empty(),
                "slid over {button}"
            );
        }
        for (button, centre, _) in &layout.face {
            input.slide(Contact::Finger(1), *centre, &layout);
            assert!(
                (input.combined() & !dpad::ALL).is_empty(),
                "slid over {button}"
            );
        }
    }

    #[test]
    fn the_first_finger_owns_the_dpad_until_it_lifts() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), arm(&layout, 0.0, 0.7), &layout);
        input.press(Contact::Finger(2), arm(&layout, 180.0, 0.7), &layout);
        assert_buttons(input.combined(), Keys::RIGHT, "a second finger");

        input.lift(Contact::Finger(1));
        input.press(Contact::Finger(3), arm(&layout, 180.0, 0.7), &layout);
        assert_buttons(input.combined(), Keys::LEFT, "free again");
    }

    #[test]
    fn a_slide_ticks_once_per_direction_change() {
        let layout = layout();
        let mut input = TouchInput::default();
        let mut ticks = 0;
        let mut seen = Vec::new();
        input.press(Contact::Finger(1), arm(&layout, 0.0, 0.7), &layout);
        for step in 0..=90 {
            input.slide(
                Contact::Finger(1),
                arm(&layout, -(step as f32), 0.7),
                &layout,
            );
            let held = input.combined();
            if input.transitions(held).tick {
                ticks += 1;
                seen.push(held);
            }
        }
        assert_eq!(seen, vec![Keys::RIGHT, Keys::RIGHT | Keys::UP, Keys::UP]);
        assert_eq!(ticks, 3);
    }

    #[test]
    fn ticks_come_on_presses_and_turns_never_on_releases() {
        let mut input = TouchInput::default();
        for (held, tick) in [
            (Keys::A, true),
            (Keys::A, false),
            (Keys::empty(), false),
            (Keys::RIGHT, true),
            (Keys::RIGHT | Keys::UP, true),
            (Keys::UP, true),
            (Keys::UP | Keys::B, true),
            (Keys::UP, false),
            (Keys::empty(), false),
        ] {
            assert_eq!(input.transitions(held).tick, tick, "{held:?}");
        }
    }

    #[test]
    fn face_and_menu_buttons_are_hit() {
        let layout = layout();
        for (button, centre, _) in &layout.face {
            assert_buttons(layout.buttons(*centre), button.key(), &button.to_string());
        }
        for (keys, label, rect) in &layout.pills {
            assert_buttons(layout.buttons(rect.center()), *keys, label);
        }
    }

    fn shoulder_phone(width: f32) -> (Rect, TouchLayout) {
        let height = TouchLayout::bar_height(width, &TouchSettings::default(), true) * 1.15;
        let area = Rect::from_min_size(pos2(0.0, 600.0), vec2(width, height));
        (
            area,
            TouchLayout::bar(area, &TouchSettings::default(), true),
        )
    }

    fn shoulder(layout: &TouchLayout, keys: Keys) -> Rect {
        layout
            .pills
            .iter()
            .find(|(bound, _, _)| *bound == keys)
            .map(|(_, _, rect)| *rect)
            .expect("the shoulder is laid out")
    }

    #[test]
    fn a_gb_layout_has_no_l_or_r() {
        for (keys, _, _) in &layout().pills {
            assert!(!keys.intersects(Keys::L | Keys::R));
        }
    }

    #[test]
    fn l_and_r_sit_at_the_top_corners_above_the_dpad_and_a_and_b() {
        let (area, layout) = shoulder_phone(412.0);
        let l = shoulder(&layout, Keys::L);
        let r = shoulder(&layout, Keys::R);
        assert!(l.center().x < area.center().x && r.center().x > area.center().x);
        assert!((l.top() - r.top()).abs() < 0.01);
        assert!(
            l.bottom() < layout.dpad.top(),
            "{l:?} over {:?}",
            layout.dpad
        );
        for (_, centre, radius) in &layout.face {
            assert!(r.bottom() < centre.y - radius);
        }
        for rect in [l, r] {
            assert!(rect.height() >= 44.0, "{rect:?} is a finger tall");
            assert!(area.contains_rect(rect));
        }
        assert!(area.contains_rect(layout.bounds()));
    }

    #[test]
    fn l_and_r_are_hit_and_nothing_else_is() {
        for width in [360.0, 412.0, 600.0] {
            let (_, layout) = shoulder_phone(width);
            let l = shoulder(&layout, Keys::L);
            let r = shoulder(&layout, Keys::R);
            assert_buttons(layout.buttons(l.center()), Keys::L, "L");
            assert_buttons(layout.buttons(r.center()), Keys::R, "R");
            assert_buttons(
                layout.buttons(l.left_top() + vec2(2.0, 2.0)),
                Keys::L,
                "L corner",
            );
            assert_buttons(
                layout.buttons(r.right_bottom() - vec2(2.0, 2.0)),
                Keys::R,
                "R corner",
            );
            let between = pos2(layout.dpad.center().x, l.bottom() + 1.0);
            assert!(
                !layout.buttons(between).intersects(Keys::L | Keys::R),
                "just below L presses no shoulder"
            );
            assert!(!layout.starts_dpad(l.center()), "L never starts the d-pad");
        }
    }

    #[test]
    fn a_finger_on_l_holds_l_beside_the_dpad() {
        let (_, layout) = shoulder_phone(412.0);
        let l = shoulder(&layout, Keys::L);
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), l.center(), &layout);
        input.press(Contact::Finger(2), arm(&layout, 0.0, 0.8), &layout);
        assert_buttons(input.combined(), Keys::L | Keys::RIGHT, "L and right");
        input.lift(Contact::Finger(1));
        assert_buttons(input.combined(), Keys::RIGHT, "L lifted");
    }

    #[test]
    fn the_shoulders_stay_put_when_the_layout_is_mirrored() {
        let area = Rect::from_min_size(Pos2::ZERO, vec2(412.0, 340.0));
        let settings = TouchSettings {
            mirrored: true,
            ..Default::default()
        };
        let layout = TouchLayout::bar(area, &settings, true);
        assert!(shoulder(&layout, Keys::L).center().x < area.center().x);
        assert!(shoulder(&layout, Keys::R).center().x > area.center().x);
        assert!(area.contains_rect(layout.bounds()));
    }

    #[test]
    fn the_shoulders_add_their_row_to_the_bar() {
        let settings = TouchSettings::default();
        let without = TouchLayout::bar_height(412.0, &settings, false);
        let with = TouchLayout::bar_height(412.0, &settings, true);
        assert!(with - without >= 44.0);
    }

    #[test]
    fn the_columns_put_l_and_r_on_top_of_each_column() {
        let left = Rect::from_min_size(Pos2::ZERO, vec2(160.0, 380.0));
        let right = Rect::from_min_size(pos2(755.0, 0.0), vec2(160.0, 380.0));
        let layout = TouchLayout::columns(left, right, &TouchSettings::default(), true);
        let l = shoulder(&layout, Keys::L);
        let r = shoulder(&layout, Keys::R);
        assert!(left.contains_rect(l) && right.contains_rect(r));
        assert!(l.bottom() < layout.dpad.top());
        assert!(l.height() >= 44.0);
        assert_buttons(layout.buttons(l.center()), Keys::L, "L");
        assert_buttons(layout.buttons(r.center()), Keys::R, "R");
        for (_, centre, radius) in &layout.face {
            assert!(r.bottom() < centre.y - radius);
        }
    }

    #[test]
    fn l_and_r_start_below_the_strip_over_the_right_column() {
        let left = Rect::from_min_size(Pos2::ZERO, vec2(110.0, 400.0));
        let right = Rect::from_min_size(pos2(805.0, 96.0), vec2(110.0, 304.0));
        let layout = TouchLayout::columns(left, right, &TouchSettings::default(), true);
        let l = shoulder(&layout, Keys::L);
        let r = shoulder(&layout, Keys::R);
        assert!(r.top() >= right.top());
        assert_eq!(l.top(), r.top());
        assert!(l.bottom() < layout.dpad.top());
    }

    #[test]
    fn a_and_b_reach_past_their_drawing() {
        let layout = layout();
        let (_, a, radius) = layout.face[0];
        assert_buttons(
            layout.buttons(a + vec2(radius * 1.5, 0.0)),
            Keys::A,
            "1.5 radii out",
        );
        assert_buttons(
            layout.buttons(a + vec2(radius * 1.7, 0.0)),
            Keys::empty(),
            "1.7 radii out",
        );
    }

    #[test]
    fn between_a_and_b_presses_both() {
        let layout = layout();
        let (_, a, _) = layout.face[0];
        let (_, b, _) = layout.face[1];
        assert_buttons(
            layout.buttons(a.lerp(b, 0.5)),
            Keys::A | Keys::B,
            "the A+B zone",
        );
        assert_buttons(layout.buttons(a), Keys::A, "A alone");
        assert_buttons(layout.buttons(b), Keys::B, "B alone");
    }

    #[test]
    fn empty_space_presses_nothing() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), pos2(-50.0, -50.0), &layout);
        assert_buttons(input.combined(), Keys::empty(), "outside the controls");
        assert_eq!(input.dpad_owner, None);
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
            let layout = TouchLayout::bar(area, &settings, false);
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
        let normal = TouchLayout::bar(area, &TouchSettings::default(), false);
        let mirrored = TouchLayout::bar(
            area,
            &TouchSettings {
                mirrored: true,
                ..Default::default()
            },
            false,
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
        let (_, a_centre, _) = layout.face[0];

        input.press(Contact::Finger(1), arm(&layout, 180.0, 0.6), &layout);
        input.press(Contact::Finger(2), a_centre, &layout);

        assert_buttons(
            input.combined(),
            Keys::LEFT | Keys::A,
            "two fingers, two buttons",
        );

        input.lift(Contact::Finger(1));
        assert_buttons(
            input.combined(),
            Keys::A,
            "lifting one finger keeps the other",
        );
    }

    #[test]
    fn only_transitions_are_reported() {
        let mut input = TouchInput::default();

        let moved = input.transitions(Keys::A);
        assert_buttons(moved.pressed, Keys::A, "first press");
        assert_buttons(moved.released, Keys::empty(), "nothing let go yet");

        let moved = input.transitions(Keys::A);
        assert_buttons(
            moved.pressed,
            Keys::empty(),
            "still held is not a new press",
        );
        assert_buttons(moved.released, Keys::empty(), "still held is not a release");

        let moved = input.transitions(Keys::B);
        assert_buttons(moved.pressed, Keys::B, "the new button");
        assert_buttons(moved.released, Keys::A, "and the one let go");
    }

    #[test]
    fn a_finger_that_never_lifts_does_not_stay_stuck() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), arm(&layout, 0.0, 0.7), &layout);
        assert_buttons(input.combined(), Keys::RIGHT, "held while touching");

        input.forget_stale_fingers(false);

        assert_buttons(
            input.combined(),
            Keys::empty(),
            "released once nothing is touching the screen",
        );
        assert_eq!(input.dpad_owner, None, "and the d-pad is free");
    }

    #[test]
    fn a_finger_still_down_is_kept() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), layout.face[0].1, &layout);

        input.forget_stale_fingers(true);

        assert_buttons(input.combined(), Keys::A, "still touching");
    }

    #[test]
    fn resetting_reports_what_it_was_holding() {
        let layout = layout();
        let mut input = TouchInput::default();
        input.press(Contact::Finger(1), layout.face[0].1, &layout);
        let held = input.combined();
        input.transitions(held);

        let stuck = input.reset();

        assert!(input.contacts.is_empty());
        assert_buttons(stuck, Keys::A, "so the caller can release it");
        assert_buttons(input.previous, Keys::empty(), "after reset");
    }

    #[test]
    fn the_columns_layout_keeps_the_dpad_and_buttons_apart() {
        let left = Rect::from_min_size(Pos2::ZERO, vec2(180.0, 320.0));
        let right = Rect::from_min_size(pos2(600.0, 0.0), vec2(180.0, 320.0));
        let layout = TouchLayout::columns(left, right, &TouchSettings::default(), false);

        assert!(left.contains(layout.dpad.center()));
        assert!(layout.reach().left() >= 24.0 - 0.01);
        assert!(layout.reach().right() <= right.right() - 24.0 + 0.01);
        for (_, centre, _) in &layout.face {
            assert!(right.expand(40.0).contains(*centre));
        }
    }

    fn phone_layout() -> TouchLayout {
        TouchLayout::bar(
            Rect::from_min_size(pos2(0.0, 622.0), vec2(412.0, 293.0)),
            &TouchSettings::default(),
            false,
        )
    }

    fn frame(ctx: &egui::Context, input: &mut TouchInput, events: Vec<egui::Event>) -> Keys {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(412.0, 915.0))),
            events,
            ..Default::default()
        };
        let mut held = Keys::empty();
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                held = input.update(ui, &phone_layout());
            });
        });
        held
    }

    fn press_at(pos: Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn finger(phase: egui::TouchPhase, pos: Pos2) -> egui::Event {
        egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(1),
            phase,
            pos,
            force: Some(1.0),
        }
    }

    #[test]
    fn a_finger_takes_the_dpad_from_the_browser_mouse_press_before_it() {
        let ctx = egui::Context::default();
        let mut input = TouchInput::default();
        let pos = arm(&phone_layout(), 0.0, 0.66);
        let mut ticks = 0;
        let mut step = |input: &mut TouchInput, events| {
            let held = frame(&ctx, input, events);
            if input.transitions(held).tick {
                ticks += 1;
            }
            held
        };

        assert_buttons(
            step(&mut input, vec![press_at(pos)]),
            Keys::RIGHT,
            "the mouse press the browser sends first",
        );
        assert_buttons(
            step(
                &mut input,
                vec![press_at(pos), finger(egui::TouchPhase::Start, pos)],
            ),
            Keys::RIGHT,
            "the finger itself",
        );
        for n in 0..5 {
            assert_buttons(
                step(&mut input, Vec::new()),
                Keys::RIGHT,
                &format!("frame {n} after"),
            );
        }
        assert_eq!(input.dpad_owner, Some(Contact::Finger(1)));
        let up = arm(&phone_layout(), -80.0, 0.66);
        assert_buttons(
            step(&mut input, vec![finger(egui::TouchPhase::Move, up)]),
            Keys::UP,
            "a slide to Up",
        );
        let release = egui::Event::PointerButton {
            pos: up,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_buttons(
            step(&mut input, vec![release.clone()]),
            Keys::UP,
            "the browser's mouse release comes first",
        );
        assert_buttons(
            step(
                &mut input,
                vec![
                    release,
                    egui::Event::PointerGone,
                    finger(egui::TouchPhase::End, up),
                ],
            ),
            Keys::empty(),
            "lifted",
        );
        drop(step);
        assert_eq!(ticks, 2, "one for the press, one for the turn to Up");
    }

    #[test]
    fn a_cancelled_touch_leaves_nothing_held() {
        let ctx = egui::Context::default();
        let mut input = TouchInput::default();
        let pos = arm(&phone_layout(), 0.0, 0.66);
        frame(&ctx, &mut input, vec![press_at(pos)]);
        frame(
            &ctx,
            &mut input,
            vec![press_at(pos), finger(egui::TouchPhase::Start, pos)],
        );
        assert_buttons(
            frame(
                &ctx,
                &mut input,
                vec![finger(egui::TouchPhase::Cancel, pos)],
            ),
            Keys::empty(),
            "cancelled",
        );
        for n in 0..3 {
            assert_buttons(
                frame(&ctx, &mut input, Vec::new()),
                Keys::empty(),
                &format!("frame {n} after, with the mouse press never released"),
            );
        }
    }
}
