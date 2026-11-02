use super::toasts::{Toast, Toasts};
use super::{Citrine, FRIENDS_DOCK_SHARE};
use super::{focus, link, tabs, viewer, widgets};
use crate::app::tabs::Tab;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::keybinds::PadButton;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::settings::console_name;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::settings::other_console;
use crate::icons;
use crate::utils::file_loader::FileLoader;
#[cfg(not(target_arch = "wasm32"))]
use citrine_gb::rom::header::RomCgbMode;
use egui::{CentralPanel, Color32, Context, TopBottomPanel};
use egui_dock::{DockState, NodeIndex, SurfaceIndex};
use strum::IntoEnumIterator;

impl Citrine {
    pub fn is_mobile(&self, ctx: &Context) -> bool {
        self.ui.settings.layout.is_touch(ctx)
    }

    pub(super) fn render(&mut self, ctx: &Context) {
        if self.is_mobile(ctx) {
            self.render_mobile(ctx);
        } else if self.ui.settings.focus_mode {
            self.render_focus_mode(ctx);
        } else {
            self.render_normal_mode(ctx);
        }
    }

    pub(super) fn render_normal_mode(&mut self, ctx: &Context) {
        TopBottomPanel::top("top_panel").show(ctx, |ui| self.top_panel(ui));
        self.toast_strip(ctx);

        self.ui.tab_fit = tabs::fit_titles(ctx, &self.dock);
        CentralPanel::default().show(ctx, |ui| {
            let mut viewer = tabs::TabViewer {
                audio: &mut self.audio,
                commonmark: &mut self.commonmark,
                emulator: &mut self.emulator,
                game_info: &mut self.game_info,
                pads: self.pads.as_ref(),
                events: &mut self.events,
                files: &mut self.files,
                media: &mut self.media,
                online: &mut self.online,
                play: &mut self.play,
                stats: &self.stats,
                ui: &mut self.ui,
            };

            egui_dock::DockArea::new(&mut self.dock)
                .style(egui_dock::Style::from_egui(ctx.style().as_ref()))
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_inside(ui, &mut viewer);
        });
    }

    pub(super) fn media_viewer(&mut self, ctx: &Context) {
        let mut viewer = tabs::TabViewer {
            audio: &mut self.audio,
            commonmark: &mut self.commonmark,
            emulator: &mut self.emulator,
            game_info: &mut self.game_info,
            pads: self.pads.as_ref(),
            events: &mut self.events,
            files: &mut self.files,
            media: &mut self.media,
            online: &mut self.online,
            play: &mut self.play,
            stats: &self.stats,
            ui: &mut self.ui,
        };
        viewer::show(&mut viewer, ctx);
    }

    pub(super) fn show_toasts(&mut self, ctx: &Context) {
        if let Some(area) = self.ui.toast_area.take() {
            self.toasts.place(area);
        }
        for event in self.toasts.show(ctx) {
            self.events.push(event);
        }
    }

    pub(super) fn toast_strip(&mut self, ctx: &Context) {
        if !self.toasts.wants_strip() {
            return;
        }
        TopBottomPanel::bottom("toast_strip")
            .exact_height(Toasts::strip_height(ctx))
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| self.toasts.place_strip(ui.max_rect()));
    }

    pub(super) fn top_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Citrine");
            ui.separator();

            ui.menu_button(icons::FOLDER, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.set_min_width(180.0);

                if ui.button("Load ROM").clicked() {
                    FileLoader::new()
                        .title("Load ROM")
                        .add_filter("Game Boy ROMs", &["gb", "gbc"])
                        .dispatch(self.files.rom_tx.clone());
                }
                if ui.button("Load boot ROM").clicked() {
                    FileLoader::new()
                        .title("Load boot ROM")
                        .add_filter("Game Boy boot ROM", &["bin"])
                        .dispatch(self.files.boot_rom_tx.clone());
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
                                    let button = egui::Button::new(name).right_text(
                                        egui::containers::menu::SubMenuButton::RIGHT_ARROW,
                                    );
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
                                    .button(name)
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

            if self.game_info.recognised()
                && ui
                    .button(icons::IDENTIFICATION_CARD)
                    .on_hover_text(Tab::GameInfo.label())
                    .clicked()
            {
                self.open_tab(Tab::GameInfo);
                self.widen_for_game_info();
            }

            if ui
                .button(icons::JOYSTICK)
                .on_hover_text(Tab::Homebrew.label())
                .clicked()
            {
                self.open_tab(Tab::Homebrew);
            }

            let saves_hint = match self.ui.settings.hotkeys.quick_keys() {
                Some(keys) => format!("{} ({keys} quick save / load)", Tab::Saves.label()),
                None => Tab::Saves.label().to_owned(),
            };
            if ui
                .button(icons::FLOPPY_DISK)
                .on_hover_text(saves_hint)
                .clicked()
            {
                self.open_tab(Tab::Saves);
            }

            if ui
                .button(icons::IMAGES)
                .on_hover_text("Screenshots and recordings")
                .clicked()
            {
                self.open_tab(Tab::Media);
            }

            if ui.button(icons::CHART_BAR).on_hover_text("Stats").clicked() {
                self.open_tab(Tab::Stats);
            }

            let entry = self.link_entry();
            let hint = match entry {
                link::LinkEntry::Blocked(why) => why,
                link::LinkEntry::Open if self.emulator.partner.is_some() => "Event partner",
                link::LinkEntry::Open => "Linked game",
                link::LinkEntry::Ready => "Link a second game",
            };
            let enabled = !matches!(entry, link::LinkEntry::Blocked(_));
            if ui
                .add_enabled(enabled, egui::Button::new(tabs::linked::LINK_ICON))
                .on_hover_text(hint)
                .on_disabled_hover_text(hint)
                .clicked()
            {
                self.pick_second_game(false);
            }

            if self.ui.settings.online.is_joined() {
                let invited = self.play.has_unseen_invite();
                let hint = tabs::chat::attention_hint(&self.online, invited);
                let response = ui.button(icons::USERS).on_hover_text(hint);
                if self.online.needs_attention() || invited {
                    widgets::avatar::status_dot(ui, response.rect);
                }
                if response.clicked() {
                    self.open_tab(Tab::Friends);
                }
            }

            if ui.button(icons::GEAR).on_hover_text("Settings").clicked() {
                self.open_tab(Tab::Settings);
            }

            ui.menu_button(icons::INFO, |ui| {
                if ui.button(Tab::Info.label()).clicked() {
                    self.open_tab(Tab::Info);
                }
                if ui.button(Tab::RomInfo.label()).clicked() {
                    self.open_tab(Tab::RomInfo);
                }
            })
            .response
            .on_hover_text("About");

            let focus = HotkeyAction::ToggleFocusMode;
            let focus_hint = match self.ui.settings.hotkeys.key_name(focus) {
                Some(key) => format!("{} ({key})", focus.label()),
                None => focus.label().to_owned(),
            };
            if ui
                .button(icons::FRAME_CORNERS)
                .on_hover_text(focus_hint)
                .clicked()
            {
                self.toggle_focus_mode();
            }

            if self.ui.settings.dev_mode {
                ui.menu_button(icons::CIRCUITRY, |ui| {
                    for tab in [Tab::Disassembly, Tab::Apu, Tab::Registers] {
                        if ui.button(tab.label()).clicked() {
                            self.open_tab(tab);
                        }
                    }
                })
                .response
                .on_hover_text("Hardware views");

                ui.menu_button(icons::BRACKETS_CURLY, |ui| {
                    for tab in [
                        Tab::TimeControl,
                        Tab::Performance,
                        Tab::DebugActions,
                        Tab::AudioDebug,
                        Tab::ControllerDebug,
                        Tab::E2ETest,
                    ] {
                        if ui.button(tab.label()).clicked() {
                            self.open_tab(tab);
                        }
                    }
                })
                .response
                .on_hover_text("Debug tools");
            }

            ui.separator();

            if ui
                .button("Reset layout")
                .on_hover_text("Restore the default tab layout")
                .clicked()
            {
                self.dock = DockState::new(vec![Tab::GameBoy]);
                if self.ui.settings.online.is_joined() {
                    self.open_tab(Tab::Friends);
                }
            }
        });
    }

    pub(super) fn transport_buttons(&mut self, ui: &mut egui::Ui) {
        let loaded = self.emulator.gb.cartridge.has_rom_loaded;
        let running = self.emulator.running;

        let play_icon = if running { icons::PAUSE } else { icons::PLAY };
        let play_hint = if running { "Pause" } else { "Resume" };
        if self
            .shortcut_button(ui, play_icon, play_hint, HotkeyAction::TogglePause, loaded)
            .clicked()
        {
            self.run_hotkey(HotkeyAction::TogglePause);
        }

        if self
            .shortcut_button(
                ui,
                icons::ARROW_COUNTER_CLOCKWISE,
                "Reset the game",
                HotkeyAction::SoftReset,
                loaded,
            )
            .clicked()
        {
            self.run_hotkey(HotkeyAction::SoftReset);
        }

        if self
            .shortcut_button(
                ui,
                icons::CAMERA,
                "Screenshot",
                HotkeyAction::Screenshot,
                loaded,
            )
            .clicked()
        {
            self.run_hotkey(HotkeyAction::Screenshot);
        }

        if self.recording_button(ui, None).clicked() {
            self.run_hotkey(HotkeyAction::ToggleRecording);
        }

        let muted = self.ui.settings.muted;
        let mute_icon = if muted {
            icons::SPEAKER_SLASH
        } else {
            icons::SPEAKER_HIGH
        };
        let mute_hint = if muted { "Unmute" } else { "Mute" };
        if self
            .shortcut_button(ui, mute_icon, mute_hint, HotkeyAction::ToggleMute, true)
            .clicked()
        {
            self.run_hotkey(HotkeyAction::ToggleMute);
        }
    }

    pub(super) fn shortcut_button(
        &self,
        ui: &mut egui::Ui,
        icon: &str,
        hint: &str,
        action: HotkeyAction,
        enabled: bool,
    ) -> egui::Response {
        let keys = self.ui.settings.hotkeys.keys_for(action);
        let hint = match keys.first() {
            Some(hotkey) => format!("{hint} ({hotkey})"),
            None => hint.to_string(),
        };
        ui.add_enabled(enabled, egui::Button::new(icon))
            .on_hover_text(hint)
    }

    pub(super) fn toggle_focus_mode(&mut self) {
        self.ui.settings.focus_mode = !self.ui.settings.focus_mode;
        if self.ui.settings.focus_mode {
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
                let place = self.ui.settings.focus_place;
                let screen = self
                    .emulator
                    .ui_placed(ui, area, |size| place.rect(area, size));
                let response = ui
                    .interact(
                        screen,
                        ui.id().with("focus-screen"),
                        egui::Sense::click_and_drag(),
                    )
                    .on_hover_cursor(egui::CursorIcon::Move);
                if response.double_clicked() {
                    self.ui.settings.focus_place = focus::ScreenPlace::default();
                    self.ui.settings.dirty = true;
                } else if response.dragged() {
                    self.ui.settings.focus_place =
                        place.dragged(response.drag_delta(), area, screen.size());
                }
                if response.drag_stopped() {
                    self.ui.settings.dirty = true;
                }
            });
    }

    pub(super) fn tools_node(&self) -> Option<(SurfaceIndex, NodeIndex)> {
        Tab::iter()
            .filter(|t| *t != Tab::GameBoy && *t != Tab::Friends)
            .find_map(|t| self.dock.find_tab(&t))
            .map(|(surface_idx, node_idx, _)| (surface_idx, node_idx))
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

        if tab == Tab::Friends
            && let Some((surface, gb_node, _)) = self.dock.find_tab(&Tab::GameBoy)
            && surface == SurfaceIndex::main()
        {
            self.dock
                .main_surface_mut()
                .split_left(gb_node, FRIENDS_DOCK_SHARE, vec![tab]);
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
}
