use super::strip::STRIP_HEIGHT;
use crate::app::ui_state::touch::HiddenButtons;
use crate::held::InputSource;
use crate::icons;
use egui::{Rect, RichText, Ui, pos2, vec2};

pub const EYE_SIZE: f32 = 44.0;
pub const EYE_GAP: f32 = 12.0;

#[derive(Debug, Default)]
pub struct HiddenUi {
    on: bool,
    controller: bool,
    by_default: bool,
    kept_shown: bool,
    pub taken: Vec<Rect>,
    pub screens: Vec<Rect>,
    #[cfg(test)]
    pub eye: Option<Rect>,
}

impl HiddenUi {
    pub fn on(&self) -> bool {
        self.on
    }

    pub fn hide(&mut self) {
        self.on = true;
        self.by_default = false;
    }

    pub fn show(&mut self) {
        self.on = false;
        self.controller = false;
        self.by_default = false;
    }

    pub fn keep_shown(&mut self) {
        self.show();
        self.kept_shown = true;
    }

    pub fn follow_default(&mut self, wanted: bool) {
        if wanted && !self.on && !self.kept_shown {
            self.on = true;
            self.by_default = true;
        } else if !wanted && self.by_default {
            self.show();
        }
    }

    pub fn forget(&mut self) {
        *self = Self::default();
    }

    pub fn note(&mut self, pressed_by: Option<InputSource>, touched: bool) {
        if touched {
            self.controller = false;
        } else if pressed_by.is_some_and(|source| source != InputSource::Touch) {
            self.controller = true;
        }
    }

    pub fn buttons(&self, rule: HiddenButtons) -> bool {
        match rule {
            HiddenButtons::Auto => !self.on || !self.controller,
            HiddenButtons::Show => true,
            HiddenButtons::Hide => !self.on,
        }
    }
}

pub fn eye_rect(
    screen: Rect,
    area: Rect,
    landscape: bool,
    taken: &[Rect],
    screens: &[Rect],
) -> Rect {
    let size = vec2(EYE_SIZE, EYE_SIZE);
    let under = Rect::from_min_size(
        pos2(
            screen.right() - EYE_GAP - EYE_SIZE,
            screen.bottom() + EYE_GAP,
        ),
        size,
    );
    let top = area.top() + (STRIP_HEIGHT - EYE_SIZE) / 2.0;
    let bottom = area.bottom() - EYE_GAP - EYE_SIZE;
    let corner = Rect::from_min_size(pos2(area.right() - EYE_GAP - EYE_SIZE, top), size);
    let rows = if landscape {
        vec![top, bottom]
    } else {
        let lowest = taken
            .iter()
            .filter(|rect| rect.left() <= area.left() + 1.0 && rect.right() >= area.right() - 1.0)
            .map(|rect| rect.bottom())
            .fold(screen.bottom(), f32::max);
        let shown = screens.iter().map(|rect| rect.bottom()).reduce(f32::max);
        let mut rows = vec![under.top()];
        if let Some(bottom) = shown {
            let mut beside: Vec<f32> = taken
                .iter()
                .filter(|rect| rect.is_positive() && rect.width() < area.width() - 1.0)
                .filter(|rect| (bottom..=bottom + EYE_SIZE).contains(&rect.top()))
                .map(|rect| rect.center().y - EYE_SIZE / 2.0)
                .filter(|y| *y >= bottom + EYE_GAP / 2.0)
                .collect();
            beside.sort_by(f32::total_cmp);
            rows.extend(beside);
            rows.push(bottom + EYE_GAP);
        }
        rows.extend([lowest + EYE_GAP, top, bottom]);
        rows
    };
    let free = |taken: &[Rect]| {
        rows.iter()
            .flat_map(|&y| [EyeSpot::Right, EyeSpot::Left, EyeSpot::Centre].map(|spot| (y, spot)))
            .find_map(|(y, spot)| eye_at(area, y, spot, taken))
    };
    free(taken)
        .or_else(|| (!screens.is_empty()).then(|| free(screens)).flatten())
        .unwrap_or(if landscape { corner } else { under })
}

#[derive(Clone, Copy)]
enum EyeSpot {
    Right,
    Left,
    Centre,
}

fn eye_at(area: Rect, y: f32, spot: EyeSpot, taken: &[Rect]) -> Option<Rect> {
    if y < area.top() || y + EYE_SIZE > area.bottom() {
        return None;
    }
    match spot {
        EyeSpot::Right => eye_in_corner(area, y, true, taken),
        EyeSpot::Left => eye_in_corner(area, y, false, taken),
        EyeSpot::Centre => {
            let eye = Rect::from_center_size(
                pos2(area.center().x, y + EYE_SIZE / 2.0),
                vec2(EYE_SIZE, EYE_SIZE),
            );
            let clear = eye.expand(EYE_GAP / 2.0);
            taken
                .iter()
                .all(|rect| !rect.is_positive() || !rect.intersects(clear))
                .then_some(eye)
        }
    }
}

fn eye_in_corner(area: Rect, y: f32, right: bool, taken: &[Rect]) -> Option<Rect> {
    let band = egui::Rangef::new(y, y + EYE_SIZE);
    let across = taken
        .iter()
        .filter(|rect| rect.y_range().intersects(band) && rect.is_positive());
    let free = if right {
        let edge = across
            .map(|rect| rect.right())
            .filter(|x| *x < area.right())
            .fold(area.left(), f32::max);
        if taken.iter().any(|rect| {
            rect.y_range().intersects(band) && rect.right() >= area.right() && rect.is_positive()
        }) {
            return None;
        }
        egui::Rangef::new(
            edge.max(area.right() - EYE_SIZE - EYE_GAP * 2.0),
            area.right(),
        )
    } else {
        let edge = across
            .map(|rect| rect.left())
            .filter(|x| *x > area.left())
            .fold(area.right(), f32::min);
        if taken.iter().any(|rect| {
            rect.y_range().intersects(band) && rect.left() <= area.left() && rect.is_positive()
        }) {
            return None;
        }
        egui::Rangef::new(
            area.left(),
            edge.min(area.left() + EYE_SIZE + EYE_GAP * 2.0),
        )
    };
    (free.span() >= EYE_SIZE + EYE_GAP / 2.0).then(|| {
        Rect::from_center_size(
            pos2(free.center(), y + EYE_SIZE / 2.0),
            vec2(EYE_SIZE, EYE_SIZE),
        )
    })
}

pub fn picture_in(area: Rect) -> Rect {
    let width = (area.height() * 160.0 / 144.0).min(area.width());
    Rect::from_center_size(area.center(), vec2(width, area.height()))
}

pub fn below_eye(controls: Rect, eye: Rect, area: Rect) -> Rect {
    let top = eye.bottom() + EYE_GAP;
    let wanted = (top - controls.top()).max(0.0);
    let room = (area.bottom() - controls.bottom()).max(0.0);
    let moved = controls.translate(vec2(0.0, wanted.min(room)));
    Rect::from_min_max(pos2(moved.left(), moved.top().max(top)), moved.max)
}

pub fn eye_button(ui: &mut Ui, rect: Rect) -> bool {
    let visuals = ui.visuals();
    let fill = visuals.widgets.inactive.bg_fill;
    let ink = visuals.strong_text_color();
    let button = egui::Button::new(RichText::new(icons::EYE).size(22.0).color(ink))
        .fill(fill)
        .corner_radius(EYE_SIZE / 2.0)
        .min_size(rect.size());
    ui.put(rect, button).on_hover_text("Show the UI").clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad() -> Option<InputSource> {
        Some(InputSource::PadButtons(0))
    }

    #[test]
    fn auto_hides_the_buttons_after_a_controller_press_and_brings_them_back_on_a_touch() {
        let mut hidden = HiddenUi::default();
        hidden.hide();
        assert!(hidden.buttons(HiddenButtons::Auto));
        hidden.note(pad(), false);
        assert!(!hidden.buttons(HiddenButtons::Auto));
        hidden.note(None, false);
        assert!(!hidden.buttons(HiddenButtons::Auto));
        hidden.note(None, true);
        assert!(hidden.buttons(HiddenButtons::Auto));
    }

    #[test]
    fn a_keyboard_press_counts_as_a_controller_and_a_touch_press_does_not() {
        let mut hidden = HiddenUi::default();
        hidden.hide();
        hidden.note(Some(InputSource::Touch), false);
        assert!(hidden.buttons(HiddenButtons::Auto));
        hidden.note(Some(InputSource::Keyboard), false);
        assert!(!hidden.buttons(HiddenButtons::Auto));
    }

    #[test]
    fn the_rule_only_applies_while_the_ui_is_hidden() {
        let mut hidden = HiddenUi::default();
        hidden.note(pad(), false);
        assert!(hidden.buttons(HiddenButtons::Auto));
        assert!(hidden.buttons(HiddenButtons::Hide));
        hidden.hide();
        assert!(!hidden.buttons(HiddenButtons::Hide));
        assert!(hidden.buttons(HiddenButtons::Show));
        hidden.show();
        assert!(hidden.buttons(HiddenButtons::Hide));
    }

    #[test]
    fn the_eye_sits_under_the_picture_in_portrait_and_in_the_strip_band_in_landscape() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(412.0, 880.0));
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(412.0, 370.0));
        let eye = eye_rect(screen, area, false, &[], &[]);
        assert!(eye.top() > screen.bottom());
        assert!((eye.right() - (412.0 - EYE_GAP)).abs() < 0.01);
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(891.0, 412.0));
        let picture = Rect::from_min_max(pos2(217.0, 0.0), pos2(674.0, 412.0));
        let corner = eye_rect(picture, wide, true, &[picture], &[]);
        assert!(!corner.intersects(picture));
        assert!(corner.top() >= 0.0 && corner.bottom() <= STRIP_HEIGHT);
        assert!((corner.right() - (891.0 - EYE_GAP)).abs() < 0.01);
    }

    #[test]
    fn the_eye_takes_the_free_column_beside_the_screens_or_the_next_free_corner() {
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(888.0, 400.0));
        let top = Rect::from_min_size(pos2(0.0, 0.0), vec2(524.0, 393.0));
        let touch = Rect::from_min_size(pos2(532.0, 0.0), vec2(296.0, 222.0));
        let face = Rect::from_min_size(pos2(714.0, 226.0), vec2(172.0, 172.0));
        let eye = eye_rect(top, wide, true, &[top, touch, face], &[]);
        assert!((eye.center().x - 858.0).abs() < 0.01, "{eye:?}");
        assert!(eye.left() - touch.right() >= 8.0 - 0.01);
        let picture = Rect::from_min_size(pos2(217.0, 0.0), vec2(457.0, 400.0));
        let r = Rect::from_min_size(pos2(740.0, 4.0), vec2(140.0, 44.0));
        let moved = eye_rect(picture, wide, true, &[picture, r], &[]);
        assert!(moved.right() < picture.left(), "{moved:?}");
        assert!(moved.top() < 50.0);
    }

    #[test]
    fn with_no_free_spot_the_eye_keeps_off_the_screens_before_the_buttons() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 888.0));
        let top = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        let touch = Rect::from_min_size(pos2(0.0, 300.0), vec2(400.0, 300.0));
        let pads = Rect::from_min_max(pos2(0.0, 600.0), pos2(400.0, 888.0));
        let eye = eye_rect(top, area, false, &[top, touch, pads], &[top, touch]);
        assert!(!eye.intersects(top) && !eye.intersects(touch), "{eye:?}");
        assert!(area.contains_rect(eye), "{eye:?}");
    }

    #[test]
    fn the_landscape_picture_is_the_centred_game_boy_shape() {
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(891.0, 412.0));
        let picture = picture_in(wide);
        assert!((picture.width() - 412.0 * 160.0 / 144.0).abs() < 0.01);
        assert!((picture.center().x - 445.5).abs() < 0.01);
    }

    #[test]
    fn the_controls_move_below_the_eye_only_as_far_as_the_screen_allows() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(412.0, 800.0));
        let eye = Rect::from_min_size(pos2(356.0, 382.0), vec2(EYE_SIZE, EYE_SIZE));
        let controls = Rect::from_min_size(pos2(0.0, 380.0), vec2(412.0, 300.0));
        let moved = below_eye(controls, eye, area);
        assert!((moved.top() - (eye.bottom() + EYE_GAP)).abs() < 0.01);
        let tight = Rect::from_min_size(pos2(0.0, 380.0), vec2(412.0, 410.0));
        let squeezed = below_eye(tight, eye, area);
        assert!((squeezed.bottom() - 800.0).abs() < 0.01);
        assert!(squeezed.top() >= eye.bottom() + EYE_GAP);
    }
}
