use crate::app::tabs::chat::{self, ChatUi};
use crate::app::tabs::playing::{self, PlayButton};
use crate::app::tabs::{Tab, TabViewer};
use crate::app::ui_state::settings::SettingsTab;
use crate::app::widgets::avatar;
use crate::app::widgets::confirm;
use crate::app::widgets::emoji;
use crate::app::widgets::settings::online::{connection_line, my_seen, retry_row};
use crate::icons;
use crate::online::{Online, RelayEntry, Seen, game_title, seen};
use crate::play::Stage;
use citrine_gb_net::{ErrorCode, Friend, FriendState, OnlineId};
use egui::{Align, CursorIcon, Label, Layout, RichText, Sense, TextStyle, Ui};
use std::time::Duration;
use web_time::Instant;

const ROW_AVATAR: f32 = 34.0;
const INVITE_ROW_MIN: f32 = 330.0;
const FOLD_BELOW: f32 = 280.0;
const COPIED_FOR: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct FriendsUi {
    add: String,
    add_error: Option<String>,
    adding: Option<OnlineId>,
    profile: Option<OnlineId>,
    ask: Option<(Ask, OnlineId, String)>,
    show_blocked: bool,
    copied_at: Option<Instant>,
    pub chat: ChatUi,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Play,
    Watch,
}

enum PlayAction {
    Invite(OnlineId),
    Watch(OnlineId),
    Cancel,
    Join(crate::play::IncomingInvite),
    Decline(citrine_gb_net::SessionId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    Remove,
    Block,
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let now = Instant::now();
    let unix_now = jiff::Timestamp::now().as_second().max(0) as u64;
    let touch = crate::app::mobile_layout(viewer.ui.settings.layout, ui.ctx());
    let game_open = viewer.emulator.open_game().is_some();
    chat::pump(viewer);
    if viewer.ui.friends.chat.current().is_some() && chat::take_escape(ui.ctx()) {
        viewer.ui.friends.chat.escape();
    }
    if viewer.ui.friends.chat.current().is_some() {
        if chat::show(viewer, ui) {
            viewer.ui.friends.profile = viewer.ui.friends.chat.current();
        }
        let profile_button = viewer
            .ui
            .friends
            .profile
            .and_then(|id| playing::play_button(viewer.play, viewer.online, id));
        let profile_watch = viewer
            .ui
            .friends
            .profile
            .is_some_and(|id| playing::can_watch(viewer.play, viewer.online, id, game_open));
        if let Some((id, way)) = profile(
            ui.ctx(),
            viewer.online,
            &mut viewer.ui.friends,
            profile_button,
            profile_watch,
            unix_now,
            now,
        ) {
            let sent = match way {
                Way::Play => playing::invite(
                    viewer.play,
                    viewer.online,
                    viewer.emulator,
                    &mut viewer.ui.playing,
                    &viewer.ui.recent,
                    id,
                ),
                Way::Watch => playing::watch(
                    viewer.play,
                    viewer.online,
                    viewer.emulator,
                    &mut viewer.ui.playing,
                    &viewer.ui.recent,
                    id,
                ),
            };
            if !sent {
                viewer.events.notify_error("Couldn't send the invite");
            }
        }
        ask(ui.ctx(), viewer.online, &mut viewer.ui.friends);
        return;
    }
    let online = &mut *viewer.online;
    let play = &mut *viewer.play;
    let emulator = &mut *viewer.emulator;
    let ui_state = &mut *viewer.ui;
    let state = &mut ui_state.friends;
    let playing_ui = &mut ui_state.playing;
    let recent = &ui_state.recent;
    let stored = ui_state.settings.online.joined_entry().cloned();
    let mut actions = Vec::new();
    play.mark_invites_seen();
    track_add(online, state);

    egui::ScrollArea::vertical()
        .id_salt("friends_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if me_row(ui, online, stored.as_ref(), now) {
                ui_state.settings.current_tab = SettingsTab::Online;
                viewer.events.open_tab(Tab::Settings);
            }
            ui.add_space(8.0);
            add_row(ui, online, state);
            ui.add_space(6.0);

            let connected = online.is_connected();
            let incoming: Vec<Friend> = online
                .with_state(FriendState::Incoming)
                .into_iter()
                .cloned()
                .collect();
            if !incoming.is_empty() {
                heading(ui, &format!("Requests ({})", incoming.len()));
                for person in &incoming {
                    let row = person_row(
                        ui,
                        person,
                        None,
                        0,
                        RowMode::Plain,
                        &person.online_id.to_string(),
                        |_| {},
                    );
                    if row.face || row.open {
                        state.profile = Some(person.online_id);
                    }
                    ui.columns(2, |columns| {
                        let size = egui::vec2(
                            columns[0].available_width(),
                            columns[0].spacing().interact_size.y,
                        );
                        if columns[0]
                            .add_enabled_ui(connected, |ui| {
                                ui.add_sized(size, egui::Button::new("Accept"))
                            })
                            .inner
                            .clicked()
                        {
                            online.answer(person.online_id, true);
                        }
                        if columns[1]
                            .add_enabled_ui(connected, |ui| {
                                ui.add_sized(size, egui::Button::new("Decline"))
                            })
                            .inner
                            .clicked()
                        {
                            online.answer(person.online_id, false);
                        }
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(6.0);
            }

            let friends: Vec<Friend> = online.friends().into_iter().cloned().collect();
            heading(ui, "Friends");
            if friends.is_empty() {
                empty_state(ui, online, state, now);
            }
            for person in &friends {
                let id = person.online_id;
                let presence = connected.then(|| seen(person.presence));
                let live = playing::play_subtitle(play, id);
                let in_session = live.is_some();
                let subtitle = live.unwrap_or_else(|| subtitle(online, person, unix_now));
                let unread = online.unread(id);
                let invite = play.invites().iter().find(|i| i.from == id).cloned();
                let sent = play.session().is_some_and(|s| {
                    s.is_live() && s.friend() == id && s.stage() == Stage::Inviting
                });
                let button = playing::play_button(play, online, id);
                let watches = playing::can_watch(play, online, id, game_open);
                let below = invite.is_some() && ui.available_width() < INVITE_ROW_MIN;
                let subtitle = match unread {
                    1.. if !in_session => last_message(online, id).unwrap_or(subtitle),
                    _ => subtitle,
                };
                let mode =
                    if !touch && !sent && invite.is_none() && ui.available_width() < FOLD_BELOW {
                        RowMode::Folded
                    } else {
                        RowMode::Chat
                    };
                let row = person_row(ui, person, presence, unread, mode, &subtitle, |ui| {
                    if let Some(invite) = &invite {
                        if !below {
                            invite_buttons(ui, invite, connected, None, &mut actions);
                        }
                        return;
                    }
                    if ui.button(icons::DOTS_THREE).on_hover_text("More").clicked() {
                        state.profile = Some(id);
                    }
                    if sent {
                        if ui
                            .button(icons::ARROW_U_UP_LEFT)
                            .on_hover_text("Take back")
                            .clicked()
                        {
                            actions.push(PlayAction::Cancel);
                        }
                    } else if watches {
                        let eye = if touch { "Watch" } else { icons::EYE };
                        if ui.button(eye).on_hover_text("Watch").clicked() {
                            actions.push(PlayAction::Watch(id));
                        }
                    } else if button.is_some()
                        && play_together(
                            ui,
                            if touch {
                                PlayStyle::Text
                            } else {
                                PlayStyle::Icon
                            },
                        )
                    {
                        actions.push(PlayAction::Invite(id));
                    }
                });
                if let (true, Some(invite)) = (below, &invite) {
                    ui.columns(2, |columns| {
                        let size = egui::vec2(
                            columns[0].available_width(),
                            columns[0].spacing().interact_size.y,
                        );
                        let (left, right) = columns.split_at_mut(1);
                        join_button(&mut left[0], invite, connected, Some(size), &mut actions);
                        decline_button(&mut right[0], invite, Some(size), &mut actions);
                    });
                    ui.add_space(4.0);
                }
                if row.face || row.menu {
                    state.profile = Some(id);
                } else if row.open {
                    state.chat.open(id);
                }
            }

            let outgoing: Vec<Friend> = online
                .with_state(FriendState::Outgoing)
                .into_iter()
                .cloned()
                .collect();
            if !outgoing.is_empty() {
                ui.add_space(6.0);
                heading(ui, "Sent");
                for person in &outgoing {
                    let row = person_row(
                        ui,
                        person,
                        None,
                        0,
                        RowMode::Plain,
                        "Waiting for an answer",
                        |ui| {
                            if ui
                                .add_enabled(connected, egui::Button::new(icons::X))
                                .on_hover_text("Cancel request")
                                .clicked()
                            {
                                online.remove_friend(person.online_id);
                            }
                        },
                    );
                    if row.face || row.open {
                        state.profile = Some(person.online_id);
                    }
                }
            }

            let blocked: Vec<Friend> = online
                .with_state(FriendState::Blocked)
                .into_iter()
                .cloned()
                .collect();
            if !blocked.is_empty() {
                ui.add_space(10.0);
                let arrow = if state.show_blocked {
                    icons::CARET_DOWN
                } else {
                    icons::CARET_RIGHT
                };
                if ui
                    .add(
                        Label::new(
                            RichText::new(format!("{arrow} Blocked ({})", blocked.len())).weak(),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .clicked()
                {
                    state.show_blocked = !state.show_blocked;
                }
                if state.show_blocked {
                    for person in &blocked {
                        person_row(
                            ui,
                            person,
                            None,
                            0,
                            RowMode::Plain,
                            &person.online_id.to_string(),
                            |ui| {
                                if ui
                                    .add_enabled(connected, egui::Button::new("Unblock"))
                                    .clicked()
                                {
                                    online.unblock(person.online_id);
                                }
                            },
                        );
                    }
                }
            }
            ui.add_space(8.0);
        });

    let profile_button = state
        .profile
        .and_then(|id| playing::play_button(play, online, id));
    let profile_watch = state
        .profile
        .is_some_and(|id| playing::can_watch(play, online, id, game_open));
    if let Some((id, way)) = profile(
        ui.ctx(),
        online,
        state,
        profile_button,
        profile_watch,
        unix_now,
        now,
    ) {
        actions.push(match way {
            Way::Play => PlayAction::Invite(id),
            Way::Watch => PlayAction::Watch(id),
        });
    }
    ask(ui.ctx(), online, state);

    for action in actions {
        match action {
            PlayAction::Invite(id) => {
                if !playing::invite(play, online, emulator, playing_ui, recent, id) {
                    viewer.events.notify_error("Couldn't send the invite");
                }
            }
            PlayAction::Watch(id) => {
                if !playing::watch(play, online, emulator, playing_ui, recent, id) {
                    viewer.events.notify_error("Couldn't send the request");
                }
            }
            PlayAction::Cancel => play.leave(online),
            PlayAction::Join(invite) => {
                if !playing::join(play, online, emulator, playing_ui, recent, &invite) {
                    viewer.events.notify_error("Couldn't join the game");
                }
            }
            PlayAction::Decline(session) => play.decline(online, session),
        }
    }
}

enum PlayStyle {
    Icon,
    Text,
    Wide(egui::Vec2),
}

fn play_together(ui: &mut Ui, style: PlayStyle) -> bool {
    let label = format!("{}  Play together", icons::GAME_CONTROLLER);
    let response = match style {
        PlayStyle::Icon => ui.add(egui::Button::new(icons::GAME_CONTROLLER)),
        PlayStyle::Text => ui.add(egui::Button::new("Play together")),
        PlayStyle::Wide(size) => ui.add_sized(size, playing::main_button(ui, label)),
    };
    response.on_hover_text("Play together").clicked()
}

fn invite_buttons(
    ui: &mut Ui,
    invite: &crate::play::IncomingInvite,
    connected: bool,
    size: Option<egui::Vec2>,
    actions: &mut Vec<PlayAction>,
) {
    decline_button(ui, invite, size, actions);
    join_button(ui, invite, connected, size, actions);
}

fn join_button(
    ui: &mut Ui,
    invite: &crate::play::IncomingInvite,
    connected: bool,
    size: Option<egui::Vec2>,
    actions: &mut Vec<PlayAction>,
) {
    let ready = connected;
    let button = playing::main_button(ui, "Join");
    let response = match size {
        Some(size) => {
            ui.add_enabled_ui(ready, |ui| ui.add_sized(size, button))
                .inner
        }
        None => ui.add_enabled(ready, button),
    };
    if response.clicked() {
        actions.push(PlayAction::Join(invite.clone()));
    }
}

fn decline_button(
    ui: &mut Ui,
    invite: &crate::play::IncomingInvite,
    size: Option<egui::Vec2>,
    actions: &mut Vec<PlayAction>,
) {
    let button = egui::Button::new("Decline");
    let response = match size {
        Some(size) => ui.add_sized(size, button),
        None => ui.add(button),
    };
    if response.clicked() {
        actions.push(PlayAction::Decline(invite.session));
    }
}

fn track_add(online: &mut Online, state: &mut FriendsUi) {
    let Some(id) = state.adding else {
        return;
    };
    if online.friend(id).is_some() {
        state.adding = None;
        state.add.clear();
        state.add_error = None;
    } else if let Some(error) = online.request_error() {
        state.adding = None;
        state.add_error = Some(match error {
            ErrorCode::UnknownUser => "No one has this online ID".into(),
            ErrorCode::RateLimited => "Too many requests. Wait a moment.".into(),
            _ => "The server couldn't send the request".into(),
        });
        online.clear_request_error();
    }
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).strong());
    ui.add_space(1.0);
}

fn me_row(ui: &mut Ui, online: &mut Online, stored: Option<&RelayEntry>, now: Instant) -> bool {
    let name = online
        .account()
        .map(|a| a.display_name.clone())
        .or_else(|| stored.and_then(|e| e.display_name.clone()))
        .unwrap_or_default();
    let seed = online
        .account()
        .map(|a| a.online_id)
        .or_else(|| stored.and_then(|e| e.online_id.as_deref()?.parse::<OnlineId>().ok()))
        .map(|id| id.value())
        .unwrap_or(0);
    let mut open = false;
    ui.horizontal(|ui| {
        open |= avatar::avatar(ui, &name, seed, 40.0, Some(my_seen(online)))
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Your account")
            .clicked();
        ui.vertical(|ui| {
            open |= ui
                .add(
                    Label::new(RichText::new(&name).strong().size(15.0))
                        .truncate()
                        .sense(Sense::click()),
                )
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked();
            if online.is_connected() {
                connection_line(ui, online, now);
            }
        });
    });
    if !online.is_connected() {
        connection_line(ui, online, now);
        retry_row(ui, online, now);
    }
    open
}

fn add_row(ui: &mut Ui, online: &mut Online, state: &mut FriendsUi) {
    let connected = online.is_connected();
    let mut add = false;
    ui.horizontal(|ui| {
        let font = TextStyle::Button.resolve(ui.style());
        let measure = |text: &str| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x
        };
        let worded = format!("{} Add", icons::USER_PLUS);
        let padding = ui.spacing().item_spacing.x + 2.0 * ui.spacing().button_padding.x;
        let margin = egui::Margin::symmetric(4, 2);
        let field_least = measure("Online ID") + margin.sum().x + ui.spacing().item_spacing.x;
        let wide = measure(&worded) + padding;
        let words = ui.available_width() - wide >= field_least;
        let label = if words {
            worded
        } else {
            icons::USER_PLUS.to_string()
        };
        let button_width = if words {
            wide
        } else {
            measure(&label) + padding
        };
        let response = ui.add_enabled(
            connected,
            egui::TextEdit::singleline(&mut state.add)
                .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                .vertical_align(egui::Align::Center)
                .hint_text("Online ID")
                .char_limit(12)
                .margin(margin)
                .desired_width((ui.available_width() - button_width - margin.sum().x).max(0.0)),
        );
        if response.changed() {
            state.add_error = None;
        }
        add |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let button = ui.add_enabled(
            connected && !state.add.trim().is_empty(),
            egui::Button::new(label),
        );
        let button = if words {
            button
        } else {
            button.on_hover_text("Add").on_disabled_hover_text("Add")
        };
        add |= button.clicked();
    });
    if let Some(error) = &state.add_error {
        ui.label(
            RichText::new(format!("{} {error}", icons::WARNING))
                .small()
                .color(ui.visuals().warn_fg_color),
        );
    }
    if !add || state.add.trim().is_empty() {
        return;
    }
    match state.add.trim().parse::<OnlineId>() {
        Ok(id) if online.account().is_some_and(|a| a.online_id == id) => {
            state.add_error = Some("That's your own online ID".into());
        }
        Ok(id) => {
            state.add_error = None;
            state.adding = Some(id);
            online.add_friend(id);
        }
        Err(_) => state.add_error = Some("An online ID looks like ABCD-1234".into()),
    }
}

fn empty_state(ui: &mut Ui, online: &Online, state: &mut FriendsUi, now: Instant) {
    let Some(account) = online.account() else {
        return;
    };
    let id = account.online_id.to_string();
    ui.label(RichText::new("No friends yet. Share your online ID to get added.").weak());
    ui.horizontal(|ui| {
        ui.label(RichText::new(&id).monospace().size(15.0));
        let copied = state
            .copied_at
            .is_some_and(|at| now.duration_since(at) < COPIED_FOR);
        let label = if copied {
            format!("{} Copied", icons::CHECK)
        } else {
            format!("{} Copy", icons::COPY)
        };
        if ui.button(label).clicked() {
            ui.ctx().copy_text(id.clone());
            state.copied_at = Some(now);
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowMode {
    Plain,
    Chat,
    Folded,
}

#[derive(Default)]
struct RowClicks {
    face: bool,
    open: bool,
    menu: bool,
}

fn person_row(
    ui: &mut Ui,
    person: &Friend,
    presence: Option<Seen>,
    unread: usize,
    mode: RowMode,
    subtitle: &str,
    actions: impl FnOnce(&mut Ui),
) -> RowClicks {
    let mut clicks = RowClicks::default();
    let chat = mode != RowMode::Plain;
    let fold = mode == RowMode::Folded;
    let height = ROW_AVATAR.max(ui.spacing().interact_size.y);
    let area = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), height));
    let background = ui.painter().add(egui::Shape::Noop);
    let back = chat.then(|| {
        ui.interact(
            area,
            ui.id().with(("friend_row", person.online_id)),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
    });
    let pointed = ui.rect_contains_pointer(area);
    let focused = focus_within(ui, area);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        Layout::left_to_right(Align::Center),
        |ui| {
            let face_response = avatar::avatar(
                ui,
                &person.display_name,
                person.online_id.value(),
                ROW_AVATAR,
                presence,
            )
            .on_hover_cursor(CursorIcon::PointingHand);
            if unread > 0 && fold {
                chat::unread_badge(ui, face_response.rect, unread);
            }
            clicks.face = face_response.clicked();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !fold || pointed || focused {
                    actions(ui);
                }
                if chat && !fold && unread > 0 {
                    count_pill(ui, unread);
                }
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let strong = ui.visuals().strong_text_color();
                    let name = emoji::line(ui, &person.display_name, TextStyle::Body, strong)
                        .on_hover_cursor(CursorIcon::PointingHand);
                    let color = if chat && unread > 0 {
                        strong
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    let line = emoji::line(ui, subtitle, TextStyle::Small, color)
                        .on_hover_cursor(CursorIcon::PointingHand);
                    clicks.open |= name.clicked() || line.clicked();
                });
            });
        },
    );
    if let Some(back) = back {
        if pointed || back.has_focus() {
            ui.painter().set(
                background,
                egui::Shape::rect_filled(
                    area.expand(2.0),
                    6.0,
                    ui.visuals()
                        .widgets
                        .hovered
                        .weak_bg_fill
                        .gamma_multiply(0.4),
                ),
            );
        }
        clicks.open |= back.clicked();
        clicks.menu = pointed && ui.input(|i| i.pointer.secondary_clicked());
    }
    ui.add_space(2.0);
    clicks
}

fn focus_within(ui: &Ui, area: egui::Rect) -> bool {
    let ctx = ui.ctx();
    ctx.memory(|m| m.focused())
        .and_then(|id| ctx.read_response(id))
        .is_some_and(|r| area.contains_rect(r.rect))
}

fn count_pill(ui: &mut Ui, count: usize) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 18.0), Sense::hover());
    chat::count_pill(ui, rect.center(), count);
}

fn last_message(online: &Online, id: OnlineId) -> Option<String> {
    let entry = online.conversation(id)?.entries.last()?;
    match &entry.body {
        citrine_gb_net::Body::Text(text) => {
            Some(text.lines().next().unwrap_or_default().to_owned())
        }
        citrine_gb_net::Body::Share(share) => Some(chat::share_label(share.kind).to_owned()),
        citrine_gb_net::Body::Receipt(_) => None,
    }
}

pub fn subtitle(online: &Online, person: &Friend, unix_now: u64) -> String {
    if !online.is_connected() {
        return person.online_id.to_string();
    }
    match seen(person.presence) {
        Seen::Online => match online.playing(person.online_id) {
            Some(game) => match game_title(game) {
                Some(title) => format!("Playing {title}"),
                None => "Playing a game".into(),
            },
            None => "Online".into(),
        },
        Seen::Away => "Away".into(),
        Seen::Offline { last_seen: 0 } => "Offline".into(),
        Seen::Offline { last_seen } => format!("Last seen {}", ago(last_seen, unix_now)),
    }
}

pub fn ago(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..60 => "just now".into(),
        60..3_600 => format!("{} min ago", secs / 60),
        3_600..86_400 => format!("{} h ago", secs / 3_600),
        86_400..172_800 => "yesterday".into(),
        172_800..1_209_600 => format!("{} days ago", secs / 86_400),
        _ => jiff::Timestamp::from_second(then as i64)
            .map(|t| {
                t.to_zoned(jiff::tz::TimeZone::system())
                    .strftime("%-d %b %Y")
                    .to_string()
            })
            .unwrap_or_default(),
    }
}

fn profile(
    ctx: &egui::Context,
    online: &mut Online,
    state: &mut FriendsUi,
    play_button: Option<PlayButton>,
    watch: bool,
    unix_now: u64,
    now: Instant,
) -> Option<(OnlineId, Way)> {
    let id = state.profile?;
    let Some(person) = online.friend(id).cloned() else {
        state.profile = None;
        return None;
    };
    let connected = online.is_connected();
    let mut close = false;
    let mut invite = None;
    let modal = egui::Modal::new(egui::Id::new("friend_profile")).show(ctx, |ui| {
        ui.set_width(280.0);
        ui.vertical_centered(|ui| {
            let presence =
                (connected && person.state == FriendState::Friend).then(|| seen(person.presence));
            avatar::avatar(ui, &person.display_name, id.value(), 72.0, presence);
            ui.add_space(4.0);
            ui.add(Label::new(RichText::new(&person.display_name).strong().size(17.0)).truncate());
            let line = match person.state {
                FriendState::Friend => subtitle(online, &person, unix_now),
                FriendState::Incoming => "Wants to be your friend".into(),
                FriendState::Outgoing => "Waiting for an answer".into(),
                FriendState::Blocked => "Blocked".into(),
            };
            ui.label(RichText::new(line).weak());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let text = id.to_string();
                let copied = state
                    .copied_at
                    .is_some_and(|at| now.duration_since(at) < COPIED_FOR);
                let width = ui.available_width();
                ui.add_space((width - 230.0).max(0.0) / 2.0);
                ui.label(RichText::new("Online ID").weak());
                ui.label(RichText::new(&text).monospace());
                let label = if copied {
                    format!("{} Copied", icons::CHECK)
                } else {
                    format!("{} Copy", icons::COPY)
                };
                if ui.button(label).clicked() {
                    ui.ctx().copy_text(text);
                    state.copied_at = Some(now);
                }
            });
        });
        ui.add_space(10.0);
        let button = |ui: &mut Ui, text: String| {
            let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y.max(32.0));
            ui.add_enabled_ui(connected, |ui| ui.add_sized(size, egui::Button::new(text)))
                .inner
                .clicked()
        };
        match person.state {
            FriendState::Friend => {
                let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y.max(32.0));
                match (play_button.is_some(), watch) {
                    (true, true) => ui.columns(2, |columns| {
                        let half = egui::vec2(columns[0].available_width(), size.y);
                        let join = egui::Button::new(format!("{}  Join", icons::GAME_CONTROLLER));
                        if columns[0].add_sized(half, join).clicked() {
                            invite = Some((id, Way::Play));
                            close = true;
                        }
                        let text = format!("{}  Watch", icons::EYE);
                        let button = playing::main_button(&columns[1], text);
                        if columns[1].add_sized(half, button).clicked() {
                            invite = Some((id, Way::Watch));
                            close = true;
                        }
                    }),
                    (true, false) => {
                        if play_together(ui, PlayStyle::Wide(size)) {
                            invite = Some((id, Way::Play));
                            close = true;
                        }
                    }
                    (false, _) => {}
                }
                let message = ui.add_sized(
                    size,
                    egui::Button::new(format!("{} Message", icons::CHAT_CIRCLE)),
                );
                let unread = online.unread(id);
                if unread > 0 {
                    chat::count_pill(
                        ui,
                        message.rect.right_center() - egui::vec2(18.0, 0.0),
                        unread,
                    );
                }
                if message.clicked() {
                    state.chat.open(id);
                    close = true;
                }
                ui.add_space(2.0);
                ui.columns(2, |columns| {
                    let size = egui::vec2(columns[0].available_width(), size.y);
                    let remove = playing::quiet_button(
                        &columns[0],
                        format!("{} Remove friend", icons::USER_MINUS),
                    );
                    if columns[0]
                        .add_enabled_ui(connected, |ui| ui.add_sized(size, remove))
                        .inner
                        .clicked()
                    {
                        state.ask = Some((Ask::Remove, id, person.display_name.clone()));
                        close = true;
                    }
                    let block =
                        playing::quiet_button(&columns[1], format!("{} Block", icons::PROHIBIT));
                    if columns[1]
                        .add_enabled_ui(connected, |ui| ui.add_sized(size, block))
                        .inner
                        .clicked()
                    {
                        state.ask = Some((Ask::Block, id, person.display_name.clone()));
                        close = true;
                    }
                });
            }
            FriendState::Incoming => {
                if button(ui, format!("{} Accept", icons::CHECK)) {
                    online.answer(id, true);
                    close = true;
                }
                if button(ui, format!("{} Decline", icons::X)) {
                    online.answer(id, false);
                    close = true;
                }
                if button(ui, format!("{} Block", icons::PROHIBIT)) {
                    state.ask = Some((Ask::Block, id, person.display_name.clone()));
                    close = true;
                }
            }
            FriendState::Outgoing => {
                if button(ui, format!("{} Cancel request", icons::X)) {
                    online.remove_friend(id);
                    close = true;
                }
            }
            FriendState::Blocked => {
                if button(ui, "Unblock".into()) {
                    online.unblock(id);
                    close = true;
                }
            }
        }
        ui.add_space(4.0);
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y.max(32.0));
        if ui.add_sized(size, egui::Button::new("Close")).clicked() {
            close = true;
        }
    });
    if close || modal.should_close() {
        state.profile = None;
    }
    invite
}

fn ask(ctx: &egui::Context, online: &mut Online, state: &mut FriendsUi) {
    let Some((ask, id, name)) = state.ask.clone() else {
        return;
    };
    let (question, action) = match ask {
        Ask::Remove => (format!("Remove {name} from your friends?"), "Remove"),
        Ask::Block => (
            format!("Block {name}? They can't send you requests or see when you're online."),
            "Block",
        ),
    };
    if let Some(yes) = confirm::confirm(ctx, "friend_ask", &question, action) {
        state.ask = None;
        if yes {
            match ask {
                Ask::Remove => online.remove_friend(id),
                Ask::Block => online.block(id),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mia() -> Friend {
        Friend {
            online_id: OnlineId::new(7).unwrap(),
            display_name: "Mia Sommer".into(),
            public_key: citrine_gb_net::PublicKey([0; 32]),
            state: FriendState::Friend,
            presence: None,
        }
    }

    fn row_frame(ctx: &egui::Context, events: Vec<egui::Event>, fold: bool) -> (bool, RowClicks) {
        let mode = if fold { RowMode::Folded } else { RowMode::Chat };
        let mut shown = false;
        let mut clicks = RowClicks::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(217.0, 300.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                clicks = person_row(ui, &mia(), None, 3, mode, "Bring the cable", |ui| {
                    shown = true;
                    let _ = ui.button(icons::DOTS_THREE);
                });
            });
        });
        (shown, clicks)
    }

    #[test]
    fn the_add_row_and_the_rows_under_it_fit_a_thin_dock() {
        let mut online = Online::default();
        for width in [120.0, 148.0, 180.0, 300.0] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(2.0);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 600.0),
                )),
                ..Default::default()
            };
            let mut state = FriendsUi::default();
            let mut inner = egui::Rect::NOTHING;
            let mut shapes = Vec::new();
            for _ in 0..2 {
                shapes = ctx
                    .run(input.clone(), |ctx| {
                        egui::SidePanel::left("dock")
                            .exact_width(width)
                            .show(ctx, |ui| {
                                inner = ui.max_rect();
                                add_row(ui, &mut online, &mut state);
                                person_row(
                                    ui,
                                    &mia(),
                                    None,
                                    0,
                                    RowMode::Folded,
                                    "Last seen 12 min ago",
                                    |_| {},
                                );
                            });
                    })
                    .shapes;
            }
            let hint = shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Text(t) if t.galley.text() == "Online ID")
            });
            assert!(hint, "{width}: the field shows its hint");
            for clipped in &shapes {
                let rect = clipped.shape.visual_bounding_rect();
                if !rect.is_positive() || rect.left() < inner.left() || rect.width() < 1.5 {
                    continue;
                }
                assert!(
                    rect.right() <= inner.right() + 0.5,
                    "{width}: {:?} ends at {} past {}",
                    clipped.shape,
                    rect.right(),
                    inner.right()
                );
            }
        }
    }

    fn press(button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(100.0, 25.0),
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn a_folded_row_shows_its_buttons_only_under_the_pointer() {
        let ctx = egui::Context::default();
        assert!(!row_frame(&ctx, vec![], true).0);
        let over = vec![egui::Event::PointerMoved(egui::pos2(100.0, 25.0))];
        assert!(row_frame(&ctx, over, true).0);
        let away = vec![egui::Event::PointerMoved(egui::pos2(100.0, 250.0))];
        assert!(!row_frame(&ctx, away, true).0);
        assert!(row_frame(&ctx, vec![], false).0);
    }

    #[test]
    fn a_folded_row_shows_its_buttons_while_it_has_keyboard_focus() {
        let ctx = egui::Context::default();
        assert!(!row_frame(&ctx, vec![], true).0);
        let tab = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        row_frame(&ctx, vec![tab], true);
        assert!(row_frame(&ctx, vec![], true).0);
    }

    #[test]
    fn a_click_on_the_row_opens_the_chat_and_a_right_click_its_menu() {
        let ctx = egui::Context::default();
        let over = vec![egui::Event::PointerMoved(egui::pos2(100.0, 25.0))];
        row_frame(&ctx, over, true);
        row_frame(&ctx, vec![press(egui::PointerButton::Primary, true)], true);
        let (_, clicks) = row_frame(&ctx, vec![press(egui::PointerButton::Primary, false)], true);
        assert!(clicks.open && !clicks.menu && !clicks.face);
        row_frame(
            &ctx,
            vec![press(egui::PointerButton::Secondary, true)],
            true,
        );
        let (_, clicks) = row_frame(
            &ctx,
            vec![press(egui::PointerButton::Secondary, false)],
            true,
        );
        assert!(clicks.menu && !clicks.open);
    }

    #[test]
    fn last_seen_reads_plainly() {
        assert_eq!(ago(1_000, 1_030), "just now");
        assert_eq!(ago(0, 7_200), "2 h ago");
        assert_eq!(ago(0, 90_000), "yesterday");
        assert_eq!(
            ago(1_761_825_600, 1_761_825_600 + 30 * 86_400),
            "30 Oct 2025"
        );
    }
}
