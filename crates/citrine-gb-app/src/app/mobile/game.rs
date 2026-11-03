use super::hide;
use super::strip::{STRIP_HEIGHT, STRIP_INSET};
use crate::app::Citrine;
use crate::app::tabs::{Tab, linked, playing};
use crate::app::ui_state::touch::TouchSettings;
use crate::app::widgets::touch_controls::{self as touch, TouchLayout};
use crate::held::InputSource;
use crate::icons;
use crate::local_link::Game;
use citrine_gb::gb::joypad::JoypadState;
use egui::{Rect, Ui, vec2};

const OVERLAY_COLUMN: f32 = 0.28;

const CONTROL_BREATHING_ROOM: f32 = 1.15;
const SCREEN_ASPECT: f32 = 144.0 / 160.0;

const WATCH_GAP: f32 = 8.0;
const WATCH_CAPTION: f32 = 16.0;
const WATCH_MIN_HEIGHT: f32 = 72.0;

impl Citrine {
    pub(super) fn game_view(&mut self, ui: &mut Ui, landscape: bool) {
        let watching = self.play.session().is_some_and(|s| s.watching_only());
        let hidden = self.mobile.hidden.on();
        let columns = landscape && self.ui.settings.touch.show_controls && !watching;
        if !columns && !hidden {
            self.strip(ui, false);
        }
        self.banners(ui);
        let area = ui.available_rect_before_wrap();
        let (buttons, screen) = self.mobile_play(ui, area, landscape);
        if hidden {
            if hide::eye_button(ui, hide::eye_rect(screen, area, landscape)) {
                self.mobile.hidden.show();
                self.release_touch();
            }
        } else if let Some(buttons) = buttons {
            let mut column = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(buttons.with_min_x(buttons.left() + 8.0))
                    .layout(egui::Layout::right_to_left(egui::Align::Min).with_main_wrap(true)),
            );
            self.strip(&mut column, true);
        }
    }

    pub(in crate::app) fn release_touch(&mut self) {
        let stuck = self.mobile.touch.reset();
        for button in ALL_BUTTONS {
            if stuck.contains(button) {
                self.emulator.release(InputSource::Touch, button);
            }
        }
    }

    fn mobile_play(&mut self, ui: &mut Ui, area: Rect, landscape: bool) -> (Option<Rect>, Rect) {
        let settings = &self.ui.settings;
        let watching = self.play.session().is_some_and(|s| s.watching_only());
        let hidden = self.mobile.hidden.on();
        let wanted = settings.touch.show_controls && !watching;
        let one_screen = self.emulator.link.is_none() && self.emulator.partner.is_none();
        let split =
            (landscape && wanted).then(|| landscape_split(area, settings.touch.landscape, !hidden));
        let kept = match split {
            _ if !landscape => portrait_split(area, &settings.touch, one_screen).0,
            Some((screen, ..)) => screen,
            None => area,
        };
        let picture = if landscape {
            hide::picture_in(kept)
        } else {
            kept
        };
        let touched = ui.input(|i| {
            i.events.iter().any(|event| {
                matches!(event, egui::Event::PointerButton { pos, pressed: true, .. } if !picture.contains(*pos))
            })
        });
        let pressed_by = self.emulator.take_pressed_by();
        self.mobile.hidden.note(pressed_by, touched);

        let mut buttons = None;
        let (screen, layout, free) = if !wanted {
            (area, None, area)
        } else if !self.mobile.hidden.buttons(settings.touch.hidden_buttons) {
            (kept, None, kept)
        } else if let Some((screen, left, right, strip)) = split {
            let right = right.with_min_y(area.top() + STRIP_HEIGHT);
            buttons = Some(strip);
            (
                screen,
                Some(TouchLayout::columns(left, right, &settings.touch)),
                Rect::from_min_max(egui::pos2(left.right(), area.top()), right.left_bottom()),
            )
        } else {
            let (screen, mut controls) = portrait_split(area, &settings.touch, one_screen);
            if hidden {
                controls = hide::below_eye(controls, hide::eye_rect(screen, area, false), area);
            }
            (
                screen,
                Some(TouchLayout::bar(controls, &settings.touch)),
                screen,
            )
        };
        let portrait = layout.is_some() && !landscape;
        self.ui.toast_area = Some(free);

        self.paint_screen(ui, screen, portrait);

        let Some(layout) = layout else {
            self.release_touch();
            return (buttons, screen);
        };

        touch::claim(ui, layout.reach());

        let held = self.mobile.touch.update(ui, &layout);
        let moved = self.mobile.touch.transitions(held);

        if moved.tick && self.ui.settings.touch.haptics {
            crate::platform::haptic_tick();
        }

        for button in ALL_BUTTONS {
            if moved.pressed.contains(button) {
                self.emulator.press(InputSource::Touch, button);
            } else if moved.released.contains(button) {
                self.emulator.release(InputSource::Touch, button);
            }
        }

        layout.paint(ui, held, &self.ui.settings.touch);
        (buttons, screen)
    }

    fn paint_screen(&mut self, ui: &mut Ui, area: Rect, portrait: bool) {
        if let Some(session) = self.play.session()
            && let Some((got, total)) = playing::receiving(session)
        {
            let title = playing::game_title(session);
            let name = playing::friend_name(&self.online, session.friend());
            playing::arriving(ui, area, &title, &name, got, total);
            return;
        }
        let linked = self.play.session().is_some_and(|s| s.runs_engine());
        if let Some(session) = self.play.session()
            && let Some(line) = playing::screen_note(
                &playing::friend_name(&self.online, session.friend()),
                session,
            )
        {
            let copy = playing::needs_copy(session).then_some("Open my copy");
            if playing::note_over(ui, area, &line, copy) {
                self.ui.playing.wants_copy = true;
                self.release_touch();
            }
            return;
        }
        if !self.emulator.gb.cartridge.has_rom_loaded && !linked {
            return;
        }
        if self.emulator.link.is_some() {
            self.paint_linked(ui, area, portrait);
            return;
        }
        if self.emulator.partner.is_some() && portrait {
            let (main, small) = watch_split(area);
            self.emulator.ui_at(ui, main);
            if let Some(small) = small
                && let Some(partner) = &self.emulator.partner
            {
                let sprite = linked::partner::sprite(&self.ui.linked.events, partner);
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(small.shrink2(egui::vec2(8.0, 4.0)))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                linked::partner::status(&mut child, partner, sprite.as_ref());
            }
            return;
        }
        if portrait
            && let Some(session) = self
                .play
                .session()
                .filter(|s| playing::watching(s) && !s.watching_only())
        {
            let (main, friend) = watch_split(area);
            let Some(friend) = friend else {
                self.emulator.ui_at(ui, main);
                return;
            };
            let swapped = self.ui.playing.swapped;
            let (mine, theirs) = if swapped {
                (friend, main)
            } else {
                (main, friend)
            };
            self.emulator.ui_at(ui, mine);
            let name = playing::friend_name(&self.online, session.friend());
            if let Some(shown) =
                playing::paint_friend_screen(&mut self.ui.playing, ui, session, theirs)
            {
                let small = if swapped { friend } else { shown };
                let caption = if swapped { "You".to_owned() } else { name };
                ui.painter().text(
                    egui::pos2(small.center().x, small.bottom() + 3.0),
                    egui::Align2::CENTER_TOP,
                    caption,
                    egui::TextStyle::Small.resolve(ui.style()),
                    ui.visuals().weak_text_color(),
                );
                let tapped = ui
                    .interact(small, egui::Id::new("friend_small"), egui::Sense::click())
                    .clicked();
                if tapped {
                    self.ui.playing.swapped = !swapped;
                }
            }
            return;
        }
        self.emulator.ui_at(ui, area);
    }

    fn paint_linked(&mut self, ui: &mut Ui, area: Rect, portrait: bool) {
        let Some(control) = self.link_control() else {
            return;
        };
        let (main, small) = if portrait {
            watch_split(area)
        } else {
            (area, None)
        };
        let paint_first = |this: &mut Self, ui: &mut Ui, rect: Rect| {
            if small.is_some_and(|small| small == rect) {
                return this.emulator.paint_fitted(ui, rect);
            }
            this.emulator.ui_at(ui, rect);
            linked::fit(rect, f32::MAX)
        };
        let paint_second = |this: &mut Self, ui: &mut Ui, rect: Rect| {
            let Some(link) = &this.emulator.link else {
                return rect;
            };
            linked::paint_screen(&mut this.ui.linked, ui, &link.gb, rect, f32::MAX)
        };
        match control {
            Game::First => paint_first(self, ui, main),
            Game::Second => paint_second(self, ui, main),
        };
        let Some(small) = small else {
            return;
        };
        let shown = match control {
            Game::First => paint_second(self, ui, small),
            Game::Second => paint_first(self, ui, small),
        };
        let caption = match control {
            Game::First => linked::title(&self.emulator, self.ui.linked.listing),
            Game::Second => self.emulator.game_name().to_owned(),
        };
        ui.painter().text(
            egui::pos2(shown.center().x, shown.bottom() + 3.0),
            egui::Align2::CENTER_TOP,
            format!("{}  {caption}", icons::HAND_TAP),
            egui::TextStyle::Small.resolve(ui.style()),
            ui.visuals().weak_text_color(),
        );
        let tapped = ui
            .interact(
                shown,
                egui::Id::new("linked_small_screen"),
                egui::Sense::click(),
            )
            .clicked();
        if tapped {
            self.release_touch();
            self.emulator.set_link_control(control.other());
        }
    }

    fn banners(&mut self, ui: &mut Ui) {
        let alone = self.play.session().is_none();
        let shown = (alone && (self.emulator.link.is_some() || self.emulator.partner.is_some()))
            || playing::pending_ask(&self.play).is_some()
            || self.ui.playing.ended.is_some();
        if !shown {
            return;
        }
        ui.add_space(2.0);
        if self.play.session().is_none()
            && let Some(link) = &self.emulator.link
        {
            if linked::strip(ui, link) {
                self.mobile.open(Tab::Playing);
                self.release_touch();
            }
        } else if self.play.session().is_none()
            && let Some(partner) = &self.emulator.partner
        {
            if linked::partner_strip(ui, partner) {
                self.mobile.open(Tab::Playing);
                self.release_touch();
            }
        } else if let Some(invite) = playing::pending_ask(&self.play) {
            match inset(ui, |ui| playing::ask_banner(ui, &self.online, &invite)) {
                Some(playing::Answer::Join) => self.answer_ask(&invite),
                Some(playing::Answer::Pick) => {
                    self.ui.playing.picking = Some(playing::PickFor::Join(invite));
                }
                Some(playing::Answer::Decline) => {
                    self.play.decline(&mut self.online, invite.session)
                }
                None => {}
            }
        } else if let Some(ended) = self.ui.playing.ended.clone() {
            let again = playing::play_button(&self.play, &self.online, ended.friend)
                == Some(playing::PlayButton::Ready);
            match inset(ui, |ui| playing::banner(ui, &ended, again)) {
                Some(playing::BannerAction::PlayAgain) => self.play_again(ended.friend),
                Some(playing::BannerAction::Resume) => {
                    self.emulator.running = true;
                    self.ui.playing.ended = None;
                }
                Some(playing::BannerAction::Dismiss) => self.ui.playing.ended = None,
                None => {}
            }
        }
        ui.add_space(2.0);
    }
}

fn inset<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(STRIP_INSET as i8, 0))
        .show(ui, add)
        .inner
}

const ALL_BUTTONS: [JoypadState; 8] = [
    JoypadState::UP,
    JoypadState::DOWN,
    JoypadState::LEFT,
    JoypadState::RIGHT,
    JoypadState::A,
    JoypadState::B,
    JoypadState::START,
    JoypadState::SELECT,
];

fn watch_split(area: Rect) -> (Rect, Option<Rect>) {
    let main_height = (area.width() * 144.0 / 160.0).min(area.height());
    let main = Rect::from_min_size(area.min, vec2(area.width(), main_height));
    let below = Rect::from_min_max(
        egui::pos2(area.left(), main.bottom() + WATCH_GAP),
        egui::pos2(area.right(), area.bottom() - WATCH_GAP - WATCH_CAPTION),
    );
    let friend = (below.height() >= WATCH_MIN_HEIGHT).then_some(below);
    (main, friend)
}

fn portrait_split(area: Rect, settings: &TouchSettings, one_screen: bool) -> (Rect, Rect) {
    let needed = TouchLayout::bar_height(area.width(), settings);
    let controls_height = (needed * CONTROL_BREATHING_ROOM)
        .max(needed)
        .clamp(0.0, area.height() * 0.6);
    let room = area.height() - controls_height;
    let screen_height = if one_screen {
        room.min(area.width() * SCREEN_ASPECT)
    } else {
        room
    };
    let screen = Rect::from_min_size(area.min, vec2(area.width(), screen_height));
    let spare = room - screen_height;
    let dpad_slack = (controls_height - needed).max(0.0) / 2.0;
    let above = ((spare - dpad_slack) / 2.0).max(0.0);
    let controls = Rect::from_min_size(
        egui::pos2(area.left(), screen.bottom() + above),
        vec2(area.width(), controls_height),
    );
    (screen, controls)
}

fn landscape_split(
    area: Rect,
    mode: crate::app::ui_state::touch::LandscapeControls,
    strip_shown: bool,
) -> (Rect, Rect, Rect, Rect) {
    use crate::app::ui_state::touch::LandscapeControls;

    let column = match mode {
        LandscapeControls::Overlay => area.width() * OVERLAY_COLUMN,
        LandscapeControls::Beside => {
            let wanted = area.height() / 144.0 * 160.0;
            ((area.width() - wanted) / 2.0).clamp(area.width() * 0.16, area.width() * 0.32)
        }
    };

    let left = Rect::from_min_size(area.min, vec2(column, area.height()));
    let right = Rect::from_min_size(
        egui::pos2(area.right() - column, area.top()),
        vec2(column, area.height()),
    );
    let band = if strip_shown {
        area.top() + STRIP_HEIGHT
    } else {
        area.top()
    };
    let (screen, strip) = match mode {
        LandscapeControls::Overlay => (area, right.with_max_y(band)),
        LandscapeControls::Beside => (
            Rect::from_min_max(egui::pos2(left.right(), band), right.left_bottom()),
            Rect::from_min_max(
                egui::pos2(left.right(), area.top()),
                egui::pos2(right.right(), band),
            ),
        ),
    };
    (screen, left, right, strip)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn toggle_fullscreen() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if document.fullscreen_element().is_some() {
        document.exit_fullscreen();
    } else if let Some(root) = document.document_element() {
        let _ = root.request_fullscreen();
    }
}

#[cfg(test)]
mod tests;
