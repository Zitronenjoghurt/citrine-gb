use crate::app::events::AppEvent;
use crate::app::hud;
use crate::app::layout_edit::{self, ResetButton};
use crate::app::layout_sheet::{self, SheetGo, SheetPick};
use crate::app::screen_extras;
use crate::app::tabs::linked;
use crate::app::tabs::playing::{self, Answer, BannerAction, PickFor};
use crate::app::tabs::{Tab, TabViewer};
use crate::app::toasts;
use crate::app::ui_state::ds_screens::ScreensAction;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::layout_picks::Place;
use crate::app::ui_state::screen_layouts::{Item, LayoutConsole, LayoutView};
use crate::app::ui_state::settings_tree::GroupId;
use crate::app::widgets::clip_panel::{self, ClipFacts};
use crate::app::widgets::layout_editor::{EditItem, PadEdit};
use crate::app::widgets::record_bar::{RecordBarFacts, record_bar};
use crate::app::widgets::replay_bar::{ReplayBarFacts, replay_bar};
use crate::app::widgets::replay_graph;
use crate::app::widgets::{action, screens_menu, tilt};
use crate::emulator::layout::ScreenSpots;
use crate::local_link::Game;
use egui::{Rect, RichText, pos2};

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let editing = viewer.ui.layout_edit.is_some();
    let row = last_tab_of(viewer, ui).filter(|_| editing);
    if let Some(last) = row {
        edit_row(viewer, ui, last);
    } else {
        let after = screen_buttons(viewer, ui);
        hud_button(viewer, ui, after);
    }
    bars(viewer, ui);
    let mut panel = ui.available_rect_before_wrap();
    if let Some(facts) = clip_facts(viewer)
        && !editing
    {
        panel = clip_bar(viewer, ui, panel, &facts);
        viewer.ui.toast_area = Some(panel);
        ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
            game(viewer, ui);
        });
    } else if editing && row.is_none() {
        panel = edit_bar(viewer, ui, panel);
        viewer.ui.toast_area = Some(panel);
        ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
            game(viewer, ui);
        });
    } else {
        viewer.ui.toast_area = Some(panel.with_max_y(pane_bottom(ui)));
        game(viewer, ui);
    }
    popups(viewer, ui.ctx(), panel);
}

fn edit_row(viewer: &mut TabViewer, ui: &egui::Ui, last: Rect) {
    let size = egui::vec2((pane_right(ui) - last.right()).max(0.0), last.height());
    let pressed = egui::Area::new(ui.id().with("layout_edit_row"))
        .order(egui::Order::Middle)
        .constrain(false)
        .fixed_pos(last.right_top())
        .show(ui.ctx(), |ui| {
            let layout = egui::Layout::right_to_left(egui::Align::Center);
            ui.allocate_ui_with_layout(size, layout, |ui| {
                ui.set_min_size(size);
                ui.add_space(6.0);
                edit_buttons(viewer, ui)
            })
            .inner
        })
        .inner;
    apply_edit(viewer, pressed);
}

fn edit_bar(viewer: &mut TabViewer, ui: &mut egui::Ui, panel: Rect) -> Rect {
    let height = action::action_height(ui) + 12.0;
    let bar = Rect::from_min_max(pos2(panel.left(), panel.bottom() - height), panel.max);
    let layout = egui::Layout::right_to_left(egui::Align::Center);
    let pressed = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(egui::vec2(10.0, 6.0)))
                .layout(layout),
            |ui| edit_buttons(viewer, ui),
        )
        .inner;
    ui.painter().hline(
        bar.x_range(),
        bar.top(),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
    apply_edit(viewer, pressed);
    panel.with_max_y(bar.top())
}

fn edit_buttons(viewer: &TabViewer, ui: &mut egui::Ui) -> (Option<bool>, bool) {
    let reset = viewer.ui.layout_reset_button();
    let label = format!("{} {}", crate::icons::CHECK, viewer.ui.layout_done_label());
    let done = ui.add(action::main_button(ui, label)).clicked();
    let cancel = ui.add(egui::Button::new(layout_edit::CANCEL)).clicked();
    let finished = if cancel {
        Some(false)
    } else {
        done.then_some(true)
    };
    let label = format!("{} {}", reset.icon(), reset.label());
    let pressed = ui
        .add_enabled(reset != ResetButton::Nothing, egui::Button::new(label))
        .clicked();
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("Edit layout").strong());
    });
    (finished, pressed)
}

fn apply_edit(viewer: &mut TabViewer, (finished, pressed): (Option<bool>, bool)) {
    if pressed {
        viewer.ui.press_layout_reset();
    }
    match finished {
        Some(true) => layout_edit::done(viewer.ui, viewer.emulator),
        Some(false) => layout_edit::cancel(viewer.ui, viewer.emulator),
        None => {}
    }
}

fn clip_facts(viewer: &TabViewer) -> Option<ClipFacts> {
    viewer.ui.clip.as_ref()?.facts(viewer.emulator, false)
}

const CLIP_WIDTH: f32 = 640.0;

fn clip_bar(viewer: &mut TabViewer, ui: &mut egui::Ui, panel: Rect, facts: &ClipFacts) -> Rect {
    let margin = 12.0;
    let height = clip_panel::panel_height(ui, false) + margin * 2.0;
    let bar = Rect::from_min_max(pos2(panel.left(), panel.bottom() - height), panel.max);
    let width = (bar.width() - margin * 2.0).min(CLIP_WIDTH);
    let inner = Rect::from_center_size(bar.center(), egui::vec2(width, height - margin * 2.0));
    let pictures = viewer
        .ui
        .clip
        .as_ref()
        .map(|view| view.pictures.clone())
        .unwrap_or_default();
    let action = ui
        .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            clip_panel::clip_panel(ui, facts, &pictures)
        })
        .inner;
    ui.painter().hline(
        bar.x_range(),
        bar.top(),
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
    if let Some(action) = action {
        viewer.events.push(AppEvent::Clip(action));
    }
    panel.with_max_y(bar.top())
}

fn record_row(viewer: &mut TabViewer, ui: &mut egui::Ui, alone: bool) {
    let overlay_id = egui::Id::new("record_bar_overlay");
    let replay = ReplayBarFacts::of(viewer.emulator, false).filter(|_| viewer.ui.clip.is_none());
    let record = RecordBarFacts::of(viewer.emulator, viewer.ui, false);
    let times = viewer.emulator.tape_times();
    if replay.is_none() && record.is_none() {
        ui.data_mut(|d| d.remove::<bool>(overlay_id));
        return;
    }
    let show = |ui: &mut egui::Ui| {
        banner_frame(ui, |ui| match (&replay, &record) {
            (Some(facts), _) => replay_bar(ui, facts, times).map(AppEvent::Replay),
            (None, Some(facts)) => record_bar(ui, facts).map(AppEvent::Record),
            (None, None) => None,
        })
    };
    let graph = if times.is_some() {
        replay_graph::widget_height(ui, false) + ui.spacing().item_spacing.y
    } else {
        0.0
    };
    let height = ui.spacing().interact_size.y + 8.0 + graph;
    let panel = ui.available_rect_before_wrap();
    let free = ui
        .data(|d| d.get_temp::<f32>(game_free_id()))
        .unwrap_or(0.0);
    let kept = ui.data(|d| d.get_temp::<bool>(overlay_id)).unwrap_or(true);
    let overlay = kept && alone && free >= height;
    ui.data_mut(|d| d.insert_temp(overlay_id, overlay));
    let event = if overlay {
        let strip = Rect::from_min_size(panel.min, egui::vec2(panel.width(), height));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip));
        show(&mut child)
    } else {
        show(ui)
    };
    if let Some(event) = event {
        viewer.events.push(event);
    }
}

fn game_free_id() -> egui::Id {
    egui::Id::new("game_free_above")
}

fn bars(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let alone = viewer.ui.layout_edit.is_none() && viewer.emulator.link().is_none();
    let drawn = session_bars(viewer, ui);
    record_row(viewer, ui, alone && !drawn);
}

fn session_bars(viewer: &mut TabViewer, ui: &mut egui::Ui) -> bool {
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
        true
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
        true
    } else {
        false
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
    let only = viewer.emulator.screen_tabs().then_some(0);
    let room = tilt::screen_area(area, tilting);
    let movable = viewer.emulator.two_screens() && !viewer.emulator.screen_tabs();
    let console = LayoutConsole::of(viewer.emulator.played_kind());
    let spots = viewer
        .ui
        .settings
        .layouts
        .spots(console, LayoutView::Desktop);
    if movable {
        viewer.emulator.screen_spots = Some(ScreenSpots {
            room,
            screen: spots.get(&Item::Screen).copied(),
            touch: spots.get(&Item::TouchScreen).copied(),
        });
    }
    let screen = ui
        .scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
            ui.vertical_centered(|ui| viewer.emulator.ui_screen(ui, only))
                .inner
        })
        .inner;
    if let Some(rect) = screen {
        let free = (rect.top() - area.top()).min(area.bottom() - rect.bottom());
        ui.data_mut(|d| d.insert_temp(game_free_id(), free));
    }
    viewer.emulator.screen_spots = None;
    let screen = screen.map(|screen| {
        if movable && !spots.is_empty() {
            let placed = viewer.emulator.placed_screens().iter().copied();
            placed.reduce(Rect::union).unwrap_or(screen)
        } else {
            screen
        }
    });
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
            editing: viewer.ui.layout_edit.is_some(),
            keep_below: None,
        };
        let boards = &viewer.achievements.boards;
        let trackers = if place.editing {
            boards.layout_lines()
        } else {
            boards.tracker_lines()
        };
        let outcome = hud::show(
            ui,
            &place,
            viewer.ui,
            viewer.emulator,
            viewer.stats,
            viewer.play,
            viewer.online,
            &trackers,
        );
        if let Some(id) = outcome.remove {
            viewer.events.remove_hud(id);
        }
    }
    let marks = match screen {
        Some(screen) if playing => {
            let touch = viewer.emulator.placed_touch();
            let main = viewer
                .emulator
                .placed_screens()
                .iter()
                .copied()
                .find(|rect| Some(*rect) != touch)
                .unwrap_or(screen);
            let taken = viewer.emulator.placed_screens();
            screen_extras::place_marks(
                ui,
                viewer.ui,
                console,
                LayoutView::Desktop,
                room,
                main,
                taken,
            )
        }
        _ => Vec::new(),
    };
    if viewer.ui.layout_edit.is_some() {
        let touch = viewer.emulator.placed_touch();
        let screens: &[egui::Rect] = if movable {
            viewer.emulator.placed_screens()
        } else {
            &[]
        };
        let mut items: Vec<EditItem> = screens
            .iter()
            .map(|rect| {
                let item = if Some(*rect) == touch {
                    Item::TouchScreen
                } else {
                    Item::Screen
                };
                EditItem::new(item, *rect, rect.size())
            })
            .collect();
        items.extend(marks);
        items.extend(viewer.ui.hud_items());
        viewer
            .ui
            .edit_layout(ui, room, area, &items, PadEdit::default());
    }
}

pub fn bottom_screen(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let area = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
        ui.vertical_centered(|ui| viewer.emulator.ui_screen(ui, Some(1)));
    });
}

fn last_tab_of(viewer: &TabViewer, ui: &egui::Ui) -> Option<egui::Rect> {
    last_tab(&viewer.ui.tab_fit.buttons, pane(ui))
}

fn pane(ui: &egui::Ui) -> Rect {
    let margin = egui_dock::Style::from_egui(ui.style())
        .tab
        .tab_body
        .inner_margin;
    ui.max_rect() + margin
}

fn pane_bottom(ui: &egui::Ui) -> f32 {
    let stroke = egui_dock::Style::from_egui(ui.style())
        .tab
        .tab_body
        .stroke
        .width;
    pane(ui).bottom() - (stroke / 2.0).ceil() - stroke
}

fn pane_right(ui: &egui::Ui) -> f32 {
    pane(ui).right()
}

fn hud_button(viewer: &mut TabViewer, ui: &egui::Ui, after: Option<egui::Rect>) {
    let Some(last) = after.or_else(|| last_tab_of(viewer, ui)) else {
        return;
    };
    let Some(label) = hud::chip::label_for(ui, pane_right(ui) - last.right()) else {
        return;
    };
    let key = viewer
        .ui
        .settings
        .hotkeys
        .key_name(HotkeyAction::ToggleHuds);
    let ds = viewer.emulator.played_kind() == citrine_gb::console::ConsoleKind::Nds;
    let settings = &viewer.ui.settings.hud;
    let (picked, _) = hud::chip::button(
        ui,
        ui.id().with("hud_tab"),
        last,
        label,
        "HUDs over the game",
        hud::chip::MENU_WIDTH,
        |ui| hud::chip::list(ui, settings, key.as_deref(), ds),
    );
    let Some(pick) = picked else {
        return;
    };
    if pick == hud::chip::Pick::More {
        viewer.events.find_group(GroupId::Huds);
        return;
    }
    hud::chip::apply(pick, &mut viewer.ui.settings.hud, &mut viewer.ui.hud);
    viewer.ui.settings.dirty = true;
}

fn screen_buttons(viewer: &mut TabViewer, ui: &egui::Ui) -> Option<egui::Rect> {
    if !viewer.emulator.has_game() || viewer.emulator.linked() {
        return None;
    }
    let last = last_tab_of(viewer, ui)?;
    let here = viewer.ui.layout_applied_place().unwrap_or(Place::Window);
    let device = layout_sheet::device(viewer.ui, viewer.emulator, here, false, true);
    let label = viewer.ui.layout_chip_label(
        device.console,
        here,
        viewer.emulator.screen_tabs(),
        viewer.emulator.lent_screen(),
    );
    let id = ui.id().with("screens_tab");
    let (picked, rect) = hud::chip::tab_button(
        ui,
        id,
        last,
        label,
        screens_menu::TITLE,
        viewer.ui.layout_sheet.is_some(),
        |response| layout_popover(viewer, response, id.with("menu"), &device),
    );
    if let Some(pick) = picked.flatten()
        && let Some(go) = viewer.ui.pick_in_sheet(&device, pick)
    {
        match go {
            SheetGo::Edit(start) => viewer.events.edit_layout(start),
            SheetGo::Lid => viewer.events.toggle_lid(),
            SheetGo::SeparateTabs => viewer.events.screens(ScreensAction::SeparateTabs),
            SheetGo::More => viewer.events.screen_settings(false),
            SheetGo::Turn(..) => {}
        }
    }
    Some(rect)
}

fn layout_popover(
    viewer: &mut TabViewer,
    chip: &egui::Response,
    id: egui::Id,
    device: &layout_sheet::Device,
) -> Option<Option<SheetPick>> {
    if chip.clicked() {
        viewer.ui.layout_sheet = match viewer.ui.layout_sheet {
            Some(_) => None,
            None => Some(Default::default()),
        };
    }
    let sheet = viewer.ui.layout_sheet.as_ref()?;
    let layer = sheet.layer_open();
    let menu = sheet.menu;
    let facts = viewer.ui.layout_facts(device);
    let close = if layer {
        egui::PopupCloseBehavior::IgnoreClicks
    } else {
        egui::PopupCloseBehavior::CloseOnClickOutside
    };
    let mut open = true;
    let picked = egui::Popup::menu(chip)
        .id(id)
        .open_bool(&mut open)
        .close_behavior(close)
        .frame(screens_menu::popover_frame(&chip.ctx.style()))
        .width(screens_menu::popover_width(facts.cards.len()))
        .show(|ui| screens_menu::popover(ui, &facts, menu))
        .and_then(|shown| shown.inner);
    if !open && let Some(sheet) = &mut viewer.ui.layout_sheet {
        let escape = chip.ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if layer {
            sheet.close_layer();
        } else if escape && sheet.other.is_some() {
            sheet.other = None;
        } else {
            viewer.ui.layout_sheet = None;
        }
    }
    Some(picked)
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
    use crate::app::Citrine;
    use crate::app::hud;
    use crate::app::tabs::Tab;
    use crate::app::tests::{
        assert_in_front, load_game, phone_app, shown_whole, tap, text_containing, text_rect,
    };
    use crate::app::ui_state::settings::LayoutMode;
    use crate::app::ui_state::settings_tree::SettingsTab;
    use egui::{Context, Rect, pos2};

    fn frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                pos2(0.0, 0.0),
                egui::vec2(1280.0, 360.0),
            )),
            events,
            max_texture_side: Some(8192),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            app.render(ctx);
            app.handle_event_queue(false);
        })
    }

    #[test]
    fn more_hud_settings_opens_the_screen_page_at_its_huds() {
        let mut app = phone_app("hud-more-desktop");
        app.ui.settings.layout = LayoutMode::Desktop;
        load_game(
            &mut app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        app.dock = egui_dock::DockState::new(vec![Tab::GameBoy]);
        app.sync_games_tab();
        app.ui.settings.current_tab = SettingsTab::Ds;
        let ctx = Context::default();
        let mut output = frame(&mut app, &ctx, Vec::new());
        for _ in 0..3 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        let chip = text_rect(&output, &hud::chip::label()).expect("the HUD chip");
        frame(&mut app, &ctx, tap(chip.center()));
        output = frame(&mut app, &ctx, Vec::new());
        let more = text_containing(&output, "More HUD settings").expect("the menu entry");
        frame(&mut app, &ctx, tap(more.center()));
        for _ in 0..30 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        assert_eq!(app.ui.settings.current_tab, SettingsTab::Screen);
        assert_in_front(&app, Tab::Settings);
        assert!(
            shown_whole(&output, "HUDs"),
            "the HUDs heading is out of view"
        );
    }

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
