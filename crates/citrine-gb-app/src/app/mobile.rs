use crate::app::Citrine;
use crate::app::tabs::stats::Page;
use crate::app::tabs::{CONTENT_MARGIN, Tab, TabViewer};
use crate::app::tabs::{linked, playing};
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::settings::{console_name, other_console, plays_on_both};
use crate::app::ui_state::touch::TouchSettings;
use crate::app::widgets::touch_controls::{self as touch, TouchInput, TouchLayout};
use crate::held::InputSource;
use crate::icons;
use crate::local_link::Game;
use citrine_gb::gb::joypad::JoypadState;
use egui::{Context, Rect, Ui, vec2};
use egui_dock::TabViewer as _;

mod history;

const OVERLAY_COLUMN: f32 = 0.28;

const CONTROL_BREATHING_ROOM: f32 = 1.15;

const WATCH_GAP: f32 = 8.0;
const WATCH_CAPTION: f32 = 16.0;
const WATCH_MIN_HEIGHT: f32 = 72.0;

pub const TOUCH_TARGET_PX: f32 = 44.0;

fn bar_icon(icon: &str) -> egui::RichText {
    egui::RichText::new(icon).size(18.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    Menu,
    Panel(Tab),
}

#[derive(Default)]
pub struct MobileState {
    pub sheet: Option<Sheet>,
    came_from: Option<Sheet>,
    touch: TouchInput,
    layer_was_open: bool,
    history: history::BrowserHistory,
}

impl MobileState {
    pub fn open(&mut self, sheet: Sheet) {
        self.came_from = self.sheet.filter(|current| *current != sheet);
        self.sheet = Some(sheet);
    }

    pub fn back(&mut self) {
        self.sheet = self.came_from.take();
    }

    pub fn close(&mut self) {
        self.sheet = None;
        self.came_from = None;
    }

    pub fn listen_for_back(&self) {
        self.history.listen();
    }

    pub fn escape(&mut self, layer_open: bool) -> bool {
        if layer_open || self.sheet.is_none() {
            return false;
        }
        self.back();
        true
    }

    fn depth(&self) -> usize {
        match (self.sheet, self.came_from) {
            (None, _) => 0,
            (Some(_), None) => 1,
            (Some(_), Some(_)) => 2,
        }
    }
}

pub fn touch_spacing(ctx: &Context, touch: bool) {
    let wanted = if touch {
        TOUCH_TARGET_PX * ctx.native_pixels_per_point().unwrap_or(1.0) / ctx.pixels_per_point()
    } else {
        egui::style::Spacing::default().interact_size.y
    };
    if (ctx.style().spacing.interact_size.y - wanted).abs() > 0.01 {
        ctx.style_mut(|style| style.spacing.interact_size.y = wanted);
    }
}

pub fn touch_square(ui: &Ui) -> f32 {
    let height = ui.spacing().interact_size.y;
    if height > egui::style::Spacing::default().interact_size.y {
        height
    } else {
        0.0
    }
}

pub fn compact<'a>(ui: &Ui, button: egui::Button<'a>) -> egui::Button<'a> {
    let side = touch_square(ui);
    if side > 0.0 {
        button.min_size(vec2(side, side))
    } else {
        button.small()
    }
}

pub fn compact_button(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    let button = compact(ui, egui::Button::new(text));
    ui.add(button)
}

pub(crate) fn layer_open(ctx: &Context) -> bool {
    egui::Popup::is_any_open(ctx)
        || ctx.memory(|m| m.top_modal_layer().is_some())
        || ctx.wants_keyboard_input()
        || crate::app::widgets::confirm::is_open(ctx)
}

impl Citrine {
    pub(super) fn render_mobile(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("mobile_bar")
            .show_separator_line(false)
            .show(ctx, |ui| self.mobile_bar(ui));
        let sheet_open = self.mobile.sheet == Some(Sheet::Panel(Tab::Playing));
        let linked = self.emulator.link.is_some() || self.emulator.partner.is_some();
        let asked = playing::pending_ask(&self.play).is_some();
        if !sheet_open
            && (self.play.is_live() || self.ui.playing.ended.is_some() || linked || asked)
        {
            egui::TopBottomPanel::top("play_strip")
                .show_separator_line(false)
                .show(ctx, |ui| self.play_strip(ui));
        }
        self.toast_strip(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| match self.mobile.sheet {
                Some(sheet) => self.mobile_sheet(ui, sheet),
                None => self.mobile_play(ui),
            });
    }

    fn mobile_bar(&mut self, ui: &mut Ui) {
        let side = ui.spacing().interact_size.y;
        let size = egui::vec2(side, side);
        ui.add_space(3.0);

        ui.horizontal(|ui| {
            if self.game_info_details_shown() {
                let back = format!("{} {}", icons::ARROW_LEFT, self.game_info.view.page.label());
                if ui
                    .add(egui::Button::new(egui::RichText::new(back).size(16.0)).min_size(size))
                    .clicked()
                {
                    self.game_info.view.details = false;
                    self.release_touch();
                }
            } else {
                self.mobile_menu_and_title(ui, size);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let controls = self.ui.settings.touch.show_controls;
                let icon = if controls {
                    icons::GAME_CONTROLLER
                } else {
                    icons::EYE_SLASH
                };
                if ui
                    .add_sized(size, egui::Button::new(bar_icon(icon)))
                    .on_hover_text("Show or hide the on-screen buttons")
                    .clicked()
                {
                    self.ui.settings.touch.show_controls = !controls;
                    self.ui.settings.dirty = true;
                    self.release_touch();
                }

                let running = self.emulator.running;
                let can_play = self.emulator.gb.cartridge.has_rom_loaded;
                let play = if running { icons::PAUSE } else { icons::PLAY };
                if ui
                    .add_enabled(can_play, egui::Button::new(bar_icon(play)).min_size(size))
                    .clicked()
                {
                    self.run_hotkey(HotkeyAction::TogglePause);
                }

                #[cfg(target_arch = "wasm32")]
                if ui
                    .add_sized(size, egui::Button::new(bar_icon(icons::FRAME_CORNERS)))
                    .on_hover_text("Fullscreen")
                    .clicked()
                {
                    toggle_fullscreen();
                }

                if self.emulator.screen_recorder.is_recording()
                    && self.recording_button(ui, Some(size)).clicked()
                {
                    self.run_hotkey(HotkeyAction::ToggleRecording);
                }
            });
        });
        ui.add_space(3.0);
    }

    fn game_info_details_shown(&self) -> bool {
        self.mobile.sheet == Some(Sheet::Panel(Tab::GameInfo)) && self.game_info.details_open()
    }

    fn mobile_menu_and_title(&mut self, ui: &mut Ui, size: egui::Vec2) {
        let open = self.mobile.sheet.is_some();
        let menu_icon = if open { icons::ARROW_LEFT } else { icons::LIST };
        let menu = ui.add_sized(size, egui::Button::new(bar_icon(menu_icon)));
        if !open && self.friends_waiting() {
            crate::app::widgets::avatar::status_dot(ui, menu.rect.shrink(6.0));
        }
        if menu.clicked() {
            if open && self.chat_open() {
                self.ui.friends.chat.close();
            } else if open && self.events_open() {
                self.ui.linked.listing = false;
            } else if open && self.stats_game().is_some() {
                self.ui.stats.page = Page::Overview;
            } else if open {
                self.mobile.back();
            } else {
                self.mobile.open(Sheet::Menu);
            }
            self.release_touch();
        }

        if let Some(name) = self.stats_game() {
            let room = ui.available_width() - 3.0 * (size.x + ui.spacing().item_spacing.x);
            ui.scope(|ui| {
                ui.set_max_width(room.max(0.0));
                ui.add(egui::Label::new(name).truncate());
            });
            return;
        }
        match (self.mobile.sheet, self.play.session()) {
            (Some(Sheet::Panel(Tab::Playing)), Some(session)) => {
                let name = playing::friend_name(&self.online, session.friend());
                ui.label(format!("Playing with {name}"));
            }
            (Some(Sheet::Panel(Tab::Playing)), None) => {
                ui.add(
                    egui::Label::new(linked::title(&self.emulator, self.ui.linked.listing))
                        .truncate(),
                );
            }
            _ => {
                ui.label(header_text(self.mobile.sheet, self.emulator.game_name()));
            }
        }
    }

    fn release_touch(&mut self) {
        let stuck = self.mobile.touch.reset();
        for button in ALL_BUTTONS {
            if stuck.contains(button) {
                self.emulator.release(InputSource::Touch, button);
            }
        }
    }

    fn mobile_play(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        let settings = &self.ui.settings;
        let watching = self.play.session().is_some_and(|s| s.watching_only());

        let (screen, layout, free) = if !settings.touch.show_controls || watching {
            (area, None, area)
        } else if area.width() >= area.height() {
            let (screen, left, right) = landscape_split(area, settings.touch.landscape);
            (
                screen,
                Some(TouchLayout::columns(left, right, &settings.touch)),
                Rect::from_min_max(egui::pos2(left.right(), area.top()), right.left_bottom()),
            )
        } else {
            let (screen, controls) = portrait_split(area, &settings.touch);
            (
                screen,
                Some(TouchLayout::bar(controls, &settings.touch)),
                screen,
            )
        };
        let portrait = layout.is_some() && area.width() < area.height();
        self.ui.toast_area = Some(free);

        self.paint_screen(ui, screen, portrait);

        let Some(layout) = layout else {
            self.release_touch();
            return;
        };

        touch::claim(ui, layout.bounds());

        let held = self.mobile.touch.update(ui, &layout);
        let (pressed, released) = self.mobile.touch.transitions(held);

        if !pressed.is_empty() && self.ui.settings.touch.haptics {
            touch::vibrate(8);
        }

        for button in ALL_BUTTONS {
            if pressed.contains(button) {
                self.emulator.press(InputSource::Touch, button);
            } else if released.contains(button) {
                self.emulator.release(InputSource::Touch, button);
            }
        }

        layout.paint(ui, held, &self.ui.settings.touch);
    }

    fn paint_screen(&mut self, ui: &mut Ui, area: Rect, portrait: bool) {
        if let Some(session) = self.play.session()
            && let Some((got, total)) = playing::receiving(session)
        {
            let title = playing::game_title(session);
            let name = playing::friend_name(&self.online, session.friend());
            playing::arriving(ui, area, &title, &name, got, total);
            return;
        }
        let linked = self.play.session().is_some_and(|s| s.runs_engine());
        if let Some(session) = self.play.session()
            && let Some(line) = playing::screen_note(
                &playing::friend_name(&self.online, session.friend()),
                session,
            )
        {
            let copy = playing::needs_copy(session).then_some("Open my copy");
            if playing::note_over(ui, area, &line, copy) {
                self.ui.playing.wants_copy = true;
                self.release_touch();
            }
            return;
        }
        if !self.emulator.gb.cartridge.has_rom_loaded && !linked {
            self.mobile_empty_state(ui, area);
            return;
        }
        if self.emulator.link.is_some() {
            self.paint_linked(ui, area, portrait);
            return;
        }
        if self.emulator.partner.is_some() && portrait {
            let (main, small) = watch_split(area);
            self.emulator.ui_at(ui, main);
            if let Some(small) = small
                && let Some(partner) = &self.emulator.partner
            {
                let sprite = linked::partner::sprite(&self.ui.linked.events, partner);
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(small.shrink2(egui::vec2(8.0, 4.0)))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                linked::partner::status(&mut child, partner, sprite.as_ref());
            }
            return;
        }
        if portrait
            && let Some(session) = self
                .play
                .session()
                .filter(|s| playing::watching(s) && !s.watching_only())
        {
            let (main, friend) = watch_split(area);
            let Some(friend) = friend else {
                self.emulator.ui_at(ui, main);
                return;
            };
            let swapped = self.ui.playing.swapped;
            let (mine, theirs) = if swapped {
                (friend, main)
            } else {
                (main, friend)
            };
            self.emulator.ui_at(ui, mine);
            let name = playing::friend_name(&self.online, session.friend());
            if let Some(shown) =
                playing::paint_friend_screen(&mut self.ui.playing, ui, session, theirs)
            {
                let small = if swapped { friend } else { shown };
                let caption = if swapped { "You".to_owned() } else { name };
                ui.painter().text(
                    egui::pos2(small.center().x, small.bottom() + 3.0),
                    egui::Align2::CENTER_TOP,
                    caption,
                    egui::TextStyle::Small.resolve(ui.style()),
                    ui.visuals().weak_text_color(),
                );
                let tapped = ui
                    .interact(small, egui::Id::new("friend_small"), egui::Sense::click())
                    .clicked();
                if tapped {
                    self.ui.playing.swapped = !swapped;
                }
            }
            return;
        }
        self.emulator.ui_at(ui, area);
    }

    fn paint_linked(&mut self, ui: &mut Ui, area: Rect, portrait: bool) {
        let Some(control) = self.link_control() else {
            return;
        };
        let (main, small) = if portrait {
            watch_split(area)
        } else {
            (area, None)
        };
        let paint_first = |this: &mut Self, ui: &mut Ui, rect: Rect| {
            if small.is_some_and(|small| small == rect) {
                return this.emulator.paint_fitted(ui, rect);
            }
            this.emulator.ui_at(ui, rect);
            linked::fit(rect, f32::MAX)
        };
        let paint_second = |this: &mut Self, ui: &mut Ui, rect: Rect| {
            let Some(link) = &this.emulator.link else {
                return rect;
            };
            linked::paint_screen(&mut this.ui.linked, ui, &link.gb, rect, f32::MAX)
        };
        match control {
            Game::First => paint_first(self, ui, main),
            Game::Second => paint_second(self, ui, main),
        };
        let Some(small) = small else {
            return;
        };
        let shown = match control {
            Game::First => paint_second(self, ui, small),
            Game::Second => paint_first(self, ui, small),
        };
        let caption = match control {
            Game::First => linked::title(&self.emulator, self.ui.linked.listing),
            Game::Second => self.emulator.game_name().to_owned(),
        };
        ui.painter().text(
            egui::pos2(shown.center().x, shown.bottom() + 3.0),
            egui::Align2::CENTER_TOP,
            format!("{}  {caption}", icons::HAND_TAP),
            egui::TextStyle::Small.resolve(ui.style()),
            ui.visuals().weak_text_color(),
        );
        let tapped = ui
            .interact(
                shown,
                egui::Id::new("linked_small_screen"),
                egui::Sense::click(),
            )
            .clicked();
        if tapped {
            self.release_touch();
            self.emulator.set_link_control(control.other());
        }
    }

    fn play_strip(&mut self, ui: &mut Ui) {
        ui.add_space(2.0);
        if self.play.session().is_none()
            && let Some(link) = &self.emulator.link
        {
            if linked::strip(ui, link) {
                self.mobile.open(Sheet::Panel(Tab::Playing));
                self.release_touch();
            }
        } else if self.play.session().is_none()
            && let Some(partner) = &self.emulator.partner
        {
            if linked::partner_strip(ui, partner) {
                self.mobile.open(Sheet::Panel(Tab::Playing));
                self.release_touch();
            }
        } else if let Some(session) = self.play.session().filter(|s| s.is_live()) {
            let friend = session.friend();
            let facts = playing::BarFacts {
                muted: self.ui.settings.muted,
                compact: true,
                friend_sound: playing::FriendSound::of(&self.ui.settings.online),
            };
            match playing::bar(ui, &self.online, session, facts) {
                Some(playing::BarAction::Mute) => {
                    self.ui.settings.muted = !self.ui.settings.muted;
                    self.ui.settings.dirty = true;
                }
                Some(playing::BarAction::FriendSound(sound)) => {
                    sound.store(&mut self.ui.settings.online);
                    self.ui.settings.dirty = true;
                }
                Some(playing::BarAction::Chat) => {
                    self.ui.friends.chat.open(friend);
                    self.mobile.open(Sheet::Panel(Tab::Friends));
                    self.release_touch();
                }
                Some(action) => {
                    playing::settle_bar(
                        action,
                        &mut self.play,
                        &mut self.online,
                        &mut self.ui.playing,
                    );
                }
                None => {}
            }
        } else if let Some(invite) = playing::pending_ask(&self.play) {
            match playing::ask_banner(ui, &self.online, &invite) {
                Some(playing::Answer::Join) => self.answer_ask(&invite),
                Some(playing::Answer::Pick) => {
                    self.ui.playing.picking = Some(playing::PickFor::Join(invite));
                }
                Some(playing::Answer::Decline) => {
                    self.play.decline(&mut self.online, invite.session)
                }
                None => {}
            }
        } else if let Some(ended) = self.ui.playing.ended.clone() {
            let again = playing::play_button(&self.play, &self.online, ended.friend)
                == Some(playing::PlayButton::Ready);
            match playing::banner(ui, &ended, again) {
                Some(playing::BannerAction::PlayAgain) => self.play_again(ended.friend),
                Some(playing::BannerAction::Dismiss) => self.ui.playing.ended = None,
                None => {}
            }
        }
        ui.add_space(2.0);
    }

    pub(super) fn friends_waiting(&self) -> bool {
        self.online.needs_attention() || self.play.has_unseen_invite()
    }

    fn mobile_empty_state(&mut self, ui: &mut Ui, area: Rect) {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        let button = vec2(
            area.width().min(260.0) * 0.85,
            child.spacing().interact_size.y,
        );

        child.add_space(area.height() * 0.2);
        child.heading("Citrine");
        child.add_space(4.0);
        child.label(if cfg!(target_arch = "wasm32") {
            "A Game Boy emulator that runs in your browser."
        } else {
            "A Game Boy emulator."
        });
        child.add_space(20.0);

        if child
            .add_sized(
                button,
                egui::Button::new(format!("{} Load a ROM", icons::FOLDER)),
            )
            .clicked()
        {
            self.pick_rom();
        }
        child.add_space(10.0);
        if child
            .add_sized(
                button,
                egui::Button::new(format!("{} Homebrew games", icons::JOYSTICK)),
            )
            .clicked()
        {
            self.mobile.open(Sheet::Panel(Tab::Homebrew));
        }
    }

    fn mobile_sheet(&mut self, ui: &mut Ui, sheet: Sheet) {
        match sheet {
            Sheet::Menu => {
                egui::Frame::new()
                    .inner_margin(CONTENT_MARGIN)
                    .show(ui, |ui| self.mobile_menu(ui));
            }
            Sheet::Panel(mut tab) => {
                let mut viewer = TabViewer {
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
                if tab.scrolls_itself() {
                    viewer.ui(ui, &mut tab);
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt(("sheet_scroll", tab))
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.style_mut().interaction.selectable_labels = false;
                            viewer.ui(ui, &mut tab);
                        });
                }
            }
        }
    }

    fn mobile_menu(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let entry = |ui: &mut Ui, icon: &str, label: &str| {
            ui.add_sized(
                vec2(width, ui.spacing().interact_size.y),
                egui::Button::new(format!("{icon}  {label}")),
            )
            .clicked()
        };

        if entry(ui, icons::FOLDER, "Load ROM") {
            self.pick_rom();
            self.mobile.close();
        }
        ui.add_space(6.0);
        if entry(ui, icons::JOYSTICK, Tab::Homebrew.label()) {
            self.mobile.open(Sheet::Panel(Tab::Homebrew));
        }
        ui.add_space(6.0);
        if self.emulator.link.is_none() && self.emulator.partner.is_none() {
            if entry(ui, linked::LINK_ICON, "Link a second game") {
                self.pick_second_game(true);
            }
            ui.add_space(6.0);
        }
        if entry(ui, icons::FLOPPY_DISK, Tab::Saves.label()) {
            self.mobile.open(Sheet::Panel(Tab::Saves));
        }
        ui.add_space(6.0);
        if entry(ui, icons::IMAGES, Tab::Media.label()) {
            self.mobile.open(Sheet::Panel(Tab::Media));
        }
        ui.add_space(6.0);
        if entry(ui, icons::CHART_BAR, Tab::Stats.label()) {
            self.mobile.open(Sheet::Panel(Tab::Stats));
        }
        ui.add_space(6.0);
        if self.ui.settings.online.is_joined() {
            let response = ui.add_sized(
                vec2(width, ui.spacing().interact_size.y),
                egui::Button::new(format!("{}  {}", icons::USERS, Tab::Friends.label())),
            );
            if self.friends_waiting() {
                crate::app::widgets::avatar::status_dot(ui, response.rect.shrink(8.0));
            }
            if response.clicked() {
                self.mobile.open(Sheet::Panel(Tab::Friends));
            }
            ui.add_space(6.0);
        }
        if entry(ui, icons::GEAR, Tab::Settings.label()) {
            self.mobile.open(Sheet::Panel(Tab::Settings));
        }
        ui.add_space(6.0);
        if entry(ui, icons::CAMERA, "Screenshot") {
            self.run_hotkey(HotkeyAction::Screenshot);
            self.mobile.close();
        }
        ui.add_space(6.0);
        let recording = self.emulator.screen_recorder.is_recording();
        let (icon, label) = if recording {
            (icons::STOP, "Stop recording")
        } else {
            (icons::RECORD, "Record video")
        };
        if entry(ui, icon, label) {
            self.run_hotkey(HotkeyAction::ToggleRecording);
            self.mobile.close();
        }
        ui.add_space(6.0);
        if entry(
            ui,
            icons::ARROW_COUNTER_CLOCKWISE,
            HotkeyAction::SoftReset.label(),
        ) {
            self.mobile.close();
            self.run_hotkey(HotkeyAction::SoftReset);
        }
        if plays_on_both(self.emulator.gb.cartridge.header.cgb_mode)
            && self.emulator.rom_key().is_some()
        {
            let model = other_console(self.emulator.console());
            ui.add_space(6.0);
            if entry(ui, icons::SWAP, &format!("Play on {}", console_name(model))) {
                self.mobile.close();
                self.ask_play_on(model);
            }
        }
        if self.game_info.recognised() {
            ui.add_space(6.0);
            if entry(ui, icons::IDENTIFICATION_CARD, Tab::GameInfo.label()) {
                self.mobile.open(Sheet::Panel(Tab::GameInfo));
            }
        }
        ui.add_space(6.0);
        if entry(ui, icons::INFO, Tab::Info.label()) {
            self.mobile.open(Sheet::Panel(Tab::Info));
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);

        let settings = &mut self.ui.settings;
        ui.horizontal(|ui| {
            settings.dirty |= ui
                .checkbox(&mut settings.touch.show_controls, "On-screen buttons")
                .changed();
            crate::app::widgets::info_mark::info_mark(
                ui,
                "Turn these off to play with a paired controller.",
            );
        });
    }

    pub(super) fn mobile_escape(&mut self, ctx: &Context) {
        let touch = self.is_mobile(ctx);
        let escape = touch && ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape && !self.mobile.layer_was_open && self.events_open() {
            self.ui.linked.listing = false;
        } else if escape && !self.mobile.layer_was_open && self.game_info_details_shown() {
            self.game_info.view.details = false;
            self.release_touch();
        } else if escape && self.mobile.escape(self.mobile.layer_was_open) {
            self.release_touch();
        }
        self.mobile.layer_was_open = layer_open(ctx) || self.ui.capture.is_waiting();
        let depth = if touch {
            self.mobile.depth()
                + usize::from(self.chat_open())
                + usize::from(self.events_open())
                + usize::from(self.game_info_details_shown())
                + usize::from(self.stats_game().is_some())
                + crate::app::viewer::depth(&self.ui.viewer)
        } else {
            0
        };
        self.mobile.history.sync(depth);
    }

    pub(super) fn browser_backs(&mut self) -> usize {
        self.mobile.history.take_backs()
    }

    fn events_open(&self) -> bool {
        self.mobile.sheet == Some(Sheet::Panel(Tab::Playing)) && self.ui.linked.listing
    }

    fn stats_game(&self) -> Option<&str> {
        match (self.mobile.sheet, &self.ui.stats.page) {
            (Some(Sheet::Panel(Tab::Stats)), Page::Game { name, .. }) => Some(name),
            _ => None,
        }
    }

    fn chat_open(&self) -> bool {
        self.mobile.sheet == Some(Sheet::Panel(Tab::Friends))
            && self.ui.friends.chat.current().is_some()
    }

    pub(super) fn show_tab(&mut self, tab: Tab, touch: bool) {
        if touch && tab == Tab::GameBoy {
            self.close_mobile_sheet();
        } else if touch {
            self.mobile.open(Sheet::Panel(tab));
            self.release_touch();
        } else {
            self.open_tab(tab);
        }
    }

    pub(super) fn close_mobile_sheet(&mut self) {
        self.mobile.close();
        self.release_touch();
    }

    fn pick_rom(&mut self) {
        self.try_start_audio();
        crate::utils::file_loader::FileLoader::new()
            .title("Load ROM")
            .add_filter("Game Boy ROMs", &["gb", "gbc"])
            .dispatch(self.files.rom_tx.clone());
    }
}

const ALL_BUTTONS: [JoypadState; 8] = [
    JoypadState::UP,
    JoypadState::DOWN,
    JoypadState::LEFT,
    JoypadState::RIGHT,
    JoypadState::A,
    JoypadState::B,
    JoypadState::START,
    JoypadState::SELECT,
];

fn header_text(sheet: Option<Sheet>, game_title: &str) -> &str {
    match sheet {
        Some(Sheet::Panel(tab)) => tab.label(),
        _ if game_title.is_empty() => "Citrine",
        _ => game_title,
    }
}

fn watch_split(area: Rect) -> (Rect, Option<Rect>) {
    let main_height = (area.width() * 144.0 / 160.0).min(area.height());
    let main = Rect::from_min_size(area.min, vec2(area.width(), main_height));
    let below = Rect::from_min_max(
        egui::pos2(area.left(), main.bottom() + WATCH_GAP),
        egui::pos2(area.right(), area.bottom() - WATCH_GAP - WATCH_CAPTION),
    );
    let friend = (below.height() >= WATCH_MIN_HEIGHT).then_some(below);
    (main, friend)
}

fn portrait_split(area: Rect, settings: &TouchSettings) -> (Rect, Rect) {
    let needed = TouchLayout::bar_height(area.width(), settings);
    let controls_height = (needed * CONTROL_BREATHING_ROOM)
        .max(needed)
        .clamp(0.0, area.height() * 0.6);

    let screen = Rect::from_min_size(
        area.min,
        vec2(area.width(), area.height() - controls_height),
    );
    let controls = Rect::from_min_max(egui::pos2(area.left(), screen.bottom()), area.max);
    (screen, controls)
}

fn landscape_split(
    area: Rect,
    mode: crate::app::ui_state::touch::LandscapeControls,
) -> (Rect, Rect, Rect) {
    use crate::app::ui_state::touch::LandscapeControls;

    let column = match mode {
        LandscapeControls::Overlay => area.width() * OVERLAY_COLUMN,
        LandscapeControls::Beside => {
            let wanted = area.height() / 144.0 * 160.0;
            ((area.width() - wanted) / 2.0).clamp(area.width() * 0.16, area.width() * 0.32)
        }
    };

    let left = Rect::from_min_size(area.min, vec2(column, area.height()));
    let right = Rect::from_min_size(
        egui::pos2(area.right() - column, area.top()),
        vec2(column, area.height()),
    );
    let screen = match mode {
        LandscapeControls::Overlay => area,
        LandscapeControls::Beside => {
            Rect::from_min_max(egui::pos2(left.right(), area.top()), right.left_bottom())
        }
    };
    (screen, left, right)
}

#[cfg(target_arch = "wasm32")]
fn toggle_fullscreen() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if document.fullscreen_element().is_some() {
        document.exit_fullscreen();
    } else if let Some(root) = document.document_element() {
        let _ = root.request_fullscreen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::touch::LandscapeControls;
    use egui::pos2;

    #[test]
    fn back_from_a_sheet_opened_in_the_menu_returns_to_the_menu() {
        let mut state = MobileState::default();
        state.open(Sheet::Menu);
        state.open(Sheet::Panel(Tab::Saves));
        state.back();
        assert_eq!(state.sheet, Some(Sheet::Menu));
        state.back();
        assert_eq!(state.sheet, None);
    }

    #[test]
    fn back_from_a_sheet_opened_elsewhere_returns_to_the_game() {
        let mut state = MobileState::default();
        state.open(Sheet::Panel(Tab::Homebrew));
        state.back();
        assert_eq!(state.sheet, None);
    }

    #[test]
    fn escape_steps_back_through_the_sheets() {
        let mut state = MobileState::default();
        state.open(Sheet::Menu);
        state.open(Sheet::Panel(Tab::Settings));
        assert!(state.escape(false));
        assert_eq!(
            state.sheet,
            Some(Sheet::Menu),
            "back to the menu it came from"
        );
        assert!(state.escape(false));
        assert_eq!(state.sheet, None);
        assert!(!state.escape(false), "the game screen stays");
    }

    #[test]
    fn escape_closes_the_top_layer_before_the_sheet() {
        let mut state = MobileState::default();
        state.open(Sheet::Panel(Tab::Saves));
        assert!(!state.escape(true));
        assert_eq!(state.sheet, Some(Sheet::Panel(Tab::Saves)));
    }

    #[test]
    fn a_panel_sheet_shows_its_own_name() {
        assert_eq!(
            header_text(Some(Sheet::Panel(Tab::Saves)), "POKEMON"),
            "Saves & snapshots"
        );
        assert_eq!(
            header_text(Some(Sheet::Panel(Tab::Media)), "POKEMON"),
            "Media"
        );
        assert_eq!(header_text(Some(Sheet::Menu), "POKEMON"), "POKEMON");
        assert_eq!(header_text(None, ""), "Citrine");
    }

    #[test]
    fn portrait_bands_cover_the_area_exactly() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(390.0, 760.0));
        let (screen, controls) = portrait_split(area, &TouchSettings::default());

        assert!((screen.height() + controls.height() - area.height()).abs() < 0.01);
        assert_eq!(screen.top(), area.top());
        assert_eq!(controls.bottom(), area.bottom());
    }

    #[test]
    fn the_control_band_fits_the_controls_without_stranding_them() {
        for height in [600.0_f32, 760.0, 900.0, 1200.0] {
            let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, height));
            let settings = TouchSettings::default();
            let (_, controls) = portrait_split(area, &settings);
            let needed = TouchLayout::bar_height(area.width(), &settings);

            assert!(
                controls.height() >= needed - 0.01,
                "controls were squeezed at height {height}: {} < {needed}",
                controls.height()
            );
            assert!(
                controls.height() <= needed * CONTROL_BREATHING_ROOM + 0.01,
                "a taller window must not strand the buttons in empty space at \
                 height {height}: {} for {needed} needed",
                controls.height()
            );
        }
    }

    #[test]
    fn bigger_buttons_get_a_taller_band() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 900.0));
        let small = TouchSettings {
            scale: TouchSettings::MIN_SCALE,
            ..Default::default()
        };
        let large = TouchSettings {
            scale: TouchSettings::MAX_SCALE,
            ..Default::default()
        };

        let (_, small_band) = portrait_split(area, &small);
        let (_, large_band) = portrait_split(area, &large);
        assert!(large_band.height() > small_band.height());
    }

    #[test]
    fn a_short_portrait_window_still_leaves_room_for_buttons() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        let (screen, controls) = portrait_split(area, &TouchSettings::default());

        assert!(
            screen.height() > 0.0,
            "the screen never collapses to nothing"
        );
        assert!(
            controls.height() <= area.height() * 0.6 + 0.01,
            "the screen keeps at least 40% of a short window"
        );
    }

    #[test]
    fn landscape_columns_do_not_overlap_the_screen() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(844.0, 390.0));
        let (screen, left, right) = landscape_split(area, LandscapeControls::Beside);

        assert!(screen.left() >= left.right());
        assert!(screen.right() <= right.left());
        assert!(screen.width() > 0.0);
    }

    #[test]
    fn the_overlay_mode_hands_the_whole_area_to_the_screen() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(844.0, 390.0));
        let (screen, left, right) = landscape_split(area, LandscapeControls::Overlay);

        assert_eq!(screen, area, "the picture uses the full width");
        assert!(left.width() > 0.0 && right.width() > 0.0);
        assert!(right.right() <= area.right());
    }

    #[test]
    fn a_very_wide_window_still_keeps_the_columns_reachable() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 400.0));
        let (_, left, _) = landscape_split(area, LandscapeControls::Beside);
        assert!(
            left.width() <= area.width() * 0.32,
            "columns never sprawl across half the screen"
        );
    }
}
