use super::{Citrine, link, tabs, widgets};
use crate::app::tabs::Tab;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::dev_menu::dev_menu;
use crate::app::widgets::shortcut::{shortcut_button, shortcut_hint};
use crate::icons;
use egui::{Button, Response, TextStyle, TextWrapMode, Ui, WidgetText};
use egui_dock::DockState;

const SEPARATOR_SPACING: f32 = 6.0;

const RESET_WORDS: &str = "Reset layout";

pub(super) const BRAND: &str = "Citrine";

pub(super) static CORE_GIVES_WAY: [HotkeyAction; 2] =
    [HotkeyAction::Screenshot, HotkeyAction::SoftReset];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Games,
    GameInfo,
    Achievements,
    Cheats,
    Homebrew,
    Walker,
    Gts,
    Saves,
    Media,
    Stats,
    SecondScreens,
    Link,
    Friends,
    Settings,
    About,
    BigScreen,
    Focus,
    Dev,
}

const GIVES_WAY: [Tool; 18] = [
    Tool::Dev,
    Tool::Stats,
    Tool::Cheats,
    Tool::Achievements,
    Tool::BigScreen,
    Tool::SecondScreens,
    Tool::Walker,
    Tool::Gts,
    Tool::Homebrew,
    Tool::Media,
    Tool::GameInfo,
    Tool::Focus,
    Tool::About,
    Tool::Link,
    Tool::Saves,
    Tool::Games,
    Tool::Friends,
    Tool::Settings,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reset {
    Words,
    Icon,
    InMore,
}

#[derive(Debug, PartialEq)]
struct Fit {
    shown: Vec<Tool>,
    more: Vec<Tool>,
    reset: Reset,
}

struct Widths {
    separator: f32,
    words: f32,
    icon: f32,
    more: f32,
}

fn fit(tools: &[(Tool, f32)], widths: &Widths, room: f32, more_needed: bool) -> Fit {
    let all: f32 = tools.iter().map(|(_, width)| width).sum();
    let every = || tools.iter().map(|(tool, _)| *tool).collect::<Vec<_>>();
    for (reset, width) in [(Reset::Words, widths.words), (Reset::Icon, widths.icon)] {
        if !more_needed && all + widths.separator + width <= room {
            return Fit {
                shown: every(),
                more: Vec::new(),
                reset,
            };
        }
    }
    let mut hidden = Vec::new();
    let mut used = all + widths.more;
    for tool in GIVES_WAY {
        if used <= room {
            break;
        }
        if let Some((_, width)) = tools.iter().find(|(t, _)| *t == tool) {
            hidden.push(tool);
            used -= width;
        }
    }
    let (more, shown) = tools
        .iter()
        .map(|(tool, _)| *tool)
        .partition(|tool| hidden.contains(tool));
    Fit {
        shown,
        more,
        reset: Reset::InMore,
    }
}

struct Look {
    icon: &'static str,
    label: String,
    hint: String,
    enabled: bool,
    dot: bool,
}

fn button_width(ui: &Ui, text: &str) -> f32 {
    let galley = WidgetText::from(text).into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    galley.size().x + ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x
}

impl Citrine {
    pub(super) fn toolbar_tools(&mut self, ui: &mut Ui) {
        let tools: Vec<(Tool, f32)> = self
            .tools()
            .into_iter()
            .map(|tool| (tool, button_width(ui, self.look(tool).icon)))
            .collect();
        let widths = Widths {
            separator: SEPARATOR_SPACING + ui.spacing().item_spacing.x,
            words: button_width(ui, RESET_WORDS),
            icon: button_width(ui, icons::LAYOUT),
            more: button_width(ui, icons::DOTS_THREE),
        };
        let folded = &CORE_GIVES_WAY[..self.ui.core_in_more];
        let fit = fit(&tools, &widths, ui.available_width(), !folded.is_empty());

        for tool in &fit.shown {
            self.tool_button(ui, *tool);
        }

        if fit.reset == Reset::InMore {
            let dot = fit.more.iter().any(|tool| self.look(*tool).dot);
            let more = ui.menu_button(icons::DOTS_THREE, |ui| {
                ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                for action in folded.iter().rev() {
                    self.core_row(ui, *action);
                }
                for tool in &fit.more {
                    self.tool_row(ui, *tool);
                }
                ui.separator();
                if ui
                    .button(format!("{}  {RESET_WORDS}", icons::LAYOUT))
                    .clicked()
                {
                    self.reset_layout();
                }
            });
            let more = more.response.on_hover_text("More");
            if dot {
                widgets::avatar::status_dot(ui, more.rect);
            }
            return;
        }

        ui.add(egui::Separator::default().spacing(SEPARATOR_SPACING));
        let reset = match fit.reset {
            Reset::Words => ui
                .button(RESET_WORDS)
                .on_hover_text("Restore the default tab layout"),
            _ => ui.button(icons::LAYOUT).on_hover_text(RESET_WORDS),
        };
        if reset.clicked() {
            self.reset_layout();
        }
    }

    pub(super) fn place_core(&mut self, ui: &Ui, room: f32) {
        for action in &CORE_GIVES_WAY[..self.ui.core_in_more] {
            self.ui.toolbar_core += button_width(ui, action.icon(false));
        }
        let more = button_width(ui, icons::DOTS_THREE);
        let mut core = self.ui.toolbar_core;
        let mut folded = 0;
        for action in CORE_GIVES_WAY {
            if core + more <= room {
                break;
            }
            core -= button_width(ui, action.icon(false));
            folded += 1;
        }
        self.ui.core_in_more = folded;
    }

    fn core_row(&mut self, ui: &mut Ui, action: HotkeyAction) {
        let text = format!("{}  {}", action.icon(false), action.label());
        let hint = shortcut_hint(&self.ui.settings.hotkeys, action, false);
        let row = ui.add_enabled(self.emulator.has_game(), Button::new(text));
        if row
            .on_hover_text(&hint)
            .on_disabled_hover_text(&hint)
            .clicked()
        {
            self.run_hotkey(action);
        }
    }

    pub(super) fn brand_fits(&self, ui: &Ui) -> bool {
        let separator = SEPARATOR_SPACING + ui.spacing().item_spacing.x;
        let brand = button_width(ui, BRAND) + separator;
        let more = button_width(ui, icons::DOTS_THREE);
        self.ui.toolbar_core + brand + more <= ui.available_width()
    }

    fn tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        if !tabs::games::idle(&self.emulator, &self.play) {
            tools.push(Tool::Games);
        }
        if self.game_info.recognised() {
            tools.push(Tool::GameInfo);
        }
        if cfg!(not(target_arch = "wasm32")) {
            tools.push(Tool::Achievements);
        }
        if self.emulator.has_game() {
            tools.push(Tool::Cheats);
        }
        tools.push(Tool::Homebrew);
        if cfg!(feature = "nds") {
            tools.push(Tool::Walker);
        }
        tools.extend([
            Tool::Saves,
            Tool::Media,
            Tool::Stats,
            Tool::SecondScreens,
            Tool::Link,
        ]);
        if self.ui.settings.online.is_joined() {
            if cfg!(feature = "nds") && self.ui.settings.ds_online.gts_here() {
                tools.push(Tool::Gts);
            }
            tools.push(Tool::Friends);
        }
        tools.extend([Tool::Settings, Tool::About, Tool::BigScreen, Tool::Focus]);
        if self.ui.settings.dev_mode {
            tools.push(Tool::Dev);
        }
        tools
    }

    fn look(&self, tool: Tool) -> Look {
        let plain = |icon, label: &str| Look {
            icon,
            label: label.to_owned(),
            hint: label.to_owned(),
            enabled: true,
            dot: false,
        };
        match tool {
            Tool::Games => plain(icons::GAME_CONTROLLER, Tab::Games.label()),
            Tool::GameInfo => plain(icons::IDENTIFICATION_CARD, Tab::GameInfo.label()),
            Tool::Achievements => plain(icons::TROPHY, Tab::Achievements.label()),
            Tool::Cheats => plain(icons::MAGIC_WAND, Tab::Cheats.label()),
            Tool::Homebrew => plain(icons::JOYSTICK, Tab::Homebrew.label()),
            Tool::Walker => plain(icons::PERSON_SIMPLE_WALK, Tab::Walker.title()),
            Tool::Gts => Look {
                dot: self.online.gts_mine().is_some_and(|mine| {
                    mine.iter()
                        .any(|d| matches!(d.state, citrine_gb_net::GtsState::Traded { .. }))
                }),
                ..plain(icons::GLOBE_HEMISPHERE_EAST, Tab::Gts.label())
            },
            Tool::Saves => Look {
                hint: match self.ui.settings.hotkeys.quick_keys() {
                    Some(keys) => format!("{} ({keys} quick save / load)", Tab::Saves.label()),
                    None => Tab::Saves.label().to_owned(),
                },
                ..plain(icons::FLOPPY_DISK, Tab::Saves.label())
            },
            Tool::Media => plain(icons::IMAGES, "Screenshots and recordings"),
            Tool::Stats => plain(icons::CHART_BAR, "Stats"),
            Tool::SecondScreens => plain(icons::DEVICES, Tab::SecondScreens.title()),
            Tool::Link => {
                let entry = self.link_entry();
                let hint = match entry {
                    link::LinkEntry::Blocked(why) => why,
                    link::LinkEntry::Open if self.emulator.partnered() => "Event partner",
                    link::LinkEntry::Open => "Linked game",
                    link::LinkEntry::Ready => "Link a second game",
                };
                Look {
                    enabled: !matches!(entry, link::LinkEntry::Blocked(_)),
                    ..plain(tabs::linked::LINK_ICON, hint)
                }
            }
            Tool::Friends => {
                let invited = self.play.has_unseen_invite();
                Look {
                    hint: tabs::chat::attention_hint(&self.online, invited).to_string(),
                    dot: self.online.needs_attention() || invited,
                    ..plain(icons::USERS, Tab::Friends.label())
                }
            }
            Tool::Settings => Look {
                dot: self.ui.update.phase().waiting(),
                ..plain(icons::GEAR, "Settings")
            },
            Tool::About => plain(icons::INFO, "About"),
            Tool::BigScreen => plain(icons::TELEVISION_SIMPLE, "Big screen"),
            Tool::Focus => {
                let action = HotkeyAction::ToggleFocusMode;
                Look {
                    hint: shortcut_hint(&self.ui.settings.hotkeys, action, false),
                    ..plain(action.icon(false), action.label())
                }
            }
            Tool::Dev => plain(icons::CODE, "Dev tools"),
        }
    }

    fn tool_button(&mut self, ui: &mut Ui, tool: Tool) {
        let look = self.look(tool);
        let response = match tool {
            Tool::About => ui.menu_button(look.icon, |ui| self.about_menu(ui)).response,
            Tool::Dev => ui.menu_button(look.icon, |ui| self.dev_tools(ui)).response,
            Tool::Focus => shortcut_button(
                ui,
                &self.ui.settings.hotkeys,
                HotkeyAction::ToggleFocusMode,
                false,
                true,
            ),
            _ => ui.add_enabled(look.enabled, Button::new(look.icon)),
        };
        self.after_tool(ui, tool, look, response);
    }

    fn tool_row(&mut self, ui: &mut Ui, tool: Tool) {
        let look = self.look(tool);
        let text = format!("{}  {}", look.icon, look.label);
        let response = match tool {
            Tool::About => ui.menu_button(text, |ui| self.about_menu(ui)).response,
            Tool::Dev => ui.menu_button(text, |ui| self.dev_tools(ui)).response,
            _ => ui.add_enabled(look.enabled, Button::new(text)),
        };
        self.after_tool(ui, tool, look, response);
    }

    fn after_tool(&mut self, ui: &Ui, tool: Tool, look: Look, response: Response) {
        let response = if tool == Tool::Focus {
            response
        } else {
            response
                .on_hover_text(&look.hint)
                .on_disabled_hover_text(&look.hint)
        };
        if look.dot {
            widgets::avatar::status_dot(ui, response.rect);
        }
        if !matches!(tool, Tool::About | Tool::Dev) && response.clicked() {
            self.run_tool(tool);
        }
    }

    fn run_tool(&mut self, tool: Tool) {
        match tool {
            Tool::Games => match self.dock.find_tab(&Tab::Games) {
                Some((surface, node, tab)) => match &self.dock[surface][node] {
                    egui_dock::Node::Leaf(leaf) if leaf.active != tab => {
                        self.dock.set_active_tab((surface, node, tab));
                    }
                    _ => self.ui.games.hidden = true,
                },
                None => self.ui.games.hidden = false,
            },
            Tool::GameInfo => {
                self.open_tab(Tab::GameInfo);
                self.widen_for_game_info();
            }
            Tool::Achievements => self.open_tab(Tab::Achievements),
            Tool::Cheats => self.open_tab(Tab::Cheats),
            Tool::Homebrew => self.open_tab(Tab::Homebrew),
            Tool::Walker => {
                #[cfg(feature = "nds")]
                self.open_walker();
            }
            Tool::Gts => self.open_tab(Tab::Gts),
            Tool::Saves => self.open_tab(Tab::Saves),
            Tool::Media => self.open_tab(Tab::Media),
            Tool::Stats => self.open_tab(Tab::Stats),
            Tool::SecondScreens => self.open_tab(Tab::SecondScreens),
            Tool::Link => self.pick_second_game(false),
            Tool::Friends => self.open_tab(Tab::Friends),
            Tool::Settings => self.open_tab(Tab::Settings),
            Tool::BigScreen => self.set_big_screen(true),
            Tool::Focus => self.toggle_focus_mode(),
            Tool::About | Tool::Dev => {}
        }
    }

    fn about_menu(&mut self, ui: &mut Ui) {
        if ui.button(Tab::Info.label()).clicked() {
            self.open_tab(Tab::Info);
        }
        if ui.button(Tab::RomInfo.label()).clicked() {
            self.open_tab(Tab::RomInfo);
        }
        ui.separator();
        if ui
            .add_enabled(
                self.emulator.has_game(),
                Button::new(super::report::SAVE_GAME_REPORT),
            )
            .clicked()
        {
            self.save_game_report();
        }
    }

    fn dev_tools(&mut self, ui: &mut Ui) {
        if let Some(tab) = dev_menu(ui) {
            self.open_tab(tab);
        }
    }

    fn reset_layout(&mut self) {
        self.dock = DockState::new(vec![Tab::GameBoy]);
        if self.ui.settings.online.is_joined() {
            self.open_tab(Tab::Friends);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{load_game, phone_app};
    use crate::app::ui_state::settings::LayoutMode;
    #[cfg(feature = "nds")]
    use egui::{Context, RawInput, Rect, pos2, vec2};

    #[cfg(feature = "nds")]
    const WEB_ZOOM_AT_1280: f32 = 2.2;

    fn widths() -> Widths {
        Widths {
            separator: 10.0,
            words: 90.0,
            icon: 30.0,
            more: 30.0,
        }
    }

    fn row(tools: &[Tool]) -> Vec<(Tool, f32)> {
        tools.iter().map(|tool| (*tool, 30.0)).collect()
    }

    const ALL: [Tool; 13] = [
        Tool::GameInfo,
        Tool::Homebrew,
        Tool::Walker,
        Tool::Saves,
        Tool::Media,
        Tool::Stats,
        Tool::SecondScreens,
        Tool::Link,
        Tool::Friends,
        Tool::Settings,
        Tool::About,
        Tool::Focus,
        Tool::Dev,
    ];

    #[test]
    fn a_wide_row_shows_every_tool_and_reset_in_words() {
        let fit = fit(&row(&ALL), &widths(), 13.0 * 30.0 + 100.0, false);
        assert_eq!(fit.shown, ALL.to_vec());
        assert_eq!(fit.reset, Reset::Words);
    }

    #[test]
    fn reset_turns_into_an_icon_before_any_tool_moves() {
        let fit = fit(&row(&ALL), &widths(), 13.0 * 30.0 + 40.0, false);
        assert_eq!(fit.shown, ALL.to_vec());
        assert_eq!(fit.reset, Reset::Icon);
    }

    #[test]
    fn a_screenshot_in_more_opens_more_even_when_every_tool_fits() {
        let fit = fit(&row(&ALL), &widths(), 13.0 * 30.0 + 100.0, true);
        assert_eq!(fit.reset, Reset::InMore);
        assert_eq!(fit.shown, ALL.to_vec());
        assert!(fit.more.is_empty());
    }

    #[test]
    fn a_narrow_row_moves_the_least_used_tools_into_more_and_keeps_settings() {
        let fit = fit(&row(&ALL), &widths(), 8.0 * 30.0, false);
        assert_eq!(fit.reset, Reset::InMore);
        assert_eq!(fit.shown.len(), 7);
        assert!(fit.shown.contains(&Tool::Settings));
        assert!(fit.shown.contains(&Tool::Friends));
        assert!(fit.more.contains(&Tool::Dev) && fit.more.contains(&Tool::Stats));
        let order: Vec<Tool> = ALL
            .iter()
            .copied()
            .filter(|t| fit.more.contains(t))
            .collect();
        assert_eq!(fit.more, order, "the menu keeps the toolbar's order");
    }

    #[cfg(feature = "nds")]
    fn game_apps() -> Vec<(&'static str, Citrine)> {
        let gb = |name: &str| {
            let mut app = phone_app(name);
            app.ui.settings.layout = LayoutMode::Desktop;
            app.ui.settings.dev_mode = true;
            app
        };
        let mut none = gb("toolbar-none");
        none.ui.settings.dev_mode = false;
        let mut crystal = gb("toolbar-crystal");
        load_game(
            &mut crystal,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        let mut gba = gb("toolbar-gba");
        let mut rom = vec![0u8; 0x400];
        rom[..4].copy_from_slice(&0xEAFF_FFFE_u32.to_le_bytes());
        gba.emulator.put_gba(&rom, "toolbar-gba");
        let mut ds = gb("toolbar-ds");
        ds.emulator
            .load_nds(include_bytes!(
                "../../fixtures/nds/blocksds-console-both-screens.nds"
            ))
            .unwrap();
        vec![
            ("no game", none),
            ("crystal", crystal),
            ("gba", gba),
            ("ds", ds),
        ]
    }

    #[cfg(feature = "nds")]
    fn web_frame(app: &mut Citrine, ctx: &Context, width: f32, height: f32) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))),
            max_texture_side: Some(8192),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            crate::app::mobile::touch_spacing(ctx, touch);
            app.render(ctx);
        });
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_toolbar_never_runs_past_the_window() {
        for (name, mut app) in game_apps() {
            for css in [1280.0, 1024.0, 800.0, 600.0] {
                let width = css / WEB_ZOOM_AT_1280;
                let ctx = Context::default();
                ctx.set_zoom_factor(WEB_ZOOM_AT_1280);
                app.ui.toolbar_reach = 0.0;
                for _ in 0..3 {
                    web_frame(&mut app, &ctx, width, 800.0 / WEB_ZOOM_AT_1280);
                }
                assert!(!app.is_mobile(&ctx), "{name} at {css} css: phone layout");
                assert!(
                    app.ui.toolbar_reach > 0.0 && app.ui.toolbar_reach <= width + 0.5,
                    "{name} at {css} css: the toolbar reaches {} of {width}",
                    app.ui.toolbar_reach
                );
            }
        }
    }

    #[test]
    fn the_games_button_shows_with_a_game_and_turns_the_pane_on_and_off() {
        let mut app = phone_app("toolbar-games");
        app.ui.settings.layout = LayoutMode::Desktop;
        assert!(!app.tools().contains(&Tool::Games));
        load_game(
            &mut app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        assert!(app.tools().contains(&Tool::Games));
        app.sync_games_tab();
        app.run_tool(Tool::Games);
        app.sync_games_tab();
        assert!(app.dock.find_tab(&Tab::Games).is_none());
        app.run_tool(Tool::Games);
        app.sync_games_tab();
        assert!(app.dock.find_tab(&Tab::Games).is_some());
    }

    #[test]
    fn the_games_button_brings_a_covered_games_pane_forward() {
        let mut app = phone_app("toolbar-games-forward");
        app.ui.settings.layout = LayoutMode::Desktop;
        load_game(
            &mut app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        app.sync_games_tab();
        app.open_tab(Tab::Cheats);
        let (surface, node, games) = app.dock.find_tab(&Tab::Games).unwrap();
        assert_eq!(app.dock.find_tab(&Tab::Cheats).unwrap().1, node);
        app.run_tool(Tool::Games);
        app.sync_games_tab();
        assert!(!app.ui.games.hidden);
        let egui_dock::Node::Leaf(leaf) = &app.dock[surface][node] else {
            panic!("the games pane is not in a leaf");
        };
        assert_eq!(leaf.active, games);
    }
}
