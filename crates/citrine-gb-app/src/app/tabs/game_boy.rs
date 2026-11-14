use crate::app::hud;
use crate::app::tabs::linked;
use crate::app::tabs::playing::{self, Answer, BannerAction, PickFor};
use crate::app::tabs::{Tab, TabViewer};
use crate::app::toasts;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::{screens_menu, tilt};
use crate::local_link::Game;
use egui::RichText;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let after = screen_buttons(viewer, ui);
    hud_button(viewer, ui, after);
    bars(viewer, ui);
    let panel = ui.available_rect_before_wrap();
    viewer.ui.toast_area = Some(panel);
    game(viewer, ui);
    popups(viewer, ui.ctx(), panel);
}

fn bars(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let sound = playing::FriendSound::of(&viewer.ui.settings.online);
    if let Some(facts) = playing::room_facts(viewer.play, viewer.online, &viewer.ui.playing)
        && let Some(mine) = playing::my_menu(viewer.play, viewer.online, sound)
    {
        let action = banner_frame(ui, |ui| {
            let row = playing::member_row(ui, &facts, &mine, false);
            playing::request_card(ui, &facts, false).or(row)
        });
        if let Some(action) = action
            && let Some(go) = playing::settle_room(
                action,
                viewer.play,
                viewer.online,
                viewer.emulator,
                &mut viewer.ui.playing,
            )
        {
            room_go(viewer, go);
        }
    } else if let Some(session) = viewer.play.session().filter(|s| s.is_live()) {
        let facts = playing::BarFacts {
            muted: viewer.ui.settings.muted,
            compact: false,
            friend_sound: sound,
        };
        let action = banner_frame(ui, |ui| {
            playing::bar(ui, viewer.play, viewer.online, session, facts)
        });
        if let Some(action) = action {
            playing::settle_bar(action, viewer.play, viewer.online, &mut viewer.ui.playing);
        }
    }
}

fn game(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    if let Some(session) = viewer.play.session()
        && let Some((label, progress)) = playing::transfer(
            session,
            &playing::member_name(viewer.play, viewer.online, session.friend()),
        )
    {
        playing::arriving(
            ui,
            ui.available_rect_before_wrap(),
            playing::friend_video(viewer.play, session),
            &label,
            progress,
        );
        return;
    }
    if playing::download_screen(
        &mut viewer.ui.playing,
        ui,
        ui.available_rect_before_wrap(),
        viewer.play,
        viewer.online,
        viewer.emulator,
    ) {
        return;
    }
    if let Some(session) = viewer.play.session()
        && let Some(line) = playing::screen_note(
            &playing::member_name(viewer.play, viewer.online, session.friend()),
            session,
        )
    {
        let copy = playing::needs_copy(session).then_some("Open my copy");
        if playing::note_over(
            ui,
            ui.available_rect_before_wrap(),
            playing::friend_video(viewer.play, session),
            &line,
            copy,
        ) {
            viewer.ui.playing.wants_copy = true;
        }
        return;
    }
    if let Some(link) = viewer.emulator.link() {
        let line = linked::driver(link, Game::First, false);
        ui.vertical_centered(|ui| ui.label(RichText::new(line).weak()));
    }
    let area = ui.available_rect_before_wrap();
    let tilting = viewer.emulator.cart().motion();
    let room = tilt::screen_area(area, tilting);
    let screen = ui
        .scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
            let only = viewer.emulator.screen_tabs().then_some(0);
            ui.vertical_centered(|ui| viewer.emulator.ui_screen(ui, only))
                .inner
        })
        .inner;
    let playing =
        viewer.emulator.has_game() || viewer.play.session().is_some_and(|s| s.runs_engine());
    if let Some(screen) = screen
        && playing
    {
        let badge = tilt::badge_rect(screen, area);
        viewer.ui.toast_picture = Some(if tilting { screen.union(badge) } else { screen });
    }
    if let Some(screen) = screen
        && tilting
        && tilt::badge(ui, viewer.emulator.gimmicks.motion, screen, area)
    {
        viewer.events.centre_tilt();
    }
    if let Some(link) = viewer.emulator.link()
        && link.takes_clicks()
        && link.control() == Game::Second
    {
        let clicked = ui
            .interact(area, egui::Id::new("first_screen"), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked();
        if clicked {
            viewer.emulator.set_link_control(Game::First);
        }
    }
    if playing {
        let place = hud::overlay::Place {
            area,
            picture: screen,
            screens: viewer.emulator.placed_screens().to_vec(),
            touch: false,
        };
        let outcome = hud::show(ui, &place, viewer.ui, viewer.emulator, viewer.stats);
        if let Some(id) = outcome.remove {
            viewer.events.remove_hud(id);
        }
    }
}

pub fn bottom_screen(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let area = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
        ui.vertical_centered(|ui| viewer.emulator.ui_screen(ui, Some(1)));
    });
}

fn last_tab_of(viewer: &TabViewer, ui: &egui::Ui) -> Option<egui::Rect> {
    let margin = egui_dock::Style::from_egui(ui.style())
        .tab
        .tab_body
        .inner_margin;
    last_tab(&viewer.ui.tab_fit.buttons, ui.max_rect() + margin)
}

fn hud_button(viewer: &mut TabViewer, ui: &egui::Ui, after: Option<egui::Rect>) {
    let Some(last) = after.or_else(|| last_tab_of(viewer, ui)) else {
        return;
    };
    let key = viewer
        .ui
        .settings
        .hotkeys
        .key_name(HotkeyAction::ToggleHuds);
    let ds = viewer.emulator.kind() == citrine_gb::console::ConsoleKind::Nds;
    let settings = &viewer.ui.settings.hud;
    let (picked, _) = hud::chip::button(
        ui,
        ui.id().with("hud_tab"),
        last,
        hud::chip::label(),
        "HUDs over the game",
        hud::chip::MENU_WIDTH,
        |ui| hud::chip::list(ui, settings, key.as_deref(), ds),
    );
    let Some(pick) = picked else {
        return;
    };
    if pick == hud::chip::Pick::More {
        viewer.events.screen_settings(false);
        return;
    }
    hud::chip::apply(pick, &mut viewer.ui.settings.hud, &mut viewer.ui.hud);
    viewer.ui.settings.dirty = true;
}

fn screen_buttons(viewer: &mut TabViewer, ui: &egui::Ui) -> Option<egui::Rect> {
    if !viewer.emulator.two_screens() {
        return None;
    }
    let last = last_tab_of(viewer, ui)?;
    let tabs = viewer.emulator.screen_tabs();
    let hotkeys = &viewer.ui.settings.hotkeys;
    let facts = screens_menu::Facts {
        placement: viewer
            .ui
            .settings
            .ds_screens
            .placement(viewer.emulator.screen_context),
        separate_tabs: Some(tabs),
        keys: screens_menu::Keys {
            swap: hotkeys.key_name(HotkeyAction::SwapScreens),
            one: hotkeys.key_name(HotkeyAction::OneScreen),
        },
    };
    let id = ui.id().with("screens_tab");
    let away = viewer.emulator.lent_screen();
    let (picked, rect) = screens_button(ui, id, last, &facts, tabs, away);
    match picked {
        Some(screens_menu::Picked::Act(action)) => viewer.events.screens(action),
        Some(screens_menu::Picked::More) => viewer.events.screen_settings(false),
        None => {}
    }
    Some(rect)
}

fn last_tab(buttons: &[egui::Rect], body: egui::Rect) -> Option<egui::Rect> {
    buttons
        .iter()
        .filter(|tab| {
            (tab.bottom() - body.top()).abs() <= 6.0
                && (body.left()..=body.right()).contains(&tab.center().x)
        })
        .max_by(|a, b| a.right().total_cmp(&b.right()))
        .copied()
}

fn screens_button(
    ui: &egui::Ui,
    id: egui::Id,
    last: egui::Rect,
    facts: &screens_menu::Facts,
    tabs: bool,
    away: Option<usize>,
) -> (Option<screens_menu::Picked>, egui::Rect) {
    hud::chip::button(
        ui,
        id,
        last,
        screens_menu::chip_label(facts.placement.with_away(away), tabs),
        "Screens",
        screens_menu::menu_width(),
        |ui| {
            let picked = screens_menu::quick_list(ui, facts);
            if picked == Some(screens_menu::Picked::More) {
                ui.close();
            }
            picked
        },
    )
}

fn room_go(viewer: &mut TabViewer, go: playing::RoomGo) {
    match go {
        playing::RoomGo::Chat => viewer.events.open_tab(Tab::RoomChat),
        playing::RoomGo::Add => viewer.events.open_tab(Tab::Friends),
        playing::RoomGo::Message(who) => {
            viewer.ui.friends.chat.open(who);
            viewer.events.open_tab(Tab::Friends);
        }
        playing::RoomGo::Profile(who) => {
            viewer.ui.friends.show_profile(who);
            viewer.events.open_tab(Tab::Friends);
        }
    }
}

fn popups(viewer: &mut TabViewer, ctx: &egui::Context, panel: egui::Rect) {
    let picture = viewer.ui.toast_picture;
    let mut taken = None;
    if let Some(invite) = playing::pending_ask(viewer.play) {
        let (answer, rect) =
            toasts::banner(ctx, egui::Id::new("ask_banner"), panel, picture, |ui| {
                playing::ask_banner(ui, viewer.play, viewer.online, &invite)
            });
        taken = Some(rect);
        match answer {
            Some(Answer::Join) => {
                let joined = playing::join(
                    viewer.play,
                    viewer.online,
                    viewer.emulator,
                    &mut viewer.ui.playing,
                    &viewer.ui.recent,
                    &invite,
                );
                if !joined {
                    viewer.events.notify_error("Couldn't answer the invite");
                }
            }
            Some(Answer::Pick) => viewer.ui.playing.picking = Some(PickFor::Join(invite)),
            Some(Answer::Decline) => viewer.play.decline(viewer.online, invite.session),
            None => {}
        }
    }
    if let Some(ended) = viewer.ui.playing.ended.clone() {
        let again =
            playing::can_play_again(viewer.play, viewer.online, viewer.emulator, ended.friend);
        let id = egui::Id::new("ended_banner");
        let panel = taken.map_or(panel, |rect| toasts::without(panel, rect));
        let (action, rect) = toasts::banner(ctx, id, panel, picture, |ui| {
            playing::banner(ui, &ended, again)
        });
        taken = Some(taken.map_or(rect, |taken| taken.union(rect)));
        match action {
            Some(BannerAction::PlayAgain) => {
                let invited = playing::invite(
                    viewer.play,
                    viewer.online,
                    viewer.emulator,
                    &mut viewer.ui.playing,
                    &viewer.ui.recent,
                    ended.friend,
                );
                if !invited {
                    viewer.events.notify_error("Couldn't send the invite");
                }
            }
            Some(BannerAction::Resume) => {
                viewer.emulator.running = true;
                viewer.ui.playing.ended = None;
            }
            Some(BannerAction::Dismiss) => viewer.ui.playing.ended = None,
            None => {}
        }
    }
    viewer.ui.toast_banner = taken;
}

fn banner_frame<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, add)
        .inner
}

#[cfg(test)]
mod tests {
    use super::last_tab;
    use egui::{Rect, pos2};

    #[test]
    fn the_screens_control_follows_the_last_tab_of_its_own_pane() {
        let body = Rect::from_min_max(pos2(0.0, 30.0), pos2(400.0, 300.0));
        let game = Rect::from_min_max(pos2(0.0, 6.0), pos2(120.0, 30.0));
        let stacked = Rect::from_min_max(pos2(120.0, 6.0), pos2(200.0, 30.0));
        let beside = Rect::from_min_max(pos2(404.0, 6.0), pos2(500.0, 30.0));
        let below = Rect::from_min_max(pos2(0.0, 306.0), pos2(90.0, 330.0));
        assert_eq!(
            last_tab(&[game, beside, stacked, below], body),
            Some(stacked)
        );
        assert_eq!(last_tab(&[beside, below], body), None);
    }
}
