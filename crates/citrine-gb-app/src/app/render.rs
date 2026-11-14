use super::toasts::Toast;
use super::toolbar::{BRAND, CORE_GIVES_WAY};
use super::{Citrine, DOCK_PANE_MIN, FRIENDS_DOCK_SHARE, FRIENDS_DOCK_WIDTH, FRIENDS_STACK_SHARE};
use super::{tabs, viewer};
use crate::app::tabs::Tab;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::keybinds::PadButton;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::settings::console_name;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::settings::kind_short;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::settings::other_console;
use crate::app::widgets::shortcut::{shortcut_button, shortcut_hint};
use crate::icons;
use crate::utils::file_loader::FileLoader;
use crate::utils::save_files::ROM_FILTER;
#[cfg(not(target_arch = "wasm32"))]
use citrine_gb::rom::header::RomCgbMode;
use egui::{CentralPanel, Color32, Context, TopBottomPanel};
use egui_dock::{DockState, NodeIndex, SurfaceIndex, TabIndex};
use strum::IntoEnumIterator;

impl Citrine {
    pub fn is_mobile(&self, ctx: &Context) -> bool {
        self.ui.settings.layout.is_touch(ctx) || self.big_screen_on()
    }

    pub(super) fn render(&mut self, ctx: &Context) {
        let mobile = self.is_mobile(ctx);
        self.sync_focus_popups(!mobile);
        if mobile {
            self.render_mobile(ctx);
            return;
        }
        crate::app::mobile::sheet::set_area(ctx, None);
        if std::mem::take(&mut self.mobile.phone) {
            self.release_phone_pause();
        }
        if self.ui.focus_mode {
            self.render_focus_mode(ctx);
        } else {
            self.render_normal_mode(ctx);
        }
    }

    pub(super) fn render_normal_mode(&mut self, ctx: &Context) {
        TopBottomPanel::top("top_panel").show(ctx, |ui| self.top_panel(ui));

        self.sync_screen_tabs();
        self.sync_games_tab();
        let screens = self.screens_chip();
        self.ui.tab_fit = tabs::fit_titles(
            ctx,
            &self.dock,
            tabs::game_title(&self.emulator),
            screens.as_deref(),
            &self.ui.ds_files,
        );
        CentralPanel::default().show(ctx, |ui| {
            self.toasts.place_page(ui.max_rect());
            let mut viewer = tabs::TabViewer {
                achievements: &mut self.achievements,
                audio: &mut self.audio,
                covers: &mut self.big.covers,
                commonmark: &mut self.commonmark,
                emulator: &mut self.emulator,
                game_info: &mut self.game_info,
                game_facts: &self.game_facts,
                pads: self.pads.as_ref(),
                events: &mut self.events,
                files: &mut self.files,
                media: &mut self.media,
                online: &mut self.online,
                play: &mut self.play,
                senses: &self.senses,
                stats: &self.stats,
                threads: &self.threads,
                ui: &mut self.ui,
            };

            let mut style = egui_dock::Style::from_egui(ctx.style().as_ref());
            style.separator.extra = DOCK_PANE_MIN;
            egui_dock::DockArea::new(&mut self.dock)
                .style(style)
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_inside(ui, &mut viewer);
        });
    }

    pub(super) fn media_viewer(&mut self, ctx: &Context) {
        let mut viewer = tabs::TabViewer {
            achievements: &mut self.achievements,
            audio: &mut self.audio,
            covers: &mut self.big.covers,
            commonmark: &mut self.commonmark,
            emulator: &mut self.emulator,
            game_info: &mut self.game_info,
            game_facts: &self.game_facts,
            pads: self.pads.as_ref(),
            events: &mut self.events,
            files: &mut self.files,
            media: &mut self.media,
            online: &mut self.online,
            play: &mut self.play,
            senses: &self.senses,
            stats: &self.stats,
            threads: &self.threads,
            ui: &mut self.ui,
        };
        viewer::show(&mut viewer, ctx);
    }

    pub(super) fn show_toasts(&mut self, ctx: &Context) {
        if let Some(area) = self.ui.toast_area.take() {
            self.toasts.place(area);
        }
        if let Some(picture) = self.ui.toast_picture.take() {
            self.toasts.place_picture(picture);
        }
        if let Some(banner) = self.ui.toast_banner.take() {
            self.toasts.place_banner(banner);
        }
        if let Some(spare) = self.ui.toast_spare.take() {
            self.toasts.place_spare(spare);
        }
        for event in self.toasts.show(ctx) {
            self.events.push(event);
        }
    }

    pub(super) fn top_panel(&mut self, ui: &mut egui::Ui) {
        let row = ui.horizontal(|ui| {
            let room = ui.available_width();
            if self.brand_fits(ui) {
                ui.label(BRAND);
                ui.separator();
            }
            let core = ui.cursor().left();

            ui.menu_button(icons::FOLDER, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.set_min_width(180.0);

                if ui.button("Load ROM").clicked() {
                    FileLoader::new()
                        .title("Load ROM")
                        .add_filter(ROM_FILTER.0, ROM_FILTER.1)
                        .dispatch(&self.files.spawner, self.files.rom_tx.clone());
                }
                if ui.button("Load boot ROM").clicked() {
                    FileLoader::new()
                        .title("Load boot ROM")
                        .add_filter("Game Boy boot ROM", &["bin"])
                        .dispatch(&self.files.spawner, self.files.boot_rom_tx.clone());
                }
                #[cfg(feature = "gba")]
                if ui.button("Load GBA BIOS").clicked() {
                    self.pick_gba_bios();
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.separator();
                    self.ui.recent.prune();
                    self.ui.recent.name_from_files();
                    if self.ui.recent.is_empty() {
                        ui.add_enabled(false, egui::Button::new("No recent ROMs"));
                    } else {
                        ui.label("Recent");
                        let mut chosen = None;
                        for entry in self.ui.recent.entries() {
                            let name = entry.display_name();
                            let clicked = match entry.rom_key.as_deref() {
                                Some(key) => {
                                    let model = other_console(
                                        self.ui.settings.console_for(key, RomCgbMode::CgbAndGb),
                                    );
                                    let arrow = egui::containers::menu::SubMenuButton::RIGHT_ARROW;
                                    let right = match entry.console {
                                        Some(kind) => format!("{}  {arrow}", kind_short(kind)),
                                        None => arrow.to_owned(),
                                    };
                                    let button = egui::Button::new(name).right_text(right);
                                    egui::containers::menu::SubMenuButton::from_button(button)
                                        .ui(ui, |ui| {
                                            if ui
                                                .button(format!("Play on {}", console_name(model)))
                                                .clicked()
                                            {
                                                chosen = Some((
                                                    entry.path.clone(),
                                                    Some((key.to_owned(), model)),
                                                ));
                                            }
                                        })
                                        .0
                                        .clicked()
                                }
                                None => ui
                                    .add(egui::Button::new(name).right_text(
                                        entry.console.map(kind_short).unwrap_or_default(),
                                    ))
                                    .on_hover_text(entry.path.display().to_string())
                                    .clicked(),
                            };
                            if clicked {
                                chosen = Some((entry.path.clone(), None));
                            }
                        }
                        if let Some((path, console)) = chosen {
                            if let Some((key, model)) = console {
                                self.ui.settings.set_game_console(&key, model);
                            }
                            self.load_recent(path);
                        }
                    }
                }
            })
            .response
            .on_hover_text("Files");

            ui.separator();
            self.transport_buttons(ui);
            ui.separator();
            self.ui.toolbar_core = ui.cursor().left() - core;
            self.place_core(ui, room);

            self.toolbar_tools(ui);
        });
        self.ui.toolbar_reach = row.response.rect.right();
    }

    pub(super) fn transport_buttons(&mut self, ui: &mut egui::Ui) {
        let loaded = self.emulator.has_game();
        let running = self.emulator.is_running();
        let muted = self.ui.settings.muted;
        let hotkeys = &self.ui.settings.hotkeys;
        let mut pressed = None;
        if shortcut_button(ui, hotkeys, HotkeyAction::TogglePause, running, loaded).clicked() {
            pressed = Some(HotkeyAction::TogglePause);
        }
        if self.emulator.rewind_length.seconds().is_some() {
            self.rewind_pressed |= self.rewind_button(ui);
        }
        self.fast_forward_button(ui);
        let hotkeys = &self.ui.settings.hotkeys;
        let folded = &CORE_GIVES_WAY[..self.ui.core_in_more];
        for action in [HotkeyAction::SoftReset, HotkeyAction::Screenshot] {
            if folded.contains(&action) {
                continue;
            }
            if shortcut_button(ui, hotkeys, action, false, loaded).clicked() {
                pressed = Some(action);
            }
        }
        if let Some(action) = pressed {
            self.run_hotkey(action);
        }

        if self.recording_button(ui, None).clicked() {
            self.run_hotkey(HotkeyAction::ToggleRecording);
        }

        let mute = HotkeyAction::ToggleMute;
        if shortcut_button(ui, &self.ui.settings.hotkeys, mute, muted, true).clicked() {
            self.run_hotkey(mute);
        }

        if let Some(closed) = self.emulator.lid() {
            let lid = HotkeyAction::ToggleLid;
            if shortcut_button(ui, &self.ui.settings.hotkeys, lid, closed, true).clicked() {
                self.run_hotkey(lid);
            }
        }
    }

    fn rewind_button(&self, ui: &mut egui::Ui) -> bool {
        let action = HotkeyAction::Rewind;
        let button = egui::Button::new(action.icon(false)).selected(self.emulator.is_rewinding());
        let response = ui
            .add_enabled(self.emulator.can_rewind(), button)
            .on_hover_text(shortcut_hint(&self.ui.settings.hotkeys, action, false))
            .on_disabled_hover_text(self.rewind_off_reason());
        response.is_pointer_button_down_on()
    }

    pub(super) fn rewind_off_reason(&self) -> &'static str {
        if self.emulator.together {
            "Rewind is off while playing together"
        } else if self.emulator.has_game() {
            "Rewind is off while a link is on"
        } else {
            "Open a game to rewind it"
        }
    }

    pub(super) fn toggle_focus_mode(&mut self) {
        self.ui.focus_mode = !self.ui.focus_mode;
        if self.ui.focus_mode {
            let names = self.focus_exit_names();
            let keys = if names.is_empty() {
                "Esc".to_owned()
            } else {
                names.join(" or ")
            };
            self.toasts.add(Toast::hint(
                format!("Press {keys} to leave focus mode"),
                crate::utils::reduced_motion(),
            ));
        } else {
            self.toasts.dismiss_all_toasts();
        }
    }

    pub(super) fn focus_exit_names(&self) -> Vec<String> {
        let hotkeys = &self.ui.settings.hotkeys;
        let toggle = HotkeyAction::ToggleFocusMode;
        let mut names: Vec<String> = hotkeys
            .keys_for(HotkeyAction::LeaveFocusMode)
            .iter()
            .chain(&hotkeys.keys_for(toggle))
            .map(ToString::to_string)
            .collect();
        names.extend(hotkeys.pads_for(toggle).iter().map(ToString::to_string));
        names.dedup();
        names
    }

    pub(super) fn run_pad_shortcuts(&mut self, presses: Vec<PadButton>) {
        for pad in presses {
            if let Some(action) = self.ui.settings.hotkeys.action_for_pad(pad)
                && !action.is_hold()
            {
                self.run_hotkey(action);
            }
        }
    }

    pub(super) fn render_focus_mode(&mut self, ctx: &Context) {
        CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                let area = ui.max_rect();
                self.ui.toast_area = Some(area);
                self.placed_game(ui, area, false);
            });
        if let Some(area) = self.ui.toast_area {
            self.show_focus_popups(ctx, area);
        }
    }

    pub(super) fn tools_node(&self) -> Option<(SurfaceIndex, NodeIndex)> {
        tools_node(&self.dock)
    }

    pub(super) fn media_would_cover_game(&self) -> bool {
        let Some((gb_surface, gb_node, _)) = self.dock.find_tab(&Tab::GameBoy) else {
            return false;
        };
        let media = match self.dock.find_tab(&Tab::Media) {
            Some((surface_idx, node_idx, _)) => Some((surface_idx, node_idx)),
            None => self.tools_node(),
        };
        media == Some((gb_surface, gb_node))
    }

    pub(super) fn sync_games_tab(&mut self) {
        let away = tabs::games::idle(&self.emulator, &self.play) || self.ui.games.hidden;
        match (away, self.dock.find_tab(&Tab::Games)) {
            (true, Some(location)) => {
                if let Some(share) = games_share(&self.dock, location) {
                    self.ui.games.share = Some(share);
                }
                self.dock.remove_tab(location);
            }
            (false, None) => {
                let tools = self
                    .tools_node()
                    .filter(|(surface, _)| *surface == SurfaceIndex::main());
                if let Some((surface, node)) = tools
                    && let egui_dock::Node::Leaf(leaf) = &mut self.dock[surface][node]
                {
                    let welcome = leaf.tabs.get(leaf.active.0) == Some(&Tab::Info);
                    leaf.tabs.insert(0, Tab::Games);
                    leaf.active = TabIndex(if welcome { 0 } else { leaf.active.0 + 1 });
                    return;
                }
                match self.dock.find_tab(&Tab::GameBoy) {
                    Some((surface, gb_node, _)) if surface == SurfaceIndex::main() => {
                        let share = self.ui.games.share.unwrap_or(GAMES_SHARE);
                        self.dock
                            .main_surface_mut()
                            .split_right(gb_node, share, vec![Tab::Games]);
                    }
                    _ => self.open_tab(Tab::Games),
                }
            }
            _ => {}
        }
    }

    pub(super) fn sync_friend_tab(&mut self, wanted: bool) {
        let found = self.dock.find_tab(&Tab::Playing);
        match (wanted, found) {
            (false, Some(location)) => {
                self.dock.remove_tab(location);
            }
            (true, None) => match self.dock.find_tab(&Tab::GameBoy) {
                Some((surface, gb_node, _)) if surface == SurfaceIndex::main() => {
                    self.dock
                        .main_surface_mut()
                        .split_right(gb_node, 0.5, vec![Tab::Playing]);
                    stack_friends_column(&mut self.dock);
                }
                _ => self.open_tab(Tab::Playing),
            },
            _ => {}
        }
    }

    pub(super) fn open_tab(&mut self, tab: Tab) {
        if let Some((surface_idx, node_idx, tab_idx)) = self.dock.find_tab(&tab) {
            self.dock.set_active_tab((surface_idx, node_idx, tab_idx));
            return;
        }

        if matches!(tab, Tab::DsFile(_)) && self.open_beside_files(tab) {
            return;
        }

        if tab == Tab::Friends && stack_friends(&mut self.dock) {
            return;
        }

        if tab == Tab::Friends
            && let Some((surface, _, _)) = self.dock.find_tab(&Tab::GameBoy)
            && surface == SurfaceIndex::main()
        {
            let main = self.dock.main_surface_mut();
            let share = main
                .root_node()
                .and_then(|node| node.rect())
                .filter(|rect| rect.width() > 1.0)
                .map_or(FRIENDS_DOCK_SHARE, |rect| {
                    (FRIENDS_DOCK_WIDTH / rect.width()).clamp(0.12, FRIENDS_DOCK_SHARE)
                });
            main.split_left(NodeIndex::root(), share, vec![tab]);
            return;
        }

        let tools_node = self.tools_node();
        let gb_loc = self.dock.find_tab(&Tab::GameBoy);
        if let Some((surface_idx, node_idx)) = tools_node {
            self.dock
                .set_focused_node_and_surface((surface_idx, node_idx));
            self.dock.main_surface_mut().push_to_focused_leaf(tab);
        } else if let Some((_gb_surface, gb_node, _)) = gb_loc {
            self.dock
                .main_surface_mut()
                .split_right(gb_node, 0.6, vec![tab]);
        } else {
            self.dock.main_surface_mut().push_to_focused_leaf(tab);
        }
    }

    fn open_beside_files(&mut self, tab: Tab) -> bool {
        if let Some((surface, node, _)) = self
            .dock
            .find_tab_from(|open| matches!(open, Tab::DsFile(_)))
        {
            self.dock.set_focused_node_and_surface((surface, node));
            self.dock[surface].push_to_focused_leaf(tab);
            return true;
        }
        let Some((surface, node, _)) = self.dock.find_tab(&Tab::DsFiles) else {
            return false;
        };
        self.dock[surface].split_right(node, FILES_SHARE, vec![tab]);
        let mut child = node;
        while let Some(parent) = child.parent() {
            if child.is_right()
                && let egui_dock::Node::Horizontal(split) = &mut self.dock[surface][parent]
            {
                split.fraction = split.fraction.min(FILES_GAME_SHARE);
                break;
            }
            child = parent;
        }
        true
    }
}

const FILES_SHARE: f32 = 0.4;
const GAMES_SHARE: f32 = 0.62;
const FILES_GAME_SHARE: f32 = 0.4;

fn games_share(
    dock: &DockState<Tab>,
    (surface, node, _): (SurfaceIndex, NodeIndex, TabIndex),
) -> Option<f32> {
    if surface != SurfaceIndex::main() || !node.is_right() || dock[surface][node].tabs_count() != 1
    {
        return None;
    }
    match &dock[surface][node.parent()?] {
        egui_dock::Node::Horizontal(split) => Some(split.fraction),
        _ => None,
    }
}

fn tools_node(dock: &DockState<Tab>) -> Option<(SurfaceIndex, NodeIndex)> {
    Tab::iter()
        .filter(|t| {
            !matches!(
                t,
                Tab::GameBoy | Tab::BottomScreen | Tab::Friends | Tab::Playing | Tab::DsFile(_)
            )
        })
        .find_map(|t| dock.find_tab(&t))
        .map(|(surface_idx, node_idx, _)| (surface_idx, node_idx))
}

fn stack_friends(dock: &mut DockState<Tab>) -> bool {
    let on_main = |tab: &Tab| {
        dock.find_tab(tab)
            .is_some_and(|(surface, _, _)| surface == SurfaceIndex::main())
    };
    if !on_main(&Tab::GameBoy) || !on_main(&Tab::Playing) {
        return false;
    }
    let Some((surface, node)) = tools_node(dock) else {
        return false;
    };
    if surface != SurfaceIndex::main() {
        return false;
    }
    dock.main_surface_mut()
        .split_above(node, FRIENDS_STACK_SHARE, vec![Tab::Friends]);
    true
}

fn stack_friends_column(dock: &mut DockState<Tab>) {
    let Some(location) = dock.find_tab(&Tab::Friends) else {
        return;
    };
    let (surface, node, _) = location;
    let alone = dock.main_surface()[node].tabs_count() == 1;
    let tools = tools_node(dock).is_some_and(|(surface, _)| surface == SurfaceIndex::main());
    if surface != SurfaceIndex::main() || node != NodeIndex::root().left() || !alone || !tools {
        return;
    }
    dock.remove_tab(location);
    stack_friends(dock);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room_dock() -> DockState<Tab> {
        let mut dock = DockState::new(vec![Tab::GameBoy]);
        let [game, _] = dock.main_surface_mut().split_right(
            NodeIndex::root(),
            0.6,
            vec![Tab::Info, Tab::RoomChat],
        );
        dock.main_surface_mut()
            .split_right(game, 0.5, vec![Tab::Playing]);
        dock
    }

    fn beside(dock: &DockState<Tab>, a: Tab, b: Tab) -> bool {
        let node = |tab| dock.find_tab(&tab).map(|(_, node, _)| node).unwrap();
        let (a, b) = (node(a), node(b));
        a.parent() == b.parent() && a.is_left() && b.is_right()
    }

    #[test]
    fn friends_stack_over_the_tools_while_a_friend_view_is_open() {
        let mut dock = room_dock();
        assert!(stack_friends(&mut dock));
        assert!(beside(&dock, Tab::Friends, Tab::Info));
        assert!(beside(&dock, Tab::GameBoy, Tab::Playing));
    }

    #[test]
    fn friends_keep_their_own_column_without_a_friend_view() {
        let mut dock = DockState::new(vec![Tab::GameBoy]);
        dock.main_surface_mut()
            .split_right(NodeIndex::root(), 0.6, vec![Tab::Info]);
        assert!(!stack_friends(&mut dock));
    }

    #[test]
    fn a_friends_column_moves_over_the_tools_when_a_friend_view_opens() {
        let mut dock = DockState::new(vec![Tab::GameBoy]);
        dock.main_surface_mut()
            .split_right(NodeIndex::root(), 0.6, vec![Tab::Info]);
        dock.main_surface_mut()
            .split_left(NodeIndex::root(), 0.2, vec![Tab::Friends]);
        let game = dock
            .find_tab(&Tab::GameBoy)
            .map(|(_, node, _)| node)
            .unwrap();
        dock.main_surface_mut()
            .split_right(game, 0.5, vec![Tab::Playing]);
        stack_friends_column(&mut dock);
        assert!(beside(&dock, Tab::Friends, Tab::Info));
        assert!(beside(&dock, Tab::GameBoy, Tab::Playing));
        assert_eq!(dock.iter_all_tabs().count(), 4);
    }

    fn games_app(name: &str) -> Citrine {
        let mut app = crate::app::tests::phone_app(name);
        app.ui.settings.layout = crate::app::ui_state::settings::LayoutMode::Desktop;
        app
    }

    fn load_crystal(app: &mut Citrine) {
        crate::app::tests::load_game(
            app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
    }

    #[test]
    fn with_no_game_the_games_pane_is_the_whole_window() {
        let mut app = games_app("games-idle");
        app.dock = DockState::new(vec![Tab::GameBoy, Tab::Games]);
        app.sync_games_tab();
        assert!(app.dock.find_tab(&Tab::Games).is_none());
        assert_eq!(app.dock.iter_all_tabs().count(), 1);
    }

    #[test]
    fn a_loaded_game_docks_the_games_pane_beside_it() {
        let mut app = games_app("games-docked");
        load_crystal(&mut app);
        app.dock = DockState::new(vec![Tab::GameBoy]);
        app.sync_games_tab();
        let (games, games_node, _) = app.dock.find_tab(&Tab::Games).unwrap();
        let (game, game_node, _) = app.dock.find_tab(&Tab::GameBoy).unwrap();
        assert_eq!(games, SurfaceIndex::main());
        assert_eq!(game, SurfaceIndex::main());
        assert_ne!(games_node, game_node);
        assert!(games_node.is_right());
        let parent = &app.dock[games][games_node.parent().unwrap()];
        let egui_dock::Node::Horizontal(split) = parent else {
            panic!("the games pane sits below the game");
        };
        assert_eq!(split.fraction, GAMES_SHARE);
    }

    #[test]
    fn the_games_pane_joins_an_open_tool_column_without_covering_it() {
        let mut app = games_app("games-tools");
        load_crystal(&mut app);
        app.open_tab(Tab::Cheats);
        let leaves = app.dock.iter_leaves().count();
        app.sync_games_tab();
        let (_, games_node, _) = app.dock.find_tab(&Tab::Games).unwrap();
        let (_, cheats_node, cheats) = app.dock.find_tab(&Tab::Cheats).unwrap();
        assert_eq!(games_node, cheats_node);
        assert_eq!(app.dock.iter_leaves().count(), leaves);
        let egui_dock::Node::Leaf(leaf) = &app.dock.main_surface()[cheats_node] else {
            panic!("the tool column is not a leaf");
        };
        assert_eq!(leaf.active, cheats);
    }

    #[test]
    fn the_games_pane_comes_in_front_of_the_welcome_tab() {
        let mut app = games_app("games-welcome");
        load_crystal(&mut app);
        app.open_tab(Tab::Info);
        app.sync_games_tab();
        let (_, games_node, games) = app.dock.find_tab(&Tab::Games).unwrap();
        let (_, info_node, _) = app.dock.find_tab(&Tab::Info).unwrap();
        assert_eq!(games_node, info_node);
        let egui_dock::Node::Leaf(leaf) = &app.dock.main_surface()[games_node] else {
            panic!("the tool column is not a leaf");
        };
        assert_eq!(leaf.active, games);
    }

    #[test]
    fn the_games_pane_comes_back_at_the_width_it_was_given() {
        let mut app = games_app("games-width");
        load_crystal(&mut app);
        app.dock = DockState::new(vec![Tab::GameBoy]);
        app.sync_games_tab();
        let (_, games_node, _) = app.dock.find_tab(&Tab::Games).unwrap();
        let parent = games_node.parent().unwrap();
        if let egui_dock::Node::Horizontal(split) = &mut app.dock.main_surface_mut()[parent] {
            split.fraction = 0.5;
        }
        app.ui.games.hidden = true;
        app.sync_games_tab();
        app.ui.games.hidden = false;
        app.sync_games_tab();
        let (_, games_node, _) = app.dock.find_tab(&Tab::Games).unwrap();
        let egui_dock::Node::Horizontal(split) =
            &app.dock.main_surface()[games_node.parent().unwrap()]
        else {
            panic!("the games pane sits below the game");
        };
        assert_eq!(split.fraction, 0.5);
    }

    #[test]
    fn a_closed_games_pane_stays_closed_and_comes_back_when_asked() {
        let mut app = games_app("games-closed");
        load_crystal(&mut app);
        app.sync_games_tab();
        app.ui.games.hidden = true;
        app.sync_games_tab();
        assert!(app.dock.find_tab(&Tab::Games).is_none());
        app.ui.games.hidden = false;
        app.sync_games_tab();
        assert!(app.dock.find_tab(&Tab::Games).is_some());
    }
}
