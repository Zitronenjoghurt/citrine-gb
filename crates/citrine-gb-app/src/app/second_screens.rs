use super::Citrine;
use super::mobile::{frame, sheet};
use super::tabs::Tab;
use super::ui_state::hotkeys::HotkeyAction;
use super::widgets::confirm::{self, Answer, Ask};
use super::widgets::security_code::{self, SecurityCode};
use super::widgets::touch_controls::{TouchInput, TouchLayout};
use crate::held::InputSource;
use crate::icons;
use crate::online::ServerAddress;
use crate::second_screens::{
    Account, Action, DeviceKeys, JoinState, OwnDevice, Presence, Relay, Suggestion, SurfaceId,
    SurfaceRow,
};
use citrine_gb::console::Keys;
use citrine_gb_net::CheckCode;
use citrine_gb_surface::link::Route;
use citrine_gb_surface::mode::{Mode, Preset, ScreenShape, View};
use citrine_gb_surface::pairing::Code;
use citrine_gb_surface::proof::{ProofKey, TagKey};
use citrine_gb_surface::proto::{GameCard, Icon, SurfaceKind};
use citrine_gb_surface::relay::DoorKey;
use citrine_gb_surface::remote::{Phase, Remote};
use citrine_gb_surface::touch::{Area, touch_in};
use egui::{
    Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2,
};

const DEFAULT_RELAY: &str = "gb.lemon.industries";
const CONTROLLER_MOST: f32 = 400.0;
const INFO_HEIGHT: f32 = 250.0;
const WINDOW_SIZE: [f32; 2] = [520.0, 640.0];
const ASK_NOTE: &str = "It can see and play this game.";

pub(crate) fn ask_code_hint(name: &str) -> String {
    format!(
        "If {name} shows other digits, someone else may be connecting to see and play this game. Choose Not now."
    )
}

fn ask_compare(name: &str) -> String {
    format!("Allow if {name} shows the same digits")
}
pub const PANES: [Tab; 5] = [
    Tab::Saves,
    Tab::Media,
    Tab::Stats,
    Tab::Friends,
    Tab::Settings,
];

#[derive(Debug, Clone, PartialEq)]
pub enum PaneAction {
    AddDevice,
    StopAdding,
    SetMode(SurfaceId, Mode),
    Remove(SurfaceId),
    OpenWindow,
    OpenDisplay,
    Join(Code),
    JoinSuggestion(Suggestion),
    CancelJoin,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub code: Option<(Code, f64)>,
    pub link: Option<String>,
    pub rows: Vec<SurfaceRow>,
    pub screens: Vec<ScreenShape>,
    pub windows: bool,
    pub display: Option<String>,
    pub join: Option<JoinState>,
    pub join_host: Option<String>,
    pub join_code: Option<CheckCode>,
    pub suggestions: Vec<Suggestion>,
    pub has_game: bool,
    pub lan_blocked: bool,
    pub code_spent: bool,
}

#[derive(Default)]
pub struct PaneState {
    pub facts: Facts,
    pub actions: Vec<PaneAction>,
    pub code_text: String,
    pub views_open: Option<SurfaceId>,
    pub device_sheet: Option<SurfaceId>,
    pub wants_suggestions: bool,
}

#[derive(Default)]
pub struct ViewState {
    textures: Vec<Option<(u32, egui::TextureHandle)>>,
    icon: Option<(String, egui::TextureHandle)>,
    touch: TouchInput,
    pub choosing: bool,
    asked: bool,
}

pub struct SurfaceWindow {
    id: egui::ViewportId,
    remote: Remote,
    view: ViewState,
}

#[cfg(target_os = "android")]
struct DisplaySurface {
    remote: Remote,
    view: ViewState,
    painter: Option<crate::platform::android::display::Painter>,
}

#[derive(Default)]
pub struct Surfaces {
    pub joined: ViewState,
    windows: Vec<SurfaceWindow>,
    #[cfg(target_os = "android")]
    display: Option<DisplaySurface>,
    opened: u32,
    playing: Option<(String, web_time::Instant)>,
    icon: Option<(String, Option<Icon>)>,
}

#[derive(Clone, Copy)]
enum Feed {
    Pictures,
    Emulator,
}

fn relay_of(address: &ServerAddress, password: Option<String>) -> Relay {
    Relay {
        url: address.socket_url(citrine_gb_surface::relay::PATH),
        web: address.web_url(""),
        password,
    }
}

#[cfg(target_arch = "wasm32")]
fn default_relay() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .filter(|origin| origin.starts_with("http"))
        .unwrap_or_else(|| DEFAULT_RELAY.to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn default_relay() -> String {
    DEFAULT_RELAY.to_owned()
}

#[cfg(any(test, target_arch = "wasm32"))]
pub fn join_code_from_link(text: &str) -> Option<Code> {
    let (_, after) = text.split_once("screen=")?;
    Code::parse(after.get(..Code::DIGITS)?)
}

impl Citrine {
    fn second_relay(&self) -> Option<Relay> {
        if let Some(joined) = self.ui.settings.online.joined_relay() {
            let password = (!joined.password.is_empty()).then_some(joined.password);
            return Some(relay_of(&joined.address, password));
        }
        ServerAddress::parse(&default_relay())
            .ok()
            .map(|address| relay_of(&address, None))
    }

    #[cfg(all(test, feature = "nds", not(target_arch = "wasm32")))]
    fn step_second_screens_for_tests(&mut self) {
        let screens = self.emulator.video().screens.len();
        self.emulator.screen_away = self.second.screen_away(screens);
    }

    fn second_account(&self) -> Option<Account> {
        self.ui.settings.online.joined_relay()?;
        let keys = self.online.keys()?;
        let (devices, own) = self.online.devices();
        let own = own?;
        let keys_of = |device| DeviceKeys {
            door: DoorKey(keys.screen_door(device)),
            proof: ProofKey(keys.screen_proof(device)),
            tag: TagKey(keys.screen_tag_key(device)),
        };
        let devices = devices
            .iter()
            .filter(|d| d.id != own && d.online)
            .map(|d| OwnDevice {
                name: d.name.clone(),
                keys: keys_of(d.id),
            })
            .collect();
        Some(Account {
            keys: keys_of(own),
            devices,
        })
    }

    fn ask_second_screens(&mut self, ctx: &egui::Context) {
        let Some(asker) = self.second.askers().into_iter().next() else {
            return;
        };
        let message = format!("{} wants to be a second screen", asker.name);
        let notes = [ASK_NOTE.to_owned()];
        let hint = ask_code_hint(&asker.name);
        let compare = ask_compare(&asker.name);
        let answer = confirm::ask_as(
            ctx,
            &format!("second_screen_ask_{}", asker.id),
            Ask {
                message: &message,
                list: &[],
                notes: &notes,
                action: "Allow",
                action_enabled: true,
                extra: None,
                load: None,
                code: Some(SecurityCode {
                    title: security_code::TITLE,
                    hint: &hint,
                    lines: &asker.code.lines(),
                    compare: Some(&compare),
                }),
            },
            "Not now",
        );
        match answer {
            Some(Answer::Action) => self.second.allow(asker.id),
            Some(Answer::Cancel) => self.second.decline(asker.id),
            Some(Answer::Extra) | None => {}
        }
    }

    pub(super) fn step_second_screens(&mut self, ctx: &egui::Context) {
        let relay = self.second_relay();
        self.second.set_relay(relay);
        let account = self.second_account();
        self.second.set_account(account);
        if std::mem::take(&mut self.ui.second.wants_suggestions) {
            self.second.want_suggestions();
        }
        self.second.step();
        for name in self.second.take_outdated() {
            self.toasts.error(format!(
                "{name} runs another version of Citrine. Update Citrine on both devices"
            ));
        }
        self.ask_second_screens(ctx);
        let screens = self.emulator.video().screens.len();
        self.emulator.screen_away = self.second.screen_away(screens);
        self.apply_pane_actions();
        self.ui.second.facts = Facts {
            code: self.second.code(),
            link: self
                .second
                .code()
                .and_then(|(code, _)| self.second.code_link(code)),
            rows: self.second.surfaces(),
            screens: self.second.host().screens().to_vec(),
            windows: cfg!(not(any(target_arch = "wasm32", target_os = "android")))
                && !self.is_mobile(ctx),
            display: self.display_name(),
            join: self.second.joining().map(|j| j.state()),
            join_host: self
                .second
                .joining()
                .and_then(|j| j.host_name().map(str::to_owned)),
            join_code: self.second.joining().and_then(|j| j.check_code()),
            suggestions: self.second.suggestions(),
            has_game: self.emulator.has_game(),
            lan_blocked: self.second.lan_blocked(),
            code_spent: self.second.code_spent(),
        };
    }

    fn apply_pane_actions(&mut self) {
        for action in std::mem::take(&mut self.ui.second.actions) {
            match action {
                PaneAction::AddDevice => {
                    self.second.add_device();
                }
                PaneAction::StopAdding => self.second.stop_adding(),
                PaneAction::SetMode(id, mode) => self.second.host_mut().set_mode(id, mode),
                PaneAction::Remove(id) => self.second.host_mut().remove(id),
                PaneAction::OpenWindow => self.open_surface_window(),
                PaneAction::OpenDisplay => self.open_display(),
                PaneAction::Join(code) => {
                    self.surface.joined = ViewState::default();
                    self.second.join(code);
                }
                PaneAction::JoinSuggestion(suggestion) => {
                    self.surface.joined = ViewState::default();
                    self.second.join_suggestion(&suggestion);
                }
                PaneAction::CancelJoin => self.second.leave(),
            }
        }
    }

    pub(super) fn second_screens_input(&mut self) {
        let local = self.emulator.take_touch();
        let video = self.emulator.video();
        let screen = video
            .screens
            .iter()
            .position(|s| s.touch)
            .and_then(|at| u8::try_from(at).ok());
        let (touch, buttons) = self.second.input(local, screen);
        if let Some(touch) = touch {
            self.emulator.set_touch(touch);
        }
        if let Some(buttons) = buttons {
            self.emulator
                .hold(InputSource::Surface, Keys::from_bits_retain(buttons));
        }
        for action in self.second.take_actions() {
            match action {
                Action::Pause if self.emulator.is_running() => self.emulator.toggle_running(),
                Action::Resume if !self.emulator.is_running() && self.emulator.has_game() => {
                    self.emulator.toggle_running();
                }
                Action::SaveState if self.emulator.has_game() => {
                    self.run_hotkey(HotkeyAction::QuickSave);
                }
                Action::Pause | Action::Resume | Action::SaveState => {}
            }
        }
    }

    pub(super) fn publish_second_screens(&mut self) {
        if self.second.card_due() {
            let card = self.game_card();
            self.second.set_game(card);
        }
        let frame = self.emulator.total_frames;
        if self.second.wants_frames(frame) {
            let video = self.emulator.video();
            let rgba = self.emulator.screens_picture();
            self.second.publish(frame, video, &rgba);
        }
    }

    pub(super) fn game_card(&mut self) -> Option<GameCard> {
        if !self.emulator.shows_console() {
            self.surface.playing = None;
            return None;
        }
        let session = self
            .play
            .session()
            .filter(|_| self.emulator.shows_session())
            .and_then(|session| session.shown_game());
        let (key, title, session_rom) = match session {
            Some((card, rom)) => (
                card.hash.iter().map(|b| format!("{b:02x}")).collect(),
                crate::game_name::name_or(&card.hash, &card.title).to_owned(),
                Some(rom),
            ),
            None => {
                let key = self
                    .emulator
                    .rom_key()
                    .unwrap_or(self.emulator.game_name())
                    .to_owned();
                let title = self
                    .ui
                    .settings
                    .game_names
                    .name_or(Some(&key), self.emulator.game_name())
                    .to_owned();
                (key, title, None)
            }
        };
        let now = web_time::Instant::now();
        if self.surface.playing.as_ref().is_none_or(|(k, _)| *k != key) {
            self.surface.playing = Some((key.clone(), now));
        }
        if self.surface.icon.as_ref().is_none_or(|(k, _)| *k != key) {
            let icon = match session_rom {
                Some(rom) => rom.as_deref().and_then(banner_icon),
                #[cfg(feature = "nds")]
                None => banner_icon(&self.emulator.rom_bytes()),
                #[cfg(not(feature = "nds"))]
                None => None,
            };
            self.surface.icon = Some((key.clone(), icon));
        }
        let video = self.emulator.video();
        let played = self
            .surface
            .playing
            .as_ref()
            .map_or(0, |(_, since)| now.duration_since(*since).as_secs());
        Some(GameCard {
            title,
            system: crate::app::ui_state::settings::kind_name(self.emulator.played_kind())
                .to_owned(),
            icon: self
                .surface
                .icon
                .as_ref()
                .and_then(|(_, icon)| icon.clone()),
            screens: video
                .screens
                .iter()
                .map(|s| ScreenShape {
                    width: u16::try_from(s.width).unwrap_or(u16::MAX),
                    height: u16::try_from(s.height).unwrap_or(u16::MAX),
                    touch: s.touch,
                })
                .collect(),
            played_secs: played,
            paused: !self.emulator.is_running(),
            saved_secs_ago: None,
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn join_from_link(&mut self) {
        let Some(location) = web_sys::window().map(|w| w.location()) else {
            return;
        };
        let hash = location.hash().unwrap_or_default();
        let Some(code) = join_code_from_link(&hash) else {
            return;
        };
        if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
            let path = location.pathname().unwrap_or_default();
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
        }
        self.ui.second.code_text = code.plain();
        self.ui.second.actions.push(PaneAction::Join(code));
        self.mobile.open(Tab::SecondScreens);
        self.open_tab(Tab::SecondScreens);
    }

    pub(super) fn joined_as_surface(&self) -> bool {
        self.second
            .joining()
            .is_some_and(|j| matches!(j.state(), JoinState::Joined | JoinState::Reconnecting))
    }

    fn open_surface_window(&mut self) {
        self.surface.opened += 1;
        let name = format!("Window {}", self.surface.opened);
        let remote = self.second.open_window(&name, SurfaceKind::Window);
        let id = egui::ViewportId::from_hash_of(("second_screen_window", self.surface.opened));
        self.surface.windows.push(SurfaceWindow {
            id,
            remote,
            view: ViewState::default(),
        });
    }

    #[cfg(target_os = "android")]
    fn display_name(&self) -> Option<String> {
        if self.surface.display.is_some() {
            return None;
        }
        crate::platform::android::display::available()
    }

    #[cfg(not(target_os = "android"))]
    fn display_name(&self) -> Option<String> {
        None
    }

    #[cfg(target_os = "android")]
    fn open_display(&mut self) {
        if self.surface.display.is_some() {
            return;
        }
        let name = crate::platform::android::display::available()
            .unwrap_or_else(|| "Second display".to_owned());
        let remote = self.second.open_window(&name, SurfaceKind::Display);
        self.surface.display = Some(DisplaySurface {
            remote,
            view: ViewState::default(),
            painter: None,
        });
        crate::platform::android::display::open();
    }

    #[cfg(not(target_os = "android"))]
    fn open_display(&mut self) {}

    #[cfg(target_os = "android")]
    fn second_display(&mut self, ctx: &egui::Context) {
        use crate::platform::android::display;
        let Some(mut shown) = self.surface.display.take() else {
            let _ = display::take_window();
            return;
        };
        let now = self.second.now();
        shown.remote.step(now);
        match display::take_window() {
            Some(Some(window)) => {
                shown.painter = display::Painter::new(window);
                if let Some(painter) = &shown.painter {
                    Self::setup_fonts(&painter.ctx);
                }
            }
            Some(None) => {
                shown.remote.leave();
                return;
            }
            None => {}
        }
        if matches!(shown.remote.phase(), Phase::Ended(_)) {
            display::close();
            return;
        }
        if let Some(mut painter) = shown.painter.take() {
            painter.ctx.set_style(ctx.style());
            painter.paint(|ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    window_bar(ui, &mut shown.remote);
                    let area = ui.available_rect_before_wrap();
                    self.draw_views(ui, area, &mut shown.remote, &mut shown.view, Feed::Pictures);
                });
            });
            shown.painter = Some(painter);
            ctx.request_repaint();
        }
        self.surface.display = Some(shown);
    }

    pub(super) fn surface_windows(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "android")]
        self.second_display(ctx);
        let mut windows = std::mem::take(&mut self.surface.windows);
        let now = self.second.now();
        windows.retain_mut(|window| {
            window.remote.step(now);
            if matches!(window.remote.phase(), Phase::Ended(_)) {
                return false;
            }
            let title = format!("Citrine: {}", window.remote.mode().label());
            let builder = egui::ViewportBuilder::default()
                .with_title(title.clone())
                .with_inner_size(WINDOW_SIZE);
            let mut keep = true;
            ctx.show_viewport_immediate(window.id, builder, |ctx, class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    keep = false;
                }
                if class == egui::ViewportClass::Embedded {
                    egui::Window::new(title.as_str())
                        .id(egui::Id::new(window.id))
                        .default_size(WINDOW_SIZE)
                        .open(&mut keep)
                        .show(ctx, |ui| self.window_contents(ui, window));
                } else {
                    egui::CentralPanel::default().show(ctx, |ui| self.window_contents(ui, window));
                }
            });
            if !keep {
                window.remote.leave();
            }
            keep
        });
        windows.append(&mut self.surface.windows);
        self.surface.windows = windows;
    }

    fn window_contents(&mut self, ui: &mut egui::Ui, window: &mut SurfaceWindow) {
        window_bar(ui, &mut window.remote);
        let area = ui.available_rect_before_wrap();
        self.draw_views(
            ui,
            area,
            &mut window.remote,
            &mut window.view,
            Feed::Emulator,
        );
    }

    pub(super) fn render_surface(&mut self, ctx: &egui::Context) {
        let Some(mut remote) = self.second.joining_mut().and_then(|j| j.take_remote()) else {
            return;
        };
        let mut view = std::mem::take(&mut self.surface.joined);
        let screens = remote.game().map(|g| g.screens.clone()).unwrap_or_default();
        let lost = remote.phase() == Phase::Lost;
        let mut leave = false;
        egui::TopBottomPanel::top("surface_top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let side = ui.spacing().interact_size.y.max(36.0);
                if ui
                    .add(super::mobile::kit::header_button(icons::ARROW_LEFT, side))
                    .on_hover_text("Leave")
                    .clicked()
                {
                    leave = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if matches!(remote.mode(), Mode::Custom(_))
                        && ui
                            .button(format!("{} Choose views", icons::SLIDERS_HORIZONTAL))
                            .clicked()
                    {
                        view.choosing = true;
                    }
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        let title = remote.game().map_or("Second screen", |g| g.title.as_str());
                        ui.add(
                            egui::Label::new(RichText::new(title).size(20.0).strong()).truncate(),
                        );
                        let line = if lost {
                            format!("{} Reconnecting…", icons::ARROWS_CLOCKWISE)
                        } else if remote.phase() == Phase::Joining {
                            "Connecting…".to_owned()
                        } else {
                            remote.host_name().to_owned()
                        };
                        ui.add(egui::Label::new(RichText::new(line).weak()).truncate());
                    });
                });
            });
        });
        let presets: Vec<Preset> = Preset::ALL
            .into_iter()
            .filter(|p| p.offered(&screens))
            .collect();
        let mut entries: Vec<frame::TaskEntry> = presets
            .iter()
            .map(|p| frame::TaskEntry {
                icon: preset_icon(*p),
                label: p.label(),
                selected: *remote.mode() == Mode::Preset(*p),
            })
            .collect();
        entries.push(frame::TaskEntry {
            icon: icons::SLIDERS_HORIZONTAL,
            label: "Custom",
            selected: matches!(remote.mode(), Mode::Custom(_)),
        });
        let fill = super::mobile::kit::card_fill(&ctx.style().visuals);
        let picked = egui::TopBottomPanel::bottom("surface_modes")
            .exact_height(frame::bar_height(&ctx.style()))
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(fill))
            .show(ctx, |ui| frame::task_bar(ui, &entries))
            .inner;
        let custom_views = self
            .second
            .joining()
            .map(|j| j.custom().to_vec())
            .unwrap_or_default();
        if let Some(index) = picked {
            let mode = match presets.get(index) {
                Some(preset) => Mode::Preset(*preset),
                None => {
                    if custom_views.is_empty() && !view.asked {
                        view.choosing = true;
                        view.asked = true;
                    }
                    Mode::Custom(custom_views.clone())
                }
            };
            remote.set_mode(mode.clone());
            if let Some(joining) = self.second.joining_mut() {
                joining.remember(&mode);
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            let area = ui.available_rect_before_wrap();
            self.draw_views(ui, area, &mut remote, &mut view, Feed::Pictures);
        });
        if view.choosing {
            let shown = sheet::show(
                ctx,
                egui::Id::new("surface_views"),
                "Show on this phone",
                |ui| {
                    let custom = matches!(remote.mode(), Mode::Custom(_));
                    for (kind, label) in choosable(&screens, false) {
                        let mut on = custom && remote.mode().views(&screens).contains(&kind);
                        if super::mobile::kit::SwitchRow::new(label, &mut on).show(ui) {
                            let base = if custom {
                                remote.mode().clone()
                            } else {
                                Mode::Custom(Vec::new())
                            };
                            let mode = base.toggled(&screens, kind);
                            remote.set_mode(mode.clone());
                            if let Some(joining) = self.second.joining_mut() {
                                joining.remember(&mode);
                            }
                        }
                    }
                },
            );
            if shown.closed {
                view.choosing = false;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && view.choosing {
            view.choosing = false;
        }
        self.surface.joined = view;
        if let Some(joining) = self.second.joining_mut() {
            joining.put_remote(remote);
        }
        if leave {
            self.second.leave();
        }
    }

    fn draw_views(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        remote: &mut Remote,
        view: &mut ViewState,
        feed: Feed,
    ) {
        let game = remote.game().cloned();
        let screens = game.as_ref().map(|g| g.screens.clone()).unwrap_or_default();
        let views: Vec<View> = remote.mode().views(&screens);
        let views: Vec<View> = match feed {
            Feed::Pictures => views.into_iter().filter(View::travels).collect(),
            Feed::Emulator => views,
        };
        if game.is_none() {
            ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                ui.centered_and_justified(|ui| ui.label(RichText::new("No game running").weak()));
            });
            remote.touch(None);
            remote.buttons(0);
            return;
        }
        if views.is_empty() {
            remote.touch(None);
            remote.buttons(0);
            return;
        }
        let dim = remote.phase() == Phase::Lost;
        let mut touch = None;
        let mut buttons = Keys::empty();
        let mut controller = false;
        for (kind, rect) in split(area, &views, &screens) {
            match kind {
                View::Screen(index) => {
                    let Some(shape) = screens.get(usize::from(index)).copied() else {
                        continue;
                    };
                    match feed {
                        Feed::Emulator => {
                            ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                                self.emulator.ui_screen(ui, Some(usize::from(index)));
                            });
                        }
                        Feed::Pictures => {
                            if let Some(t) =
                                picture_screen(ui, rect, remote, view, index, shape, dim)
                            {
                                touch = Some(t);
                            }
                        }
                    }
                }
                View::Controller => {
                    controller = true;
                    let shoulders = game.as_ref().is_some_and(|g| {
                        !matches!(g.system.as_str(), "Game Boy" | "Game Boy Color")
                    });
                    let xy = game.as_ref().is_some_and(|g| g.system == "Nintendo DS");
                    let layout =
                        TouchLayout::bar(rect, &self.ui.settings.touch, shoulders).with_xy(xy);
                    layout.claim(ui);
                    let held = view.touch.update(ui, &layout);
                    layout.paint(ui, held, &self.ui.settings.touch, &[]);
                    buttons = held;
                }
                View::GameInfo => {
                    if let Some(game) = &game {
                        self.game_info_card(ui, rect, game, remote, view);
                    }
                }
                View::Pane(name) => {
                    if let Some(mut tab) = PANES.into_iter().find(|t| t.label() == name) {
                        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                            self.viewer_ui(ui, &mut tab);
                        });
                    }
                }
            }
        }
        if !controller {
            let _ = view.touch.reset();
        }
        remote.touch(touch);
        remote.buttons(buttons.bits());
    }

    fn game_info_card(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        game: &GameCard,
        remote: &mut Remote,
        view: &mut ViewState,
    ) {
        let icon = game
            .icon
            .as_ref()
            .filter(|icon| icon.is_whole())
            .map(|icon| {
                let fresh = view
                    .icon
                    .as_ref()
                    .is_none_or(|(title, _)| *title != game.title);
                if fresh {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [usize::from(icon.width), usize::from(icon.height)],
                        &icon.rgba,
                    );
                    let texture =
                        ui.ctx()
                            .load_texture("surface_icon", image, egui::TextureOptions::NEAREST);
                    view.icon = Some((game.title.clone(), texture));
                }
                view.icon.as_ref().map(|(_, t)| t.id())
            });
        let card = rect.shrink(12.0);
        ui.scope_builder(UiBuilder::new().max_rect(card), |ui| {
            super::mobile::kit::card(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if let Some(Some(id)) = icon {
                        ui.add(egui::Image::new((id, vec2(64.0, 64.0))).corner_radius(8));
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&game.title).strong().size(20.0));
                        ui.label(RichText::new(&game.system).weak());
                    });
                });
                ui.add_space(10.0);
                egui::Grid::new("surface_info")
                    .num_columns(2)
                    .spacing([24.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Playing for").weak());
                        ui.label(played(game.played_secs));
                        ui.end_row();
                        ui.label(RichText::new("Connection").weak());
                        ui.label(remote.route().label());
                        ui.end_row();
                    });
                ui.add_space(12.0);
                ui.columns(2, |columns| {
                    let (icon, label, action) = if game.paused {
                        (icons::PLAY, "Resume", Action::Resume)
                    } else {
                        (icons::PAUSE, "Pause", Action::Pause)
                    };
                    if columns[0]
                        .add_sized(
                            [columns[0].available_width(), 40.0],
                            egui::Button::new(format!("{icon} {label}")),
                        )
                        .clicked()
                    {
                        remote.action(action);
                    }
                    if columns[1]
                        .add_sized(
                            [columns[1].available_width(), 40.0],
                            egui::Button::new(format!("{} Save state", icons::SAVE_STATE)),
                        )
                        .clicked()
                    {
                        remote.action(Action::SaveState);
                    }
                });
                ui.add_space(6.0);
            });
        });
    }

    fn viewer_ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        let mut viewer = self.viewer();
        egui_dock::TabViewer::ui(&mut viewer, ui, tab);
    }
}

fn picture_screen(
    ui: &mut Ui,
    rect: Rect,
    remote: &Remote,
    view: &mut ViewState,
    index: u8,
    shape: ScreenShape,
    dim: bool,
) -> Option<citrine_gb_surface::touch::ScreenTouch> {
    let at = usize::from(index);
    if view.textures.len() <= at {
        view.textures.resize_with(at + 1, || None);
    }
    if let Some(picture) = remote.picture(index) {
        let stale = view.textures[at]
            .as_ref()
            .is_none_or(|(seq, _)| *seq != picture.seq);
        if stale {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [usize::from(picture.width), usize::from(picture.height)],
                &picture.rgba,
            );
            match &mut view.textures[at] {
                Some((seq, texture)) => {
                    texture.set(image, egui::TextureOptions::NEAREST);
                    *seq = picture.seq;
                }
                None => {
                    let texture = ui.ctx().load_texture(
                        format!("surface_screen_{at}"),
                        image,
                        egui::TextureOptions::NEAREST,
                    );
                    view.textures[at] = Some((picture.seq, texture));
                }
            }
        }
    }
    let size = vec2(f32::from(shape.width), f32::from(shape.height));
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let shown = Rect::from_center_size(rect.center(), size * scale);
    let response = ui.interact(
        shown,
        ui.id().with(("surface_screen", index)),
        Sense::click_and_drag(),
    );
    if let Some((_, texture)) = &view.textures[at] {
        let tint = if dim {
            Color32::from_gray(110)
        } else {
            Color32::WHITE
        };
        ui.painter().image(
            texture.id(),
            shown,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            tint,
        );
    } else {
        ui.painter()
            .rect_filled(shown, CornerRadius::ZERO, ui.visuals().extreme_bg_color);
        ui.painter().text(
            shown.center(),
            Align2::CENTER_CENTER,
            icons::HOURGLASS,
            FontId::proportional(28.0),
            ui.visuals().weak_text_color(),
        );
    }
    if !response.is_pointer_button_down_on() || dim {
        return None;
    }
    let pos = response.interact_pointer_pos()?;
    let area = Area {
        left: shown.left(),
        top: shown.top(),
        width: shown.width(),
        height: shown.height(),
    };
    touch_in(area, 0, index, shape, pos.x, pos.y)
}

fn window_bar(ui: &mut Ui, remote: &mut Remote) {
    let screens = remote.game().map(|g| g.screens.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        for preset in Preset::ALL.into_iter().filter(|p| p.offered(&screens)) {
            let on = *remote.mode() == Mode::Preset(preset);
            if ui
                .selectable_label(on, format!("{} {}", preset_icon(preset), preset.label()))
                .clicked()
            {
                remote.set_mode(Mode::Preset(preset));
            }
        }
        let custom = matches!(remote.mode(), Mode::Custom(_));
        ui.menu_button(format!("{} Custom", icons::SLIDERS_HORIZONTAL), |ui| {
            for (view_kind, label) in choosable(&screens, true) {
                let mut shown = custom && remote.mode().views(&screens).contains(&view_kind);
                if ui.checkbox(&mut shown, label).changed() {
                    let base = if custom {
                        remote.mode().clone()
                    } else {
                        Mode::Custom(Vec::new())
                    };
                    remote.set_mode(base.toggled(&screens, view_kind));
                }
            }
        })
        .response
        .on_hover_text("Choose views");
    });
    ui.separator();
}

fn played(secs: u64) -> String {
    let minutes = secs / 60;
    match minutes {
        0 => "Under a minute".to_owned(),
        1..=59 => format!("{minutes} min"),
        _ => format!("{} h {} min", minutes / 60, minutes % 60),
    }
}

pub fn preset_icon(preset: Preset) -> &'static str {
    match preset {
        Preset::BottomScreen => icons::SQUARE_HALF_BOTTOM,
        Preset::Controller => icons::GAME_CONTROLLER,
        Preset::GameInfo => icons::INFO,
    }
}

pub fn choosable(screens: &[ScreenShape], panes: bool) -> Vec<(View, &'static str)> {
    let mut out = Vec::new();
    let two = screens.len() > 1;
    for (at, _) in screens.iter().enumerate() {
        let Ok(index) = u8::try_from(at) else {
            continue;
        };
        let label = match (two, at) {
            (false, _) => "Game screen",
            (true, 0) => "Top screen",
            (true, _) => "Bottom screen",
        };
        out.push((View::Screen(index), label));
    }
    out.push((View::Controller, "Controller"));
    out.push((View::GameInfo, "Game info"));
    if panes {
        for tab in PANES {
            out.push((View::Pane(tab.label().to_owned()), tab.label()));
        }
    }
    out
}

pub fn row_line(row: &SurfaceRow) -> String {
    match row.presence {
        Presence::Reconnecting => "Reconnecting…".to_owned(),
        Presence::Connected => match (row.route, row.ping_ms) {
            (Route::InApp, _) => Route::InApp.label().to_owned(),
            (route, Some(ms)) => format!("{}, {ms} ms", route.label()),
            (route, None) => route.label().to_owned(),
        },
    }
}

fn split(area: Rect, views: &[View], screens: &[ScreenShape]) -> Vec<(View, Rect)> {
    if views.len() == 1 {
        return vec![(views[0].clone(), area)];
    }
    if area.width() > area.height() * 1.25 {
        let width = area.width() / views.len() as f32;
        return views
            .iter()
            .enumerate()
            .map(|(at, v)| {
                let left = area.left() + width * at as f32;
                (
                    v.clone(),
                    Rect::from_min_size(pos2(left, area.top()), vec2(width, area.height())),
                )
            })
            .collect();
    }
    let want = |view: &View| match view {
        View::Screen(index) => screens.get(usize::from(*index)).map_or(0.0, |s| {
            area.width() * f32::from(s.height) / f32::from(s.width.max(1))
        }),
        View::Controller => CONTROLLER_MOST.min(area.height() * 0.5),
        View::GameInfo => INFO_HEIGHT,
        View::Pane(_) => 0.0,
    };
    let panes = views.iter().filter(|v| matches!(v, View::Pane(_))).count();
    let fixed: f32 = views.iter().map(want).sum();
    let pane_room = if panes > 0 {
        (area.height() - fixed).max(area.height() * 0.3) / panes as f32
    } else {
        0.0
    };
    let total = fixed + pane_room * panes as f32;
    let scale = if total > area.height() {
        area.height() / total
    } else {
        1.0
    };
    let gap = if panes == 0 && total < area.height() {
        (area.height() - total) / (views.len() + 1) as f32
    } else {
        0.0
    };
    let mut top = area.top() + gap;
    views
        .iter()
        .map(|v| {
            let height = match v {
                View::Pane(_) => pane_room,
                other => want(other),
            } * scale;
            let rect = Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), height));
            top += height + gap;
            (v.clone(), rect)
        })
        .collect()
}

#[cfg(feature = "nds")]
fn banner_icon(rom: &[u8]) -> Option<Icon> {
    let banner = crate::ds_banner::banner(rom)?;
    let rgba = citrine_gb::rom::nds::icon_rgba(&banner.icon)?;
    Some(Icon {
        width: 32,
        height: 32,
        rgba,
    })
}

#[cfg(not(feature = "nds"))]
fn banner_icon(_rom: &[u8]) -> Option<Icon> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: [ScreenShape; 2] = [
        ScreenShape {
            width: 256,
            height: 192,
            touch: false,
        },
        ScreenShape {
            width: 256,
            height: 192,
            touch: true,
        },
    ];

    fn area(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h))
    }

    #[test]
    fn one_view_takes_the_whole_area() {
        let parts = split(area(412.0, 700.0), &[View::Screen(1)], &DS);
        assert_eq!(parts, vec![(View::Screen(1), area(412.0, 700.0))]);
    }

    #[test]
    fn a_screen_and_the_controller_stack_without_overlap() {
        let parts = split(
            area(412.0, 760.0),
            &[View::Screen(1), View::Controller],
            &DS,
        );
        assert_eq!(parts.len(), 2);
        let (screen, pad) = (parts[0].1, parts[1].1);
        assert!((screen.height() - 309.0).abs() < 1.0, "{screen:?}");
        assert!(screen.bottom() <= pad.top() + 0.01);
        assert!(pad.bottom() <= 760.01);
        assert!((pad.height() - CONTROLLER_MOST.min(380.0)).abs() < 1.0);
    }

    #[test]
    fn too_many_views_shrink_to_fit() {
        let views = [
            View::Screen(0),
            View::Screen(1),
            View::Controller,
            View::GameInfo,
        ];
        let parts = split(area(412.0, 600.0), &views, &DS);
        let bottom = parts.last().unwrap().1.bottom();
        assert!(bottom <= 600.01, "{bottom}");
        assert!(
            parts
                .windows(2)
                .all(|w| w[0].1.bottom() <= w[1].1.top() + 0.01)
        );
    }

    #[test]
    fn wide_areas_lay_views_side_by_side() {
        let parts = split(
            area(1200.0, 500.0),
            &[View::Screen(0), View::Screen(1)],
            &DS,
        );
        assert_eq!(parts[0].1, area(600.0, 500.0));
        assert_eq!(parts[1].1.left(), 600.0);
    }

    #[test]
    fn panes_take_the_room_that_is_left() {
        let views = [View::Screen(1), View::Pane("Saves".into())];
        let parts = split(area(500.0, 900.0), &views, &DS);
        assert!((parts[1].1.height() - (900.0 - 375.0)).abs() < 1.0);
    }

    #[test]
    fn links_carry_the_code() {
        assert_eq!(
            join_code_from_link("https://gb.lemon.industries/#screen=482913"),
            Code::parse("482913")
        );
        assert_eq!(join_code_from_link("https://gb.lemon.industries/"), None);
        assert_eq!(join_code_from_link("#screen=12"), None);
    }

    #[test]
    fn row_lines_name_the_route_and_ping() {
        let row = SurfaceRow {
            id: SurfaceId(1),
            name: "Phone".into(),
            kind: SurfaceKind::Device,
            mode: Mode::default(),
            presence: Presence::Connected,
            route: Route::Network,
            ping_ms: Some(4),
        };
        assert_eq!(row_line(&row), "Same network, 4 ms");
        let lost = SurfaceRow {
            presence: Presence::Reconnecting,
            ..row.clone()
        };
        assert_eq!(row_line(&lost), "Reconnecting…");
    }

    #[test]
    fn the_custom_list_names_screens_by_place() {
        let names: Vec<_> = choosable(&DS, false).into_iter().map(|(_, n)| n).collect();
        assert_eq!(
            names,
            ["Top screen", "Bottom screen", "Controller", "Game info"]
        );
        let one = [DS[0]];
        let names: Vec<_> = choosable(&one, true).into_iter().map(|(_, n)| n).collect();
        assert_eq!(names[0], "Game screen");
        assert!(names.contains(&"Saves & snapshots"));
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    mod two_apps {
        use super::super::super::tests::phone_app;
        use super::*;
        use crate::second_screens::Reach;
        use citrine_gb_surface::touch::ScreenTouch;
        use std::net::SocketAddr;
        use std::time::{Duration, Instant};

        const ROM: &[u8] = include_bytes!("../../fixtures/nds/blocksds-console-both-screens.nds");

        fn tick(host: &mut Citrine, phone: &mut Citrine) {
            host.emulator.lend_one_frame();
            host.emulator.total_frames += 1;
            host.second.step();
            host.second_screens_input();
            host.publish_second_screens();
            phone.second.step();
        }

        fn until(
            host: &mut Citrine,
            phone: &mut Citrine,
            mut done: impl FnMut(&mut Citrine, &mut Citrine) -> bool,
        ) -> Option<Duration> {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(20) {
                tick(host, phone);
                if done(host, phone) {
                    return Some(start.elapsed());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            None
        }

        fn remote(app: &mut Citrine) -> &mut Remote {
            app.second
                .joining_mut()
                .and_then(|j| j.remote_mut())
                .expect("a link")
        }

        fn shown_seq(app: &Citrine) -> Option<u32> {
            app.second.joining()?.remote()?.picture(1).map(|p| p.seq)
        }

        #[test]
        fn a_phone_touch_reaches_the_ds_core_over_a_real_socket() {
            let mut host = phone_app("second-screens-host");
            host.emulator.load_nds(ROM).expect("the DS fixture loads");
            host.emulator.running = true;
            host.publish_second_screens();
            let both = host.screens_chip().expect("a DS chip");
            assert!(!both.contains("Top only"), "{both}");
            host.second.discovery_port = 0;
            let code = host.second.add_device();
            let port = host.second.lan_discovery_port().expect("discovery is open");
            let mut phone = phone_app("second-screens-phone");
            phone.second.discovery_targets = Some(vec![SocketAddr::from(([127, 0, 0, 1], port))]);
            phone.second.join(code);

            let joined = until(&mut host, &mut phone, |_, phone| shown_seq(phone).is_some());
            assert!(
                joined.is_some(),
                "the phone joins over the LAN and shows a frame"
            );
            let phone_remote = remote(&mut phone);
            assert_eq!(phone_remote.route(), Route::Network);
            assert_eq!(phone_remote.mode(), &Mode::Preset(Preset::BottomScreen));
            assert_eq!(host.emulator.screen_away, None);
            host.step_second_screens_for_tests();
            assert_eq!(
                host.emulator.screen_away,
                Some(1),
                "the PC keeps the top screen"
            );
            let lent = host.screens_chip().expect("a DS chip");
            assert!(lent.contains("Top only"), "{lent}");

            let before = shown_seq(&phone).expect("a frame");
            let frame = until(&mut host, &mut phone, |_, phone| {
                shown_seq(phone).is_some_and(|seq| seq != before)
            })
            .expect("frames keep coming");

            assert!(!host.emulator.pen_down());
            let pressed = Instant::now();
            remote(&mut phone).touch(Some(ScreenTouch {
                screen: 1,
                x: 128,
                y: 96,
            }));
            let reached = until(&mut host, &mut phone, |host, _| host.emulator.pen_down())
                .expect("the touch reaches the DS core");
            let touch = pressed.elapsed().min(reached);
            let ticked = Instant::now();
            tick(&mut host, &mut phone);
            println!(
                "second screens latency: frame to phone {:.1} ms, touch to core {:.1} ms, one emulated frame {:.1} ms",
                frame.as_secs_f64() * 1000.0,
                touch.as_secs_f64() * 1000.0,
                ticked.elapsed().as_secs_f64() * 1000.0
            );

            remote(&mut phone).cut();
            let released = until(&mut host, &mut phone, |host, _| !host.emulator.pen_down());
            assert!(released.is_some(), "a lost phone lets go of the stylus");
            let rows = host.second.surfaces();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].presence, Presence::Reconnecting);
            assert!(host.emulator.running, "the game keeps running");
            let frames = host.emulator.total_frames;

            let back = until(&mut host, &mut phone, |host, _| {
                host.second
                    .surfaces()
                    .first()
                    .is_some_and(|row| row.presence == Presence::Connected)
            });
            assert!(back.is_some(), "the phone comes back with its token");
            assert!(host.emulator.total_frames > frames);
            assert_eq!(
                remote(&mut phone).mode(),
                &Mode::Preset(Preset::BottomScreen)
            );

            phone.second.leave();
            let gone = until(&mut host, &mut phone, |host, _| {
                host.second.surfaces().is_empty()
            });
            assert!(gone.is_some(), "leaving removes the phone at once");
            assert!(host.emulator.running);
            host.step_second_screens_for_tests();
            assert_eq!(host.emulator.screen_away, None);
            assert_eq!(host.screens_chip(), Some(both));
        }

        #[test]
        fn a_lent_bottom_screen_shows_its_picture_on_the_phone() {
            let mut host = phone_app("second-screens-lent-host");
            host.emulator.load_nds(ROM).expect("the DS fixture loads");
            host.emulator.running = true;
            host.publish_second_screens();
            host.second.discovery_port = 0;
            let code = host.second.add_device();
            let port = host.second.lan_discovery_port().expect("discovery is open");
            let mut phone = phone_app("second-screens-lent-phone");
            phone.second.discovery_targets = Some(vec![SocketAddr::from(([127, 0, 0, 1], port))]);
            phone.second.join(code);
            until(&mut host, &mut phone, |_, phone| shown_seq(phone).is_some())
                .expect("the phone joins");
            host.step_second_screens_for_tests();
            assert_eq!(host.emulator.screen_away, Some(1));

            let before = shown_seq(&phone).expect("a frame");
            until(&mut host, &mut phone, |_, phone| {
                shown_seq(phone).is_some_and(|seq| seq >= before + 4)
            })
            .expect("frames keep coming");
            let picture = phone
                .second
                .joining()
                .and_then(|j| j.remote())
                .and_then(|r| r.picture(1))
                .expect("a bottom screen picture");
            assert_eq!((picture.width, picture.height), (256, 192));
            let lit = picture
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[..3] != [0, 0, 0])
                .count();
            assert!(lit > 0, "the phone's bottom screen is black");
        }

        #[test]
        fn a_phone_finds_a_host_that_cannot_answer_discovery() {
            let mut host = phone_app("second-screens-quiet-host");
            host.emulator.load_nds(ROM).expect("the DS fixture loads");
            host.emulator.running = true;
            host.publish_second_screens();
            host.second.discovery_port = 0;
            let code = host.second.add_device();
            let port = host.second.lan_port().expect("the host listens");
            let mut other = phone_app("second-screens-other-host");
            other.second.discovery_port = 0;
            other.second.add_device();
            let other_port = other.second.lan_port().expect("the other host listens");
            let quiet = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let mut phone = phone_app("second-screens-sweeping-phone");
            phone.second.discovery_targets = Some(vec![quiet.local_addr().unwrap()]);
            let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
            phone.second.set_spawner(threads.spawner());
            phone.second.sweep_targets = Some(vec![
                SocketAddr::from(([127, 0, 0, 1], other_port)),
                SocketAddr::from(([127, 0, 0, 1], port)),
            ]);
            phone.second.join(code);
            let joined = until(&mut host, &mut phone, |_, phone| {
                other.second.step();
                shown_seq(phone).is_some()
            });
            assert!(
                joined.is_some(),
                "the phone joins without a discovery answer"
            );
            assert_eq!(remote(&mut phone).route(), Route::Network);
            assert_eq!(host.second.surfaces().len(), 1);
        }

        fn nearby_pair(host_name: &str) -> (Citrine, Citrine) {
            let mut host = phone_app(host_name);
            host.emulator.load_nds(ROM).expect("the DS fixture loads");
            host.emulator.running = true;
            host.publish_second_screens();
            host.second.discovery_port = 0;
            host.second.advertise = true;
            host.second.step();
            let port = host
                .second
                .lan_discovery_port()
                .expect("the host answers nearby probes");
            let mut phone = phone_app(&format!("{host_name}-phone"));
            phone.second.nearby_targets = Some(vec![SocketAddr::from(([127, 0, 0, 1], port))]);
            (host, phone)
        }

        fn first_suggestion(host: &mut Citrine, phone: &mut Citrine) -> Suggestion {
            let mut found = None;
            until(host, phone, |_, phone| {
                phone.second.want_suggestions();
                found = phone.second.suggestions().into_iter().next();
                found.is_some()
            })
            .expect("the phone lists the host");
            found.expect("a suggestion")
        }

        fn keys_of(seed: u8, device: u64) -> DeviceKeys {
            let keys = citrine_gb_net::RelayId::from_bytes([seed; 32]).keys();
            DeviceKeys {
                door: DoorKey(keys.screen_door(device)),
                proof: ProofKey(keys.screen_proof(device)),
                tag: TagKey(keys.screen_tag_key(device)),
            }
        }

        fn account_of(seed: u8, own: u64, devices: &[(u64, &str)]) -> Account {
            Account {
                keys: keys_of(seed, own),
                devices: devices
                    .iter()
                    .map(|&(id, name)| OwnDevice {
                        name: name.into(),
                        keys: keys_of(seed, id),
                    })
                    .collect(),
            }
        }

        fn account(own: u64, devices: &[(u64, &str)]) -> Account {
            account_of(7, own, devices)
        }

        fn tap(
            to: SocketAddr,
            spawner: &citrine_gb_threads::Spawner,
        ) -> (SocketAddr, std::sync::mpsc::Receiver<(bool, Vec<u8>)>) {
            use std::io::{Read, Write};
            use std::net::{TcpListener, TcpStream};
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let at = listener.local_addr().unwrap();
            let (seen, heard) = std::sync::mpsc::channel();
            let pumps = spawner.clone();
            spawner.run("screen tap", move || {
                let Ok((phone, _)) = listener.accept() else {
                    return;
                };
                let host = TcpStream::connect(to).unwrap();
                let pump = |up: bool, mut from: TcpStream, mut to: TcpStream| {
                    let seen = seen.clone();
                    pumps.run("screen tap pump", move || {
                        let mut buf = [0; 4096];
                        while let Ok(n) = from.read(&mut buf) {
                            if n == 0 || to.write_all(&buf[..n]).is_err() {
                                break;
                            }
                            let _ = seen.send((up, buf[..n].to_vec()));
                        }
                    });
                };
                pump(true, phone.try_clone().unwrap(), host.try_clone().unwrap());
                pump(false, host, phone);
            });
            (at, heard)
        }

        #[test]
        fn a_nearby_host_is_listed_and_joined_after_allow() {
            let (mut host, mut phone) = nearby_pair("second-screens-nearby-host");
            let suggestion = first_suggestion(&mut host, &mut phone);
            assert!(!suggestion.own);
            assert_eq!(suggestion.keys, None);
            phone.second.join_suggestion(&suggestion);
            until(&mut host, &mut phone, |host, _| {
                !host.second.askers().is_empty()
            })
            .expect("the host asks");
            let asker = host.second.askers().remove(0);
            assert_eq!(asker.kind, SurfaceKind::Device);
            assert!(host.second.surfaces().is_empty());
            until(&mut host, &mut phone, |_, phone| {
                phone.second.joining().map(|j| j.state()) == Some(JoinState::Asking)
            })
            .expect("the phone waits for the answer");
            assert_eq!(
                phone.second.joining().and_then(|j| j.host_name()),
                Some(suggestion.name.as_str())
            );
            host.second.allow(asker.id);
            until(&mut host, &mut phone, |_, phone| shown_seq(phone).is_some())
                .expect("frames arrive after allow");
            assert_eq!(remote(&mut phone).route(), Route::Network);
            assert_eq!(host.second.surfaces().len(), 1);
        }

        fn ask_frame(
            app: &mut Citrine,
            ctx: &egui::Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| app.ask_second_screens(ctx))
        }

        #[test]
        fn both_apps_show_one_security_code_and_allow_is_one_tap() {
            use crate::app::tests::{tap, text_rect};
            let (mut host, mut phone) = nearby_pair("second-screens-check-code");
            let suggestion = first_suggestion(&mut host, &mut phone);
            phone.second.join_suggestion(&suggestion);
            until(&mut host, &mut phone, |host, phone| {
                !host.second.askers().is_empty()
                    && phone.second.joining().map(|j| j.state()) == Some(JoinState::Asking)
            })
            .expect("the host asks and the phone waits");
            let asker = host.second.askers().remove(0);
            assert_eq!(
                phone.second.joining().and_then(|j| j.check_code()),
                Some(asker.code)
            );
            let ctx = egui::Context::default();
            ask_frame(&mut host, &ctx, Vec::new());
            let shown = ask_frame(&mut host, &ctx, Vec::new());
            assert!(text_rect(&shown, &asker.code.to_string()).is_some());
            let compare = format!("Allow if {} shows the same digits", asker.name);
            assert!(text_rect(&shown, &compare).is_some());
            let allow = text_rect(&shown, "Allow").expect("an Allow button");
            ask_frame(&mut host, &ctx, tap(allow.center()));
            assert!(host.second.askers().is_empty());
            until(&mut host, &mut phone, |_, phone| shown_seq(phone).is_some())
                .expect("frames arrive after one tap");
            assert_eq!(phone.second.joining().and_then(|j| j.check_code()), None);
        }

        #[test]
        fn a_citrine_without_a_game_is_not_listed() {
            let (mut host, mut phone) = nearby_pair("second-screens-listed-host");
            let mut idle = phone_app("second-screens-idle-host");
            idle.second.discovery_port = 0;
            idle.second.advertise = true;
            idle.second.step();
            let idle_port = idle
                .second
                .lan_discovery_port()
                .expect("the idle app listens");
            phone
                .second
                .nearby_targets
                .as_mut()
                .expect("the phone has targets")
                .push(SocketAddr::from(([127, 0, 0, 1], idle_port)));
            first_suggestion(&mut host, &mut phone);
            for _ in 0..300 {
                idle.second.step();
                tick(&mut host, &mut phone);
                phone.second.want_suggestions();
                std::thread::sleep(Duration::from_millis(1));
            }
            let listed = phone.second.suggestions();
            assert_eq!(listed.len(), 1, "{listed:?}");
        }

        #[test]
        fn not_now_tells_the_phone() {
            let (mut host, mut phone) = nearby_pair("second-screens-not-now-host");
            let suggestion = first_suggestion(&mut host, &mut phone);
            phone.second.join_suggestion(&suggestion);
            until(&mut host, &mut phone, |host, _| {
                !host.second.askers().is_empty()
            })
            .expect("the host asks");
            let asker = host.second.askers().remove(0);
            host.second.decline(asker.id);
            until(&mut host, &mut phone, |_, phone| {
                matches!(
                    phone.second.joining().map(|j| j.state()),
                    Some(JoinState::Failed(_))
                )
            })
            .expect("the phone hears the answer");
            assert_eq!(
                phone.second.joining().map(|j| j.state()),
                Some(JoinState::Failed("The other device said not now"))
            );
            assert!(host.second.surfaces().is_empty());
        }

        #[test]
        fn an_own_device_nearby_joins_with_one_tap() {
            let (mut host, mut phone) = nearby_pair("second-screens-own-host");
            host.second.set_account(Some(account(1, &[(2, "Pixel 8")])));
            phone
                .second
                .set_account(Some(account(2, &[(1, "MacBook")])));
            let suggestion = first_suggestion(&mut host, &mut phone);
            assert!(suggestion.own);
            assert_eq!(suggestion.name, "MacBook");
            assert_eq!(suggestion.keys, Some(keys_of(7, 1)));
            phone.second.join_suggestion(&suggestion);
            until(&mut host, &mut phone, |_, phone| shown_seq(phone).is_some())
                .expect("frames arrive without an ask");
            assert!(host.second.askers().is_empty());
            assert_eq!(host.second.surfaces().len(), 1);
        }

        #[test]
        fn a_stranger_with_a_wrong_key_is_refused_without_a_prompt() {
            let (mut host, mut phone) = nearby_pair("second-screens-wrong-key-host");
            host.second.set_account(Some(account(1, &[])));
            phone
                .second
                .set_account(Some(account_of(9, 5, &[(1, "MacBook")])));
            let mut suggestion = first_suggestion(&mut host, &mut phone);
            assert!(!suggestion.own);
            suggestion.keys = Some(keys_of(9, 1));
            phone.second.join_suggestion(&suggestion);
            until(&mut host, &mut phone, |_, phone| {
                matches!(
                    phone.second.joining().map(|j| j.state()),
                    Some(JoinState::Failed(_))
                )
            })
            .expect("the phone gives up");
            assert_eq!(
                phone.second.joining().map(|j| j.state()),
                Some(JoinState::Failed("Couldn't reach the other device"))
            );
            assert!(host.second.askers().is_empty());
            assert!(host.second.surfaces().is_empty());
        }

        #[test]
        fn the_door_and_proof_keys_never_cross_the_network() {
            let (mut host, mut phone) = nearby_pair("second-screens-tap-host");
            host.second.set_account(Some(account(1, &[(2, "Pixel 8")])));
            phone
                .second
                .set_account(Some(account(2, &[(1, "MacBook")])));
            let mut suggestion = first_suggestion(&mut host, &mut phone);
            assert!(suggestion.own);
            let Reach::Lan(addr) = suggestion.reach else {
                panic!("a nearby host");
            };
            let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
            let (via, heard) = tap(addr, &threads.spawner());
            suggestion.reach = Reach::Lan(via);
            phone.second.join_suggestion(&suggestion);
            until(&mut host, &mut phone, |_, phone| shown_seq(phone).is_some())
                .expect("frames arrive without an ask");
            assert!(host.second.askers().is_empty());
            let (mut up, mut down) = (Vec::new(), Vec::new());
            for (to_host, bytes) in heard.try_iter() {
                let side = if to_host { &mut up } else { &mut down };
                side.extend(bytes);
            }
            assert!(up.len() > 32 && down.len() > 1000, "the tap saw the join");
            let keys = keys_of(7, 1);
            for seen in [&up, &down] {
                assert!(!seen.windows(16).any(|w| w == keys.door.0));
                assert!(!seen.windows(32).any(|w| w == keys.proof.0));
                assert!(!seen.windows(32).any(|w| w == keys.proof.link()));
                assert!(!seen.windows(32).any(|w| w == keys.tag.0));
            }
            let host_name = crate::online::cloud::device_name();
            assert!(host_name.len() >= 4);
            assert!(
                !down
                    .windows(host_name.len())
                    .any(|w| w == host_name.as_bytes())
            );
        }
    }
}
