use crate::app::Citrine;
use crate::app::big_screen::Backdrop;
use crate::app::tabs::stats::Page;
use crate::app::tabs::{Tab, TabViewer, linked, playing};
use crate::app::ui_state::screen_layouts::LayoutView;
use crate::app::widgets::avatar;
use crate::app::widgets::touch_controls::TouchInput;
use egui::{Context, Frame, Margin, Rect, Ui, pos2, vec2};
use egui_dock::TabViewer as _;
use nav::Place;

pub(crate) mod achievements;
#[cfg(test)]
mod chat_feel_tests;
pub(crate) mod cheats;
pub(crate) mod frame;
mod game;
pub(in crate::app) mod games;
mod hide;
mod history;
mod homebrew;
pub(super) mod hud;
pub mod kit;
#[cfg(test)]
mod kit_guard;
#[cfg(all(test, feature = "nds"))]
mod layout_edit_tests;
mod library;
mod more;
pub mod nav;
#[cfg(test)]
mod page_toast_tests;
mod room;
mod screens;
pub(super) mod settings_list;
mod settings_pages;
pub mod sheet;
mod strip;
mod this_game;
#[cfg(feature = "nds")]
pub(crate) mod walker;
mod you;

pub const OPEN_ROM: &str = "Open a ROM";

pub const TOUCH_TARGET_PX: f32 = 44.0;

const PLACE_MARGIN: Margin = Margin::symmetric(kit::SIDE as i8, 0);
const TAB_PLACE_MARGIN: Margin =
    Margin::symmetric(kit::SIDE as i8 - crate::app::tabs::CONTENT_MARGIN.left, 0);

#[derive(Default)]
pub struct MobileState {
    pub nav: nav::Nav,
    pub phone: bool,
    touch: TouchInput,
    pub(crate) hidden: hide::HiddenUi,
    pub(crate) held_buttons: Vec<crate::app::ui_state::screen_layouts::Item>,
    layer_was_open: bool,
    history: history::BrowserHistory,
    held_pause: bool,
    more: more::MoreState,
    games_search: games::GamesSearch,
    pub(in crate::app) files_sheet: Option<games::FilesView>,
    sheet: Option<settings_pages::Sheet>,
    you_sheet: Option<you::YouSheet>,
    join_sheet: Option<crate::app::widgets::settings::online::JoinSheet>,
    media_filter: bool,
    pub(super) hud_sheet: Option<hud::HudSheet>,
    homebrew_info: Option<usize>,
    pub(super) library: library::LibrarySheets,
    about_doc: Option<crate::app::widgets::info::InfoTab>,
}

impl MobileState {
    #[cfg(test)]
    pub fn ui_hidden(&self) -> bool {
        self.hidden.on()
    }

    pub fn showing(&self, tab: Tab) -> bool {
        self.nav.showing(tab)
    }

    pub fn open(&mut self, tab: Tab) {
        self.nav.open(tab);
    }

    pub fn close(&mut self) {
        self.nav.show_game();
    }

    #[cfg(test)]
    pub fn sheet_open(&self) -> bool {
        self.sheet.is_some()
            || self.you_sheet.is_some()
            || self.join_sheet.is_some()
            || self.media_filter
            || self.hud_sheet.is_some()
            || self.homebrew_info.is_some()
            || self.library.open()
            || self.files_sheet.is_some()
    }

    pub fn listen_for_back(&self) {
        self.history.listen();
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

pub fn bar_track(ui: &Ui) -> Option<egui::Color32> {
    (touch_square(ui) > 0.0).then(|| kit::bar_track(ui.visuals()))
}

pub fn switch(ui: &Ui, rect: Rect, t: f32) -> bool {
    let touch = touch_square(ui) > 0.0;
    if touch {
        kit::paint_switch(ui, rect, t);
    }
    touch
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

pub(super) fn landscape(ctx: &Context) -> bool {
    let window = crate::app::ui_state::settings::window_pixels(ctx);
    window.x > window.y
}

const PLACE_MAX_WIDTH: f32 = 560.0;
const HINT_HEIGHT: f32 = 40.0;

impl Citrine {
    pub(super) fn render_mobile(&mut self, ctx: &Context) {
        self.mobile.phone = true;
        let big = self.big_screen_on();
        if big {
            self.big_screen_begin(ctx);
        }
        if !self.game_open() {
            self.mobile.nav.leave_game();
            self.mobile.hidden.forget();
        }
        if !big {
            let view = if landscape(ctx) {
                LayoutView::Landscape
            } else {
                LayoutView::Portrait
            };
            let wanted = self.mobile.nav.on_game()
                && self.plays_phone_default(view)
                && !self.editing_layout();
            self.mobile.hidden.follow_default(wanted);
        }
        let bare = if big {
            self.mobile.nav.on_game() && !self.mobile.nav.more_open()
        } else {
            self.mobile.hidden.on() && self.mobile.nav.on_game()
        };
        if !bare || big {
            self.mobile.hidden.show();
        }
        crate::platform::immersive(bare || big);
        self.hold_game_off_screen();
        let landscape = big || landscape(ctx);
        let screen = ctx.content_rect();
        let typing = self.chat_typing(ctx);
        let rail = if landscape && !bare && !big {
            frame::rail_width(&ctx.style())
        } else {
            0.0
        };
        let tabs = if big && !bare {
            crate::app::big_screen::TAB_BAR_HEIGHT
        } else {
            0.0
        };
        let bar = if landscape {
            0.0
        } else if let Some(covered) = typing {
            (frame::bar_height(&ctx.style()) - covered).max(0.0)
        } else {
            frame::bar_height(&ctx.style())
        };
        let hints = if big && !bare { HINT_HEIGHT } else { 0.0 };
        crate::app::big_screen::set_hint_room(ctx, hints);
        crate::app::big_screen::set_tab_bottom(ctx, screen.top() + tabs);
        crate::app::big_screen::set_named_tab(ctx, None);
        let page = Rect::from_min_max(
            pos2(screen.left() + rail, screen.top() + tabs),
            pos2(screen.right(), screen.bottom() - bar - hints),
        );
        sheet::set_area(ctx, Some(page));
        let entries = self.frame_entries();
        let bar_fill = kit::card_fill(&ctx.style().visuals);
        let picked = if bare {
            None
        } else if big {
            egui::TopBottomPanel::top("big_tabs")
                .exact_height(tabs)
                .show_separator_line(false)
                .frame(Frame::NONE.fill(bar_fill))
                .show(ctx, |ui| self.big_tab_bar(ui));
            None
        } else if typing.is_some() && !landscape {
            if bar > 0.0 {
                egui::TopBottomPanel::bottom("place_bar_room")
                    .exact_height(bar)
                    .show_separator_line(false)
                    .frame(Frame::NONE.fill(kit::page_fill(&ctx.style().visuals)))
                    .show(ctx, |_| {});
            }
            None
        } else if landscape {
            egui::SidePanel::left("place_rail")
                .exact_width(frame::rail_width(&ctx.style()))
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::NONE.fill(bar_fill))
                .show(ctx, |ui| frame::rail(ui, &entries))
                .inner
        } else {
            egui::TopBottomPanel::bottom("place_bar")
                .exact_height(frame::bar_height(&ctx.style()))
                .show_separator_line(false)
                .frame(Frame::NONE.fill(bar_fill))
                .show(ctx, |ui| frame::bar(ui, &entries))
                .inner
        };
        if let Some(place) = picked {
            self.go_place(place);
        }
        if hints > 0.0 {
            egui::TopBottomPanel::bottom("big_hints")
                .exact_height(hints)
                .show_separator_line(false)
                .frame(
                    Frame::NONE
                        .fill(bar_fill)
                        .inner_margin(Margin::symmetric(16, 0)),
                )
                .show(ctx, |ui| self.big_hints(ui));
        }
        let keys = self.pad_keys_room(ctx) - hints;
        if keys > 0.0 {
            egui::TopBottomPanel::bottom("pad_keys_room")
                .exact_height(keys)
                .show_separator_line(false)
                .frame(Frame::NONE.fill(kit::page_fill(&ctx.style().visuals)))
                .show(ctx, |_| {});
        }

        let on_game = self.mobile.nav.on_game();
        let visuals = &ctx.style().visuals;
        let fill = if on_game && big && self.ui.settings.big_screen.backdrop == Backdrop::Black {
            egui::Color32::BLACK
        } else if on_game {
            visuals.panel_fill
        } else {
            kit::page_fill(visuals)
        };
        let (need, room) = self.toasts.page_strip(ctx);
        if room > 0.0 {
            egui::TopBottomPanel::bottom("toast_room")
                .exact_height(room)
                .show_separator_line(false)
                .frame(Frame::NONE.fill(fill))
                .show(ctx, |ui| {
                    self.toasts
                        .place_strip(ui.max_rect(), need.unwrap_or(room), kit::SIDE);
                });
        }
        let fade = hints > 0.0 && !on_game;
        crate::app::big_screen::scroll::set_fade(ctx, fade, room);
        let shown = egui::CentralPanel::default()
            .frame(Frame::NONE.fill(fill))
            .show(ctx, |ui| {
                if room == 0.0 {
                    self.toasts.place_page(ui.max_rect());
                }
                self.show_place(ui, landscape);
            })
            .response
            .rect;
        if fade {
            crate::app::big_screen::scroll::fade_top(ctx, shown, fill);
            crate::app::big_screen::scroll::fade_bottom(ctx, shown, fill, room);
        }
        if big {
            self.big_home_menu(ctx, page);
        } else {
            self.more_sheet(ctx);
        }
        self.phone_layout_sheet(ctx);
        self.layout_turn_wait(self.mobile.nav.on_game());
        self.hud_sheet(ctx);
    }

    pub(super) fn game_open(&self) -> bool {
        self.emulator.has_game()
            || self.emulator.linked()
            || self.emulator.partnered()
            || self.play.session().is_some()
    }

    fn frame_entries(&self) -> Vec<frame::Entry> {
        let place = self.mobile.nav.place();
        let face = self
            .online
            .account()
            .filter(|_| self.ui.settings.online.is_joined())
            .map(|account| (account.display_name.clone(), account.online_id.value()));
        let joined = self.ui.settings.online.is_joined();
        Place::ALL
            .into_iter()
            .filter(|entry| joined || *entry != Place::Friends)
            .map(|entry| frame::Entry {
                place: entry,
                selected: entry == place,
                dot: match entry {
                    Place::Friends => self.friends_waiting().then_some(avatar::DOT),
                    Place::Settings => self.ui.update.phase().waiting().then_some(avatar::DOT),
                    Place::Games => {
                        (self.game_open() && !self.mobile.nav.on_game()).then_some(avatar::ONLINE)
                    }
                    _ => None,
                },
                face: face.clone().filter(|_| entry == Place::You),
            })
            .collect()
    }

    pub(super) fn go_place(&mut self, place: Place) {
        if place == self.mobile.nav.place() {
            match place {
                Place::Friends => self.ui.friends.chat.close(),
                Place::You => self.ui.stats.page = Page::Overview,
                Place::Games => {
                    self.ui.linked.listing = false;
                    self.close_games_search();
                }
                Place::Media | Place::Settings => {}
            }
        }
        self.mobile.nav.go(place);
        self.release_touch();
    }

    fn hold_game_off_screen(&mut self) {
        let loaded = self.emulator.has_game();
        if self.editing_layout() && (!loaded || !self.mobile.nav.game_in_view()) {
            self.cancel_layout_edit();
        }
        if !loaded || self.play.session().is_some() {
            self.mobile.held_pause = false;
            return;
        }
        let in_view =
            (self.mobile.nav.game_in_view() && !self.editing_layout()) || self.walker_needs_game();
        if !in_view && self.emulator.running && !self.load_needs_game() && self.emulator.try_pause()
        {
            self.mobile.held_pause = true;
        } else if in_view && self.mobile.held_pause {
            self.emulator.running = true;
            self.mobile.held_pause = false;
        }
    }

    #[cfg(feature = "nds")]
    fn walker_needs_game(&self) -> bool {
        self.mobile.nav.showing(Tab::Walker) && self.emulator.pokewalker.is_lent()
    }

    #[cfg(not(feature = "nds"))]
    fn walker_needs_game(&self) -> bool {
        false
    }

    fn load_needs_game(&self) -> bool {
        self.mobile.nav.showing(Tab::Performance) && self.ui.performance.measuring_before_hold()
    }

    pub(super) fn release_phone_pause(&mut self) {
        self.emulator.fast_forward = false;
        if std::mem::take(&mut self.mobile.held_pause) {
            self.emulator.running = true;
        }
    }

    pub(crate) fn resume_game(&mut self) {
        self.mobile.held_pause = false;
        if self.emulator.has_game() {
            self.emulator.running = true;
        }
        self.mobile.nav.show_game();
        self.release_touch();
    }

    fn show_place(&mut self, ui: &mut Ui, landscape: bool) {
        let page = self.mobile.nav.page();
        let big = self.big_screen_on();
        if page.is_none() && self.mobile.nav.on_game() {
            if big {
                self.big_game(ui);
            } else {
                self.game_view(ui, landscape);
            }
            return;
        }
        let full = ui.max_rect();
        let most = if big {
            crate::app::big_screen::COLUMN_MAX_WIDTH
        } else {
            PLACE_MAX_WIDTH
        };
        let width = full.width().min(most);
        let column = egui::Rect::from_min_size(
            egui::pos2(full.center().x - width / 2.0, full.top()),
            vec2(width, full.height()),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
            self.place_column(ui, page)
        });
    }

    fn place_column(&mut self, ui: &mut Ui, page: Option<Tab>) {
        if self.mobile.nav.place() == Place::Friends && !self.ui.settings.online.is_joined() {
            self.mobile.nav.go(Place::You);
            return;
        }
        if let Some(tab) = page {
            self.mobile_page(ui, tab);
            return;
        }
        match self.mobile.nav.place() {
            Place::Games => self.games_place(ui),
            Place::Friends => self.friends_place(ui),
            Place::Media => self.media_place(ui),
            Place::Settings => self.settings_place(ui),
            Place::You => self.you_place(ui),
        }
    }

    fn mobile_page(&mut self, ui: &mut Ui, tab: Tab) {
        if tab == Tab::Info && self.mobile.about_doc.is_none() {
            if self.page_title(ui, "About") {
                self.page_back();
            }
            self.about_page(ui);
            return;
        }
        let title = self.page_heading(tab);
        if self.page_title(ui, &title) {
            self.page_back();
        }
        if tab == Tab::Homebrew {
            self.homebrew_page(ui);
            return;
        }
        if tab == Tab::RomFolders {
            self.rom_folders_page(ui);
            return;
        }
        if tab == Tab::ThisGame {
            self.this_game_page(ui);
            return;
        }
        if tab == Tab::Stats {
            Frame::new()
                .inner_margin(TAB_PLACE_MARGIN)
                .show(ui, |ui| self.tab_body(ui, tab));
            return;
        }
        Frame::new()
            .inner_margin(Margin::symmetric(10, 0))
            .show(ui, |ui| self.tab_body(ui, tab));
    }

    fn page_title(&self, ui: &mut Ui, title: &str) -> bool {
        Frame::new()
            .inner_margin(Margin::symmetric(4, 0))
            .show(ui, |ui| kit::page_title(ui, title, |_| {}))
            .inner
    }

    fn page_heading(&self, tab: Tab) -> String {
        if let Some(doc) = self.about_doc_shown() {
            return doc.to_string();
        }
        if let Some(name) = self.stats_game() {
            return name.to_owned();
        }
        if tab == Tab::Achievements
            && let Some(title) = self.achievements.boards.open_title()
        {
            return title.to_owned();
        }
        if self.game_info_details_shown() {
            return self.game_info.view.page.label().to_owned();
        }
        match (tab, self.play.session()) {
            (Tab::Playing, Some(session)) => {
                let name = playing::member_name(&self.play, &self.online, session.friend());
                format!("Playing with {name}")
            }
            (Tab::Playing, None) => linked::title(&self.emulator, self.ui.linked.listing),
            (Tab::Saves, _) => "Saves".to_owned(),
            (Tab::DsFile(slot), _) => self.ui.ds_files.title(slot),
            (Tab::DsFiles, _) => self
                .ui
                .ds_files
                .folder_name()
                .unwrap_or(Tab::DsFiles.title())
                .to_owned(),
            (Tab::Info, _) => "About".to_owned(),
            (Tab::ThisGame, _) => self.this_game_title(),
            (tab, _) => tab.label().to_owned(),
        }
    }

    pub(super) fn page_back(&mut self) {
        if self.games_search_open() {
            self.close_games_search();
        } else if self.events_open() {
            self.ui.linked.listing = false;
        } else if self.about_doc_shown().is_some() {
            self.mobile.about_doc = None;
            self.mobile.nav.back();
        } else if self.game_info_details_shown() {
            self.game_info.view.details = false;
        } else if self.stats_game().is_some() {
            self.ui.stats.page = Page::Overview;
        } else if self.mobile.showing(Tab::Achievements) && self.achievements.boards.open.is_some()
        {
            self.achievements.boards.open = None;
        } else if !(self.mobile.showing(Tab::DsFiles) && self.ui.ds_files.folder_up()) {
            self.mobile.nav.back();
        }
        self.release_touch();
    }

    pub(super) fn viewer(&mut self) -> TabViewer<'_> {
        TabViewer {
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
            #[cfg(feature = "nds")]
            mic_test: self.mic.test(),
        }
    }

    pub(super) fn tab_body(&mut self, ui: &mut Ui, mut tab: Tab) {
        let mut viewer = self.viewer();
        if tab.scrolls_itself() {
            viewer.ui(ui, &mut tab);
        } else {
            let scroll = egui::ScrollArea::vertical()
                .id_salt(("page_scroll", tab))
                .auto_shrink(false);
            kit::clear_of_toasts(ui, scroll, |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                viewer.ui(ui, &mut tab);
            });
        }
    }

    fn friends_place(&mut self, ui: &mut Ui) {
        if self.chat_open() {
            self.tab_body(ui, Tab::Friends);
            return;
        }
        Frame::new().inner_margin(PLACE_MARGIN).show(ui, |ui| {
            kit::place_title(ui, Place::Friends.label(), |ui| {
                let side = ui.spacing().interact_size.y;
                if self.ui.settings.online.is_joined()
                    && ui
                        .add(kit::header_button(crate::icons::USER_PLUS, side))
                        .on_hover_text("Add a friend")
                        .clicked()
                {
                    self.ui.friends.toggle_adding();
                }
            });
        });
        Frame::new()
            .inner_margin(TAB_PLACE_MARGIN)
            .show(ui, |ui| self.tab_body(ui, Tab::Friends));
    }

    fn media_place(&mut self, ui: &mut Ui) {
        Frame::new().inner_margin(PLACE_MARGIN).show(ui, |ui| {
            kit::place_title(ui, Place::Media.label(), |ui| {
                if crate::app::tabs::media::filter_button(ui, &self.ui.media.browse) {
                    self.mobile.media_filter = true;
                    self.release_touch();
                }
            });
        });
        Frame::new()
            .inner_margin(TAB_PLACE_MARGIN)
            .show(ui, |ui| self.tab_body(ui, Tab::Media));
        if self.mobile.media_filter {
            self.media_filter_sheet(ui.ctx());
        }
    }

    fn media_filter_sheet(&mut self, ctx: &Context) {
        use crate::app::tabs::media::{game_name, games, kind_label};
        use crate::media::MediaKind;
        let games = games(self.media.items());
        let browse = &mut self.ui.media.browse;
        let current_game = browse
            .game
            .as_ref()
            .and_then(|game| games.iter().position(|g| g == game))
            .map_or(0, |index| index + 1);
        let shown = sheet::show(ctx, egui::Id::new("media_filter"), "Filter", |ui| {
            let kind = sheet::choices(
                ui,
                [
                    None,
                    Some(MediaKind::Screenshot),
                    Some(MediaKind::Recording),
                    Some(MediaKind::Print),
                ],
                browse.kind,
                |kind| kind_label(kind).to_owned(),
            );
            let game = (games.len() > 1)
                .then(|| {
                    kit::section_label(ui, "Game");
                    sheet::choices(ui, 0..=games.len(), current_game, |index| match index {
                        0 => "All games".to_owned(),
                        index => game_name(&games[index - 1]).to_owned(),
                    })
                })
                .flatten();
            (kind, game)
        });
        let (kind, game) = shown.inner;
        if let Some(kind) = kind {
            browse.kind = kind;
            browse.page = 0;
        }
        if let Some(game) = game {
            browse.game = game.checked_sub(1).map(|index| games[index].clone());
            browse.page = 0;
        }
        if shown.closed {
            self.mobile.media_filter = false;
        }
    }

    pub(super) fn friends_waiting(&self) -> bool {
        self.online.needs_attention() || self.play.has_unseen_invite()
    }

    pub(super) fn mobile_escape(&mut self, ctx: &Context) {
        let touch = self.is_mobile(ctx);
        let big = self.big_screen_on();
        let escape = touch && ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape && !self.mobile.layer_was_open {
            if big && self.editing_layout() {
                self.cancel_layout_edit();
            } else if big && self.mobile.nav.more_open() && self.ui.layout_sheet.is_some() {
                self.close_layout_sheet_layer(false);
            } else if big && self.mobile.nav.on_game() && !self.mobile.nav.more_open() {
                self.open_game_menu();
            } else if self.editing_layout() {
                self.cancel_layout_edit();
            } else if self.mobile.hidden.on() {
                self.mobile.hidden.keep_shown();
            } else if self.in_game_scope() {
                self.ui.settings.leave_scope();
            } else {
                self.page_back();
            }
        }
        let open = if big {
            crate::app::big_screen::layer_open(ctx)
        } else {
            layer_open(ctx)
        };
        self.mobile.layer_was_open = open || self.ui.capture.is_waiting();
        let depth = if touch {
            self.mobile.nav.depth()
                + usize::from(self.mobile.hidden.on())
                + usize::from(self.editing_layout())
                + usize::from(self.games_search_open())
                + usize::from(self.chat_open())
                + usize::from(self.events_open())
                + usize::from(self.game_info_details_shown())
                + usize::from(self.stats_game().is_some())
                + usize::from(self.in_game_scope())
                + crate::app::viewer::depth(&self.ui.viewer)
        } else {
            0
        };
        self.mobile.history.sync(depth);
        #[cfg(target_os = "android")]
        crate::platform::android::lifecycle::at_root(
            depth == 0
                && !self.mobile.layer_was_open
                && !self.ui.focus_mode
                && !self.ui.viewer.is_open()
                && ctx.memory(|m| m.focused().is_none()),
        );
    }

    pub(super) fn browser_backs(&mut self) -> usize {
        self.mobile.history.take_backs()
    }

    fn in_game_scope(&self) -> bool {
        self.scope_bar_shown()
            && self.ui.settings.scope() == crate::app::ui_state::per_game::Scope::ThisGame
    }

    fn events_open(&self) -> bool {
        self.mobile.showing(Tab::Playing) && self.ui.linked.listing
    }

    fn about_doc_shown(&self) -> Option<crate::app::widgets::info::InfoTab> {
        self.mobile
            .about_doc
            .filter(|_| self.mobile.showing(Tab::Info))
    }

    fn game_info_details_shown(&self) -> bool {
        self.mobile.showing(Tab::GameInfo) && self.game_info.details_open()
    }

    fn stats_game(&self) -> Option<&str> {
        match &self.ui.stats.page {
            Page::Game { name, .. } if self.mobile.showing(Tab::Stats) => Some(name),
            _ => None,
        }
    }

    fn chat_typing(&self, ctx: &Context) -> Option<f32> {
        let keyboard = crate::app::widgets::soft_keyboard::shown(ctx);
        let chat = &self.ui.friends.chat;
        (self.chat_open() && (keyboard > 0.0 || chat.panel_open()))
            .then(|| keyboard + chat.emoji_room(ctx))
    }

    fn chat_open(&self) -> bool {
        self.mobile.nav.place() == Place::Friends
            && self.mobile.nav.page().is_none()
            && self.ui.friends.chat.current().is_some()
    }

    pub(super) fn show_tab(&mut self, tab: Tab, touch: bool) {
        if touch {
            self.mobile.open(tab);
            self.release_touch();
        } else {
            self.open_tab(tab);
        }
    }

    pub(super) fn close_mobile_sheet(&mut self) {
        self.mobile.close();
        self.release_touch();
    }

    pub(super) fn pick_rom(&mut self) {
        use crate::utils::save_files::ROM_FILTER;
        self.try_start_audio();
        crate::utils::file_loader::FileLoader::new()
            .title(OPEN_ROM)
            .add_filter(ROM_FILTER.0, ROM_FILTER.1)
            .dispatch(&self.files.spawner, self.files.rom_tx.clone());
    }
}
