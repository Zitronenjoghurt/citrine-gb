use crate::app::Citrine;
use crate::app::tabs::{Tab, friends, playing};
use crate::app::toasts::{free_spot, reading_time};
use crate::app::widgets::action::{action_height, dismiss_button, main_button};
use crate::app::widgets::avatar;
use citrine_gb_net::FriendState;
use citrine_gb_net::{OnlineId, SessionId};
use egui::{
    Align2, Button, Context, CursorIcon, Id, Label, Order, Rect, RichText, Sense, Ui, UiBuilder,
    pos2, vec2,
};

const MESSAGE_LIFETIME: f32 = 6.0;
const FADE: f32 = 0.15;
const AVATAR: f32 = 32.0;
const MIN_WIDTH: f32 = 220.0;
const MAX_WIDTH: f32 = 340.0;
const MARGIN: f32 = 8.0;
const SPACING: f32 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask {
    Invite(SessionId),
    Friend,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Message,
    Ask(Ask),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Waiting {
    pub ask: Ask,
    pub from: OnlineId,
    pub name: String,
    pub line: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    OpenChat(OnlineId),
    Profile(OnlineId),
    Answer(Ask, OnlineId, bool),
}

struct Popup {
    id: u64,
    kind: Kind,
    from: OnlineId,
    name: String,
    line: String,
    remaining: Option<f32>,
    age: f32,
    closing: Option<f32>,
    height: Option<f32>,
}

impl Popup {
    fn key(&self) -> Option<(Ask, OnlineId)> {
        match self.kind {
            Kind::Ask(ask) => Some((ask, self.from)),
            Kind::Message => None,
        }
    }

    fn opacity(&self, fade: f32) -> f32 {
        let part = |time: f32| {
            if fade > 0.0 {
                (time / fade).min(1.0)
            } else {
                1.0
            }
        };
        part(self.age) * (1.0 - self.closing.map_or(0.0, part))
    }

    fn advance(&mut self, dt: f32, hovered: bool) {
        self.age += dt;
        if let Some(closing) = &mut self.closing {
            *closing += dt;
            return;
        }
        if hovered {
            return;
        }
        if let Some(remaining) = &mut self.remaining {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.closing = Some(0.0);
            }
        }
    }

    fn close(&mut self) {
        self.closing.get_or_insert(0.0);
    }
}

#[derive(Default)]
pub struct FocusPopups {
    popups: Vec<Popup>,
    next_id: u64,
    dismissed: Vec<(Ask, OnlineId)>,
}

impl FocusPopups {
    pub fn message(&mut self, from: OnlineId, name: String, line: String) {
        if let Some(popup) = self
            .popups
            .iter_mut()
            .find(|p| p.kind == Kind::Message && p.from == from && p.closing.is_none())
        {
            popup.remaining = Some(reading_time(&line, MESSAGE_LIFETIME));
            popup.line = line;
            return;
        }
        let life = reading_time(&line, MESSAGE_LIFETIME);
        self.push(Kind::Message, from, name, line, Some(life));
    }

    pub fn sync(&mut self, waiting: &[Waiting]) {
        let wanted = |key: (Ask, OnlineId)| waiting.iter().any(|w| (w.ask, w.from) == key);
        for popup in &mut self.popups {
            if popup.key().is_some_and(|key| !wanted(key)) {
                popup.close();
            }
        }
        self.dismissed.retain(|key| wanted(*key));
        for item in waiting {
            let key = (item.ask, item.from);
            let shown = self
                .popups
                .iter()
                .any(|p| p.key() == Some(key) && p.closing.is_none());
            if !shown && !self.dismissed.contains(&key) {
                self.push(
                    Kind::Ask(item.ask),
                    item.from,
                    item.name.clone(),
                    item.line.clone(),
                    None,
                );
            }
        }
    }

    pub fn clear(&mut self) {
        self.popups.clear();
    }

    pub fn lines(&self) -> Vec<&str> {
        self.popups
            .iter()
            .filter(|p| p.closing.is_none())
            .map(|p| p.line.as_str())
            .collect()
    }

    fn push(&mut self, kind: Kind, from: OnlineId, name: String, line: String, life: Option<f32>) {
        self.next_id += 1;
        self.popups.push(Popup {
            id: self.next_id,
            kind,
            from,
            name,
            line,
            remaining: life,
            age: 0.0,
            closing: None,
            height: None,
        });
    }

    fn stack_height(&self, ui_style: &egui::Style) -> f32 {
        let heights: Vec<f32> = self
            .popups
            .iter()
            .map(|p| p.height.unwrap_or_else(|| estimate(ui_style, p.kind)))
            .collect();
        heights.iter().sum::<f32>() + SPACING * heights.len().saturating_sub(1) as f32
    }

    pub fn show(
        &mut self,
        ctx: &Context,
        area: Rect,
        picture: Option<Rect>,
        fade: f32,
    ) -> (Option<Rect>, Vec<Choice>) {
        let mut choices = Vec::new();
        self.popups.retain(|p| p.closing.is_none_or(|c| c < fade));
        if self.popups.is_empty() {
            return (None, choices);
        }
        let stack = place(area, picture, self.stack_height(&ctx.style()));
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let mut top = stack.top();
        for popup in &mut self.popups {
            let response = egui::Area::new(Id::new(("focus-popup", popup.id)))
                .order(Order::Middle)
                .fixed_pos(pos2(stack.left(), top))
                .pivot(Align2::LEFT_TOP)
                .constrain(false)
                .show(ctx, |ui| {
                    ui.set_opacity(popup.opacity(fade));
                    body(ui, popup, stack.width())
                });
            let rect = response.response.rect;
            popup.height = Some(rect.height());
            if popup.closing.is_none()
                && let Some(choice) = response.inner
            {
                popup.close();
                match choice {
                    Picked::Dismiss => {
                        if let Some(key) = popup.key() {
                            self.dismissed.push(key);
                        }
                    }
                    Picked::Choice(choice) => choices.push(choice),
                }
            }
            popup.advance(dt, pointer.is_some_and(|p| rect.contains(p)));
            top += rect.height() + SPACING;
        }
        let used = Rect::from_min_max(stack.min, pos2(stack.right(), top - SPACING));
        (Some(used), choices)
    }
}

fn estimate(style: &egui::Style, kind: Kind) -> f32 {
    let row = style.spacing.interact_size.y;
    let gap = style.spacing.item_spacing.y;
    let frame = style.spacing.menu_margin.sum().y + style.visuals.window_stroke.width * 2.0;
    let text = (row * 2.0 + gap).max(AVATAR);
    let buttons = match kind {
        Kind::Ask(_) => gap + row.max(32.0),
        Kind::Message => 0.0,
    };
    frame + text + buttons
}

pub fn place(area: Rect, picture: Option<Rect>, height: f32) -> Rect {
    let need = vec2(MIN_WIDTH + MARGIN * 2.0, height + MARGIN * 2.0);
    if let Some(spot) = picture.and_then(|picture| free_spot(area, picture, need)) {
        let width = (spot.width() - MARGIN * 2.0).min(MAX_WIDTH);
        let left = spot.center().x - width / 2.0;
        return Rect::from_min_size(pos2(left, spot.top() + MARGIN), vec2(width, height));
    }
    let inner = area.shrink(MARGIN);
    let size = vec2(inner.width().clamp(0.0, MAX_WIDTH), height);
    let corners = [
        Align2::RIGHT_TOP,
        Align2::LEFT_TOP,
        Align2::RIGHT_BOTTOM,
        Align2::LEFT_BOTTOM,
    ]
    .map(|corner| corner.align_size_within_rect(size, inner));
    let covered = |rect: &Rect| picture.map_or(0.0, |p| p.intersect(*rect).area().max(0.0));
    corners
        .into_iter()
        .min_by(|a, b| covered(a).total_cmp(&covered(b)))
        .unwrap_or(corners[0])
}

enum Picked {
    Dismiss,
    Choice(Choice),
}

fn body(ui: &mut Ui, popup: &Popup, width: f32) -> Option<Picked> {
    let mut picked = None;
    let open = ui
        .scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let inner = width - ui.spacing().menu_margin.sum().x;
                ui.set_width(inner);
                ui.horizontal_top(|ui| {
                    let (face, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(AVATAR), Sense::hover());
                    avatar::paint(ui, face, &popup.name, popup.from.value(), None);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        egui::Sides::new().shrink_left().truncate().show(
                            ui,
                            |ui| {
                                ui.add(
                                    Label::new(RichText::new(&popup.name).strong())
                                        .truncate()
                                        .selectable(false),
                                );
                            },
                            |ui| {
                                if ui.add(dismiss_button()).on_hover_text("Dismiss").clicked() {
                                    picked = Some(Picked::Dismiss);
                                }
                            },
                        );
                        ui.add(Label::new(&popup.line).truncate().selectable(false));
                        if let Kind::Ask(ask) = popup.kind {
                            let yes = match ask {
                                Ask::Invite(_) => "Join",
                                Ask::Friend => "Accept",
                            };
                            let height = action_height(ui);
                            ui.horizontal(|ui| {
                                if ui
                                    .add(main_button(ui, yes).min_size(vec2(0.0, height)))
                                    .clicked()
                                {
                                    picked =
                                        Some(Picked::Choice(Choice::Answer(ask, popup.from, true)));
                                }
                                if ui
                                    .add(Button::new("Decline").min_size(vec2(0.0, height)))
                                    .clicked()
                                {
                                    picked = Some(Picked::Choice(Choice::Answer(
                                        ask, popup.from, false,
                                    )));
                                }
                            });
                        }
                    });
                });
            });
        })
        .response
        .on_hover_cursor(CursorIcon::PointingHand);
    if picked.is_none() && open.clicked() {
        picked = Some(Picked::Choice(match popup.kind {
            Kind::Message => Choice::OpenChat(popup.from),
            Kind::Ask(_) => Choice::Profile(popup.from),
        }));
    }
    picked
}

impl Citrine {
    pub(in crate::app) fn sync_focus_popups(&mut self, desktop: bool) {
        let arrived = self.online.take_arrived();
        let on = desktop && self.ui.focus_mode && self.ui.settings.online.focus_popups;
        if !on {
            self.focus_popups.clear();
            return;
        }
        for from in arrived {
            if !self.online.can_chat(from) {
                continue;
            }
            if let Some(line) = friends::last_message(&self.online, from) {
                let name = self.online.display_name(from);
                self.focus_popups.message(from, name, line);
            }
        }
        let mut waiting: Vec<Waiting> = playing::pending_ask(&self.play)
            .map(|invite| Waiting {
                ask: Ask::Invite(invite.session),
                from: invite.from,
                name: playing::full_name(&self.play, &self.online, invite.from),
                line: playing::invite_line(&invite),
            })
            .into_iter()
            .collect();
        if self.online.is_connected() {
            waiting.extend(
                self.online
                    .with_state(FriendState::Incoming)
                    .into_iter()
                    .map(|friend| Waiting {
                        ask: Ask::Friend,
                        from: friend.online_id,
                        name: friend.display_name.clone(),
                        line: "Wants to be your friend".to_owned(),
                    }),
            );
        }
        self.focus_popups.sync(&waiting);
    }

    pub(in crate::app) fn show_focus_popups(&mut self, ctx: &Context, area: Rect) {
        let picture = self.ui.toast_picture;
        let fade = if crate::utils::reduced_motion() {
            0.0
        } else {
            FADE
        };
        let (used, choices) = self.focus_popups.show(ctx, area, picture, fade);
        if let (Some(used), Some(picture)) = (used, picture) {
            self.ui.toast_picture = Some(picture.union(used));
        }
        for choice in choices {
            self.focus_choice(choice);
        }
    }

    fn focus_choice(&mut self, choice: Choice) {
        match choice {
            Choice::OpenChat(id) => {
                self.toggle_focus_mode();
                self.ui.friends.chat.open(id);
                self.events.open_tab(Tab::Friends);
            }
            Choice::Profile(id) => {
                self.toggle_focus_mode();
                self.ui.friends.show_profile(id);
                self.events.open_tab(Tab::Friends);
            }
            Choice::Answer(Ask::Invite(session), _, accept) => {
                let invite = self
                    .play
                    .invites()
                    .iter()
                    .find(|i| i.session == session)
                    .cloned();
                match (invite, accept) {
                    (Some(invite), true) => self.answer_ask(&invite),
                    (Some(_), false) => self.play.decline(&mut self.online, session),
                    (None, _) => {}
                }
            }
            Choice::Answer(Ask::Friend, from, accept) => {
                self.online.answer(from, accept);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> OnlineId {
        OnlineId::new(n).expect("id")
    }

    fn invite(session: SessionId, from: u64) -> Waiting {
        Waiting {
            ask: Ask::Invite(session),
            from: id(from),
            name: "Ana".into(),
            line: "Wants to play Tetris".into(),
        }
    }

    fn run(popups: &mut FocusPopups, ctx: &Context, seconds: f32, hover: Option<egui::Pos2>) {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let picture = Rect::from_center_size(area.center(), vec2(800.0, 720.0));
        let steps = (seconds / 0.1).ceil() as usize;
        for _ in 0..steps {
            let mut input = egui::RawInput {
                screen_rect: Some(area),
                predicted_dt: 0.1,
                ..Default::default()
            };
            if let Some(pos) = hover {
                input.events.push(egui::Event::PointerMoved(pos));
            }
            let _ = ctx.run(input, |ctx| {
                popups.show(ctx, area, Some(picture), FADE);
            });
        }
    }

    #[test]
    fn a_message_popup_goes_after_six_seconds() {
        let ctx = Context::default();
        let mut popups = FocusPopups::default();
        popups.message(id(3), "Ana".into(), "hi".into());
        run(&mut popups, &ctx, 5.5, None);
        assert_eq!(popups.lines(), ["hi"]);
        run(&mut popups, &ctx, 1.0, None);
        assert!(popups.lines().is_empty());
    }

    #[test]
    fn a_hovered_message_popup_stays() {
        let ctx = Context::default();
        let mut popups = FocusPopups::default();
        popups.message(id(3), "Ana".into(), "hi".into());
        run(&mut popups, &ctx, 0.2, None);
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let picture = Rect::from_center_size(area.center(), vec2(800.0, 720.0));
        let stack = place(area, Some(picture), popups.stack_height(&ctx.style()));
        run(&mut popups, &ctx, 10.0, Some(stack.center()));
        assert_eq!(popups.lines(), ["hi"]);
    }

    #[test]
    fn a_second_message_from_the_same_friend_replaces_the_line() {
        let mut popups = FocusPopups::default();
        popups.message(id(3), "Ana".into(), "hi".into());
        popups.message(id(3), "Ana".into(), "you there?".into());
        assert_eq!(popups.lines(), ["you there?"]);
    }

    #[test]
    fn an_invite_popup_stays_until_the_invite_is_gone() {
        let ctx = Context::default();
        let mut popups = FocusPopups::default();
        popups.sync(&[invite(1, 3)]);
        run(&mut popups, &ctx, 30.0, None);
        popups.sync(&[invite(1, 3)]);
        assert_eq!(popups.lines(), ["Wants to play Tetris"]);
        popups.sync(&[]);
        assert!(popups.lines().is_empty());
    }

    #[test]
    fn a_dismissed_invite_stays_away_while_it_waits() {
        let mut popups = FocusPopups::default();
        popups.sync(&[invite(1, 3)]);
        let key = popups.popups[0].key().expect("an ask");
        popups.popups[0].close();
        popups.dismissed.push(key);
        popups.sync(&[invite(1, 3)]);
        assert!(popups.lines().is_empty());
        popups.sync(&[invite(2, 3)]);
        assert_eq!(popups.lines(), ["Wants to play Tetris"]);
    }

    #[test]
    fn the_popups_sit_beside_the_game_when_there_is_room() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        for width in [640.0, 720.0, 800.0] {
            let picture = Rect::from_center_size(area.center(), vec2(width, 720.0));
            let stack = place(area, Some(picture), 200.0);
            assert!(
                !stack.intersects(picture),
                "{width}: {stack:?} covers {picture:?}"
            );
            assert!(
                area.contains_rect(stack),
                "{width}: {stack:?} leaves the window"
            );
        }
    }

    #[test]
    fn without_room_the_popups_take_the_corner_with_the_least_game() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let picture = Rect::from_min_size(pos2(100.0, 0.0), vec2(1100.0, 800.0));
        let stack = place(area, Some(picture), 120.0);
        assert!(stack.left() < 100.0, "{stack:?} should take a left corner");
    }
}
