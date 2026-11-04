use crate::app::mobile::kit;
use crate::app::tabs::chat::{self, ChatUi};
use crate::app::tabs::playing;
use crate::app::tabs::{Tab, TabViewer};
use crate::app::ui_state::settings::SettingsTab;
use crate::app::widgets::action::{action_height, main_button, quiet_button, wide_button};
use crate::app::widgets::avatar;
use crate::app::widgets::badge;
use crate::app::widgets::confirm;
use crate::app::widgets::copy::copy_button;
use crate::app::widgets::emoji;
use crate::app::widgets::field::row_field;
use crate::app::widgets::problem::problem_text;
use crate::app::widgets::row::row_highlight;
use crate::app::widgets::settings::online::{connection_line, my_seen, retry_row};
use crate::app::widgets::typing;
use crate::icons;
use crate::online::{Online, RelayEntry, Seen, game_title, seen};
use crate::play::{Play, Stage};
use citrine_gb_net::{ErrorCode, Friend, FriendState, OnlineId};
use egui::{Align, CursorIcon, Label, Layout, RichText, Sense, TextStyle, Ui};
use web_time::Instant;

const ROW_AVATAR: f32 = 34.0;
const PHONE_AVATAR: f32 = 40.0;
const INVITE_ROW_MIN: f32 = 330.0;
const FOLD_BELOW: f32 = 280.0;

#[derive(Default)]
pub struct FriendsUi {
    add: String,
    add_error: Option<String>,
    adding: Option<OnlineId>,
    profile: Option<OnlineId>,
    ask: Option<(Ask, OnlineId, String)>,
    show_blocked: bool,
    copied_at: Option<Instant>,
    show_add: bool,
    pub chat: ChatUi,
}

impl FriendsUi {
    pub fn toggle_adding(&mut self) {
        self.show_add = !self.show_add;
    }

    pub fn show_profile(&mut self, id: OnlineId) {
        self.profile = Some(id);
    }
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
    let touch = viewer.ui.settings.layout.is_touch(ui.ctx());
    let game_open = viewer.emulator.open_game().is_some();
    chat::pump(viewer);
    if viewer.ui.friends.chat.current().is_some() && chat::take_escape(ui.ctx()) {
        viewer.ui.friends.chat.escape();
    }
    if viewer.ui.friends.chat.current().is_some() {
        if chat::show(viewer, ui) {
            viewer.ui.friends.profile = viewer.ui.friends.chat.current();
        }
        let offer = viewer
            .ui
            .friends
            .profile
            .map(|id| Offer::of(viewer.play, viewer.online, id, game_open))
            .unwrap_or_default();
        if let Some((id, way)) = profile(
            ui.ctx(),
            viewer.online,
            &mut viewer.ui.friends,
            offer,
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
            if !touch {
                if me_row(ui, online, stored.as_ref(), now) {
                    ui_state.settings.current_tab = SettingsTab::Online;
                    viewer.events.open_tab(Tab::Settings);
                }
                ui.add_space(8.0);
            }
            if touch && (state.show_add || online.friends().is_empty()) {
                phone_add_card(ui, online, state);
                ui.add_space(6.0);
            } else if !touch {
                add_row(ui, online, state);
                ui.add_space(6.0);
            }

            let connected = online.is_connected();
            let incoming: Vec<Friend> = online
                .with_state(FriendState::Incoming)
                .into_iter()
                .cloned()
                .collect();
            if !incoming.is_empty() && touch {
                kit::section_label(ui, "Wants to be your friend");
                kit::list(ui, |list| {
                    for person in &incoming {
                        let id = person.online_id;
                        let row = list.custom(|ui| {
                            phone_row(ui, person, None, 0, RowMode::Plain, "", |ui| {
                                ui.add_enabled_ui(connected, |ui| {
                                    if kit::pill(ui, "Accept", true).clicked() {
                                        online.answer(id, true);
                                    }
                                    if kit::pill(ui, "Decline", false).clicked() {
                                        online.answer(id, false);
                                    }
                                });
                            })
                        });
                        if row.face || row.open {
                            state.profile = Some(id);
                        }
                    }
                });
            } else if !incoming.is_empty() {
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
            if touch && !incoming.is_empty() {
                kit::section_label(ui, "Friends");
            } else if touch {
                ui.add_space(8.0);
            } else {
                heading(ui, "Friends");
            }
            if friends.is_empty() {
                if touch {
                    kit::card(ui, |ui| {
                        kit::padded(ui, |ui| empty_state(ui, online, state, now))
                    });
                } else {
                    empty_state(ui, online, state, now);
                }
            }
            let mut friend_rows = |ui: &mut Ui| {
                for (index, person) in friends.iter().enumerate() {
                    if touch && index > 0 {
                        kit::divider(ui);
                    }
                    let id = person.online_id;
                    let presence = online.presence(person);
                    let note = playing::invite_note(play, online, id);
                    let shown = playing::play_subtitle(play, id);
                    let in_session = shown.is_some();
                    let subtitle = shown.unwrap_or_else(|| subtitle(online, person, unix_now));
                    let unread = online.unread(id);
                    let invite = play.invites().iter().find(|i| i.from == id).cloned();
                    let sent = play.session().is_some_and(|s| {
                        s.is_live() && s.friend() == id && s.stage() == Stage::Inviting
                    });
                    let button = playing::play_button(play, online, id)
                        .filter(|_| !playing::invite_closed(play, online, id));
                    let watches = playing::can_watch(play, online, id, game_open);
                    let row_width = ui.available_width() - if touch { 24.0 } else { 0.0 };
                    let below = invite.is_some() && row_width < INVITE_ROW_MIN;
                    let subtitle = match unread {
                        1.. if !in_session => last_message(online, id).unwrap_or(subtitle),
                        _ => subtitle,
                    };
                    let subtitle = if online.is_typing(id, now) {
                        Line::Typing
                    } else {
                        Line::Text(&subtitle)
                    };
                    let mode = if !touch && !sent && invite.is_none() && row_width < FOLD_BELOW {
                        RowMode::Folded
                    } else {
                        RowMode::Chat
                    };
                    let row_actions = |ui: &mut Ui| {
                        if let Some(invite) = &invite {
                            if !below {
                                invite_buttons(ui, invite, connected, None, &mut actions);
                            }
                            return;
                        }
                        if !touch && ui.button(icons::DOTS_THREE).on_hover_text("More").clicked() {
                            state.profile = Some(id);
                        }
                        if sent {
                            let back = if touch {
                                kit::pill(ui, "Take back", false)
                            } else {
                                ui.button(icons::ARROW_U_UP_LEFT).on_hover_text("Take back")
                            };
                            if back.clicked() {
                                actions.push(PlayAction::Cancel);
                            }
                        } else if watches {
                            let watch = if touch {
                                kit::pill(ui, "Watch", false)
                            } else {
                                ui.button(icons::EYE).on_hover_text("Watch")
                            };
                            if watch.clicked() {
                                actions.push(PlayAction::Watch(id));
                            }
                        } else if button.is_some() {
                            let asked = if touch {
                                kit::pill(ui, "Play", false).clicked()
                            } else {
                                play_together(ui, PlayStyle::Icon)
                            };
                            if asked {
                                actions.push(PlayAction::Invite(id));
                            }
                        }
                    };
                    let line = Subtitle {
                        line: subtitle,
                        note: note.as_deref(),
                    };
                    let row = if touch {
                        phone_row(ui, person, presence, unread, mode, line, row_actions)
                    } else {
                        person_row(ui, person, presence, unread, mode, line, row_actions)
                    };
                    if let (true, Some(invite)) = (below, &invite) {
                        let margin = if touch { 12 } else { 0 };
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: margin,
                                right: margin,
                                top: 0,
                                bottom: margin,
                            })
                            .show(ui, |ui| {
                                ui.columns(2, |columns| {
                                    let size = egui::vec2(
                                        columns[0].available_width(),
                                        columns[0].spacing().interact_size.y,
                                    );
                                    let (left, right) = columns.split_at_mut(1);
                                    join_button(
                                        &mut left[0],
                                        invite,
                                        connected,
                                        Some(size),
                                        &mut actions,
                                    );
                                    decline_button(&mut right[0], invite, Some(size), &mut actions);
                                });
                            });
                        ui.add_space(4.0);
                    }
                    if row.face || row.menu {
                        state.profile = Some(id);
                    } else if row.open {
                        state.chat.open(id);
                    }
                }
            };
            if touch && !friends.is_empty() {
                kit::card(ui, friend_rows);
            } else {
                friend_rows(ui);
            }

            let outgoing: Vec<Friend> = online
                .with_state(FriendState::Outgoing)
                .into_iter()
                .cloned()
                .collect();
            if !outgoing.is_empty() {
                if touch {
                    kit::section_label(ui, "Sent");
                } else {
                    ui.add_space(6.0);
                    heading(ui, "Sent");
                }
                let mut sent_rows = |ui: &mut Ui| {
                    for (index, person) in outgoing.iter().enumerate() {
                        if touch && index > 0 {
                            kit::divider(ui);
                        }
                        let cancel = |ui: &mut Ui| {
                            let response = if touch {
                                ui.add_enabled_ui(connected, |ui| kit::pill(ui, "Cancel", false))
                                    .inner
                            } else {
                                ui.add_enabled(connected, egui::Button::new(icons::X))
                                    .on_hover_text("Cancel request")
                            };
                            if response.clicked() {
                                online.remove_friend(person.online_id);
                            }
                        };
                        let waiting = "Waiting for an answer";
                        let row = if touch {
                            phone_row(ui, person, None, 0, RowMode::Plain, waiting, cancel)
                        } else {
                            person_row(ui, person, None, 0, RowMode::Plain, waiting, cancel)
                        };
                        if row.face || row.open {
                            state.profile = Some(person.online_id);
                        }
                    }
                };
                if touch {
                    kit::card(ui, sent_rows);
                } else {
                    sent_rows(ui);
                }
            }

            let blocked: Vec<Friend> = online
                .with_state(FriendState::Blocked)
                .into_iter()
                .cloned()
                .collect();
            if !blocked.is_empty() {
                ui.add_space(if touch { 16.0 } else { 10.0 });
                let arrow = if state.show_blocked {
                    icons::CARET_DOWN
                } else {
                    icons::CARET_RIGHT
                };
                let toggle = ui
                    .horizontal(|ui| {
                        if touch {
                            ui.add_space(6.0);
                        }
                        ui.add(
                            Label::new(
                                RichText::new(format!("{arrow} Blocked ({})", blocked.len()))
                                    .weak(),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    })
                    .inner;
                if toggle {
                    state.show_blocked = !state.show_blocked;
                }
                if state.show_blocked {
                    let mut blocked_rows = |ui: &mut Ui| {
                        for (index, person) in blocked.iter().enumerate() {
                            if touch && index > 0 {
                                kit::divider(ui);
                            }
                            let unblock = |ui: &mut Ui| {
                                let response = if touch {
                                    ui.add_enabled_ui(connected, |ui| {
                                        kit::pill(ui, "Unblock", false)
                                    })
                                    .inner
                                } else {
                                    ui.add_enabled(connected, egui::Button::new("Unblock"))
                                };
                                if response.clicked() {
                                    online.unblock(person.online_id);
                                }
                            };
                            if touch {
                                phone_row(ui, person, None, 0, RowMode::Plain, "", unblock);
                            } else {
                                let id = person.online_id.to_string();
                                person_row(ui, person, None, 0, RowMode::Plain, &id, unblock);
                            }
                        }
                    };
                    if touch {
                        ui.add_space(4.0);
                        kit::card(ui, blocked_rows);
                    } else {
                        blocked_rows(ui);
                    }
                }
            }
            ui.add_space(8.0);
        });

    let offer = state
        .profile
        .map(|id| Offer::of(play, online, id, game_open))
        .unwrap_or_default();
    if let Some((id, way)) = profile(ui.ctx(), online, state, offer, unix_now, now) {
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
    Wide(egui::Vec2),
}

fn play_together(ui: &mut Ui, style: PlayStyle) -> bool {
    let label = format!("{}  Play together", icons::GAME_CONTROLLER);
    let response = match style {
        PlayStyle::Icon => ui.add(egui::Button::new(icons::GAME_CONTROLLER)),
        PlayStyle::Wide(size) => ui.add_sized(size, main_button(ui, label)),
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
    let button = main_button(ui, "Join");
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
        state.show_add = false;
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
            row_field(ui, &mut state.add)
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
        ui.label(problem_text(ui, error).small());
    }
    if add {
        submit_add(online, state);
    }
}

fn phone_add_card(ui: &mut Ui, online: &mut Online, state: &mut FriendsUi) {
    let connected = online.is_connected();
    let mut add = false;
    kit::card(ui, |ui| {
        kit::padded(ui, |ui| {
            ui.horizontal(|ui| {
                let pill = ui.spacing().interact_size.y.max(36.0) * 1.8;
                let width = ui.available_width() - pill - ui.spacing().item_spacing.x;
                let response = ui
                    .add_enabled_ui(connected, |ui| {
                        ui.set_max_width(width.max(0.0));
                        crate::app::mobile::kit::field(
                            ui,
                            egui::TextEdit::singleline(&mut state.add)
                                .hint_text("Online ID")
                                .char_limit(12),
                        )
                    })
                    .inner;
                if response.changed() {
                    state.add_error = None;
                }
                add |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let ready = connected && !state.add.trim().is_empty();
                add |= ui
                    .add_enabled_ui(ready, |ui| kit::pill(ui, "Add", true))
                    .inner
                    .clicked();
            });
            if let Some(error) = &state.add_error {
                ui.label(problem_text(ui, error).small());
            }
        })
    });
    if add {
        submit_add(online, state);
    }
}

fn submit_add(online: &mut Online, state: &mut FriendsUi) {
    if state.add.trim().is_empty() {
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
        if copy_button(ui, &id, state.copied_at, now) {
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

#[derive(Clone, Copy)]
struct Subtitle<'a> {
    line: Line<'a>,
    note: Option<&'a str>,
}

impl<'a> From<&'a str> for Subtitle<'a> {
    fn from(text: &'a str) -> Self {
        Self {
            line: Line::Text(text),
            note: None,
        }
    }
}

impl<'a> From<&'a String> for Subtitle<'a> {
    fn from(text: &'a String) -> Self {
        Self::from(text.as_str())
    }
}

#[derive(Default)]
struct RowClicks {
    face: bool,
    open: bool,
    menu: bool,
}

#[derive(Clone, Copy)]
enum Line<'a> {
    Text(&'a str),
    Typing,
}

impl Line<'_> {
    fn is_empty(&self) -> bool {
        matches!(self, Line::Text(""))
    }
}

impl<'a> From<&'a str> for Line<'a> {
    fn from(text: &'a str) -> Self {
        Line::Text(text)
    }
}

impl<'a> From<&'a String> for Line<'a> {
    fn from(text: &'a String) -> Self {
        Line::Text(text)
    }
}

fn person_row<'a>(
    ui: &mut Ui,
    person: &Friend,
    presence: Option<Seen>,
    unread: usize,
    mode: RowMode,
    subtitle: impl Into<Subtitle<'a>>,
    actions: impl FnOnce(&mut Ui),
) -> RowClicks {
    let subtitle = subtitle.into();
    let clicks = ui
        .scope(|ui| {
            ui.style_mut()
                .text_styles
                .insert(TextStyle::Small, egui::FontId::proportional(11.0));
            person_row_sized(
                ui, ROW_AVATAR, person, presence, unread, mode, subtitle, actions,
            )
        })
        .inner;
    ui.add_space(2.0);
    clicks
}

fn phone_row<'a>(
    ui: &mut Ui,
    person: &Friend,
    presence: Option<Seen>,
    unread: usize,
    mode: RowMode,
    subtitle: impl Into<Subtitle<'a>>,
    actions: impl FnOnce(&mut Ui),
) -> RowClicks {
    let subtitle = subtitle.into();
    let size = PHONE_AVATAR;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            let styles = &mut ui.style_mut().text_styles;
            styles.insert(TextStyle::Small, egui::FontId::proportional(13.0));
            styles.insert(TextStyle::Body, egui::FontId::proportional(16.0));
            person_row_sized(ui, size, person, presence, unread, mode, subtitle, actions)
        })
        .inner
}

#[allow(clippy::too_many_arguments)]
fn person_row_sized(
    ui: &mut Ui,
    size: f32,
    person: &Friend,
    presence: Option<Seen>,
    unread: usize,
    mode: RowMode,
    subtitle: Subtitle,
    actions: impl FnOnce(&mut Ui),
) -> RowClicks {
    let mut clicks = RowClicks::default();
    let chat = mode != RowMode::Plain;
    let fold = mode == RowMode::Folded;
    let small = ui.text_style_height(&TextStyle::Small);
    let mut block = ui.text_style_height(&TextStyle::Body);
    if !subtitle.line.is_empty() {
        block += 1.0 + small;
    }
    if subtitle.note.is_some() {
        block += 1.0 + small;
    }
    let height = size.max(ui.spacing().interact_size.y).max(block);
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
                size,
                presence,
            )
            .on_hover_cursor(CursorIcon::PointingHand);
            if unread > 0 && fold {
                badge::unread_badge(ui, face_response.rect, unread);
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
                    ui.add_space(((height - block) / 2.0).max(0.0));
                    let strong = ui.visuals().strong_text_color();
                    let name = emoji::line(ui, &person.display_name, TextStyle::Body, strong)
                        .on_hover_cursor(CursorIcon::PointingHand);
                    let color = if chat && unread > 0 {
                        strong
                    } else {
                        ui.visuals().text_color()
                    };
                    clicks.open |= name.clicked();
                    let line = match subtitle.line {
                        Line::Text("") => None,
                        Line::Text(text) => Some(emoji::line(ui, text, TextStyle::Small, color)),
                        Line::Typing => {
                            let accent = crate::app::ui_state::ui_theme::brand_mark(ui.visuals());
                            Some(typing::line(ui, "typing", TextStyle::Small, accent))
                        }
                    };
                    if let Some(line) = line {
                        clicks.open |= line.on_hover_cursor(CursorIcon::PointingHand).clicked();
                    }
                    if let Some(note) = subtitle.note {
                        let warn = ui.visuals().warn_fg_color;
                        let note = emoji::line(ui, note, TextStyle::Small, warn);
                        clicks.open |= note.on_hover_cursor(CursorIcon::PointingHand).clicked();
                    }
                });
            });
        },
    );
    if let Some(back) = back {
        if pointed || back.has_focus() {
            ui.painter().set(background, row_highlight(ui, area));
        }
        clicks.open |= back.clicked();
        clicks.menu = pointed && ui.input(|i| i.pointer.secondary_clicked());
    }
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
    badge::count_pill(ui, rect.center(), count);
}

fn last_message(online: &Online, id: OnlineId) -> Option<String> {
    let entry = online.conversation(id)?.entries.last()?;
    if entry.is_deleted() {
        return Some("Message deleted".to_owned());
    }
    match &entry.body {
        citrine_gb_net::Body::Text(text) => {
            Some(text.lines().next().unwrap_or_default().to_owned())
        }
        citrine_gb_net::Body::Share(share) => Some(chat::share::share_label(share.kind).to_owned()),
        citrine_gb_net::Body::Palette(_) => Some("Palette".to_owned()),
        citrine_gb_net::Body::Receipt(_)
        | citrine_gb_net::Body::Edit(_)
        | citrine_gb_net::Body::Delete(_) => None,
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

#[derive(Default)]
struct Offer {
    play: bool,
    note: Option<String>,
    watch: bool,
}

impl Offer {
    fn of(play: &Play, online: &Online, id: OnlineId, game_open: bool) -> Self {
        let note = playing::invite_note(play, online, id);
        Self {
            play: !playing::invite_closed(play, online, id)
                && playing::play_button(play, online, id).is_some(),
            watch: playing::can_watch(play, online, id, game_open),
            note,
        }
    }
}

fn profile(
    ctx: &egui::Context,
    online: &mut Online,
    state: &mut FriendsUi,
    offer: Offer,
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
            let presence = online
                .presence(&person)
                .filter(|_| person.state == FriendState::Friend);
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
                let width = ui.available_width();
                ui.add_space((width - 230.0).max(0.0) / 2.0);
                ui.label(RichText::new("Online ID").weak());
                ui.label(RichText::new(&text).monospace());
                if copy_button(ui, &text, state.copied_at, now) {
                    state.copied_at = Some(now);
                }
            });
        });
        ui.add_space(10.0);
        let button = |ui: &mut Ui, text: String| {
            let size = egui::vec2(ui.available_width(), action_height(ui));
            ui.add_enabled_ui(connected, |ui| ui.add_sized(size, egui::Button::new(text)))
                .inner
                .clicked()
        };
        match person.state {
            FriendState::Friend => {
                let size = egui::vec2(ui.available_width(), action_height(ui));
                if let Some(note) = &offer.note {
                    let warn = ui.visuals().warn_fg_color;
                    ui.vertical_centered(|ui| ui.label(RichText::new(note).color(warn)));
                    ui.add_space(4.0);
                }
                match (offer.play, offer.watch) {
                    (true, true) => ui.columns(2, |columns| {
                        let half = egui::vec2(columns[0].available_width(), size.y);
                        let join = egui::Button::new(format!("{}  Join", icons::GAME_CONTROLLER));
                        if columns[0].add_sized(half, join).clicked() {
                            invite = Some((id, Way::Play));
                            close = true;
                        }
                        let text = format!("{}  Watch", icons::EYE);
                        let button = main_button(&columns[1], text);
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
                    badge::count_pill(
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
                    let remove =
                        quiet_button(&columns[0], format!("{} Remove friend", icons::USER_MINUS));
                    if columns[0]
                        .add_enabled_ui(connected, |ui| ui.add_sized(size, remove))
                        .inner
                        .clicked()
                    {
                        state.ask = Some((Ask::Remove, id, person.display_name.clone()));
                        close = true;
                    }
                    let block = quiet_button(&columns[1], format!("{} Block", icons::PROHIBIT));
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
        if wide_button(ui, egui::Button::new("Close")).clicked() {
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
mod tests;
