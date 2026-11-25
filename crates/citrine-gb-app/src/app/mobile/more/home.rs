use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SliderRow};
use crate::app::mobile::nav::Place;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::avatar;
use crate::icons;
use crate::online::Seen;
use egui::{Align2, Context, CornerRadius, Frame, Id, Margin, Rect};

const WIDTH: f32 = 360.0;

#[derive(Debug, Clone, Copy, PartialEq)]
enum HomeAction {
    Friends,
    Volume(f32),
    Quit,
}

impl Citrine {
    pub(in crate::app) fn big_home_menu(&mut self, ctx: &Context, page: Rect) {
        if !self.big.home_open() {
            return;
        }
        if self.game_open() {
            self.big.close_home();
            return;
        }
        let id = Id::new("big_home_menu");
        let online = self.friends_online();
        let waiting = self.friends_waiting().then_some(avatar::DOT);
        let mut volume = self.menu_volume();
        let focus_first = self.big.take_home_focus();
        let width = WIDTH.min(page.width() - 32.0);
        let modal = egui::Modal::new(id)
            .area(
                egui::Area::new(id)
                    .kind(egui::UiKind::Modal)
                    .order(egui::Order::Foreground)
                    .pivot(Align2::CENTER_CENTER)
                    .fixed_pos(page.center())
                    .constrain(false),
            )
            .frame(Frame::NONE)
            .backdrop_color(egui::Color32::TRANSPARENT)
            .show(ctx, |ui| {
                let screen = ui.ctx().content_rect();
                let hints = crate::app::big_screen::hint_room(ui.ctx());
                let mut behind = screen;
                behind.max.y -= hints;
                ui.ctx().layer_painter(ui.layer_id()).rect_filled(
                    behind,
                    CornerRadius::ZERO,
                    crate::app::big_screen::dim(ui.visuals()),
                );
                Frame::new()
                    .fill(kit::card_fill(ui.visuals()))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(Margin::same(12))
                    .shadow(ui.visuals().popup_shadow)
                    .show(ui, |ui| {
                        ui.set_width(width - 24.0);
                        let mut action = None;
                        kit::list(ui, |list| {
                            let friends = Row::new(Place::Friends.label())
                                .icon(Place::Friends.icon())
                                .dot(waiting);
                            let friends = match online {
                                Some(count) if count > 0 => {
                                    friends.value(format!("{count} online"))
                                }
                                _ => friends,
                            };
                            let friends = list.row(friends);
                            if focus_first {
                                friends.request_focus();
                            }
                            if friends.clicked() {
                                action = Some(HomeAction::Friends);
                            }
                            let silent = volume == 0.0;
                            let slid = list.slider(
                                SliderRow::new("Volume", &mut volume, 0.0..=1.0, 0.01)
                                    .icon(HotkeyAction::ToggleMute.icon(silent)),
                            );
                            if slid.changed() {
                                action = Some(HomeAction::Volume(volume));
                            }
                            if crate::platform::CAN_QUIT
                                && list
                                    .row(Row::new("Quit Citrine").icon(icons::POWER).no_chevron())
                                    .clicked()
                            {
                                action = Some(HomeAction::Quit);
                            }
                        });
                        action
                    })
                    .inner
            });
        if modal.should_close() {
            self.big.close_home();
        }
        match modal.inner {
            Some(HomeAction::Friends) => {
                self.big.close_home();
                self.go_place(Place::Friends);
            }
            Some(HomeAction::Volume(volume)) => self.set_menu_volume(volume),
            Some(HomeAction::Quit) => {
                self.big.close_home();
                #[cfg(not(target_arch = "wasm32"))]
                if self.held_by_unsent(crate::achievements::Leaving::Quit) {
                    return;
                }
                crate::platform::quit(ctx);
            }
            None => {}
        }
    }

    fn friends_online(&self) -> Option<usize> {
        if !self.ui.settings.online.is_joined() {
            return None;
        }
        let online = self
            .online
            .friends()
            .into_iter()
            .filter(|friend| self.online.presence(friend) == Some(Seen::Online))
            .count();
        Some(online)
    }
}
