use crate::app::Citrine;
use crate::app::mobile::nav::Place;
use crate::app::ui_state::keybinds::{BindingKind, PadButton};
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::settings::scope::USE_ALL_HINT;
use crate::controller_debug::PadInfo;
use crate::pad_input::{PadEvent, PadInput, PadMotion, PadSource};
use egui::{Context, Id, Ui};
use glyphs::{Family, Glyph, Hint};
use pad_nav::{Command, Dir};
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Display;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

pub(crate) mod glyphs;
mod keyboard;
mod move_screen;
mod pad_nav;
mod recall;
pub(crate) mod ring;
pub(crate) mod scroll;
mod tabs;

pub const DESIGN_POINTS: f32 = 540.0;
pub const COLUMN_MAX_WIDTH: f32 = 880.0;
pub const TAB_BAR_HEIGHT: f32 = tabs::HEIGHT;
const TAB_ACTIONS_MARGIN: f32 = 16.0;
const TV_SIDES: f32 = 48.0;
const TV_ENDS: f32 = 27.0;
const FLAT_SHARE: f32 = 0.8;
const MENU_TIP_SECONDS: f64 = 6.0;
const MENU_TIP_HEIGHT: f32 = 40.0;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum ConfirmButton {
    #[default]
    Automatic,
    Bottom,
    Right,
}

impl ConfirmButton {
    pub const NOTE: &str =
        "Nintendo controllers confirm with the right button, others with the bottom one.";
}

impl Display for ConfirmButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Automatic => "Automatic",
            Self::Bottom => "Bottom",
            Self::Right => "Right",
        })
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum ButtonIcons {
    #[default]
    Automatic,
    Xbox,
    PlayStation,
    Nintendo,
    Positions,
}

impl Display for ButtonIcons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Automatic => "Automatic",
            Self::Xbox => "Xbox",
            Self::PlayStation => "PlayStation",
            Self::Nintendo => "Nintendo",
            Self::Positions => "Positions",
        })
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum Backdrop {
    #[default]
    Black,
    Theme,
}

impl Display for Backdrop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Black => "Black",
            Self::Theme => "Theme colour",
        })
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BigScreenSettings {
    pub on: bool,
    pub confirm: ConfirmButton,
    pub icons: ButtonIcons,
    pub backdrop: Backdrop,
    pub menu_opened: bool,
}

pub const MENU_BUTTON: &str = "Home, or hold Select + Start";
pub const OPEN: &str = "Choose";

pub fn cycle<T: IntoEnumIterator + PartialEq + Copy>(value: T, step: i32) -> T {
    let all: Vec<T> = T::iter().collect();
    let at = all.iter().position(|v| *v == value).unwrap_or(0) as i32;
    let len = all.len() as i32;
    all[(at + step).rem_euclid(len) as usize]
}

fn stepper_id() -> Id {
    Id::new("big_screen_stepper")
}

fn hint_room_id() -> Id {
    Id::new("big_screen_hint_room")
}

fn tab_bottom_id() -> Id {
    Id::new("big_screen_tab_bottom")
}

fn named_tab_id() -> Id {
    Id::new("big_screen_named_tab")
}

pub fn set_named_tab(ctx: &Context, label: Option<String>) {
    ctx.data_mut(|d| d.insert_temp(named_tab_id(), label));
}

pub fn tab_names(ctx: &Context, title: &str) -> bool {
    ctx.data(|d| d.get_temp::<Option<String>>(named_tab_id()))
        .flatten()
        .is_some_and(|label| label == title)
}

pub fn set_tab_bottom(ctx: &Context, bottom: f32) {
    ctx.data_mut(|d| d.insert_temp(tab_bottom_id(), bottom));
}

fn tab_bottom(ctx: &Context) -> f32 {
    ctx.data(|d| d.get_temp(tab_bottom_id()))
        .unwrap_or(f32::NEG_INFINITY)
}

pub fn tab_actions_rect(ctx: &Context) -> egui::Rect {
    let bottom = tab_bottom(ctx);
    let screen = ctx.content_rect();
    egui::Rect::from_min_max(
        egui::pos2(screen.center().x, bottom - TAB_BAR_HEIGHT),
        egui::pos2(screen.right() - TAB_ACTIONS_MARGIN, bottom),
    )
}

pub fn set_hint_room(ctx: &Context, height: f32) {
    ctx.data_mut(|d| d.insert_temp(hint_room_id(), height));
}

pub fn dim(visuals: &egui::Visuals) -> egui::Color32 {
    if visuals.dark_mode {
        egui::Color32::from_black_alpha(DARK_DIM)
    } else {
        crate::app::mobile::kit::scrim(visuals)
    }
}

pub fn hint_room(ctx: &Context) -> f32 {
    ctx.data(|d| d.get_temp(hint_room_id())).unwrap_or(0.0)
}

fn step_id() -> Id {
    Id::new("big_screen_step")
}

fn first_choice_id() -> Id {
    Id::new("big_screen_first_choice")
}

pub fn first_choice(response: &egui::Response) {
    let pass = response.ctx.cumulative_pass_nr();
    response
        .ctx
        .data_mut(|d| d.insert_temp(first_choice_id(), (pass, response.id)));
}

pub fn offer_steps(ctx: &Context, response: &egui::Response) {
    if response.has_focus() {
        ctx.data_mut(|d| d.insert_temp(stepper_id(), response.id));
    }
}

fn press_id() -> Id {
    Id::new("big_screen_press")
}

pub fn offer_press(ctx: &Context, response: &egui::Response, label: &'static str) {
    if response.has_focus() {
        ctx.data_mut(|d| d.insert_temp(press_id(), (response.id, label)));
    }
}

fn stepped_page(
    page: crate::app::ui_state::settings_tree::SettingsTab,
    step: i32,
    dev_mode: bool,
) -> crate::app::ui_state::settings_tree::SettingsTab {
    use crate::app::ui_state::settings_tree::SettingsTab;
    let pages: Vec<SettingsTab> = SettingsTab::iter_with_dev_mode(dev_mode)
        .filter(|page| *page != SettingsTab::About)
        .collect();
    let at = pages.iter().position(|p| *p == page).unwrap_or(0) as i32;
    pages[(at + step).rem_euclid(pages.len() as i32) as usize]
}

pub fn take_step(response: &egui::Response) -> Option<i32> {
    if !response.has_focus() {
        return None;
    }
    let step = response.ctx.data_mut(|d| {
        let step = d.get_temp::<(Id, i32)>(step_id());
        d.remove::<(Id, i32)>(step_id());
        step
    })?;
    (step.0 == response.id).then_some(step.1)
}

fn details_offer_id() -> Id {
    Id::new("big_screen_details_offer")
}

fn details_id() -> Id {
    Id::new("big_screen_details")
}

fn details_open_id() -> Id {
    Id::new("big_screen_details_open")
}

pub fn offer_details(response: &egui::Response, label: &'static str) {
    if response.has_focus() {
        response
            .ctx
            .data_mut(|d| d.insert_temp(details_offer_id(), (response.id, label)));
    }
}

pub fn take_details(response: &egui::Response) -> bool {
    if !response.has_focus() {
        return false;
    }
    response.ctx.data_mut(|d| {
        let asked = d.get_temp::<Id>(details_id()) == Some(response.id);
        if asked {
            d.remove::<Id>(details_id());
        }
        asked
    })
}

pub fn details_shown(response: &egui::Response) -> bool {
    let ctx = &response.ctx;
    let open = ctx.data(|d| d.get_temp::<Id>(details_open_id())) == Some(response.id);
    if !response.has_focus() {
        if open {
            ctx.data_mut(|d| d.remove::<Id>(details_open_id()));
        }
        return false;
    }
    let shown = open != take_details(response);
    ctx.data_mut(|d| {
        if shown {
            d.insert_temp(details_open_id(), response.id);
        } else {
            d.remove::<Id>(details_open_id());
        }
    });
    offer_details(response, if shown { "Hide info" } else { "Info" });
    shown
}

pub struct PadGate {
    inner: Box<dyn PadSource>,
    held: VecDeque<PadEvent>,
    gated: bool,
}

impl Default for PadGate {
    fn default() -> Self {
        Self::new(crate::pad_input::open_source())
    }
}

impl PadGate {
    pub fn new(inner: Box<dyn PadSource>) -> Self {
        Self {
            inner,
            held: VecDeque::new(),
            gated: false,
        }
    }

    pub fn as_mut(&mut self) -> &mut dyn PadSource {
        self
    }

    pub fn as_ref(&self) -> &dyn PadSource {
        self
    }

    fn pull(&mut self) -> Option<PadEvent> {
        self.inner.next_event()
    }
}

impl PadSource for PadGate {
    fn next_event(&mut self) -> Option<PadEvent> {
        self.held
            .pop_front()
            .or_else(|| (!self.gated).then(|| self.inner.next_event()).flatten())
    }

    fn pads(&self, seen: &[usize]) -> Vec<PadInfo> {
        self.inner.pads(seen)
    }

    fn api(&self) -> String {
        self.inner.api()
    }

    fn rumble(&mut self, strength: f32) {
        self.inner.rumble(strength);
    }

    fn motion(&mut self) -> Option<PadMotion> {
        self.inner.motion()
    }
}

#[derive(Default)]
pub struct BigScreen {
    nav: pad_nav::PadNav,
    keyboard: keyboard::PadKeyboard,
    families: BTreeMap<usize, Family>,
    last_pad: Option<usize>,
    ring: bool,
    keys_last: bool,
    keys_shown: bool,
    playing: bool,
    typing: bool,
    menu_pressed: bool,
    home: bool,
    home_focus: bool,
    page_step: i32,
    search: bool,
    wants_focus: bool,
    focus_menu: bool,
    focused: Option<Id>,
    recall: recall::Recall,
    details: Option<(Id, &'static str)>,
    last_input: Option<web_time::Instant>,
    clock: Option<(web_time::Instant, f64)>,
    tip_from: Option<f64>,
    tip_game: u32,
    scrolled: bool,
    layout_pad: crate::app::widgets::layout_editor::PadEdit,
    pub(super) covers: Covers,
    pub(super) missing: Vec<Id>,
    last_stepper: Option<Id>,
    last_press: Option<(Id, &'static str)>,
    pub(super) focused_own: Option<RowId>,
}

#[derive(Clone)]
pub struct Cover {
    texture: egui::TextureHandle,
    pub uv: egui::Rect,
    pub size: egui::Vec2,
    pub boxed: bool,
}

impl Cover {
    pub fn id(&self) -> egui::TextureId {
        self.texture.id()
    }
}

enum Slot {
    Loading(u64),
    Ready(Option<Cover>),
}

enum Picture {
    Box(crate::media::Frame),
    Autosave {
        size: [usize; 2],
        rgba: Vec<u8>,
        shown: f32,
    },
}

struct Landed {
    ticket: u64,
    rom_key: String,
    picture: Option<Picture>,
}

pub struct Covers {
    playing: Option<String>,
    boxes: bool,
    tickets: u64,
    loaded: std::collections::HashMap<String, Slot>,
    spawner: Option<citrine_gb_threads::Spawner>,
    landing: std::sync::mpsc::Sender<Landed>,
    landed: std::sync::mpsc::Receiver<Landed>,
}

impl Default for Covers {
    fn default() -> Self {
        let (landing, landed) = std::sync::mpsc::channel();
        Self {
            playing: None,
            boxes: false,
            tickets: 0,
            loaded: std::collections::HashMap::new(),
            spawner: None,
            landing,
            landed,
        }
    }
}

impl Covers {
    pub fn follow_spawner(&mut self, spawner: &citrine_gb_threads::Spawner) {
        if self.spawner.is_none() {
            self.spawner = Some(spawner.clone());
        }
    }

    pub fn get(
        &mut self,
        ctx: &Context,
        store: &crate::storage::SaveStore,
        rom_key: &str,
        playing: Option<&str>,
        boxes: bool,
    ) -> Option<Cover> {
        if self.playing.as_deref() != playing || self.boxes != boxes {
            self.playing = playing.map(str::to_owned);
            self.boxes = boxes;
            self.loaded.clear();
        }
        self.take_landed(ctx);
        match self.loaded.get(rom_key) {
            Some(Slot::Ready(cover)) => return cover.clone(),
            Some(Slot::Loading(_)) => return None,
            None => {}
        }
        let Some(spawner) = &self.spawner else {
            let cover = upload(ctx, rom_key, load(store, rom_key, boxes));
            self.loaded
                .insert(rom_key.to_owned(), Slot::Ready(cover.clone()));
            return cover;
        };
        self.tickets += 1;
        let ticket = self.tickets;
        self.loaded
            .insert(rom_key.to_owned(), Slot::Loading(ticket));
        let landing = self.landing.clone();
        let store = store.clone();
        let rom_key = rom_key.to_owned();
        let ctx = ctx.clone();
        spawner.run("cover picture", move || {
            let picture = load(&store, &rom_key, boxes);
            let _ = landing.send(Landed {
                ticket,
                rom_key,
                picture,
            });
            ctx.request_repaint();
        });
        None
    }

    pub fn forget(&mut self, rom_key: &str) {
        self.loaded.remove(rom_key);
    }

    fn take_landed(&mut self, ctx: &Context) {
        for landed in self.landed.try_iter() {
            let Some(slot) = self.loaded.get_mut(&landed.rom_key) else {
                continue;
            };
            if matches!(slot, Slot::Loading(ticket) if *ticket == landed.ticket) {
                *slot = Slot::Ready(upload(ctx, &landed.rom_key, landed.picture));
            }
        }
    }
}

fn load(store: &crate::storage::SaveStore, rom_key: &str, boxes: bool) -> Option<Picture> {
    boxes
        .then(|| box_art(store, rom_key))
        .flatten()
        .or_else(|| autosave_picture(store, rom_key))
}

fn box_art(store: &crate::storage::SaveStore, rom_key: &str) -> Option<Picture> {
    crate::covers::decode(&store.covers().art(rom_key)?).map(Picture::Box)
}

fn autosave_picture(store: &crate::storage::SaveStore, rom_key: &str) -> Option<Picture> {
    let info = store.newest_autosave_info(rom_key)?;
    let size = info.thumb_size();
    let two_screens = info.console == Some(citrine_gb::console::ConsoleKind::Nds);
    let shown = if two_screens { 0.5 } else { 1.0 };
    let shown_bytes = size[0] * (size[1] as f32 * shown) as usize * 4;
    if mostly_one_colour(&info.thumbnail[..shown_bytes.min(info.thumbnail.len())]) {
        return None;
    }
    Some(Picture::Autosave {
        size,
        rgba: info.thumbnail,
        shown,
    })
}

fn upload(ctx: &Context, rom_key: &str, picture: Option<Picture>) -> Option<Cover> {
    match picture? {
        Picture::Box(frame) => {
            let size = [frame.width as usize, frame.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.rgba);
            let texture = ctx.load_texture(
                format!("box_{rom_key}"),
                image,
                egui::TextureOptions::LINEAR,
            );
            Some(Cover {
                texture,
                uv: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                size: egui::vec2(frame.width as f32, frame.height as f32),
                boxed: true,
            })
        }
        Picture::Autosave { size, rgba, shown } => {
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
            let texture = ctx.load_texture(
                format!("cover_{rom_key}"),
                image,
                egui::TextureOptions::NEAREST,
            );
            Some(Cover {
                texture,
                uv: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, shown)),
                size: egui::vec2(size[0] as f32, size[1] as f32 * shown),
                boxed: false,
            })
        }
    }
}

fn mostly_one_colour(rgba: &[u8]) -> bool {
    let mut counts = std::collections::HashMap::new();
    for [r, g, b, _] in rgba.as_chunks::<4>().0 {
        *counts.entry([*r, *g, *b]).or_insert(0usize) += 1;
    }
    let most = counts.values().copied().max().unwrap_or(0);
    let total = rgba.len() / 4;
    total == 0 || most as f32 >= total as f32 * FLAT_SHARE
}

impl BigScreen {
    fn now(&mut self, time: Option<f64>) -> web_time::Instant {
        let wall = web_time::Instant::now();
        let Some(time) = time else {
            return wall;
        };
        let (start, at) = *self.clock.get_or_insert((wall, time));
        start + std::time::Duration::from_secs_f64((time - at).max(0.0))
    }

    pub fn menu_was_pressed(&mut self) -> bool {
        std::mem::take(&mut self.menu_pressed)
    }

    pub fn focus_menu(&mut self) {
        self.focus_menu = true;
    }

    pub fn take_menu_focus(&mut self) -> bool {
        std::mem::take(&mut self.focus_menu)
    }

    #[cfg(test)]
    pub(crate) fn show_ring_for_tests(&mut self) {
        self.ring = true;
    }

    pub fn home_open(&self) -> bool {
        self.home
    }

    pub fn close_home(&mut self) {
        self.home = false;
    }

    pub fn take_home_focus(&mut self) -> bool {
        std::mem::take(&mut self.home_focus)
    }
}

impl Citrine {
    pub(super) fn pad_keys_room(&self, ctx: &Context) -> f32 {
        if self.big.keys_shown {
            keyboard::room(ctx.content_rect())
        } else {
            0.0
        }
    }

    pub(super) fn pad_keys_shown(&self) -> bool {
        self.big.keys_shown
    }

    pub fn big_screen_on(&self) -> bool {
        self.ui.settings.big_screen.on || crate::platform::is_tv()
    }

    pub(super) fn set_big_screen(&mut self, on: bool) {
        self.ui.settings.big_screen.on = on;
        self.ui.settings.dirty = true;
        if on && self.game_open() {
            self.mobile.nav.show_game();
        }
    }

    pub(super) fn big_screen_input(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        let on = self.big_screen_on();
        self.pads.gated = on;
        if !on {
            return;
        }
        self.note_ui_input(raw);
        if self.editing_layout()
            && !self.naming_layout()
            && move_screen::keys(&mut self.big.layout_pad, &raw.events)
        {
            self.done_layout_edit();
        }
        if crate::platform::is_tv() {
            tv_margins(raw);
        }
        let now = self.big.now(raw.time);
        let playing = self.big.playing || self.ui.capture.wants(BindingKind::Gamepad);
        let mut commands = Vec::new();
        while let Some(event) = self.pads.pull() {
            self.note_pad(&event);
            let east = self.confirm_east(Some(event.pad));
            let pad = event.pad;
            let fed = self.big.nav.feed(event.input, playing, east, now);
            if fed.to_game {
                self.pads.held.push_back(event);
            }
            if !fed.commands.is_empty() {
                self.big.last_pad = Some(pad);
            }
            commands.extend(fed.commands);
        }
        commands.extend(self.big.nav.tick(now));
        let speed = self.big.nav.scroll_speed();
        let since = self
            .big
            .last_input
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.1));
        self.big.last_input = Some(now);
        if speed != 0.0 && !playing && !self.big.typing && !self.editing_layout() {
            self.big.ring = true;
            scroll::by_lines(ctx, speed * scroll::STICK_SPEED * since);
            ctx.request_repaint();
        }
        let (stepper, press) = ctx.data_mut(|d| {
            d.remove::<(Id, i32)>(step_id());
            let stepper = d.get_temp::<Id>(stepper_id());
            d.remove::<Id>(stepper_id());
            let press = d.get_temp::<(Id, &'static str)>(press_id());
            d.remove::<(Id, &'static str)>(press_id());
            (stepper, press)
        });
        self.big.last_stepper = stepper;
        self.big.last_press = press;
        self.big.details = ctx.data_mut(|d| {
            d.remove::<Id>(details_id());
            let offer = d.get_temp::<(Id, &'static str)>(details_offer_id());
            d.remove::<(Id, &'static str)>(details_offer_id());
            offer
        });
        for command in commands {
            self.big_command(ctx, raw, command, playing, stepper);
        }
        if self.big.nav.holding_combo() {
            ctx.request_repaint();
        }
    }

    fn note_ui_input(&mut self, raw: &egui::RawInput) {
        for event in &raw.events {
            match event {
                egui::Event::Key {
                    key:
                        egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::ArrowLeft
                        | egui::Key::ArrowRight
                        | egui::Key::Tab,
                    pressed: true,
                    ..
                } if !self.big.playing => {
                    self.big.ring = true;
                    self.big.keys_last = true;
                }
                egui::Event::PointerMoved(_)
                | egui::Event::Touch { .. }
                | egui::Event::PointerButton { .. } => self.big.ring = false,
                _ => {}
            }
        }
    }

    fn note_pad(&mut self, event: &PadEvent) {
        match event.input {
            PadInput::Disconnected => {
                self.big.families.remove(&event.pad);
            }
            _ if !self.big.families.contains_key(&event.pad) => {
                let family = self
                    .pads
                    .pads(&[event.pad])
                    .into_iter()
                    .find(|info| info.id == event.pad)
                    .map_or(Family::Plain, |info| glyphs::family_of(&info));
                self.big.families.insert(event.pad, family);
            }
            _ => {}
        }
    }

    fn pad_family(&self, pad: Option<usize>) -> Family {
        pad.or(self.big.last_pad)
            .and_then(|pad| self.big.families.get(&pad).copied())
            .unwrap_or(Family::Plain)
    }

    fn confirm_east(&self, pad: Option<usize>) -> bool {
        match self.ui.settings.big_screen.confirm {
            ConfirmButton::Bottom => false,
            ConfirmButton::Right => true,
            ConfirmButton::Automatic => self.pad_family(pad) == Family::Nintendo,
        }
    }

    fn confirm_button(&self) -> PadButton {
        if self.confirm_east(None) {
            PadButton::East
        } else {
            PadButton::South
        }
    }

    fn back_button(&self) -> PadButton {
        if self.confirm_east(None) {
            PadButton::South
        } else {
            PadButton::East
        }
    }

    pub(crate) fn shown_family(&self) -> Family {
        if self.big.keys_last {
            return Family::Keys;
        }
        match self.ui.settings.big_screen.icons {
            ButtonIcons::Xbox => Family::Xbox,
            ButtonIcons::PlayStation => Family::PlayStation,
            ButtonIcons::Nintendo => Family::Nintendo,
            ButtonIcons::Positions => Family::Plain,
            ButtonIcons::Automatic if cfg!(target_os = "android") => Family::Plain,
            ButtonIcons::Automatic => self.pad_family(None),
        }
    }

    fn big_command(
        &mut self,
        ctx: &Context,
        raw: &mut egui::RawInput,
        command: Command,
        playing: bool,
        stepper: Option<Id>,
    ) {
        self.big.ring = true;
        self.big.keys_last = false;
        if command == Command::Menu {
            self.big.menu_pressed = true;
            return;
        }
        if self.editing_layout() && !self.naming_layout() {
            match move_screen::command(&mut self.big.layout_pad, command) {
                Some(true) => self.done_layout_edit(),
                Some(false) => self.cancel_layout_edit(),
                None => {}
            }
            return;
        }
        if playing {
            return;
        }
        if self.big.typing {
            match self.big.keyboard.command(command) {
                keyboard::Typed::Events(events) => raw.events.extend(events),
                keyboard::Typed::Hide => raw.events.extend(tap(egui::Key::Escape)),
            }
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        match command {
            Command::Move(dir @ (Dir::Left | Dir::Right))
                if focused.is_some() && focused == stepper =>
            {
                let step = if dir == Dir::Left { -1 } else { 1 };
                ctx.data_mut(|d| d.insert_temp(step_id(), (focused.unwrap_or(Id::NULL), step)));
            }
            Command::Move(_) if focused.is_none() => self.big.wants_focus = true,
            Command::Move(dir) => {
                if let Some(next) = focused.and_then(|from| next_focus(ctx, from, dir)) {
                    ctx.memory_mut(|m| m.request_focus(next));
                }
            }
            Command::FaceWest
                if focused.is_some()
                    && self.big.focused_own.is_none()
                    && self.big.details.map(|(id, _)| id) == focused =>
            {
                ctx.data_mut(|d| d.insert_temp(details_id(), focused.unwrap_or(Id::NULL)));
            }
            Command::Back
                if focused.is_some()
                    && ctx.data(|d| d.get_temp::<Id>(details_open_id())) == focused =>
            {
                ctx.data_mut(|d| d.insert_temp(details_id(), focused.unwrap_or(Id::NULL)));
            }
            Command::Confirm => raw.events.extend(tap(egui::Key::Enter)),
            Command::Back => raw.events.extend(tap(egui::Key::Escape)),
            Command::PagePrevious => self.big.page_step -= 1,
            Command::PageNext => self.big.page_step += 1,
            Command::FaceNorth => self.big.search = true,
            Command::FaceWest if self.mobile.nav.settings_page().is_some() => {
                if let Some(row) = self.big.focused_own.take() {
                    self.ui.settings.drop_row(row, Surface::Phone);
                }
            }
            Command::TriggerLeft | Command::TriggerRight if self.layout_panel_games() => {
                let scope = if command == Command::TriggerLeft {
                    Scope::AllGames
                } else {
                    Scope::ThisGame
                };
                let device = self.layout_device(ctx);
                self.ui
                    .pick_in_sheet(&device, crate::app::layout_sheet::SheetPick::Games(scope));
            }
            Command::TriggerLeft | Command::TriggerRight if self.scope_bar_shown() => {
                let scope = if command == Command::TriggerLeft {
                    Scope::AllGames
                } else {
                    Scope::ThisGame
                };
                self.ui.settings.set_scope(scope);
            }
            Command::Start if !self.game_open() => self.big.menu_pressed = true,
            Command::TriggerLeft => scroll::by_pages(ctx, -1),
            Command::TriggerRight => scroll::by_pages(ctx, 1),
            _ => {}
        }
    }

    pub(super) fn big_screen_begin(&mut self, ctx: &Context) {
        if self.big.menu_was_pressed() {
            self.toggle_game_menu();
        }
        let step = std::mem::take(&mut self.big.page_step);
        if step != 0 && self.ui.viewer.is_open() {
            self.ui.viewer.queue_step(step as isize);
        } else if step != 0 && !self.overlay_open(ctx) {
            self.step_page(step);
        }
        if std::mem::take(&mut self.big.search) {
            match self.mobile.nav.place() {
                Place::Games => self.open_games_search(),
                Place::Settings if self.mobile.nav.settings_page().is_none() => {
                    self.ui.settings.focus_search = true;
                }
                _ => {}
            }
        }
        self.big
            .recall
            .drawing(recall::Screen::of(&self.mobile.nav, self.big.home));
        let focused = ctx.memory(|m| m.focused());
        if focused != self.big.focused {
            self.big.focused = focused;
            if self.big.ring && focused.is_some() {
                scroll::focus_moved(ctx);
            }
        }
        scroll::keys_shown(ctx, self.big.keys_shown);
        ring::set_shown(ctx, self.big.ring);
    }

    pub(super) fn toggle_game_menu(&mut self) {
        if !self.game_open() {
            self.big.home = !self.big.home;
            self.big.home_focus = self.big.home;
            return;
        }
        self.big.home = false;
        let nav = &self.mobile.nav;
        if nav.on_game() && nav.more_open() {
            self.resume_game();
        } else {
            self.open_game_menu();
        }
    }

    fn overlay_open(&self, ctx: &Context) -> bool {
        self.big.home_open() || ctx.memory(|m| m.top_modal_layer().is_some())
    }

    fn step_page(&mut self, step: i32) {
        self.big.recall.stepped();
        if let Some(page) = self.mobile.nav.settings_page() {
            let next = stepped_page(page, step, self.ui.settings.dev_mode);
            self.mobile.nav.open_settings(next);
            return;
        }
        let list = self.tab_list();
        let count = list.len() as i32;
        let at = self
            .current_tab()
            .and_then(|tab| list.iter().position(|t| *t == tab));
        let next = match at {
            Some(at) => (at as i32 + step).rem_euclid(count),
            None if step > 0 => 0,
            None => count - 1,
        };
        self.pick_tab(list[next as usize]);
    }

    fn tab_list(&self) -> Vec<tabs::Tab> {
        let game = self.game_open().then_some(tabs::Tab::Game);
        game.into_iter()
            .chain([Place::Games, Place::Media, Place::Settings, Place::You].map(tabs::Tab::Place))
            .collect()
    }

    fn current_tab(&self) -> Option<tabs::Tab> {
        let nav = &self.mobile.nav;
        if nav.on_game() {
            Some(tabs::Tab::Game)
        } else if nav.place() == Place::Friends {
            None
        } else {
            Some(tabs::Tab::Place(nav.place()))
        }
    }

    fn pick_tab(&mut self, tab: tabs::Tab) {
        self.big.home = false;
        let nav = &self.mobile.nav;
        let library = nav.on_game() || nav.place() != Place::Games;
        match tab {
            tabs::Tab::Game => self.open_game_menu(),
            tabs::Tab::Place(Place::Games) if self.game_open() && library => {
                self.mobile.nav.go_library();
                self.release_touch();
            }
            tabs::Tab::Place(place) => self.go_place(place),
        }
    }

    pub(super) fn big_tab_bar(&mut self, ui: &mut Ui) {
        let current = self.current_tab();
        let face = self
            .online
            .account()
            .filter(|_| self.ui.settings.online.is_joined())
            .map(|account| (account.display_name.clone(), account.online_id.value()));
        let update = self.ui.update.phase().waiting();
        let entries: Vec<tabs::Entry> = self
            .tab_list()
            .into_iter()
            .map(|tab| {
                let (label, icon) = match tab {
                    tabs::Tab::Game => (self.emulator.game_name().to_owned(), crate::icons::PLAY),
                    tabs::Tab::Place(place) => (place.label().to_owned(), place.icon()),
                };
                tabs::Entry {
                    tab,
                    label,
                    icon,
                    selected: current == Some(tab),
                    dot: (tab == tabs::Tab::Place(Place::Settings) && update)
                        .then_some(crate::app::widgets::avatar::DOT),
                    face: face.clone().filter(|_| tab == tabs::Tab::Place(Place::You)),
                }
            })
            .collect();
        set_named_tab(
            ui.ctx(),
            entries
                .iter()
                .find(|entry| entry.selected)
                .map(|entry| entry.label.clone()),
        );
        let lift = self.overlay_open(ui.ctx());
        let marks = self.mobile.nav.settings_page().is_none() && !self.ui.viewer.is_open() && !lift;
        let family = self.shown_family();
        let confirm = self.confirm_button();
        if let Some(tab) = tabs::bar(ui, &entries, marks, family, confirm, lift) {
            self.pick_tab(tab);
        }
    }

    pub(super) fn big_screen_end(&mut self, ctx: &Context) {
        if !self.big_screen_on() {
            self.big = BigScreen {
                covers: std::mem::take(&mut self.big.covers),
                ..BigScreen::default()
            };
            crate::platform::pad_keyboard(false);
            return;
        }
        let nav = &self.mobile.nav;
        let typing = ctx.output(|o| o.ime.is_some());
        if typing && !self.big.typing {
            self.big.keyboard.reset();
        }
        self.big.typing = typing;
        let in_game = nav.on_game() && !nav.more_open() && self.game_open();
        self.big.playing = in_game && !self.editing_layout();
        let paging = !in_game && !typing;
        let scrolled = scroll::end_pass(ctx, paging);
        if paging && self.big.ring && std::mem::replace(&mut self.big.scrolled, scrolled) {
            scroll::refocus_in_view(ctx);
        }
        let screen = recall::Screen::of(nav, self.big.home);
        let back_to = self
            .big
            .recall
            .end(ctx, screen, !in_game && self.big.ring && !typing);
        let menu_open = nav.on_game() && nav.more_open() && self.game_open();
        if menu_open && !self.ui.settings.big_screen.menu_opened {
            self.ui.settings.big_screen.menu_opened = true;
            self.ui.settings.dirty = true;
        }
        let time = ctx.input(|i| i.time);
        let started = self.emulator.games_started();
        if !self.game_open() || std::mem::replace(&mut self.big.tip_game, started) != started {
            self.big.tip_from = None;
        }
        if self.big.playing && self.big.tip_from.is_none() {
            self.big.tip_from = Some(time);
        }
        if in_game {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        } else if let Some(id) = back_to {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
        } else if self.big.ring
            && !typing
            && !self.big.wants_focus
            && ctx.memory(|m| m.focused().is_none())
            && let Some(near) = self
                .big
                .focused
                .and_then(|old| nearest_to_disabled(ctx, old))
        {
            ctx.memory_mut(|m| m.request_focus(near));
            ctx.request_repaint();
        } else if self.big.ring
            && !typing
            && (self.big.wants_focus
                || ctx.memory(|m| m.focused().is_none())
                || focus_under_modal(ctx))
            && let Some(first) = first_focus(ctx, Pass::This)
        {
            ctx.memory_mut(|m| m.request_focus(first));
            ctx.request_repaint();
        }
        self.big.wants_focus = false;
        let pad_keyboard = typing && self.big.ring && !self.big.keys_last;
        crate::platform::pad_keyboard(pad_keyboard);
        if pad_keyboard != self.big.keys_shown {
            self.big.keys_shown = pad_keyboard;
            ctx.request_repaint();
        }
        let family = self.shown_family();
        let confirm = self.confirm_button();
        if pad_keyboard {
            self.big.keyboard.show(ctx, family, confirm);
        }
        if self.big.ring && !in_game && !pad_keyboard {
            ring::paint(ctx);
        }
        if let Some(tip) = self.menu_tip(ctx) {
            menu_tip_bar(ctx, &tip, family, confirm);
            let left = self
                .big
                .tip_from
                .map_or(0.0, |from| from + MENU_TIP_SECONDS - time);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.0)));
        }
        let now = self.big.last_input.unwrap_or_else(web_time::Instant::now);
        if let Some(progress) = self.big.nav.hold_progress(now) {
            let screen = ctx.content_rect();
            let centre = egui::pos2(screen.right() - 48.0, screen.top() + 48.0);
            egui::Area::new(Id::new("menu_hold"))
                .order(egui::Order::Tooltip)
                .fixed_pos(screen.min)
                .interactable(false)
                .show(ctx, |ui| glyphs::hold_ring(ui, centre, progress));
        }
    }

    fn layout_panel_games(&self) -> bool {
        self.mobile.nav.more_open()
            && self.ui.layout_sheet.is_some()
            && self.ui.settings.playing_key().is_some()
    }

    pub(super) fn scope_bar_shown(&self) -> bool {
        self.mobile.nav.settings_page().is_some_and(|tab| {
            crate::app::widgets::settings::scope::shows_bar(&self.ui.settings, tab, Surface::Phone)
        })
    }

    fn menu_tip(&self, ctx: &Context) -> Option<Hint> {
        let time = ctx.input(|i| i.time);
        let fresh = self
            .big
            .tip_from
            .is_some_and(|from| time - from < MENU_TIP_SECONDS);
        let tip = Hint::new([Glyph::Select, Glyph::Start], "Hold for menu");
        let family = self.shown_family();
        let confirm = self.confirm_button();
        let drawn = tip
            .glyphs
            .iter()
            .all(|glyph| glyphs::shown(family, *glyph, confirm));
        (self.big_screen_on()
            && self.big.playing
            && fresh
            && drawn
            && !self.ui.settings.big_screen.menu_opened)
            .then_some(tip)
    }

    pub(super) fn big_hints(&self, ui: &mut Ui) {
        let focused = ui.ctx().memory(|m| m.focused());
        let hints = self.hint_list(ui.ctx(), focused);
        glyphs::hint_bar(ui, &hints, self.shown_family(), self.confirm_button());
    }

    fn hint_list(&self, ctx: &Context, focused: Option<Id>) -> Vec<Hint> {
        let confirm = self.confirm_button();
        let back = self.back_button();
        let nav = &self.mobile.nav;
        let mut hints = if self.big.typing && !self.big.keys_last {
            vec![
                Hint::new([Glyph::Face(confirm)], "Type"),
                Hint::new([Glyph::Face(back)], "Hide keyboard"),
            ]
        } else if self.ui.viewer.is_open() {
            vec![
                Hint::new([Glyph::Face(confirm)], "Choose"),
                Hint::new([Glyph::Face(back)], "Close"),
                Hint::new(
                    [
                        Glyph::Shoulder(PadButton::LeftTrigger),
                        Glyph::Shoulder(PadButton::RightTrigger),
                    ],
                    "Previous, next",
                ),
            ]
        } else if ctx.memory(|m| m.top_modal_layer().is_some()) {
            vec![
                Hint::new([Glyph::Face(confirm)], "Choose"),
                Hint::new([Glyph::Face(back)], "Close"),
            ]
        } else if nav.on_game() && nav.more_open() {
            let back_to = if self.ui.layout_sheet.is_some() {
                "Back"
            } else {
                "Back to game"
            };
            vec![
                Hint::new([Glyph::Face(confirm)], "Choose"),
                Hint::new([Glyph::Face(back)], back_to),
            ]
        } else if nav.settings_page().is_some() {
            let mut hints = Vec::new();
            if let Some((id, press)) = self.big.last_press
                && focused == Some(id)
            {
                hints.push(Hint::new([Glyph::Face(confirm)], press));
            }
            if focused.is_some() && focused == self.big.last_stepper {
                hints.push(Hint::new([Glyph::Sideways], "Change"));
            }
            if self.big.focused_own.is_some() {
                hints.push(Hint::new([Glyph::Face(PadButton::West)], USE_ALL_HINT));
            }
            hints.push(Hint::new([Glyph::Face(back)], "Back"));
            hints.push(Hint::new(
                [
                    Glyph::Shoulder(PadButton::LeftTrigger),
                    Glyph::Shoulder(PadButton::RightTrigger),
                ],
                "Pages",
            ));
            hints
        } else {
            let missing = focused.is_some_and(|id| self.big.missing.contains(&id));
            let open = if missing { "Remove" } else { OPEN };
            let mut hints = vec![Hint::new([Glyph::Face(confirm)], open)];
            if self.mobile.nav.depth() > 0 {
                hints.push(Hint::new([Glyph::Face(back)], "Back"));
            }
            let games_search =
                nav.place() == Place::Games && nav.page().is_none() && self.games_searchable();
            if games_search || nav.place() == Place::Settings {
                hints.push(Hint::new([Glyph::Face(PadButton::North)], "Search"));
            }
            hints
        };
        if let Some((id, label)) = self.big.details
            && Some(id) == focused
            && self.big.focused_own.is_none()
        {
            let at = hints.len().min(1);
            hints.insert(at, Hint::new([Glyph::Face(PadButton::West)], label));
        }
        hints
    }
}

fn menu_tip_bar(ctx: &Context, tip: &Hint, family: Family, confirm: PadButton) {
    let fill = crate::app::mobile::kit::card_fill(&ctx.style().visuals);
    let margin = (MENU_TIP_HEIGHT - 20.0) / 2.0;
    egui::Area::new(Id::new("big_menu_tip"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::Vec2::ZERO)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(16, margin as i8))
                .corner_radius(egui::CornerRadius {
                    nw: 16,
                    ..egui::CornerRadius::ZERO
                })
                .show(ui, |ui| glyphs::hint(ui, tip, family, confirm));
        });
}

fn tap(key: egui::Key) -> [egui::Event; 2] {
    [true, false].map(|pressed| egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    })
}

fn tv_margins(raw: &mut egui::RawInput) {
    let insets = raw.safe_area_insets.get_or_insert_default();
    insets.0.left = insets.0.left.max(TV_SIDES);
    insets.0.right = insets.0.right.max(TV_SIDES);
    insets.0.top = insets.0.top.max(TV_ENDS);
    insets.0.bottom = insets.0.bottom.max(TV_ENDS);
}

pub fn layer_open(ctx: &Context) -> bool {
    egui::Popup::is_any_open(ctx)
        || ctx.memory(|m| m.top_modal_layer().is_some())
        || ctx.output(|o| o.ime.is_some())
        || crate::app::widgets::confirm::is_open(ctx)
}

const DARK_DIM: u8 = 150;
const FIRST_FOCUS_MIN_WIDTH: f32 = 48.0;
const FIRST_FOCUS_MIN_HEIGHT: f32 = 24.0;
const WIDE_ROW_SHARE: f32 = 0.5;
const STOP_MIN_SIDE: f32 = 6.0;

pub(super) fn is_stop(widget: &egui::WidgetRect) -> bool {
    let rect = widget.rect;
    widget.sense.is_focusable()
        && widget.enabled
        && (widget.sense.senses_click() || !widget.sense.senses_drag())
        && rect.width().min(rect.height()) >= STOP_MIN_SIDE
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pass {
    Last,
    This,
}

fn focus_candidates(ctx: &Context, pass: Pass) -> Vec<(Id, egui::Rect, bool)> {
    let modal = ctx.memory(|m| m.top_modal_layer());
    let toasts = crate::app::toasts::layers(ctx);
    let candidates = ctx.viewport(|viewport| {
        let widgets = match pass {
            Pass::Last => &viewport.prev_pass.widgets,
            Pass::This => &viewport.this_pass.widgets,
        };
        let focusable = |layer: &egui::LayerId| {
            widgets
                .get_layer(*layer)
                .filter(|widget| is_stop(widget))
                .map(|widget| (widget.id, widget.rect, *layer, widget.sense))
                .collect::<Vec<_>>()
        };
        match modal {
            Some(layer) => focusable(&layer),
            None => widgets
                .layers()
                .flat_map(|(layer, _)| focusable(layer))
                .collect(),
        }
    });
    let (toast, rest): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|(_, _, layer, _)| toasts.contains(layer));
    let Some(top) = rest
        .iter()
        .map(|(_, _, layer, _)| layer.order)
        .max()
        .or_else(|| toast.iter().map(|(_, _, layer, _)| layer.order).max())
    else {
        return Vec::new();
    };
    let top: Vec<(Id, egui::Rect, bool, bool)> = rest
        .into_iter()
        .filter(|(_, _, layer, _)| layer.order == top)
        .chain(if top <= egui::Order::Middle {
            toast
        } else {
            Vec::new()
        })
        .map(|(id, rect, layer, sense)| (id, rect, sense.senses_click(), toasts.contains(&layer)))
        .collect();
    top.iter()
        .filter(|(id, rect, clicks, _)| {
            *clicks
                || !top
                    .iter()
                    .any(|(other, inner, _, _)| other != id && rect.contains_rect(*inner))
        })
        .map(|(id, rect, _, toast)| (*id, *rect, *toast))
        .collect()
}

fn focus_under_modal(ctx: &Context) -> bool {
    ctx.memory(|m| m.top_modal_layer()).is_some()
        && ctx.memory(|m| m.focused()).is_some_and(|id| {
            [Pass::Last, Pass::This].into_iter().all(|pass| {
                !focus_candidates(ctx, pass)
                    .iter()
                    .any(|(candidate, _, _)| *candidate == id)
            })
        })
}

fn small_stop(rect: &egui::Rect) -> bool {
    rect.width() < FIRST_FOCUS_MIN_WIDTH || rect.height() < FIRST_FOCUS_MIN_HEIGHT
}

fn in_content(ctx: &Context, rect: &egui::Rect) -> bool {
    rect.top() >= tab_bottom(ctx) - 1.0
}

fn first_focus(ctx: &Context, pass: Pass) -> Option<Id> {
    let content = |rect: &egui::Rect| in_content(ctx, rect);
    let key = |rect: &egui::Rect| (rect.top() / 8.0).round() * 10_000.0 + rect.left();
    let candidates = focus_candidates(ctx, pass);
    let now = ctx.cumulative_pass_nr();
    let chosen = ctx
        .data(|d| d.get_temp::<(u64, Id)>(first_choice_id()))
        .filter(|(at, _)| at + 1 >= now)
        .map(|(_, id)| id);
    if let Some(id) = chosen.filter(|id| candidates.iter().any(|(c, _, _)| c == id)) {
        return Some(id);
    }
    candidates
        .into_iter()
        .min_by(|a, b| {
            a.2.cmp(&b.2)
                .then((!content(&a.1)).cmp(&!content(&b.1)))
                .then(small_stop(&a.1).cmp(&small_stop(&b.1)))
                .then(key(&a.1).total_cmp(&key(&b.1)))
        })
        .map(|(id, _, _)| id)
}

fn next_focus(ctx: &Context, from: Id, dir: Dir) -> Option<Id> {
    let (toast, page): (Vec<_>, Vec<_>) = focus_candidates(ctx, Pass::Last)
        .into_iter()
        .partition(|(_, _, toast)| *toast);
    let toast: Vec<_> = toast.into_iter().map(|(id, rect, _)| (id, rect)).collect();
    let page: Vec<_> = page.into_iter().map(|(id, rect, _)| (id, rect)).collect();
    let response = ctx.read_response(from)?;
    if crate::app::toasts::layers(ctx).contains(&response.layer_id) {
        let area = ctx
            .memory(|m| m.area_rect(response.layer_id.id))
            .unwrap_or(response.rect);
        return nearest(toast, from, response.rect, dir).or_else(|| nearest(page, from, area, dir));
    }
    nearest(page, from, response.rect, dir).or_else(|| nearest(toast, from, response.rect, dir))
}

fn nearest(
    candidates: Vec<(Id, egui::Rect)>,
    from: Id,
    from_rect: egui::Rect,
    dir: Dir,
) -> Option<Id> {
    let ahead = |rect: &egui::Rect| match dir {
        Dir::Down => rect.top() - from_rect.bottom(),
        Dir::Up => from_rect.top() - rect.bottom(),
        Dir::Right => rect.left() - from_rect.right(),
        Dir::Left => from_rect.left() - rect.right(),
    };
    let across = |rect: &egui::Rect| {
        let (lo, hi, from_lo, from_hi) = match dir {
            Dir::Down | Dir::Up => (
                rect.left(),
                rect.right(),
                from_rect.left(),
                from_rect.right(),
            ),
            Dir::Left | Dir::Right => (
                rect.top(),
                rect.bottom(),
                from_rect.top(),
                from_rect.bottom(),
            ),
        };
        (lo - from_hi).max(from_lo - hi).max(0.0)
    };
    let far = |rect: &egui::Rect| match dir {
        Dir::Down => rect.bottom() - from_rect.bottom(),
        Dir::Up => from_rect.top() - rect.top(),
        Dir::Right => rect.right() - from_rect.right(),
        Dir::Left => from_rect.left() - rect.left(),
    };
    let vertical = matches!(dir, Dir::Down | Dir::Up);
    let in_beam = |rect: &egui::Rect| across(rect) <= 0.0;
    let widest = candidates
        .iter()
        .map(|(_, rect)| rect.width())
        .fold(0.0, f32::max);
    let wide_row = vertical && from_rect.width() >= WIDE_ROW_SHARE * widest;
    let bigger_ahead = wide_row
        && candidates
            .iter()
            .any(|(id, rect)| *id != from && ahead(rect) > -1.0 && !small_stop(rect));
    let beside = |rect: &egui::Rect| across(rect) < from_rect.width();
    let overlap = |rect: &egui::Rect| match dir {
        Dir::Down | Dir::Up => {
            rect.right().min(from_rect.right()) - rect.left().max(from_rect.left())
        }
        Dir::Left | Dir::Right => {
            rect.bottom().min(from_rect.bottom()) - rect.top().max(from_rect.top())
        }
    };
    let score = |rect: &egui::Rect| ahead(rect).max(0.0) + 2.0 * across(rect);
    let rank = |a: &egui::Rect, b: &egui::Rect| {
        score(a)
            .total_cmp(&score(b))
            .then(overlap(b).total_cmp(&overlap(a)))
            .then(a.left().total_cmp(&b.left()))
            .then(a.top().total_cmp(&b.top()))
    };
    let (beam, off): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, rect)| ahead(rect) > -1.0)
        .filter(|(_, rect)| !(bigger_ahead && small_stop(rect)))
        .partition(|(_, rect)| !vertical || in_beam(rect));
    let best = |list: Vec<(Id, egui::Rect)>| list.into_iter().min_by(|a, b| rank(&a.1, &b.1));
    let near = best(
        off.iter()
            .copied()
            .filter(|(_, rect)| beside(rect))
            .collect(),
    );
    match (best(beam), near, best(off)) {
        (Some(a), Some(b), _) if far(&b.1) <= ahead(&a.1) => Some(b.0),
        (Some(a), _, _) => Some(a.0),
        (None, _, b) => b.map(|(id, _)| id),
    }
}

fn nearest_to_disabled(ctx: &Context, old: Id) -> Option<Id> {
    let (from, disabled) = ctx.viewport(|viewport| {
        let widgets = &viewport.this_pass.widgets;
        let from = widgets
            .get(old)
            .filter(|widget| !widget.enabled)
            .map(|widget| widget.rect.center());
        let disabled: Vec<Id> = widgets
            .layers()
            .flat_map(|(_, layer)| layer.iter())
            .filter(|widget| !widget.enabled)
            .map(|widget| widget.id)
            .collect();
        (from, disabled)
    });
    let from = from?;
    focus_candidates(ctx, Pass::Last)
        .into_iter()
        .filter(|(id, _, _)| *id != old && !disabled.contains(id))
        .min_by(|a, b| {
            (a.1.center() - from)
                .length_sq()
                .total_cmp(&(b.1.center() - from).length_sq())
        })
        .map(|(id, _, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::phone_app;
    use crate::app::ui_state::settings_tree::SettingsTab;
    use crate::pad_input::PadKey;
    use citrine_gb::gb::GbModel;
    use citrine_gb::rom::Rom;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct Pads(Rc<RefCell<VecDeque<PadEvent>>>);

    impl PadSource for Pads {
        fn next_event(&mut self) -> Option<PadEvent> {
            self.0.borrow_mut().pop_front()
        }

        fn pads(&self, _seen: &[usize]) -> Vec<PadInfo> {
            Vec::new()
        }

        fn api(&self) -> String {
            "fake".to_owned()
        }
    }

    struct Couch {
        app: Citrine,
        ctx: Context,
        pads: Pads,
        size: egui::Vec2,
        time: f64,
        storage: bool,
        asking: bool,
        data: bool,
        toasts: bool,
        touch: bool,
    }

    impl Couch {
        fn new(name: &str) -> Self {
            Self::sized(name, egui::vec2(1280.0, 720.0))
        }

        fn sized(name: &str, size: egui::Vec2) -> Self {
            let mut app = phone_app(name);
            app.ui.settings.big_screen.on = true;
            let pads = Pads::default();
            app.pads = PadGate::new(Box::new(pads.clone()));
            let mut couch = Self {
                app,
                ctx: Context::default(),
                pads,
                size,
                time: 0.0,
                storage: false,
                asking: false,
                data: false,
                toasts: false,
                touch: false,
            };
            couch.frame();
            couch
        }

        fn playing(name: &str) -> Self {
            let mut couch = Self::new(name);
            couch.start_game();
            couch
        }

        fn start_game(&mut self) {
            self.app
                .emulator
                .load_rom(
                    &Rom::new(&[0u8; 0x8000]),
                    GbModel::Dmg,
                    #[cfg(not(target_arch = "wasm32"))]
                    None,
                )
                .expect("load");
            self.app.emulator.running = true;
            self.app.mobile.nav.show_game();
            self.frame();
        }

        fn frame(&mut self) {
            self.frame_with(Vec::new());
        }

        fn frame_with(&mut self, events: Vec<egui::Event>) {
            self.output_with(events);
        }

        fn wait(&mut self, seconds: f64) {
            self.time += seconds;
        }

        fn output_with(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            self.time += 1.0 / 60.0;
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let app = &mut self.app;
            let storage = self.storage;
            let toasts = self.toasts;
            let touch = self.touch;
            app.big_screen_input(&self.ctx, &mut raw);
            let asking = self.asking;
            let data = self.data;
            self.ctx.run(raw, |ctx| {
                if asking {
                    second_screen_ask(ctx);
                }
                if touch {
                    crate::app::mobile::touch_spacing(ctx, app.is_mobile(ctx));
                }
                app.render(ctx);
                app.media_viewer(ctx);
                if storage {
                    app.handle_storage(ctx);
                }
                if data {
                    app.handle_your_data(ctx);
                }
                if toasts {
                    app.show_toasts(ctx);
                }
                app.mobile_escape(ctx);
                app.big_screen_end(ctx);
            })
        }

        fn press(&mut self, button: PadButton) {
            let key = PadKey::Known(button);
            self.pads
                .0
                .borrow_mut()
                .extend(
                    [PadInput::Pressed(key), PadInput::Released(key)].map(|input| PadEvent {
                        pad: 0,
                        input,
                        code: None,
                    }),
                );
            self.frame();
            self.frame();
        }

        fn typed(&mut self, text: &str) {
            self.frame_with(vec![egui::Event::Text(text.to_owned())]);
            self.frame();
        }

        fn focused(&self) -> Option<Id> {
            self.ctx.memory(|m| m.focused())
        }

        fn focused_text(&mut self) -> String {
            let output = self.output_with(Vec::new());
            let rect = self.focused_rect();
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text)
                        if rect.contains(text.visual_bounding_rect().center()) =>
                    {
                        Some(text.galley.text().trim().to_owned())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ")
        }

        fn page_tops(&self) -> Vec<(Id, f32)> {
            let bar = tab_bottom(&self.ctx);
            self.ctx.viewport(|v| {
                v.prev_pass
                    .widgets
                    .layers()
                    .filter(|(layer, _)| layer.order == egui::Order::Background)
                    .flat_map(|(_, widgets)| widgets.iter())
                    .filter(|w| w.rect.top() >= bar - 1.0)
                    .map(|w| (w.id, w.rect.top()))
                    .collect()
            })
        }

        fn on_stepper(&self) -> bool {
            self.focused().is_some()
                && self.ctx.data(|d| d.get_temp::<Id>(stepper_id())) == self.focused()
        }
    }

    impl Couch {
        fn long_games(name: &str, size: egui::Vec2) -> Self {
            let mut couch = Self::sized(name, size);
            couch.fill_library("game", 30);
            couch.frame();
            couch
        }

        fn fill_library(&mut self, stem: &str, count: usize) {
            use crate::app::ui_state::library::FileFound;
            let library = &mut self.app.ui.library;
            library.add_folder("/games");
            let files = (0..count)
                .map(|n| FileFound {
                    name: format!("{stem} {n}"),
                    path: format!("/games/{stem}{n}.gb"),
                })
                .collect();
            library.apply_listing("/games", Some(files), 0);
        }

        fn page_rect(&self) -> egui::Rect {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, self.size);
            screen.with_max_y(screen.bottom() - 40.0)
        }

        fn focused_rect(&self) -> egui::Rect {
            self.focused()
                .and_then(|id| self.ctx.read_response(id))
                .expect("something has focus")
                .rect
        }

        fn row_tops(&self) -> Vec<f32> {
            self.ctx.viewport(|v| {
                v.prev_pass
                    .widgets
                    .layers()
                    .flat_map(|(_, widgets)| widgets.iter())
                    .filter(|w| w.sense.is_focusable())
                    .map(|w| w.rect.top())
                    .collect()
            })
        }

        fn settle(&mut self) {
            for _ in 0..20 {
                self.frame();
            }
        }

        fn move_stick(&mut self, value: f32) {
            self.pads.0.borrow_mut().push_back(PadEvent {
                pad: 0,
                input: PadInput::Moved(crate::pad_input::PadAxis::RightStickY, value),
                code: None,
            });
        }
    }

    fn second_screen_ask(ctx: &Context) {
        use crate::app::widgets::confirm::{Ask, ask_as};
        use crate::app::widgets::security_code::{SecurityCode, TITLE};
        ask_as(
            ctx,
            "couch_second_screen_ask",
            Ask {
                message: "Pixel 8 wants to be a second screen",
                list: &[],
                notes: &[],
                action: "Allow",
                action_enabled: true,
                extra: None,
                load: None,
                code: Some(SecurityCode {
                    title: TITLE,
                    hint: "If Pixel 8 shows other digits, choose Not now.",
                    lines: &["48392".to_owned()],
                    compare: Some("Allow if Pixel 8 shows the same digits"),
                }),
            },
            "Not now",
        );
    }

    #[test]
    fn a_confirm_sheet_starts_on_its_safe_choice_with_the_action_one_press_away() {
        let mut couch = Couch::new("big-ask-first");
        couch.press(PadButton::DPadDown);
        couch.asking = true;
        couch.settle();
        assert_eq!(couch.focused_text(), "Not now");
        couch.press(PadButton::DPadUp);
        assert_eq!(couch.focused_text(), "Allow", "allow is one press away");
        couch.press(PadButton::DPadUp);
        assert_eq!(couch.focused_text(), crate::icons::INFO);
        let (mark, ring) = ring::current(&couch.ctx).expect("a focused widget");
        let (ring, _) = ring.expect("a ring on the info mark");
        assert!(
            (ring.width() - ring.height()).abs() < 0.5 && mark.rect.contains(ring.center()),
            "the ring {ring:?} is round on the mark, not its touch square {:?}",
            mark.rect
        );
        let output = couch.output_with(Vec::new());
        let title = crate::app::tests::text_rect(&output, "Pixel 8 wants to be a second screen")
            .expect("the title is drawn");
        let hint = couch
            .ctx
            .memory(|m| m.area_rect(egui::Popup::default_response_id(&mark)))
            .expect("the hint is open on the focused mark");
        assert!(
            hint.contains_rect(title),
            "the hint {hint:?} covers the title {title:?}"
        );
        assert!(
            hint.bottom() < ring.top() - ring::WIDTH,
            "the hint {hint:?} ends above the ring {ring:?}"
        );
        couch.press(PadButton::DPadUp);
        assert_eq!(
            couch.focused_text(),
            crate::icons::INFO,
            "the grab handle is not in the pad order"
        );

        let mut couch = Couch::new("big-delete-first");
        couch.press(PadButton::DPadDown);
        couch.data = true;
        couch.app.ui.your_data.request =
            Some(crate::app::widgets::settings::your_data::Request::Delete);
        couch.settle();
        assert_eq!(couch.focused_text(), "Cancel");
        couch.press(PadButton::DPadUp);
        assert_eq!(
            couch.focused_text(),
            "Cancel",
            "with nothing stored, Delete everything and Export first are off"
        );
        let saves = crate::your_data::Kind::of(crate::storage::kinds::Kind::Saves);
        let tally = crate::your_data::Tally { count: 2 };
        couch.app.ui.your_data.rows = vec![(saves, tally)];
        couch.settle();
        couch.press(PadButton::DPadUp);
        assert_eq!(couch.focused_text(), "Delete everything");
        couch.press(PadButton::DPadUp);
        assert_eq!(couch.focused_text(), "Export first");
    }

    #[test]
    fn pad_steps_in_a_sheet_never_move_the_page_under_it() {
        let mut couch = Couch::long_games("big-ask-page", egui::vec2(1280.0, 720.0));
        couch.press(PadButton::DPadDown);
        couch.settle();
        let page = couch.page_tops();
        assert!(!page.is_empty());
        couch.asking = true;
        couch.settle();
        assert_eq!(couch.page_tops(), page, "the sheet opens over a still page");
        for button in [
            PadButton::DPadUp,
            PadButton::DPadUp,
            PadButton::DPadDown,
            PadButton::DPadDown,
            PadButton::DPadDown,
        ] {
            couch.press(button);
            couch.settle();
            assert_eq!(couch.page_tops(), page, "{button:?} keeps the page still");
        }
    }

    #[test]
    fn focus_moving_off_the_screen_scrolls_it_into_view() {
        for size in [egui::vec2(960.0, 540.0), egui::vec2(1280.0, 720.0)] {
            let mut couch = Couch::long_games("big-follow", size);
            let page = couch.page_rect();
            let mut seen = Vec::new();
            for _ in 0..40 {
                couch.press(PadButton::DPadDown);
                couch.settle();
                let rect = couch.focused_rect();
                assert!(
                    page.contains_rect(rect),
                    "the focused {rect:?} is in view {page:?} on {size:?}"
                );
                seen.extend(couch.focused().filter(|id| !seen.contains(id)));
            }
            assert!(seen.len() > 10, "focus walked down the page on {size:?}");
            for _ in 0..40 {
                couch.press(PadButton::DPadUp);
                couch.settle();
                let rect = couch.focused_rect();
                assert!(page.contains_rect(rect), "back up, {rect:?} is in view");
            }
        }
    }

    #[test]
    fn a_focused_row_under_a_page_toast_scrolls_clear_of_it() {
        let mut couch = Couch::long_games("big-toast-focus", egui::vec2(1280.0, 720.0));
        couch.toasts = true;
        let start = web_time::Instant::now();
        let progress = crate::progress::Progress::unknown();
        for at in [0, 1] {
            couch.app.toasts.load(
                "hold",
                "Getting your save from Steam Deck",
                progress,
                start + std::time::Duration::from_secs(at),
            );
        }
        couch.app.toasts.load_note(
            "hold",
            "The game starts when it arrives, or after 4 seconds",
        );
        couch.settle();
        let mut seen = Vec::new();
        let mut near = 0;
        let mut on_toast = 0;
        for _ in 0..60 {
            couch.press(PadButton::DPadDown);
            couch.settle();
            let toasts = couch.app.toasts.rects(&couch.ctx);
            assert_eq!(toasts.len(), 1, "the toast stays");
            let toast = toasts[0];
            let rect = couch.focused_rect();
            if toast.contains_rect(rect) {
                assert!(
                    rect.area() < toast.area() / 4.0,
                    "focus sits on a toast button, {rect:?}"
                );
                on_toast += 1;
                continue;
            }
            assert_eq!(on_toast, 0, "focus stays on the toast at the page end");
            assert!(
                rect.bottom() <= toast.top(),
                "the focused {rect:?} sits clear of the toast {toast:?}"
            );
            if rect.bottom() > toast.top() - 80.0 {
                near += 1;
            }
            seen.extend(couch.focused().filter(|id| !seen.contains(id)));
        }
        assert!(seen.len() > 10, "focus walked down the page");
        assert!(near > 5, "focus moved along the toast's top edge");
        assert!(
            on_toast > 0,
            "focus reached the toast's close button at the page end"
        );
        couch.press(PadButton::DPadUp);
        couch.settle();
        assert_eq!(
            couch.focused(),
            seen.last().copied(),
            "up from the toast goes back to the last row, {:?}",
            couch.focused_rect()
        );
    }

    #[test]
    fn choosing_a_page_keeps_focus_on_the_pager() {
        let mut couch = Couch::long_games("big-pager", egui::vec2(960.0, 540.0));
        couch.app.big.ring = true;
        couch.settle();
        let mut trail = Vec::new();
        let mut past_tiles = false;
        for _ in 0..40 {
            couch.press(PadButton::DPadDown);
            couch.settle();
            let rect = couch.focused_rect();
            trail.push(rect);
            past_tiles |= rect.width() > 200.0 && rect.height() > 200.0;
            if past_tiles && rect.width() < 100.0 && rect.height() < 80.0 {
                break;
            }
        }
        let cell = couch.focused_rect();
        assert!(
            cell.width() < 100.0 && cell.height() < 80.0,
            "Down from the right-hand tiles reaches the pager, walked {trail:?}"
        );
        let mut along = vec![cell];
        for _ in 0..6 {
            if couch.focus_holds(crate::icons::CARET_RIGHT) {
                break;
            }
            couch.press(PadButton::DPadRight);
            couch.settle();
            along.push(couch.focused_rect());
        }
        assert!(
            couch.focus_holds(crate::icons::CARET_RIGHT),
            "Right reaches Next, walked {along:?}"
        );
        assert_eq!(couch.app.ui.games.page, 0);
        let next = couch.focused_rect();
        couch.press(PadButton::South);
        couch.settle();
        assert_eq!(couch.app.ui.games.page, 1, "Next turns the page");
        let held = couch.focused().and_then(|id| couch.rect_of(id));
        assert!(
            held.is_some_and(|rect| (rect.size() - next.size()).length() < 1.0
                && (rect.center().x - next.center().x).abs() < 1.0),
            "focus stays on Next, {held:?} against {next:?}"
        );
        couch.press(PadButton::DPadLeft);
        couch.settle();
        couch.press(PadButton::DPadLeft);
        couch.settle();
        couch.press(PadButton::South);
        couch.settle();
        assert_eq!(
            couch.app.ui.games.page, 0,
            "the pager still works from there"
        );
        assert!(couch.focused().is_some(), "focus stays on the pager");
    }

    #[test]
    fn the_pager_never_lands_on_a_disabled_arrow() {
        let mut couch = Couch::long_games("big-pager-arrow", egui::vec2(960.0, 540.0));
        couch.app.big.ring = true;
        couch.settle();
        let mut past_tiles = false;
        for _ in 0..40 {
            couch.press(PadButton::DPadDown);
            couch.settle();
            let rect = couch.focused_rect();
            past_tiles |= rect.width() > 200.0 && rect.height() > 200.0;
            if past_tiles && rect.width() < 100.0 && rect.height() < 80.0 {
                break;
            }
        }
        assert!(
            !couch.focus_holds(crate::icons::CARET_LEFT),
            "Down lands on a page, not the disabled Back on page 1"
        );
        for _ in 0..4 {
            couch.press(PadButton::DPadLeft);
            couch.settle();
            assert!(
                !couch.focus_holds(crate::icons::CARET_LEFT),
                "Left stops before the disabled Back"
            );
        }
    }

    #[test]
    fn the_right_stick_and_the_triggers_scroll_the_page() {
        for slow in [0.0, 1.0 / 30.0, 0.1] {
            stick_and_triggers(slow);
        }
    }

    fn stick_and_triggers(slow: f64) {
        let mut couch = Couch::long_games("big-stick", egui::vec2(1280.0, 720.0));
        couch.press(PadButton::DPadDown);
        let before = couch.row_tops();
        couch.move_stick(-1.0);
        for _ in 0..30 {
            couch.wait(slow);
            couch.frame();
        }
        couch.move_stick(0.0);
        couch.frame();
        couch.frame();
        let after = couch.row_tops();
        let moved = before.iter().zip(&after).any(|(a, b)| b < a);
        assert!(moved, "the stick scrolls the page down at {slow} s a frame");
        let rect = couch.focused_rect();
        assert!(
            couch.page_rect().contains_rect(rect),
            "focus follows into view at {slow} s a frame, {rect:?}"
        );
        let low = couch.row_tops();
        couch.press(PadButton::LeftTrigger2);
        couch.frame();
        let up = couch.row_tops();
        assert!(
            low.iter().zip(&up).any(|(a, b)| b > a),
            "the left trigger pages up at {slow} s a frame"
        );
        couch.press(PadButton::RightTrigger2);
        couch.frame();
        let down = couch.row_tops();
        assert!(
            up.iter().zip(&down).any(|(a, b)| b < a),
            "the right trigger pages down at {slow} s a frame, focus {:?}, low {:?}, up {:?}, down {:?}",
            couch.focused_rect(),
            &low[..low.len().min(6)],
            &up[..up.len().min(6)],
            &down[..down.len().min(6)]
        );
    }

    #[test]
    fn the_wheel_scrolls_the_page_from_its_margins() {
        let mut couch = Couch::long_games("big-wheel", egui::vec2(1280.0, 720.0));
        let column_left = (1280.0 - COLUMN_MAX_WIDTH) / 2.0;
        let margin = egui::pos2(column_left - 40.0, 400.0);
        couch.frame_with(vec![egui::Event::PointerMoved(margin)]);
        let before = couch.row_tops();
        for _ in 0..10 {
            couch.frame_with(vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -60.0),
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        for _ in 0..10 {
            couch.frame();
        }
        let after = couch.row_tops();
        assert!(
            before.iter().zip(&after).any(|(a, b)| b < a),
            "a wheel beside the column scrolls the page"
        );
    }

    #[test]
    fn highlights_hug_their_item() {
        let mut couch = Couch::playing("big-hug");
        couch.app.go_place(Place::Games);
        couch
            .app
            .ui
            .recent
            .record(std::path::Path::new("/games/tobu.gb"), "Tobu", None);
        couch.frame();
        let mut kinds = std::collections::BTreeSet::new();
        let mut check = |couch: &Couch, at: &str| {
            let (response, ring) = ring::current(&couch.ctx).expect("a focused widget");
            let item = response.rect;
            match ring {
                None => panic!("{at}: no ring"),
                Some((ring, _)) => {
                    let outer = ring.expand(ring::WIDTH);
                    assert!(
                        item.expand(ring::MARGIN + 0.5).contains_rect(outer),
                        "{at}: the ring {outer:?} stays within {item:?} plus the margin"
                    );
                    if item.contains_rect(outer)
                        && outer.height() + 4.0 >= item.height()
                        && outer.width() + 10.0 >= item.width()
                    {
                        kinds.insert("row");
                    } else if item.bottom() <= tab_bottom(&couch.ctx) + 1.0 {
                        kinds.insert("tab");
                    } else if ring.height() + 1.0 < item.height() {
                        kinds.insert("tile");
                    } else {
                        kinds.insert("ring");
                    }
                }
            }
        };
        for step in 0..8 {
            couch.press(PadButton::DPadDown);
            check(&couch, &format!("games, down {step}"));
        }
        for _ in 0..12 {
            if couch.focused_rect().bottom() <= tab_bottom(&couch.ctx) + 1.0 {
                break;
            }
            couch.press(PadButton::DPadUp);
        }
        check(&couch, "games, tab bar");
        assert!(
            couch.focused_rect().bottom() <= tab_bottom(&couch.ctx) + 1.0,
            "up from the first row reaches the tab bar"
        );
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::Controls);
        couch.frame();
        for step in 0..14 {
            couch.press(PadButton::DPadDown);
            check(&couch, &format!("controls, down {step}"));
        }
        drop(check);
        assert!(
            kinds.contains("row"),
            "rows ring inside their own edge, {kinds:?}"
        );
        assert!(
            kinds.contains("tile"),
            "a tile rings its picture only, {kinds:?}"
        );
        assert!(kinds.contains("tab"), "a tab rings itself, {kinds:?}");
        assert!(kinds.contains("ring"), "a button rings itself, {kinds:?}");
    }

    #[test]
    fn the_pad_keyboard_leaves_the_search_field_in_view() {
        for size in [egui::vec2(960.0, 540.0), egui::vec2(1280.0, 720.0)] {
            let mut couch = Couch::sized("big-keys-room", size);
            couch.fill_library("tobu", 20);
            couch.frame();
            couch.press(PadButton::DPadDown);
            couch.press(PadButton::North);
            couch.typed("tob");
            couch.frame();
            couch.frame();
            couch.frame();
            assert!(
                couch.app.pad_keys_shown(),
                "the keyboard is open at {size:?}"
            );
            let field = couch
                .focused()
                .and_then(|id| couch.ctx.read_response(id))
                .expect("the search field has focus");
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let keys_top = screen.bottom() - keyboard::room(screen) + 8.0;
            assert!(
                field.rect.top() < TAB_BAR_HEIGHT + 16.0,
                "the field {:?} moves to the top of the page on {size:?}",
                field.rect
            );
            assert!(
                field.rect.top() >= 0.0 && field.rect.bottom() <= keys_top,
                "the field {:?} sits above the keyboard at {keys_top} on {size:?}",
                field.rect
            );
        }
    }

    #[test]
    fn the_d_pad_moves_focus_through_the_games_page() {
        let mut couch = Couch::new("big-games-focus");
        assert_eq!(couch.app.mobile.nav.place(), Place::Games);
        couch.press(PadButton::DPadDown);
        let first = couch.focused().expect("the first press focuses something");
        couch.press(PadButton::DPadDown);
        let second = couch.focused().expect("focus stays on the page");
        assert_ne!(first, second, "down moves to the next row");
        couch.press(PadButton::DPadUp);
        assert_eq!(couch.focused(), Some(first), "up comes back");
    }

    #[test]
    fn guide_opens_the_menu_and_back_closes_it() {
        let mut couch = Couch::playing("big-guide");
        assert!(!couch.app.mobile.nav.more_open());
        couch.press(PadButton::Mode);
        assert!(couch.app.mobile.nav.more_open(), "guide opens the menu");
        assert!(couch.focused().is_some(), "the menu takes focus");
        couch.press(PadButton::East);
        assert!(!couch.app.mobile.nav.more_open(), "back closes the menu");
        assert!(couch.app.mobile.nav.on_game(), "and stays in the game");
    }

    #[test]
    fn the_menu_combo_opens_the_menu_after_a_second() {
        let mut couch = Couch::playing("big-combo");
        let held = |input: fn(PadKey) -> PadInput| {
            [PadButton::Select, PadButton::Start].map(|button| PadEvent {
                pad: 0,
                input: input(PadKey::Known(button)),
                code: None,
            })
        };
        couch.pads.0.borrow_mut().extend(held(PadInput::Pressed));
        couch.frame();
        assert!(!couch.app.mobile.nav.more_open(), "not before the hold");
        couch.wait(1.1);
        couch.frame();
        couch.frame();
        assert!(couch.app.mobile.nav.more_open(), "the hold opens the menu");
        couch.pads.0.borrow_mut().extend(held(PadInput::Released));
        couch.frame();
        assert!(couch.app.mobile.nav.more_open(), "letting go keeps it open");
    }

    fn menu_tip_drawn(couch: &mut Couch) -> bool {
        let output = couch.output_with(Vec::new());
        couch.app.menu_tip(&couch.ctx).is_some()
            && crate::app::tests::text_rect(&output, "Hold for menu").is_some()
    }

    #[test]
    fn a_new_game_shows_the_menu_combo_for_its_first_seconds() {
        let mut couch = Couch::playing("big-menu-tip");
        let tip = couch.app.menu_tip(&couch.ctx).expect("the tip shows");
        assert_eq!(tip.glyphs, [Glyph::Select, Glyph::Start]);
        assert_eq!(tip.label, "Hold for menu");
        assert!(menu_tip_drawn(&mut couch), "the first frames show it");
        couch.wait(MENU_TIP_SECONDS - 1.0);
        assert!(menu_tip_drawn(&mut couch), "it stays until the limit");
        couch.wait(1.5);
        assert!(!menu_tip_drawn(&mut couch), "it goes after the limit");
        let output = couch.output_with(Vec::new());
        assert!(
            crate::app::tests::text_rect(&output, "Hold for menu").is_none(),
            "nothing of it is drawn"
        );
    }

    #[test]
    fn the_menu_combo_tip_stops_once_the_menu_was_opened() {
        let mut couch = Couch::playing("big-menu-tip-once");
        assert!(menu_tip_drawn(&mut couch), "a first game shows the tip");
        couch.press(PadButton::Mode);
        assert!(couch.app.mobile.nav.more_open(), "Home opens the menu");
        assert!(
            couch.app.menu_tip(&couch.ctx).is_none(),
            "the menu itself has no tip"
        );
        couch.press(PadButton::East);
        assert!(!couch.app.mobile.nav.more_open(), "Back closes the menu");
        assert!(couch.app.ui.settings.big_screen.menu_opened);
        assert!(couch.app.ui.settings.dirty, "the flag is saved");
        assert!(!menu_tip_drawn(&mut couch), "back in the game it is gone");
        let saved = serde_json::to_value(&couch.app.ui.settings.big_screen).expect("save");
        let mut again = Couch::new("big-menu-tip-again");
        again.app.ui.settings.big_screen = serde_json::from_value(saved).expect("load");
        assert!(again.app.ui.settings.big_screen.menu_opened);
        again.start_game();
        assert!(again.app.big.playing, "the new game is playing");
        assert!(!menu_tip_drawn(&mut again), "a new game shows no tip");
    }

    #[test]
    fn a_game_loaded_over_an_open_one_shows_the_tip_again() {
        let mut couch = Couch::playing("big-menu-tip-second");
        assert!(menu_tip_drawn(&mut couch), "the first game shows the tip");
        couch.wait(MENU_TIP_SECONDS + 0.5);
        assert!(!menu_tip_drawn(&mut couch), "its seconds are over");
        couch.start_game();
        assert!(couch.app.big.playing, "the second game is playing");
        assert!(menu_tip_drawn(&mut couch), "the second game shows it");
        couch.wait(MENU_TIP_SECONDS - 1.0);
        assert!(menu_tip_drawn(&mut couch), "for its own first seconds");
        couch.wait(1.5);
        assert!(!menu_tip_drawn(&mut couch), "and stops after them");
    }

    #[test]
    fn big_screen_steps_through_the_b_pages() {
        assert_eq!(
            super::stepped_page(SettingsTab::Screen, 1, false),
            SettingsTab::Sound
        );
        assert_eq!(
            super::stepped_page(SettingsTab::Capture, 1, false),
            SettingsTab::GameBoy
        );
        assert_eq!(
            super::stepped_page(SettingsTab::Sync, 1, false),
            SettingsTab::Storage
        );
        assert_eq!(
            super::stepped_page(SettingsTab::Storage, 1, false),
            SettingsTab::Screen
        );
        assert_eq!(
            super::stepped_page(SettingsTab::Storage, 1, true),
            SettingsTab::Developer
        );
        assert_eq!(
            super::stepped_page(SettingsTab::Screen, -1, false),
            SettingsTab::Storage
        );
    }

    #[test]
    fn back_closes_the_top_layer_then_goes_back() {
        let mut couch = Couch::new("big-back");
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::App);
        couch.frame();
        assert_eq!(couch.app.mobile.nav.settings_page(), Some(SettingsTab::App));
        couch.press(PadButton::East);
        assert_eq!(
            couch.app.mobile.nav.settings_page(),
            None,
            "back leaves the page"
        );
        assert_eq!(couch.app.mobile.nav.place(), Place::Settings);
    }

    #[test]
    fn north_opens_the_settings_search_and_the_first_move_lands_on_a_page() {
        let mut couch = Couch::new("big-settings-search");
        couch.app.go_place(Place::Settings);
        couch.frame();
        couch.press(PadButton::DPadDown);
        assert!(couch.focused().is_some());
        assert!(
            !couch.app.pad_keys_shown(),
            "the first move opened the keyboard"
        );
        couch.press(PadButton::North);
        couch.typed("updates");
        couch.frame();
        assert!(couch.app.pad_keys_shown(), "North opens the keyboard");
        assert_eq!(couch.app.ui.settings.search, "updates");
    }

    #[test]
    fn the_confirm_button_follows_the_setting() {
        for (confirm, press, back) in [
            (ConfirmButton::Bottom, PadButton::South, PadButton::East),
            (ConfirmButton::Right, PadButton::East, PadButton::South),
        ] {
            let mut couch = Couch::new("big-confirm");
            couch.app.ui.settings.big_screen.confirm = confirm;
            couch.app.go_place(Place::Settings);
            couch.frame();
            couch.press(PadButton::DPadDown);
            assert!(couch.focused().is_some());
            couch.press(press);
            assert!(
                couch.app.mobile.nav.settings_page().is_some(),
                "{confirm} opens the focused page"
            );
            couch.press(back);
            assert_eq!(couch.app.mobile.nav.settings_page(), None, "{confirm} back");
        }
    }

    #[test]
    fn the_d_pad_walks_down_a_page_taller_than_the_screen() {
        let mut couch = Couch::new("big-long-page");
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::Controls);
        couch.frame();
        let mut seen = Vec::new();
        for _ in 0..40 {
            if couch.on_stepper() {
                break;
            }
            couch.press(PadButton::DPadDown);
            seen.extend(couch.focused().filter(|id| !seen.contains(id)));
        }
        assert!(seen.len() > 3, "down moves row by row");
        assert!(couch.on_stepper(), "down reaches the Confirm button row");
    }

    #[test]
    fn a_clear_ask_takes_the_focus_and_names_choose_and_close() {
        let mut couch = Couch::new("big-clear-ask");
        couch.storage = true;
        crate::storage::Folders::default()
            .write(crate::storage::kinds::Folder::Covers, "a.png", &[0; 4096])
            .unwrap();
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::Storage);
        couch.app.count_storage();
        couch.frame();
        let on_clear = |couch: &Couch| {
            couch
                .app
                .hint_list(&couch.ctx, couch.focused())
                .iter()
                .any(|hint| hint.label == "Clear")
        };
        for _ in 0..30 {
            if on_clear(&couch) {
                break;
            }
            couch.press(PadButton::DPadDown);
        }
        assert!(on_clear(&couch), "no cache row offered Clear");
        let row = couch.focused();
        couch.press(couch.app.confirm_button());
        couch.frame();
        assert!(
            couch.app.ui.storage.asking.is_some(),
            "A on a cache row asks to clear it"
        );
        assert!(
            couch.ctx.memory(|m| m.top_modal_layer()).is_some(),
            "the clear ask shows"
        );
        let focused = couch.focused();
        assert!(
            focused.is_some() && focused != row,
            "the focus stays on the row"
        );
        assert!(
            !focus_under_modal(&couch.ctx),
            "the focus sits under the ask"
        );
        let response = focused
            .and_then(|id| couch.ctx.read_response(id))
            .expect("a focused button");
        assert!(
            response.sense.senses_click(),
            "the focus sits on something that can't be pressed"
        );
        assert!(ring::shown(&response), "the focused button has no ring");
        let labels: Vec<&str> = couch
            .app
            .hint_list(&couch.ctx, couch.focused())
            .iter()
            .map(|hint| hint.label)
            .collect();
        assert!(
            labels.contains(&"Choose") && labels.contains(&"Close"),
            "{labels:?}"
        );
    }

    #[test]
    fn a_danger_ask_opened_by_pad_focuses_cancel_and_a_closes_it_with_nothing_removed() {
        let mut couch = Couch::new("big-danger-ask");
        couch.storage = true;
        let folders = crate::storage::Folders::default();
        folders
            .write(crate::storage::kinds::Folder::Covers, "a.png", &[0; 4096])
            .unwrap();
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::Storage);
        couch.app.count_storage();
        couch.frame();
        let on_clear = |couch: &Couch| {
            couch
                .app
                .hint_list(&couch.ctx, couch.focused())
                .iter()
                .any(|hint| hint.label == "Clear")
        };
        for _ in 0..30 {
            if on_clear(&couch) {
                break;
            }
            couch.press(PadButton::DPadDown);
        }
        assert!(on_clear(&couch), "no cache row offered Clear");
        couch.press(couch.app.confirm_button());
        couch.frame();
        assert!(
            couch.app.ui.storage.asking.is_some(),
            "A on a cache row asks to clear it"
        );
        let lowest = focus_candidates(&couch.ctx, Pass::Last)
            .into_iter()
            .max_by(|a, b| a.1.top().total_cmp(&b.1.top()))
            .map(|(id, _, _)| id);
        assert!(lowest.is_some(), "the ask has buttons");
        assert_eq!(couch.focused(), lowest, "the focus starts on Cancel");
        couch.press(couch.app.confirm_button());
        couch.frame();
        assert!(
            couch.app.ui.storage.asking.is_none(),
            "A on Cancel closes the ask"
        );
        assert!(
            couch.ctx.memory(|m| m.top_modal_layer()).is_none(),
            "the ask is gone"
        );
        assert!(
            folders.exists(crate::storage::kinds::Folder::Covers, "a.png"),
            "closing the ask removed the covers"
        );
    }

    #[test]
    fn the_big_screen_storage_page_holds_your_data_and_the_pad_reaches_its_buttons() {
        use crate::app::widgets::settings::your_data::{DELETE, EXPORT, IMPORT};
        let mut couch = Couch::sized("big-storage-data", egui::vec2(1280.0, 2400.0));
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::Storage);
        couch.app.count_storage();
        couch.frame();
        let output = couch.showing();
        let at = |wanted: &str| {
            crate::app::tests::text_rect(&output, wanted)
                .unwrap_or_else(|| panic!("{wanted} missing"))
        };
        let heading = at("Your data");
        let cache = at("Cache");
        for button in [EXPORT, IMPORT, DELETE] {
            let rect = at(button);
            assert!(heading.bottom() <= rect.top(), "{button} under Your data");
            assert!(rect.bottom() <= cache.top(), "{button} above Cache");
        }
        let mut reached = Vec::new();
        for _ in 0..40 {
            couch.press(PadButton::DPadDown);
            let text = couch.focused_text();
            let button = [EXPORT, IMPORT, DELETE]
                .into_iter()
                .find(|button| text.ends_with(button));
            if let Some(button) = button
                && !reached.contains(&button)
            {
                reached.push(button);
            }
        }
        assert_eq!(
            reached,
            [EXPORT, IMPORT, DELETE],
            "the pad steps through them in order"
        );
    }

    #[test]
    fn hints_name_the_press_of_a_cache_row_and_change_only_where_a_row_steps() {
        let mut couch = Couch::new("big-hints");
        crate::storage::Folders::default()
            .write(crate::storage::kinds::Folder::Covers, "a.png", &[0; 4096])
            .unwrap();
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::Storage);
        couch.app.count_storage();
        couch.frame();
        assert!(
            couch
                .app
                .ui
                .storage
                .bytes(crate::storage::kinds::Kind::Covers)
                > 0,
            "{:?}",
            couch.app.ui.storage.usage
        );
        let labels = |couch: &Couch| -> Vec<&'static str> {
            couch
                .app
                .hint_list(&couch.ctx, couch.focused())
                .iter()
                .map(|hint| hint.label)
                .collect()
        };
        let mut seen = Vec::new();
        for _ in 0..30 {
            couch.press(PadButton::DPadDown);
            seen.push(labels(&couch));
            if seen.last().is_some_and(|labels| labels.contains(&"Clear")) {
                break;
            }
        }
        assert!(
            seen.last().is_some_and(|labels| labels.contains(&"Clear")),
            "{seen:?}"
        );
        assert!(
            seen.iter().all(|labels| !labels.contains(&"Change")),
            "no Storage row steps: {seen:?}"
        );

        let mut couch = Couch::new("big-hints-app");
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::App);
        couch.frame();
        for _ in 0..12 {
            if couch.on_stepper() {
                break;
            }
            couch.press(PadButton::DPadDown);
        }
        assert!(couch.on_stepper(), "the App page has a row that steps");
        couch.frame();
        assert!(labels(&couch).contains(&"Change"), "{:?}", labels(&couch));
    }

    #[test]
    fn left_and_right_change_a_setting_in_place() {
        let mut couch = Couch::new("big-stepper");
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::App);
        couch.frame();
        for _ in 0..12 {
            if couch.on_stepper() {
                break;
            }
            couch.press(PadButton::DPadDown);
        }
        assert!(couch.on_stepper(), "the page has a row that steps");
        let before = (couch.app.ui.settings.layout, couch.app.ui.settings.ui_theme);
        let focused = couch.focused();
        couch.press(PadButton::DPadRight);
        let after = (couch.app.ui.settings.layout, couch.app.ui.settings.ui_theme);
        assert_ne!(before, after, "right steps the value");
        assert_eq!(couch.focused(), focused, "focus stays on the row");
        couch.press(PadButton::DPadLeft);
        let back = (couch.app.ui.settings.layout, couch.app.ui.settings.ui_theme);
        assert_eq!(before, back, "left steps it back");
    }

    #[test]
    fn focused_settings_rows_stay_clear_of_both_fades() {
        let mut tabs = vec![SettingsTab::App];
        if cfg!(feature = "nds") {
            tabs.push(SettingsTab::Ds);
        }
        for tab in tabs {
            let mut couch = Couch::sized("big-fade", egui::vec2(960.0, 540.0));
            couch.app.ui.settings.ds_3d.resolution =
                crate::app::ui_state::ds_3d::Resolution::Double;
            couch.app.go_place(Place::Settings);
            couch.app.mobile.nav.open_settings(tab);
            couch.frame();
            let clear = scroll::faded(&couch.ctx)
                .expect("the page fades above the hint bar")
                .top();
            let mut seen = Vec::new();
            let mut still = 0;
            let mut rect = egui::Rect::NOTHING;
            for _ in 0..150 {
                let before = couch.focused();
                couch.press(PadButton::DPadDown);
                couch.settle();
                rect = couch.focused_rect();
                assert!(
                    rect.bottom() <= clear + 0.5,
                    "{tab:?}: the focused {rect:?} ends under the fade above {clear}"
                );
                seen.extend(couch.focused().filter(|id| !seen.contains(id)));
                still = if couch.focused() == before {
                    still + 1
                } else {
                    0
                };
                if still == 3 {
                    break;
                }
            }
            assert_eq!(still, 3, "{tab:?}: focus reached the last row");
            assert!(
                rect.bottom() > clear - 80.0,
                "{tab:?}: the last of {} rows {rect:?} sits at the scrolled foot above {clear}",
                seen.len()
            );
            assert!(
                scroll::faded_top(&couch.ctx).is_some(),
                "{tab:?}: a page scrolled down fades under its top edge"
            );
            still = 0;
            for _ in 0..150 {
                let before = couch.focused();
                couch.press(PadButton::DPadUp);
                couch.settle();
                let rect = couch.focused_rect();
                if let Some(fade) = scroll::faded_top(&couch.ctx)
                    && rect.bottom() > fade.top()
                {
                    assert!(
                        rect.top() >= fade.bottom() - 0.5,
                        "{tab:?}: the focused {rect:?} starts under the top fade {fade:?}"
                    );
                }
                still = if couch.focused() == before {
                    still + 1
                } else {
                    0
                };
                if still == 3 {
                    break;
                }
            }
            assert_eq!(still, 3, "{tab:?}: focus reached the first row");
        }
    }

    fn frame(white: usize, dark: usize) -> Vec<u8> {
        let mut rgba = Vec::new();
        for _ in 0..white {
            rgba.extend_from_slice(&[255, 255, 255, 255]);
        }
        for shade in 0..dark {
            rgba.extend_from_slice(&[(shade % 3) as u8, 0, 0, 255]);
        }
        rgba
    }

    #[test]
    fn a_frame_four_fifths_one_colour_shows_the_icon_tile_instead() {
        assert!(mostly_one_colour(&frame(80, 20)));
        assert!(mostly_one_colour(&frame(100, 0)));
        assert!(mostly_one_colour(&[]));
        assert!(!mostly_one_colour(&frame(79, 21)));
        assert!(!mostly_one_colour(&frame(50, 50)));
    }

    fn picture(output: &egui::FullOutput) -> egui::Rect {
        crate::app::tests::picture_rect(output).expect("the game is drawn")
    }

    impl Couch {
        fn showing(&mut self) -> egui::FullOutput {
            self.frame();
            self.output_with(Vec::new())
        }

        fn focus_holds(&mut self, label: &str) -> bool {
            let output = self.showing();
            let focus = self.focused_rect();
            output.shapes.iter().any(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    text.galley.text().trim() == label
                        && focus.contains_rect(text.visual_bounding_rect())
                }
                _ => false,
            })
        }

        fn drag(&mut self, from: egui::Pos2, by: egui::Vec2) {
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            self.frame_with(vec![egui::Event::PointerMoved(from), button(from, true)]);
            for step in 1..=10 {
                let at = from + by * (step as f32 / 10.0);
                self.frame_with(vec![egui::Event::PointerMoved(at)]);
            }
            self.frame_with(vec![button(from + by, false)]);
            self.frame();
        }
    }

    #[test]
    fn a_running_game_shows_the_leaderboard_tracker() {
        let mut couch = Couch::playing("big-tracker");
        couch.app.emulator.clear_frame(&couch.ctx);
        couch.app.achievements.boards.attempts = vec![citrine_gb_achievements::Attempt {
            board: 1,
            title: "Brock Speedrun".to_owned(),
            score: "12:38.40".to_owned(),
        }];
        let output = couch.showing();
        crate::app::tests::text_containing(&output, "Brock Speedrun").expect("the tracker shows");
        crate::app::tests::text_containing(&output, "12:38.40").expect("its score shows");
    }

    #[test]
    fn back_leaves_the_rankings_before_the_achievements_page() {
        let mut couch = Couch::new("big-rankings-back");
        couch.app.mobile.open(crate::app::tabs::Tab::Achievements);
        couch.frame();
        couch.app.achievements.boards.open = Some(1);
        couch.frame();
        couch.press(PadButton::East);
        assert_eq!(couch.app.achievements.boards.open, None);
        assert!(
            couch
                .app
                .mobile
                .showing(crate::app::tabs::Tab::Achievements)
        );
    }

    #[test]
    fn more_hud_settings_opens_the_screen_page_at_its_huds() {
        use crate::app::tests::{shown_whole, tap, text_containing};
        let mut couch = Couch::sized("big-hud-more", egui::vec2(1280.0, 480.0));
        couch
            .app
            .emulator
            .load_rom(
                &Rom::new(&[0u8; 0x8000]),
                GbModel::Dmg,
                #[cfg(not(target_arch = "wasm32"))]
                None,
            )
            .expect("load");
        couch.app.mobile.nav.show_game();
        couch.app.mobile.hud_sheet = Some(crate::app::mobile::hud::HudSheet::List);
        let mut output = couch.output_with(Vec::new());
        for _ in 0..20 {
            output = couch.output_with(Vec::new());
        }
        let more = text_containing(&output, "More HUD settings").expect("the sheet entry");
        couch.frame_with(tap(more.center()));
        for _ in 0..40 {
            output = couch.output_with(Vec::new());
        }
        assert_eq!(
            couch.app.mobile.nav.settings_page(),
            Some(SettingsTab::Screen)
        );
        assert!(
            shown_whole(&output, "HUDs"),
            "the HUDs heading is out of view"
        );
    }

    #[test]
    fn a_running_game_shows_the_huds_where_focus_mode_does() {
        use crate::app::ui_state::settings::LayoutMode;
        let mut couch = Couch::playing("big-hud");
        couch.app.emulator.clear_frame(&couch.ctx);
        couch.app.ui.settings.hud.add();
        let output = couch.showing();
        let big = crate::app::tests::text_containing(&output, "FPS").expect("the HUD shows");
        couch.app.ui.settings.big_screen.on = false;
        couch.app.ui.settings.layout = LayoutMode::Desktop;
        couch.app.ui.focus_mode = true;
        let output = couch.showing();
        let focus =
            crate::app::tests::text_containing(&output, "FPS").expect("focus mode shows it");
        assert_eq!(big.min, focus.min);
    }

    #[test]
    fn the_screen_moves_by_drag_and_the_pad_edits_the_layout() {
        let mut couch = Couch::playing("big-move");
        couch.app.emulator.clear_frame(&couch.ctx);
        let middle = picture(&couch.showing());
        couch.drag(middle.center(), egui::vec2(-200.0, 0.0));
        let dragged = picture(&couch.showing());
        assert_eq!(dragged, middle.translate(egui::vec2(-200.0, 0.0)));

        couch.press(PadButton::Mode);
        let output = couch.showing();
        let entry = crate::app::tests::text_rect(&output, "Layout").expect("a menu entry");
        couch.frame_with(crate::app::tests::tap(entry.center()));
        couch.frame();
        assert!(
            couch.app.mobile.nav.more_open(),
            "the panel shows beside the game"
        );
        assert!(
            couch.app.ui.layout_sheet.is_some(),
            "Layout opens the panel"
        );
        let output = couch.showing();
        assert!(crate::app::tests::text_containing(&output, "Game Boy, big screen").is_none());
        assert!(crate::app::tests::text_rect(&output, "Standard").is_some());
        let edit = crate::app::tests::text_rect(&output, "Edit layout").expect("the panel row");
        couch.frame_with(crate::app::tests::tap(edit.center()));
        couch.frame();
        assert!(!couch.app.mobile.nav.more_open(), "the menu closes");
        assert!(couch.app.editing_layout(), "the panel row starts editing");
        assert!(
            !couch.app.emulator.running,
            "the game waits while the layout changes"
        );
        couch.press(PadButton::DPadRight);
        let stepped = picture(&couch.showing());
        assert!(
            stepped.left() > dragged.left(),
            "{stepped:?} after {dragged:?}"
        );
        couch.press(PadButton::LeftTrigger2);
        let shrunk = picture(&couch.showing());
        assert!(
            shrunk.width() < stepped.width(),
            "{shrunk:?} after {stepped:?}"
        );
        couch.press(PadButton::North);
        assert_eq!(picture(&couch.showing()), middle, "North resets the layout");
        couch.press(PadButton::North);
        assert_eq!(picture(&couch.showing()), shrunk, "North again undoes it");
        couch.press(PadButton::East);
        assert!(!couch.app.editing_layout(), "Back ends editing");
        assert!(couch.app.emulator.running, "the game goes on");
        assert_eq!(
            picture(&couch.showing()),
            dragged,
            "Back leaves nothing behind"
        );
    }

    #[test]
    fn the_layout_panel_opens_with_the_ring_on_the_chosen_card() {
        let mut couch = Couch::playing("big-layout-focus");
        couch.app.open_game_menu();
        couch.app.big.show_ring_for_tests();
        couch.frame();
        couch.app.ui.layout_sheet = Some(Default::default());
        let output = couch.showing();
        let focused = couch.focused().expect("the panel takes pad focus");
        let response = couch
            .ctx
            .read_response(focused)
            .expect("the focused widget");
        let chosen = crate::app::tests::text_rect(&output, "Standard").expect("the chosen card");
        assert!(
            response.rect.contains(chosen.center()),
            "focus sits on {:?}, the card is at {chosen:?}",
            response.rect
        );
        assert!(ring::shown(&response), "the focus ring shows");
    }

    #[test]
    fn back_leaves_the_layout_panel_for_the_game_menu() {
        let mut couch = Couch::playing("big-layout-back");
        couch.press(PadButton::Mode);
        let output = couch.showing();
        let entry = crate::app::tests::text_rect(&output, "Layout").expect("a menu entry");
        couch.frame_with(crate::app::tests::tap(entry.center()));
        couch.frame();
        assert!(couch.app.ui.layout_sheet.is_some());
        couch.press(PadButton::DPadDown);
        assert!(couch.focused().is_some(), "the pad moves through the panel");
        couch.press(PadButton::East);
        assert!(couch.app.ui.layout_sheet.is_none(), "Back closes the panel");
        assert!(couch.app.mobile.nav.more_open(), "and shows the menu again");
    }

    #[test]
    fn a_focused_button_that_turns_disabled_hands_the_focus_to_its_nearest_neighbour() {
        let mut couch = Couch::new("big-disabled-focus");
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::Storage);
        couch.frame();
        let set_count = |couch: &mut Couch, count: usize| {
            let held = web_time::Instant::now() + std::time::Duration::from_secs(3600);
            couch.app.ui.your_data.counted_at = Some(held);
            let saves = crate::your_data::Kind::of(crate::storage::kinds::Kind::Saves);
            let tally = crate::your_data::Tally { count };
            couch.app.ui.your_data.rows = vec![(saves, tally)];
        };
        let focusable = |couch: &Couch| -> Vec<(Id, egui::Rect, bool)> {
            couch.ctx.viewport(|v| {
                v.prev_pass
                    .widgets
                    .layers()
                    .flat_map(|(_, widgets)| widgets.iter())
                    .filter(|w| w.sense.is_focusable())
                    .map(|w| (w.id, w.rect, w.enabled))
                    .collect()
            })
        };
        set_count(&mut couch, 1);
        couch.frame();
        couch.frame();
        let stored = focusable(&couch);
        set_count(&mut couch, 0);
        couch.frame();
        couch.frame();
        let empty = focusable(&couch);
        let old = stored
            .iter()
            .filter(|(id, _, on)| *on && empty.iter().any(|(e, _, off)| e == id && !off))
            .max_by(|a, b| {
                let (a, b) = (a.1.center(), b.1.center());
                a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x))
            })
            .map(|w| w.0)
            .expect("Delete everything turns disabled once nothing is stored");
        set_count(&mut couch, 1);
        couch.frame();
        couch.frame();
        for _ in 0..60 {
            if couch.focused() == Some(old) {
                break;
            }
            couch.press(PadButton::DPadDown);
        }
        assert_eq!(
            couch.focused(),
            Some(old),
            "the pad reaches Delete everything"
        );
        let old_rect = couch.focused_rect();
        set_count(&mut couch, 0);
        couch.frame();
        couch.frame();
        let disabled = couch
            .ctx
            .viewport(|v| v.prev_pass.widgets.get(old).map(|widget| !widget.enabled));
        assert_eq!(
            disabled,
            Some(true),
            "the last button of the Your data section turns disabled once nothing is stored"
        );
        let new = couch.focused().expect("the focus moves on, not away");
        assert_ne!(new, old);
        let enabled = couch
            .ctx
            .viewport(|v| v.prev_pass.widgets.get(new).map(|widget| widget.enabled));
        assert_eq!(enabled, Some(true), "the focus lands on an enabled control");
        let new_rect = couch.focused_rect();
        assert!(
            (new_rect.center().y - old_rect.center().y).abs() < 3.0 * old_rect.height(),
            "the focus jumps from {old_rect:?} to {new_rect:?}, not to its neighbour"
        );
    }

    impl Couch {
        fn rect_of(&self, id: Id) -> Option<egui::Rect> {
            self.ctx
                .viewport(|v| v.prev_pass.widgets.get(id).map(|widget| widget.rect))
        }
    }

    fn settings_page(page: SettingsTab, size: egui::Vec2) -> Couch {
        let mut couch = Couch::sized("big-settings-page", size);
        couch.touch = true;
        Citrine::setup_fonts(&couch.ctx);
        couch.app.ui.settings.dev_mode = true;
        couch.app.ui.settings.dirty = true;
        couch.app.big.ring = true;
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(page);
        couch.settle();
        couch
    }

    fn settings_pages() -> impl Iterator<Item = SettingsTab> {
        crate::app::mobile::settings_list::sections(true)
            .into_iter()
            .flat_map(|(_, pages)| pages)
    }

    #[test]
    fn no_settings_page_offers_a_sliver_as_a_stop() {
        let mut slivers = 0;
        let mut faults = Vec::new();
        for page in settings_pages() {
            let couch = settings_page(page, egui::vec2(960.0, 540.0));
            slivers += couch.ctx.viewport(|v| {
                v.prev_pass
                    .widgets
                    .layers()
                    .flat_map(|(_, widgets)| widgets.iter())
                    .filter(|w| {
                        w.sense.is_focusable()
                            && w.rect.width() > 0.0
                            && w.rect.width() < STOP_MIN_SIDE
                            && w.rect.height() >= 100.0
                    })
                    .count()
            });
            for (_, rect, _) in focus_candidates(&couch.ctx, Pass::Last) {
                if rect.width().min(rect.height()) < STOP_MIN_SIDE {
                    faults.push(format!("{page:?}: {rect:?}"));
                }
            }
        }
        assert!(
            slivers > 0,
            "no page drew a scroll bar, the walk proves nothing"
        );
        assert!(faults.is_empty(), "{faults:#?}");
    }

    fn pick_from_the_game_menu(name: &str, label: &str) -> Couch {
        let mut couch = Couch::playing(name);
        couch.app.big.ring = true;
        couch.press(PadButton::Mode);
        couch.settle();
        for _ in 0..30 {
            if couch.focus_holds(label) {
                couch.press(PadButton::South);
                couch.settle();
                return couch;
            }
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        panic!("the pad never reaches {label} in the game menu");
    }

    #[test]
    fn record_video_follows_screenshot_in_the_game_menu() {
        let mut couch = Couch::playing("big-menu-record");
        couch.app.big.ring = true;
        couch.press(PadButton::Mode);
        couch.settle();
        for _ in 0..30 {
            if couch.focus_holds("Screenshot") {
                break;
            }
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        assert!(
            couch.focus_holds("Screenshot"),
            "the pad reaches Screenshot"
        );
        couch.press(PadButton::DPadDown);
        couch.settle();
        assert!(
            couch.focus_holds("Record"),
            "Record comes right after Screenshot"
        );
    }

    #[test]
    fn the_game_menu_opens_rom_info() {
        let couch = pick_from_the_game_menu("big-menu-rom", "ROM info");
        assert_eq!(
            couch.app.mobile.nav.page(),
            Some(crate::app::tabs::Tab::RomInfo)
        );
    }

    #[test]
    fn the_game_menu_reaches_what_the_phone_menu_has() {
        let couch = pick_from_the_game_menu("big-menu-cheats", "Cheats");
        assert_eq!(
            couch.app.mobile.nav.page(),
            Some(crate::app::tabs::Tab::Cheats)
        );
        assert!(couch.focused().is_some(), "focus lands on the Cheats page");
        let couch = pick_from_the_game_menu("big-menu-trophies", "Achievements");
        assert_eq!(
            couch.app.mobile.nav.page(),
            Some(crate::app::tabs::Tab::Achievements)
        );
        let couch = pick_from_the_game_menu("big-menu-hud", "HUD");
        assert!(couch.app.mobile.hud_sheet.is_some(), "the HUD sheet opens");
    }

    #[test]
    fn the_game_menu_leaves_settings_to_its_tab() {
        let mut couch = Couch::playing("big-menu-no-settings");
        couch.app.big.ring = true;
        couch.press(PadButton::Mode);
        couch.settle();
        for step in 0..30 {
            assert!(
                !couch.focus_holds("Settings"),
                "step {step}: the game menu has no Settings row"
            );
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        for _ in 0..3 {
            couch.press(PadButton::RightTrigger);
        }
        assert_eq!(couch.app.mobile.nav.place(), Place::Settings);
        assert!(couch.focused().is_some(), "focus lands in Settings");
    }

    #[test]
    fn the_game_menu_volume_goes_off_at_zero_and_back_on() {
        let mut couch = Couch::playing("big-menu-volume");
        couch.app.ui.settings.volume = 0.03;
        couch.app.big.ring = true;
        couch.press(PadButton::Mode);
        couch.settle();
        for _ in 0..30 {
            if couch.on_stepper() {
                break;
            }
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        assert!(
            couch.on_stepper(),
            "the pad reaches the Volume slider in the game menu"
        );
        for _ in 0..3 {
            couch.press(PadButton::DPadLeft);
            couch.settle();
        }
        assert!(
            couch.app.ui.settings.muted,
            "Left down to 0 turns the sound off, volume {}",
            couch.app.ui.settings.volume
        );
        couch.press(PadButton::DPadRight);
        couch.settle();
        assert!(
            !couch.app.ui.settings.muted,
            "Right from 0 turns the sound on"
        );
        couch.press(PadButton::DPadRight);
        couch.settle();
        assert!(
            (couch.app.ui.settings.volume - 0.02).abs() < 1e-4,
            "Right raises the volume, got {}",
            couch.app.ui.settings.volume
        );
    }

    #[test]
    fn a_sheet_slider_steps_with_left_and_right() {
        let mut couch = settings_page(SettingsTab::Sound, egui::vec2(960.0, 540.0));
        couch.app.ui.settings.volume = 0.5;
        couch.settle();
        let row = couch.focused_rect();
        couch.press(PadButton::South);
        couch.settle();
        let slider = couch.focused_rect();
        assert!(
            couch.on_stepper(),
            "the Volume sheet's slider takes Left and Right, focus on {slider:?} from the row {row:?}"
        );
        couch.press(PadButton::DPadRight);
        couch.settle();
        assert!(
            (couch.app.ui.settings.volume - 0.51).abs() < 1e-4,
            "Right adds a step, got {}",
            couch.app.ui.settings.volume
        );
        couch.press(PadButton::DPadLeft);
        couch.settle();
        couch.press(PadButton::DPadLeft);
        couch.settle();
        assert!(
            (couch.app.ui.settings.volume - 0.49).abs() < 1e-4,
            "Left takes a step off, got {}",
            couch.app.ui.settings.volume
        );
    }

    #[test]
    fn west_opens_the_focused_games_sheet() {
        let mut couch = Couch::long_games("big-west-game", egui::vec2(1280.0, 720.0));
        couch.app.big.ring = true;
        for _ in 0..10 {
            if couch.focused().is_some() && couch.focused_rect().height() > 100.0 {
                break;
            }
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        assert!(
            couch.focused_rect().height() > 100.0,
            "the pad reaches a game tile, focus on {:?}",
            couch.focused_rect()
        );
        assert_eq!(
            couch.app.big.details.map(|(_, label)| label),
            Some("Details"),
            "the hint bar offers the game's details"
        );
        couch.press(PadButton::West);
        couch.settle();
        assert!(
            couch.app.mobile.library.game().is_some(),
            "West opens the focused game's sheet"
        );
    }

    #[test]
    fn an_open_sheet_offers_choose_and_close() {
        let mut couch = Couch::long_games("big-sheet-hints", egui::vec2(1280.0, 720.0));
        couch.app.big.ring = true;
        for _ in 0..10 {
            if couch.focused().is_some() && couch.focused_rect().height() > 100.0 {
                break;
            }
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        let labels = |couch: &Couch| -> Vec<&'static str> {
            couch
                .app
                .hint_list(&couch.ctx, couch.focused())
                .iter()
                .map(|hint| hint.label)
                .collect()
        };
        assert!(
            labels(&couch).contains(&"Search"),
            "the Games page offers Search, got {:?}",
            labels(&couch)
        );
        couch.press(PadButton::West);
        couch.settle();
        assert!(
            couch.app.mobile.library.game().is_some(),
            "West opens the game's sheet"
        );
        let hints = couch.app.hint_list(&couch.ctx, couch.focused());
        let shown = labels(&couch);
        assert!(
            !shown.contains(&"Search"),
            "a sheet hides the page's Search, got {shown:?}"
        );
        assert!(
            shown.contains(&"Choose"),
            "a sheet offers Choose, got {shown:?}"
        );
        assert!(
            hints
                .iter()
                .any(|hint| hint.label == "Close" && hint.glyphs == [Glyph::Face(PadButton::East)]),
            "a sheet offers Close on East, got {shown:?}"
        );
        couch.press(PadButton::East);
        couch.settle();
        assert!(
            couch.app.mobile.library.game().is_none(),
            "East closes the sheet"
        );
    }

    #[test]
    fn west_shows_a_rows_hint_and_back_hides_it() {
        use crate::app::ui_state::settings::GHOSTING_NOTE;
        let mut couch = settings_page(SettingsTab::Screen, egui::vec2(1280.0, 720.0));
        let mut reached = false;
        for _ in 0..40 {
            let output = couch.showing();
            let label = crate::app::tests::text_rect(&output, "Ghosting");
            if label.is_some_and(|label| couch.focused_rect().contains_rect(label)) {
                reached = true;
                break;
            }
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        assert!(reached, "the pad never reaches the Ghosting row");
        let shown = |couch: &mut Couch| {
            crate::app::tests::text_rect(&couch.showing(), GHOSTING_NOTE).is_some()
        };
        assert!(!shown(&mut couch), "the hint waits for West");
        assert_eq!(
            couch.app.big.details.map(|(_, label)| label),
            Some("Info"),
            "the hint bar offers the row's hint"
        );
        couch.press(PadButton::West);
        couch.settle();
        assert!(shown(&mut couch), "West shows the focused row's hint");
        couch.press(PadButton::East);
        couch.settle();
        assert!(!shown(&mut couch), "Back hides the hint");
        assert_eq!(
            couch.app.mobile.nav.settings_page(),
            Some(SettingsTab::Screen),
            "Back hides the hint before it leaves the page"
        );
        couch.press(PadButton::West);
        couch.settle();
        assert!(shown(&mut couch), "West shows it again");
        couch.press(PadButton::DPadDown);
        couch.settle();
        assert!(!shown(&mut couch), "moving on hides it");
    }

    #[test]
    fn walking_down_a_settings_page_never_scrolls_back_up() {
        let mut faults = Vec::new();
        for size in [egui::vec2(960.0, 540.0), egui::vec2(1280.0, 720.0)] {
            for page in settings_pages() {
                if let Err(fault) = walk_down(page, size) {
                    faults.push(format!("{page:?} on {size:?}: {fault}"));
                }
            }
        }
        assert!(faults.is_empty(), "{faults:#?}");
    }

    fn walk_down(page: SettingsTab, size: egui::Vec2) -> Result<(), String> {
        let mut couch = settings_page(page, size);
        let anchor = couch.focused().ok_or("the page takes no focus")?;
        let mut last = couch.rect_of(anchor).ok_or("no first row")?;
        if last.top() < tab_bottom(&couch.ctx) {
            return Err("focus starts on the tab bar".to_owned());
        }
        let mut depth = 0.0;
        let mut still = 0;
        let mut from = anchor;
        for step in 0..80 {
            couch.press(PadButton::DPadDown);
            couch.settle();
            let to = couch
                .focused()
                .ok_or(format!("step {step}: focus is lost"))?;
            let top = couch
                .rect_of(anchor)
                .ok_or(format!("step {step}: the first row is gone"))?;
            let rect = couch
                .rect_of(to)
                .ok_or(format!("step {step}: the focused widget is not shown"))?;
            if top.top() > last.top() + 0.5 {
                return Err(format!(
                    "step {step}: the page scrolled up from {} to {}",
                    last.top(),
                    top.top()
                ));
            }
            let at = rect.top() - top.top();
            if at < depth - 0.5 {
                return Err(format!("step {step}: focus went up from {depth} to {at}"));
            }
            last = top;
            depth = at;
            still = if to == from { still + 1 } else { 0 };
            from = to;
            if still == 3 {
                return Ok(());
            }
        }
        Err("the walk never reached the foot".to_owned())
    }

    #[test]
    fn shoulders_step_through_the_viewer_not_the_places() {
        use crate::app::viewer::{ItemKey, Source};
        use crate::media::{Frame, MediaKind, NewMedia};
        let _dirs = crate::storage::TestDirs::new("couch-viewer");
        let mut couch = Couch::new("viewer");
        for created_at in [100, 200] {
            let frame = Frame {
                width: 160,
                height: 144,
                rgba: vec![9; 160 * 144 * 4],
            };
            let png = crate::utils::screenshot::encode_png(160, 144, &frame.rgba).unwrap();
            let shot = NewMedia {
                kind: MediaKind::Screenshot,
                game: "Tobu Tobu Girl".into(),
                rom_key: None,
                model: None,
                created_at,
                width: 160,
                height: 144,
                length_ms: None,
                frame_times: false,
                video: false,
                replay: None,
                frame,
            };
            couch.app.media.add_screenshot(shot, &png).unwrap();
        }
        let first = ItemKey::Media(couch.app.media.items()[0].id.clone());
        couch.app.ui.viewer.open(Source::Media, first.clone());
        couch.frame();
        let place = couch.app.mobile.nav.place();
        couch.press(PadButton::RightTrigger);
        assert_eq!(
            couch.app.mobile.nav.place(),
            place,
            "the place behind stays"
        );
        assert!(couch.app.ui.viewer.is_open(), "the viewer stays open");
        assert_ne!(
            couch.app.ui.viewer.current(),
            Some(&first),
            "RB shows the next shot"
        );
        couch.press(PadButton::LeftTrigger);
        assert_eq!(couch.app.ui.viewer.current(), Some(&first), "LB goes back");
    }

    #[test]
    fn back_returns_focus_to_the_row_that_opened_the_page() {
        let mut couch = Couch::new("big-back-opener");
        couch.app.go_place(Place::Settings);
        couch.frame();
        couch.press(PadButton::DPadDown);
        let first = couch.focused().expect("the first row has focus");
        for _ in 0..3 {
            couch.press(PadButton::DPadDown);
        }
        let opener = couch.focused().expect("a settings row has focus");
        assert_ne!(first, opener, "the opener is not the first row");
        couch.press(PadButton::South);
        assert!(
            couch.app.mobile.nav.settings_page().is_some(),
            "the row opens its page"
        );
        couch.press(PadButton::DPadDown);
        couch.press(PadButton::East);
        assert_eq!(
            couch.app.mobile.nav.settings_page(),
            None,
            "back leaves the page"
        );
        assert_eq!(
            couch.focused(),
            Some(opener),
            "focus is on the row that opened it"
        );
    }

    #[test]
    fn l1_and_r1_bring_focus_back_to_where_it_was() {
        let mut couch = Couch::new("big-place-recall");
        couch.press(PadButton::DPadDown);
        let first = couch.focused().expect("a Games row has focus");
        couch.press(PadButton::DPadDown);
        couch.press(PadButton::DPadDown);
        let row = couch.focused().expect("a Games row has focus");
        assert_ne!(first, row);
        couch.press(PadButton::RightTrigger);
        assert_eq!(couch.app.mobile.nav.place(), Place::Media);
        couch.press(PadButton::LeftTrigger);
        assert_eq!(couch.app.mobile.nav.place(), Place::Games);
        assert_eq!(couch.focused(), Some(row), "Games comes back to the row");
    }

    #[test]
    fn r1_in_the_game_menu_moves_focus_into_the_new_place() {
        let mut couch = Couch::playing("big-menu-r1");
        couch.press(PadButton::Mode);
        assert!(couch.app.mobile.nav.more_open());
        couch.press(PadButton::DPadUp);
        let bar = tab_bottom(&couch.ctx);
        assert!(
            couch.focused_rect().bottom() <= bar + 1.0,
            "up reaches the tab bar"
        );
        couch.press(PadButton::RightTrigger);
        assert!(!couch.app.mobile.nav.on_game(), "R1 leaves the game tab");
        assert_eq!(
            couch.app.mobile.nav.place(),
            Place::Games,
            "for the library"
        );
        assert!(
            couch.focused_rect().top() >= bar - 1.0,
            "focus moves into the new place, not the tab bar"
        );
    }

    fn bar_labels(output: &egui::FullOutput, bottom: f32) -> Vec<String> {
        const SHOWN: [&str; 9] = [
            "L1", "R1", "LB", "RB", "Games", "Media", "Settings", "You", "Friends",
        ];
        let mut labels: Vec<(String, f32)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    let rect = text.visual_bounding_rect();
                    let label = text.galley.text().trim();
                    (rect.bottom() <= bottom + 1.0 && SHOWN.contains(&label))
                        .then(|| (label.to_owned(), rect.center().x))
                }
                _ => None,
            })
            .collect();
        labels.sort_by(|a, b| a.1.total_cmp(&b.1));
        labels.into_iter().map(|(label, _)| label).collect()
    }

    #[test]
    fn the_tab_bar_names_the_places_between_the_shoulder_marks() {
        let mut couch = Couch::sized("big-tabs", egui::vec2(960.0, 540.0));
        let output = couch.showing();
        let bottom = tab_bottom(&couch.ctx);
        assert!(
            (bottom - TAB_BAR_HEIGHT).abs() <= 1.0,
            "the bar is at the top, its foot at {bottom}"
        );
        let labels = bar_labels(&output, bottom);
        let shoulders = if labels.first().map(String::as_str) == Some("LB") {
            ["LB", "RB"]
        } else {
            ["L1", "R1"]
        };
        assert_eq!(
            labels,
            [
                shoulders[0],
                "Games",
                "Media",
                "Settings",
                "You",
                shoulders[1]
            ],
            "four tabs in order between the shoulder marks"
        );
    }

    fn ring_around(output: &egui::FullOutput, rect: egui::Rect) -> bool {
        let wanted = rect.expand(ring::GAP);
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) => {
                shape.stroke.width == ring::WIDTH
                    && (shape.rect.min - wanted.min).length() <= 1.0
                    && (shape.rect.max - wanted.max).length() <= 1.0
            }
            _ => false,
        })
    }

    #[test]
    fn the_pad_reaches_every_tab_and_each_shows_the_ring() {
        let mut couch = Couch::sized("big-tab-ring", egui::vec2(960.0, 540.0));
        couch.press(PadButton::DPadDown);
        couch.settle();
        for _ in 0..12 {
            if couch.focused_rect().bottom() <= tab_bottom(&couch.ctx) {
                break;
            }
            couch.press(PadButton::DPadUp);
            couch.settle();
        }
        for _ in 0..8 {
            if couch.focus_holds("Games") {
                break;
            }
            couch.press(PadButton::DPadLeft);
            couch.settle();
        }
        for (n, label) in ["Games", "Media", "Settings", "You"]
            .into_iter()
            .enumerate()
        {
            if n > 0 {
                couch.press(PadButton::DPadRight);
                couch.settle();
            }
            assert!(couch.focus_holds(label), "{label} has focus");
            let output = couch.showing();
            let rect = couch.focused_rect();
            assert!(
                rect.bottom() <= tab_bottom(&couch.ctx),
                "{label} is in the tab bar"
            );
            assert!(
                ring_around(&output, rect),
                "{label} shows the ring around its pill, selected {}",
                n == 0
            );
        }
    }

    fn shoulder_marks(couch: &mut Couch) -> (egui::Rect, egui::Rect) {
        let output = couch.showing();
        let bottom = tab_bottom(&couch.ctx);
        let mut marks: Vec<egui::Rect> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if ["L1", "R1", "LB", "RB"].contains(&text.galley.text().trim()) =>
                {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .filter(|rect| rect.bottom() <= bottom)
            .collect();
        marks.sort_by(|a, b| a.left().total_cmp(&b.left()));
        let [left, right] = marks[..] else {
            panic!("two shoulder marks, got {marks:?}");
        };
        (left, right)
    }

    fn tab_ring(couch: &mut Couch) -> egui::Rect {
        let output = couch.showing();
        let bottom = tab_bottom(&couch.ctx);
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.stroke.width == ring::WIDTH && shape.rect.bottom() <= bottom =>
                {
                    Some(clipped.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .expect("the ring on a tab")
    }

    fn focus_tab(couch: &mut Couch, label: &str) {
        couch.app.big.ring = true;
        couch.press(PadButton::DPadDown);
        couch.settle();
        for _ in 0..12 {
            if couch.focused_rect().bottom() <= tab_bottom(&couch.ctx) {
                break;
            }
            couch.press(PadButton::DPadUp);
            couch.settle();
        }
        for _ in 0..8 {
            if couch.focus_holds("Games") {
                break;
            }
            couch.press(PadButton::DPadLeft);
            couch.settle();
        }
        for _ in 0..8 {
            if couch.focus_holds(label) {
                break;
            }
            couch.press(PadButton::DPadRight);
            couch.settle();
        }
        assert!(couch.focus_holds(label), "{label} has focus");
    }

    #[test]
    fn the_shoulder_marks_stay_still_and_clear_of_a_ringed_end_tab() {
        let mut couch = Couch::sized("big-tab-marks", egui::vec2(960.0, 540.0));
        couch.app.go_place(Place::Games);
        let still = shoulder_marks(&mut couch);
        for place in [Place::Media, Place::Settings, Place::You, Place::Games] {
            couch.app.go_place(place);
            assert_eq!(shoulder_marks(&mut couch), still, "{place:?} selected");
        }
        for label in ["Games", "Media", "You"] {
            focus_tab(&mut couch, label);
            let (left, right) = shoulder_marks(&mut couch);
            assert_eq!((left, right), still, "{label} focused");
            let ring = tab_ring(&mut couch);
            assert!(
                ring.left() - left.right() >= tabs::MARK_GAP
                    && right.left() - ring.right() >= tabs::MARK_GAP,
                "{label}'s ring {ring:?} keeps its gap to the marks {left:?} {right:?}"
            );
        }
    }

    #[test]
    fn a_ringed_tab_keeps_the_end_gap_to_the_selected_pill() {
        let mut couch = Couch::sized("big-tab-ring-gap", egui::vec2(960.0, 540.0));
        couch.app.go_place(Place::Games);
        focus_tab(&mut couch, "Media");
        let output = couch.showing();
        let brand = couch.ctx.style().visuals.selection.bg_fill;
        let bottom = tab_bottom(&couch.ctx);
        let pill = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.fill == brand && shape.rect.bottom() <= bottom =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the Games pill");
        let ring = tab_ring(&mut couch);
        assert!(
            ring.left() - pill.right() >= tabs::MARK_GAP - 0.5,
            "the ring sits {} from the Games pill",
            ring.left() - pill.right()
        );
    }

    #[test]
    fn the_home_menu_dims_the_tabs_but_not_the_selected_pill() {
        let mut couch = Couch::sized("big-home-pill", egui::vec2(960.0, 540.0));
        couch.app.go_place(Place::Media);
        couch.press(PadButton::Mode);
        couch.settle();
        assert!(couch.app.big.home_open(), "Home opens the Home menu");
        dims_all_but_the_pill(&mut couch, "the Home menu", 960.0);
        couch.press(PadButton::RightTrigger);
        couch.settle();
        assert!(couch.app.big.home_open(), "R1 leaves the Home menu open");
        assert_eq!(
            couch.app.mobile.nav.place(),
            Place::Media,
            "R1 under the Home menu"
        );
    }

    #[test]
    fn a_sheet_dims_the_tabs_but_not_the_selected_pill() {
        let mut couch = Couch::long_games("big-sheet-pill", egui::vec2(1280.0, 720.0));
        couch.app.big.ring = true;
        for _ in 0..10 {
            if couch.focused().is_some() && couch.focused_rect().height() > 100.0 {
                break;
            }
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        couch.press(PadButton::West);
        couch.settle();
        assert!(
            couch.app.mobile.library.game().is_some(),
            "West opens the game's sheet"
        );
        dims_all_but_the_pill(&mut couch, "a sheet", 1280.0);
        couch.press(PadButton::RightTrigger);
        couch.settle();
        assert!(
            couch.app.mobile.library.game().is_some(),
            "R1 leaves the sheet open"
        );
        assert_eq!(
            couch.app.mobile.nav.place(),
            Place::Games,
            "R1 under a sheet"
        );
    }

    fn dims_all_but_the_pill(couch: &mut Couch, what: &str, width: f32) {
        let output = couch.showing();
        let brand = couch.ctx.style().visuals.selection.bg_fill;
        let bottom = tab_bottom(&couch.ctx);
        let hints = couch.ctx.content_rect().bottom() - hint_room(&couch.ctx);
        let dim = output
            .shapes
            .iter()
            .position(|clipped| {
                matches!(crate::app::widgets::confirm::flat_fill(&clipped.shape),
                    Some((rect, fill)) if rect.top() <= 0.5
                        && (rect.bottom() - hints).abs() <= 0.5
                        && rect.width() >= width - 1.0
                        && fill.a() > 0
                        && fill.a() < 255)
            })
            .unwrap_or_else(|| panic!("{what}: a dim from the top to the hints"));
        let pill = output
            .shapes
            .iter()
            .rposition(|clipped| {
                matches!(&clipped.shape, egui::Shape::Rect(shape)
                    if shape.fill == brand && shape.rect.bottom() <= bottom)
            })
            .unwrap_or_else(|| panic!("{what}: the selected pill"));
        assert!(pill > dim, "{what}: the selected pill paints over the dim");
        let marks = output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    ["L1", "R1", "LB", "RB"].contains(&text.galley.text().trim())
                        && text.visual_bounding_rect().bottom() <= bottom
                }
                _ => false,
            })
            .count();
        assert_eq!(marks, 0, "{what}: no shoulder marks while it is open");
    }

    #[test]
    fn the_game_menu_shows_its_scroll_bar_while_rows_follow() {
        let mut couch = Couch::sized("big-menu-bar", egui::vec2(960.0, 540.0));
        couch.start_game();
        couch.press(PadButton::Mode);
        couch.settle();
        assert!(couch.app.mobile.nav.more_open());
        let output = couch.showing();
        let bottom = tab_bottom(&couch.ctx);
        let handle = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.rect.left() < 300.0
                        && shape.rect.top() >= bottom
                        && shape.rect.width() <= 4.0
                        && shape.rect.height() >= 12.0
                        && shape.fill.a() > 0 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            });
        assert!(
            handle.is_some(),
            "the menu's scroll bar shows that more rows follow"
        );
    }

    #[cfg(feature = "nds")]
    #[test]
    fn down_from_the_walker_row_walks_the_games_page_column() {
        let mut couch = Couch::sized("big-games-down", egui::vec2(960.0, 540.0));
        couch.fill_library("game", 12);
        couch.app.big.ring = true;
        couch.press(PadButton::DPadDown);
        couch.settle();
        for _ in 0..6 {
            if couch.focus_holds("Pokéwalker") {
                break;
            }
            couch.press(PadButton::DPadUp);
            couch.settle();
        }
        assert!(couch.focus_holds("Pokéwalker"), "the walker row has focus");
        let left = couch.focused_rect().left();
        for _ in 0..10 {
            couch.press(PadButton::DPadDown);
            couch.settle();
            let rect = couch.focused_rect();
            assert!(
                (rect.left() - left).abs() <= 1.0 && rect.width() >= FIRST_FOCUS_MIN_WIDTH,
                "Down walks the page's left column, got {rect:?}"
            );
            if couch.focus_holds(crate::app::tabs::Tab::RomFolders.title()) {
                return;
            }
        }
        panic!("Down from the walker row reaches More games");
    }

    #[test]
    fn the_filter_ring_has_even_room_inside_the_tile_column() {
        let mut couch = Couch::sized("big-filter-ring", egui::vec2(960.0, 540.0));
        couch.fill_library("game", 12);
        couch.app.big.ring = true;
        couch.press(PadButton::DPadDown);
        couch.settle();
        let label = format!("{} Filter", crate::icons::FUNNEL);
        let output = couch.showing();
        let words = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == label => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .expect("the Filter text");
        let candidates = focus_candidates(&couch.ctx, Pass::Last);
        let Some((button, _, _)) = candidates
            .iter()
            .filter(|(_, rect, _)| rect.contains(words.center()))
            .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
            .copied()
        else {
            let rects: Vec<egui::Rect> = candidates.iter().map(|(_, rect, _)| *rect).collect();
            panic!("the Filter button at {words:?} takes focus, candidates {rects:?}");
        };
        couch.ctx.memory_mut(|m| m.request_focus(button));
        couch.settle();
        assert_eq!(
            couch.ctx.memory(|m| m.focused()),
            Some(button),
            "Filter has focus"
        );
        let output = couch.showing();
        let (_, ring) = ring::current(&couch.ctx).expect("a focused widget");
        let (ring, _) = ring.expect("Filter rings");
        let outer = ring.expand(ring::WIDTH);
        let text_of = |words: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().trim() == words => {
                        Some(text.visual_bounding_rect())
                    }
                    _ => None,
                })
        };
        let filter = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == label => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()))
                }
                _ => None,
            })
            .expect("the Filter text");
        let count = text_of("12 games").expect("the count");
        let column = focus_candidates(&couch.ctx, Pass::Last)
            .into_iter()
            .filter(|(_, rect, _)| rect.height() > 100.0)
            .map(|(_, rect, _)| rect.right())
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            outer.right() <= column + 0.5,
            "the ring {outer:?} stays inside the tile column ending at {column}"
        );
        let (left, right) = (filter.left() - ring.left(), ring.right() - filter.right());
        assert!(
            (left - right).abs() <= 1.0 && left.min(right) >= 8.0,
            "Filter sits evenly in its ring: {left} left, {right} right"
        );
        assert!(
            outer.left() - count.right() >= 8.0,
            "the ring {outer:?} keeps clear of the count {count:?}"
        );
    }

    #[test]
    fn tile_names_match_cover_names_and_clear_the_ring() {
        let mut couch = Couch::sized("big-tile-names", egui::vec2(960.0, 540.0));
        couch.fill_library("game", 12);
        couch
            .app
            .ui
            .recent
            .record(std::path::Path::new("/games/Tobu.gb"), "Tobu", None);
        couch.app.big.ring = true;
        for _ in 0..8 {
            if couch.focus_holds("Tobu") {
                break;
            }
            couch.press(PadButton::DPadDown);
            couch.settle();
        }
        assert!(couch.focus_holds("Tobu"), "the cover has focus");
        let size_of = |output: &egui::FullOutput, below: egui::Rect| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text)
                        if text.visual_bounding_rect().top() >= below.bottom()
                            && text.visual_bounding_rect().left() >= below.left() - 0.5
                            && text.visual_bounding_rect().right() <= below.right() + 0.5 =>
                    {
                        Some((
                            text.visual_bounding_rect(),
                            text.galley.job.sections[0].format.font_id.size,
                        ))
                    }
                    _ => None,
                })
                .expect("a name under the picture")
        };
        let output = couch.showing();
        let (_, cover) = ring::current(&couch.ctx).expect("a focused cover");
        let (cover, _) = cover.expect("the cover rings");
        let (_, cover_size) = size_of(&output, cover.expand(ring::WIDTH));
        couch.press(PadButton::DPadDown);
        couch.settle();
        let output = couch.showing();
        let (_, ring) = ring::current(&couch.ctx).expect("a focused tile");
        let (ring, _) = ring.expect("the tile rings");
        let outer = ring.expand(ring::WIDTH);
        let (name, tile_size) = size_of(&output, outer);
        assert_eq!(
            tile_size, cover_size,
            "tile names are the size of cover names"
        );
        assert!(
            name.top() - outer.bottom() >= ring::GAP - 0.5,
            "the name {name:?} keeps {} pt under the ring {outer:?}",
            ring::GAP
        );
        let frame = ring.shrink(ring::GAP);
        let below = couch
            .ctx
            .viewport(|v| {
                v.prev_pass
                    .widgets
                    .layers()
                    .flat_map(|(_, widgets)| widgets.iter())
                    .filter(|w| w.sense.is_focusable())
                    .filter(|w| (w.rect.left() - frame.left()).abs() <= 1.0)
                    .filter(|w| w.rect.top() > name.bottom())
                    .map(|w| w.rect.top())
                    .reduce(f32::min)
            })
            .expect("a tile in the next row");
        let own = name.top() - frame.bottom();
        let next = below - ring::MARGIN - name.bottom();
        assert!(
            next > 2.0 * own,
            "the name sits {own} pt under its tile and {next} pt over the next row's ring"
        );
    }

    #[test]
    fn down_from_a_shelf_cover_lands_on_the_stop_below_it() {
        let names = ["Tobu", "Libbet", "Deadeus", "Jetpak", "Citycard"];
        for pick in [0, 2, 4] {
            let mut couch = Couch::sized("big-shelf-down", egui::vec2(960.0, 540.0));
            couch.fill_library("game", 12);
            for name in names.iter().rev() {
                let path = format!("/games/{name}.gb");
                couch
                    .app
                    .ui
                    .recent
                    .record(std::path::Path::new(&path), name, None);
            }
            couch.app.big.ring = true;
            couch.press(PadButton::DPadDown);
            couch.settle();
            for _ in 0..8 {
                if couch.focus_holds(names[0]) {
                    break;
                }
                couch.press(PadButton::DPadDown);
                couch.settle();
            }
            assert!(couch.focus_holds(names[0]), "the first cover has focus");
            for _ in 0..pick {
                couch.press(PadButton::DPadRight);
                couch.settle();
            }
            assert!(couch.focus_holds(names[pick]), "cover {pick} has focus");
            let cover = couch.focused_rect();
            let overlap = |rect: &egui::Rect| {
                (rect.right().min(cover.right()) - rect.left().max(cover.left())).max(0.0)
            };
            let below: Vec<(Id, egui::Rect)> = focus_candidates(&couch.ctx, Pass::Last)
                .into_iter()
                .filter(|(_, rect, _)| rect.top() > cover.bottom() && overlap(rect) > 0.0)
                .map(|(id, rect, _)| (id, rect))
                .collect();
            couch.press(PadButton::DPadDown);
            couch.settle();
            let landed = couch
                .focused()
                .and_then(|id| below.iter().find(|(stop, _)| *stop == id))
                .map(|(_, rect)| *rect);
            let first_row = below
                .iter()
                .map(|(_, rect)| rect.top())
                .fold(f32::INFINITY, f32::min);
            let most = below
                .iter()
                .filter(|(_, rect)| (rect.top() - first_row).abs() <= 1.0)
                .map(|(_, rect)| overlap(rect))
                .fold(0.0, f32::max);
            assert!(
                landed
                    .is_some_and(|rect| (rect.top() - first_row).abs() <= 1.0
                        && overlap(&rect) >= most - 1.0),
                "Down from cover {pick} {cover:?} lands on the stop below it, got {landed:?} of {below:?}"
            );
        }
    }

    #[test]
    fn the_places_hold_their_x_when_a_game_opens() {
        let labels = ["Media", "You"];
        let lefts = |output: &egui::FullOutput| {
            labels.map(|label| {
                crate::app::tests::text_rect(output, label)
                    .unwrap_or_else(|| panic!("the {label} tab"))
                    .left()
            })
        };
        let mut couch = Couch::new("big-tabs-held");
        let output = couch.showing();
        let home = lefts(&output);
        couch.start_game();
        couch.press(PadButton::Mode);
        assert!(couch.app.mobile.nav.more_open(), "the game menu is open");
        let output = couch.showing();
        let game = lefts(&output);
        for (without, with) in home.iter().zip(&game) {
            assert!(
                (without - with).abs() <= 0.5,
                "places at {home:?} without a game, {game:?} with one"
            );
        }
    }

    #[test]
    fn the_volume_track_ends_at_the_value_column() {
        let mut couch = Couch::playing("big-volume-edge");
        couch.press(PadButton::Mode);
        let output = couch.showing();
        let off = crate::app::tests::text_rect(&output, "Off").expect("Fast forward's value");
        let volume = crate::app::tests::text_rect(&output, "Volume").expect("the Volume row");
        let slider = focus_candidates(&couch.ctx, Pass::Last)
            .into_iter()
            .map(|(_, rect, _)| rect)
            .find(|rect| {
                (rect.center().y - volume.center().y).abs() <= 2.0 && rect.left() > volume.right()
            })
            .expect("the volume slider");
        assert!(
            (slider.right() - off.right()).abs() <= 2.0,
            "the track ends at {}, the values at {}",
            slider.right(),
            off.right()
        );
    }

    #[test]
    fn page_actions_sit_in_the_tab_bar() {
        let mut couch = Couch::sized("big-page-actions", egui::vec2(960.0, 540.0));
        couch.fill_library("game", 12);
        couch.frame();
        let output = couch.showing();
        let bottom = tab_bottom(&couch.ctx);
        let you = crate::app::tests::text_rect(&output, "You").expect("the You tab");
        let in_bar = couch.ctx.viewport(|v| {
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .filter(|w| {
                    w.sense.is_focusable()
                        && w.rect.left() > you.right() + 20.0
                        && w.rect.bottom() <= bottom + 0.5
                })
                .count()
        });
        assert!(in_bar >= 1, "Search sits in the tab bar right of the tabs");
    }

    #[test]
    fn in_a_game_the_game_is_the_first_tab() {
        let mut couch = Couch::playing("big-tabs-game");
        assert_eq!(couch.app.tab_list().first(), Some(&tabs::Tab::Game));
        couch.press(PadButton::Mode);
        assert!(
            !couch.app.big.home_open(),
            "Home in a game is the game menu"
        );
        assert!(couch.app.mobile.nav.more_open());
        assert_eq!(couch.app.current_tab(), Some(tabs::Tab::Game));
        couch.press(PadButton::RightTrigger);
        assert_eq!(
            couch.app.current_tab(),
            Some(tabs::Tab::Place(Place::Games)),
            "R1 from the game goes to the library"
        );
        couch.press(PadButton::RightTrigger);
        assert_eq!(couch.app.mobile.nav.place(), Place::Media);
        couch.press(PadButton::LeftTrigger);
        couch.press(PadButton::LeftTrigger);
        let nav = &couch.app.mobile.nav;
        assert!(
            nav.on_game() && nav.more_open(),
            "L1 back to the game tab returns to the game with its menu open"
        );
        couch.press(PadButton::LeftTrigger);
        assert_eq!(
            couch.app.mobile.nav.place(),
            Place::You,
            "L1 from the first tab goes to the last"
        );
    }

    fn home_rows(couch: &mut Couch) -> Vec<&'static str> {
        let output = couch.showing();
        ["Friends", "Volume", "Quit Citrine"]
            .into_iter()
            .filter(|label| crate::app::tests::text_rect(&output, label).is_some())
            .collect()
    }

    #[test]
    fn home_outside_a_game_opens_the_home_menu_and_back_closes_it() {
        let mut couch = Couch::sized("big-home", egui::vec2(960.0, 540.0));
        couch.app.big.ring = true;
        couch.press(PadButton::DPadDown);
        couch.settle();
        let before = couch.focused().expect("a Games row has focus");
        couch.press(PadButton::Mode);
        couch.settle();
        assert!(couch.app.big.home_open(), "Home opens the Home menu");
        assert_eq!(home_rows(&mut couch), ["Friends", "Volume", "Quit Citrine"]);
        assert!(couch.focus_holds("Friends"), "focus starts on Friends");
        couch.press(PadButton::East);
        couch.settle();
        assert!(!couch.app.big.home_open(), "Back closes it");
        assert!(home_rows(&mut couch).is_empty(), "nothing of it is drawn");
        assert_eq!(couch.app.mobile.nav.place(), Place::Games, "Games stays");
        assert_eq!(couch.focused(), Some(before), "focus returns to the row");
    }

    #[test]
    fn a_page_switch_with_the_ring_on_focuses_the_new_page_at_once() {
        let mut couch = Couch::sized("big-switch-focus", egui::vec2(960.0, 540.0));
        couch.fill_library("game", 6);
        couch.frame();
        assert_eq!(couch.focused(), None, "no focus before the ring is on");
        couch.app.big.ring = true;
        couch.app.go_place(Place::Media);
        couch.frame();
        assert_eq!(couch.app.mobile.nav.place(), Place::Media);
        let focused = couch.focused().expect("the ring puts focus on the page");
        let drawn = couch.ctx.viewport(|v| {
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .any(|w| w.id == focused)
        });
        assert!(drawn, "focus is on a widget of the frame that drew Media");
    }

    #[test]
    fn start_outside_a_game_opens_the_home_menu_too() {
        let mut couch = Couch::new("big-home-start");
        couch.press(PadButton::Start);
        assert!(couch.app.big.home_open(), "Start opens the Home menu");
        couch.press(PadButton::Start);
        assert!(!couch.app.big.home_open(), "Start again closes it");
    }

    #[test]
    fn the_home_menu_friends_row_shows_a_waiting_request() {
        let mut couch = Couch::new("big-home-waiting");
        let account = citrine_gb_net::Account {
            online_id: citrine_gb_net::OnlineId::new(0x1234_5678).unwrap(),
            display_name: "Clara".into(),
            storage_used: 0,
            quota_bytes: 0,
            two_factor: false,
            codes_left: 0,
        };
        let relay = citrine_gb_net::RelayId::random();
        couch.app.ui.settings.online.remember(
            &crate::online::address::ServerAddress::parse("relay.example").unwrap(),
            &relay,
            "pw",
            &account,
            &Default::default(),
        );
        let (mut online, _wire, _) = crate::online::tests::signed_in(&relay);
        online.befriend_for_test(citrine_gb_net::Friend {
            online_id: citrine_gb_net::OnlineId::new(7).unwrap(),
            display_name: "Ana".into(),
            public_key: citrine_gb_net::PublicKey([4; 32]),
            state: citrine_gb_net::FriendState::Incoming,
            presence: None,
        });
        couch.app.online = online;
        couch.app.big.ring = true;
        couch.press(PadButton::Mode);
        couch.settle();
        assert!(couch.app.big.home_open());
        assert!(couch.focus_holds("Friends"), "the menu opens on Friends");
        let output = couch.showing();
        let row = couch.focused_rect();
        let dot = output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Circle(circle) => {
                circle.fill == crate::app::widgets::avatar::DOT && row.contains(circle.center)
            }
            _ => false,
        });
        assert!(dot, "the Friends row carries the waiting dot");
    }

    #[test]
    fn the_home_menu_opens_friends() {
        for signed_in in [true, false] {
            let mut couch = Couch::new("big-home-friends");
            if signed_in {
                let account = citrine_gb_net::Account {
                    online_id: citrine_gb_net::OnlineId::new(0x1234_5678).unwrap(),
                    display_name: "Clara".into(),
                    storage_used: 0,
                    quota_bytes: 0,
                    two_factor: false,
                    codes_left: 0,
                };
                couch.app.ui.settings.online.remember(
                    &crate::online::address::ServerAddress::parse("relay.example").unwrap(),
                    &citrine_gb_net::RelayId::random(),
                    "pw",
                    &account,
                    &Default::default(),
                );
            }
            couch.app.big.ring = true;
            couch.press(PadButton::Mode);
            couch.settle();
            couch.press(PadButton::South);
            couch.settle();
            assert!(!couch.app.big.home_open(), "the menu closes");
            let place = if signed_in {
                Place::Friends
            } else {
                Place::You
            };
            assert_eq!(couch.app.mobile.nav.place(), place, "signed in {signed_in}");
            assert!(couch.focused().is_some(), "focus lands on {place:?}");
        }
    }

    #[test]
    fn the_home_menu_volume_goes_off_at_zero_and_back_on() {
        let mut couch = Couch::new("big-home-volume");
        couch.app.ui.settings.volume = 0.02;
        couch.app.big.ring = true;
        couch.press(PadButton::Mode);
        couch.settle();
        couch.press(PadButton::DPadDown);
        couch.settle();
        assert!(couch.on_stepper(), "Volume comes after Friends");
        for _ in 0..3 {
            couch.press(PadButton::DPadLeft);
            couch.settle();
        }
        assert!(
            couch.app.ui.settings.muted,
            "Left down to 0 turns the sound off, volume {}",
            couch.app.ui.settings.volume
        );
        couch.press(PadButton::DPadRight);
        couch.settle();
        assert!(!couch.app.ui.settings.muted, "Right from 0 turns it on");
        assert!(couch.app.big.home_open(), "the menu stays open");
    }

    #[test]
    fn the_big_screen_library_shows_four_games_a_row() {
        let mut couch = Couch::long_games("big-columns", egui::vec2(960.0, 540.0));
        let output = couch.showing();
        let mut tops: Vec<f32> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().starts_with("game ") => {
                    Some(text.visual_bounding_rect().top())
                }
                _ => None,
            })
            .collect();
        tops.sort_by(f32::total_cmp);
        let first = *tops.first().expect("the library draws its games");
        let row = tops.iter().filter(|top| (*top - first).abs() < 1.0).count();
        assert_eq!(row, 4, "the first row holds 4 games, tops {tops:?}");
    }

    #[test]
    fn the_recent_shelf_shows_five_games_in_one_row() {
        let mut couch = Couch::sized("big-recent-five", egui::vec2(960.0, 540.0));
        let names = ["Tobu", "Libbet", "Deadeus", "Jetpak", "Citycard"];
        for name in names {
            let path = format!("/games/{name}.gb");
            couch
                .app
                .ui
                .recent
                .record(std::path::Path::new(&path), name, None);
        }
        let output = couch.showing();
        let page = couch.page_rect();
        let rects: Vec<egui::Rect> = names
            .iter()
            .map(|name| {
                crate::app::tests::text_rect(&output, name)
                    .unwrap_or_else(|| panic!("{name} is drawn"))
            })
            .collect();
        for rect in &rects {
            assert!(
                (rect.top() - rects[0].top()).abs() < 1.0,
                "one row, {rects:?}"
            );
            assert!(page.contains_rect(*rect), "{rect:?} in {page:?}");
        }
    }
}
