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
}

impl HiddenUi {
    pub fn on(&self) -> bool {
        self.on
    }

    pub fn hide(&mut self) {
        self.on = true;
    }

    pub fn show(&mut self) {
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

pub fn eye_rect(screen: Rect, area: Rect, landscape: bool) -> Rect {
    let size = vec2(EYE_SIZE, EYE_SIZE);
    let min = if landscape {
        pos2(
            area.right() - EYE_GAP - EYE_SIZE,
            area.top() + (STRIP_HEIGHT - EYE_SIZE) / 2.0,
        )
    } else {
        pos2(
            screen.right() - EYE_GAP - EYE_SIZE,
            screen.bottom() + EYE_GAP,
        )
    };
    Rect::from_min_size(min, size)
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
        let eye = eye_rect(screen, area, false);
        assert!(eye.top() > screen.bottom());
        assert!((eye.right() - (412.0 - EYE_GAP)).abs() < 0.01);
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(891.0, 412.0));
        let picture = Rect::from_min_max(pos2(217.0, 0.0), pos2(674.0, 412.0));
        let corner = eye_rect(picture, wide, true);
        assert!(!corner.intersects(picture));
        assert!(corner.top() >= 0.0 && corner.bottom() <= STRIP_HEIGHT);
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
