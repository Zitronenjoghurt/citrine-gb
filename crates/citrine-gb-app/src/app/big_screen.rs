use crate::app::Citrine;
use crate::app::mobile::nav::Place;
use crate::app::ui_state::keybinds::{BindingKind, PadButton};
use crate::controller_debug::PadInfo;
use crate::pad_input::{PadEvent, PadInput, PadMotion, PadSource};
use egui::{Context, Id, Ui};
use glyphs::{Family, Glyph, Hint};
use pad_nav::{Command, Dir};
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Display;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

mod glyphs;
mod keyboard;
mod move_screen;
mod pad_nav;
pub(crate) mod ring;
pub(crate) mod scroll;

pub const DESIGN_POINTS: f32 = 540.0;
pub const COLUMN_MAX_WIDTH: f32 = 880.0;
const TV_SIDES: f32 = 48.0;
const TV_ENDS: f32 = 27.0;
const FLAT_SHARE: f32 = 0.8;

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
}

pub const MENU_BUTTON: &str = "Home, or hold Select + Start";
pub const BACKDROP: &str = "Around the game";

pub fn cycle<T: IntoEnumIterator + PartialEq + Copy>(value: T, step: i32) -> T {
    let all: Vec<T> = T::iter().collect();
    let at = all.iter().position(|v| *v == value).unwrap_or(0) as i32;
    let len = all.len() as i32;
    all[(at + step).rem_euclid(len) as usize]
}

fn stepper_id() -> Id {
    Id::new("big_screen_stepper")
}

fn step_id() -> Id {
    Id::new("big_screen_step")
}

pub fn offer_steps(ctx: &Context, response: &egui::Response) {
    if response.has_focus() {
        ctx.data_mut(|d| d.insert_temp(stepper_id(), response.id));
    }
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
    page_step: i32,
    search: bool,
    wants_focus: bool,
    focus_menu: bool,
    focused: Option<Id>,
    last_input: Option<web_time::Instant>,
    scrolled: bool,
    moving: bool,
    screen_move: move_screen::ScreenMove,
    pub(super) covers: Covers,
    pub(super) missing: Vec<Id>,
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

#[derive(Default)]
pub struct Covers {
    playing: Option<String>,
    boxes: bool,
    loaded: std::collections::HashMap<String, Option<Cover>>,
}

impl Covers {
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
        self.loaded
            .entry(rom_key.to_owned())
            .or_insert_with(|| {
                boxes
                    .then(|| box_art(ctx, store, rom_key))
                    .flatten()
                    .or_else(|| autosave_picture(ctx, store, rom_key))
            })
            .clone()
    }

    pub fn forget(&mut self, rom_key: &str) {
        self.loaded.remove(rom_key);
    }
}

fn box_art(ctx: &Context, store: &crate::storage::SaveStore, rom_key: &str) -> Option<Cover> {
    let frame = crate::covers::decode(&store.covers().art(rom_key)?)?;
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

fn autosave_picture(
    ctx: &Context,
    store: &crate::storage::SaveStore,
    rom_key: &str,
) -> Option<Cover> {
    let info = store.newest_autosave_info(rom_key)?;
    let [width, height] = info.thumb_size();
    let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &info.thumbnail);
    let texture = ctx.load_texture(
        format!("cover_{rom_key}"),
        image,
        egui::TextureOptions::NEAREST,
    );
    let two_screens = info.console == Some(citrine_gb::console::ConsoleKind::Nds);
    let shown = if two_screens { 0.5 } else { 1.0 };
    let shown_bytes = width * (height as f32 * shown) as usize * 4;
    if mostly_one_colour(&info.thumbnail[..shown_bytes.min(info.thumbnail.len())]) {
        return None;
    }
    Some(Cover {
        texture,
        uv: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, shown)),
        size: egui::vec2(width as f32, height as f32 * shown),
        boxed: false,
    })
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
    pub fn menu_was_pressed(&mut self) -> bool {
        std::mem::take(&mut self.menu_pressed)
    }

    pub fn focus_menu(&mut self) {
        self.focus_menu = true;
    }

    pub fn take_menu_focus(&mut self) -> bool {
        std::mem::take(&mut self.focus_menu)
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
        if self.big.moving && self.big.screen_move.keys(&raw.events) {
            self.big.moving = false;
        }
        if crate::platform::is_tv() {
            tv_margins(raw);
        }
        let now = web_time::Instant::now();
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
        if speed != 0.0 && !playing && !self.big.typing && !self.big.moving {
            self.big.ring = true;
            scroll::by_lines(ctx, speed * scroll::STICK_SPEED * since);
            ctx.request_repaint();
        }
        let stepper = ctx.data_mut(|d| {
            d.remove::<(Id, i32)>(step_id());
            let stepper = d.get_temp::<Id>(stepper_id());
            d.remove::<Id>(stepper_id());
            stepper
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

    fn shown_family(&self) -> Family {
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
        if self.big.moving {
            if self.big.screen_move.command(command) {
                self.big.moving = false;
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
            Command::Confirm => raw.events.extend(tap(egui::Key::Enter)),
            Command::Back => raw.events.extend(tap(egui::Key::Escape)),
            Command::PagePrevious => self.big.page_step -= 1,
            Command::PageNext => self.big.page_step += 1,
            Command::FaceNorth => self.big.search = true,
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
        if step != 0 {
            self.step_page(step);
        }
        if std::mem::take(&mut self.big.search) && self.mobile.nav.place() == Place::Games {
            self.open_games_search();
        }
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
            self.go_place(Place::Games);
            return;
        }
        let nav = &self.mobile.nav;
        if nav.on_game() && nav.more_open() {
            self.resume_game();
        } else {
            self.open_game_menu();
        }
    }

    fn step_page(&mut self, step: i32) {
        use crate::app::mobile::settings_list::Entry;
        let nav = &self.mobile.nav;
        if let Some(page) = nav.settings_page() {
            let pages: Vec<_> =
                crate::app::mobile::settings_list::entries(self.ui.settings.dev_mode)
                    .into_iter()
                    .filter_map(|entry| match entry {
                        Entry::Page(page) => Some(page),
                        Entry::About => None,
                    })
                    .collect();
            let at = pages.iter().position(|p| *p == page).unwrap_or(0) as i32;
            let next = pages[(at + step).rem_euclid(pages.len() as i32) as usize];
            self.mobile.nav.open_settings(next);
            return;
        }
        let places: Vec<Place> = Place::ALL
            .into_iter()
            .filter(|place| self.ui.settings.online.is_joined() || *place != Place::Friends)
            .collect();
        let at = places
            .iter()
            .position(|p| *p == self.mobile.nav.place())
            .unwrap_or(0) as i32;
        let next = places[(at + step).rem_euclid(places.len() as i32) as usize];
        self.go_place(next);
    }

    pub(super) fn big_screen_end(&mut self, ctx: &Context) {
        if !self.big_screen_on() {
            self.big = BigScreen::default();
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
        self.big.moving &= in_game;
        self.big.playing = in_game && !self.big.moving;
        let paging = !in_game && !typing;
        let scrolled = scroll::end_pass(ctx, paging);
        if paging && self.big.ring && std::mem::replace(&mut self.big.scrolled, scrolled) {
            scroll::refocus_in_view(ctx);
        }
        if in_game {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        } else if self.big.ring
            && !typing
            && (self.big.wants_focus || ctx.memory(|m| m.focused().is_none()))
            && let Some(first) = first_focus(ctx)
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
        if let Some(progress) = self.big.nav.hold_progress(web_time::Instant::now()) {
            let screen = ctx.content_rect();
            let centre = egui::pos2(screen.right() - 48.0, screen.top() + 48.0);
            egui::Area::new(Id::new("menu_hold"))
                .order(egui::Order::Tooltip)
                .fixed_pos(screen.min)
                .interactable(false)
                .show(ctx, |ui| glyphs::hold_ring(ui, centre, progress));
        }
    }

    pub(super) fn big_hints(&self, ui: &mut Ui) {
        let confirm = self.confirm_button();
        let back = self.back_button();
        let nav = &self.mobile.nav;
        let hints = if self.big.typing && !self.big.keys_last {
            vec![
                Hint::new([Glyph::Face(confirm)], "Type"),
                Hint::new([Glyph::Face(back)], "Hide keyboard"),
            ]
        } else if nav.on_game() && nav.more_open() {
            vec![
                Hint::new([Glyph::Face(confirm)], "Choose"),
                Hint::new([Glyph::Face(back)], "Back to game"),
            ]
        } else if nav.settings_page().is_some() {
            vec![
                Hint::new([Glyph::Sideways], "Change"),
                Hint::new([Glyph::Face(back)], "Back"),
                Hint::new(
                    [
                        Glyph::Shoulder(PadButton::LeftTrigger),
                        Glyph::Shoulder(PadButton::RightTrigger),
                    ],
                    "Pages",
                ),
            ]
        } else {
            let focused = ui.ctx().memory(|m| m.focused());
            let missing = focused.is_some_and(|id| self.big.missing.contains(&id));
            let open = if missing { "Remove" } else { "Open" };
            let mut hints = vec![Hint::new([Glyph::Face(confirm)], open)];
            if self.mobile.nav.depth() > 0 {
                hints.push(Hint::new([Glyph::Face(back)], "Back"));
            }
            if nav.place() == Place::Games && nav.page().is_none() && self.games_searchable() {
                hints.push(Hint::new([Glyph::Face(PadButton::North)], "Search"));
            }
            hints
        };
        glyphs::hint_bar(ui, &hints, self.shown_family(), confirm);
    }
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

const FIRST_FOCUS_MIN_WIDTH: f32 = 48.0;

fn focus_candidates(ctx: &Context) -> Vec<(Id, egui::Rect)> {
    let modal = ctx.memory(|m| m.top_modal_layer());
    let candidates = ctx.viewport(|viewport| {
        let widgets = &viewport.prev_pass.widgets;
        let focusable = |layer: &egui::LayerId| {
            widgets
                .get_layer(*layer)
                .filter(|widget| widget.sense.is_focusable() && widget.enabled)
                .filter(|widget| widget.rect.area() > 0.0)
                .map(|widget| (widget.id, widget.rect, layer.order, widget.sense))
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
    let Some(top) = candidates.iter().map(|(_, _, order, _)| *order).max() else {
        return Vec::new();
    };
    let top: Vec<(Id, egui::Rect, bool)> = candidates
        .into_iter()
        .filter(|(_, _, order, _)| *order == top)
        .map(|(id, rect, _, sense)| (id, rect, sense.senses_click()))
        .collect();
    top.iter()
        .filter(|(id, rect, clicks)| {
            *clicks
                || !top
                    .iter()
                    .any(|(other, inner, _)| other != id && rect.contains_rect(*inner))
        })
        .map(|(id, rect, _)| (*id, *rect))
        .collect()
}

fn first_focus(ctx: &Context) -> Option<Id> {
    let rail = crate::app::mobile::frame::rail_width(&ctx.style());
    let content = |rect: &egui::Rect| rect.left() >= rail - 1.0;
    let small = |rect: &egui::Rect| rect.width() < FIRST_FOCUS_MIN_WIDTH;
    let key = |rect: &egui::Rect| (rect.top() / 8.0).round() * 10_000.0 + rect.left();
    focus_candidates(ctx)
        .into_iter()
        .min_by(|a, b| {
            (!content(&a.1))
                .cmp(&!content(&b.1))
                .then(small(&a.1).cmp(&small(&b.1)))
                .then(key(&a.1).total_cmp(&key(&b.1)))
        })
        .map(|(id, _)| id)
}

fn next_focus(ctx: &Context, from: Id, dir: Dir) -> Option<Id> {
    let candidates = focus_candidates(ctx);
    let from_rect = ctx.read_response(from).map(|response| response.rect)?;
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
    candidates
        .into_iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, rect)| ahead(rect) > -1.0)
        .min_by(|a, b| {
            let score = |rect: &egui::Rect| ahead(rect).max(0.0) + 2.0 * across(rect);
            score(&a.1)
                .total_cmp(&score(&b.1))
                .then(a.1.left().total_cmp(&b.1.left()))
                .then(a.1.top().total_cmp(&b.1.top()))
        })
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::phone_app;
    use crate::app::ui_state::settings::SettingsTab;
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
            };
            couch.frame();
            couch
        }

        fn playing(name: &str) -> Self {
            let mut couch = Self::new(name);
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
            couch.app.emulator.running = true;
            couch.app.mobile.nav.show_game();
            couch.frame();
            couch
        }

        fn frame(&mut self) {
            self.frame_with(Vec::new());
        }

        fn frame_with(&mut self, events: Vec<egui::Event>) {
            self.output_with(events);
        }

        fn output_with(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                events,
                ..Default::default()
            };
            let app = &mut self.app;
            app.big_screen_input(&self.ctx, &mut raw);
            self.ctx.run(raw, |ctx| {
                app.render(ctx);
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
    fn the_right_stick_and_the_triggers_scroll_the_page() {
        let mut couch = Couch::long_games("big-stick", egui::vec2(1280.0, 720.0));
        couch.press(PadButton::DPadDown);
        let before = couch.row_tops();
        couch.move_stick(-1.0);
        for _ in 0..30 {
            couch.frame();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        couch.move_stick(0.0);
        couch.frame();
        couch.frame();
        let after = couch.row_tops();
        let moved = before.iter().zip(&after).any(|(a, b)| b < a);
        assert!(moved, "the stick scrolls the page down");
        let rect = couch.focused_rect();
        assert!(
            couch.page_rect().contains_rect(rect),
            "focus follows into view, {rect:?}"
        );
        let low = couch.row_tops();
        couch.press(PadButton::LeftTrigger2);
        couch.frame();
        let up = couch.row_tops();
        assert!(
            low.iter().zip(&up).any(|(a, b)| b > a),
            "the left trigger pages up"
        );
        couch.press(PadButton::RightTrigger2);
        couch.frame();
        let down = couch.row_tops();
        assert!(
            up.iter().zip(&down).any(|(a, b)| b < a),
            "the right trigger pages down"
        );
    }

    #[test]
    fn the_wheel_scrolls_the_page_from_its_margins() {
        let mut couch = Couch::long_games("big-wheel", egui::vec2(1280.0, 720.0));
        let column_left = (1280.0 + crate::app::mobile::frame::RAIL_WIDTH - COLUMN_MAX_WIDTH) / 2.0;
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
        let mut check = |couch: &Couch| {
            let (response, ring) = ring::current(&couch.ctx).expect("a focused widget");
            let item = response.rect;
            match ring {
                None => {
                    kinds.insert("fill");
                }
                Some((ring, _)) => {
                    let outer = ring.expand(ring::WIDTH);
                    assert!(
                        item.expand(ring::MARGIN + 0.5).contains_rect(outer),
                        "the ring {outer:?} stays within {item:?} plus the margin"
                    );
                    let rail = item.right() <= crate::app::mobile::frame::RAIL_WIDTH + 1.0;
                    if rail && ring.width() + 1.0 < item.width() {
                        kinds.insert("rail");
                    } else if ring.height() + 1.0 < item.height() {
                        kinds.insert("tile");
                    } else {
                        kinds.insert("ring");
                    }
                }
            }
        };
        for _ in 0..8 {
            couch.press(PadButton::DPadDown);
            check(&couch);
        }
        couch.press(PadButton::DPadLeft);
        check(&couch);
        assert!(couch.focused_rect().right() <= crate::app::mobile::frame::RAIL_WIDTH + 1.0);
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::Controls);
        couch.frame();
        for _ in 0..14 {
            couch.press(PadButton::DPadDown);
            check(&couch);
        }
        drop(check);
        assert!(kinds.contains("fill"), "rows fill, {kinds:?}");
        assert!(
            kinds.contains("tile"),
            "a tile rings its picture only, {kinds:?}"
        );
        assert!(
            kinds.contains("rail"),
            "a rail entry rings its pill, {kinds:?}"
        );
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
                field.rect.top() < 16.0,
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
        std::thread::sleep(std::time::Duration::from_millis(1100));
        couch.frame();
        couch.frame();
        assert!(couch.app.mobile.nav.more_open(), "the hold opens the menu");
        couch.pads.0.borrow_mut().extend(held(PadInput::Released));
        couch.frame();
        assert!(couch.app.mobile.nav.more_open(), "letting go keeps it open");
    }

    #[test]
    fn back_closes_the_top_layer_then_goes_back() {
        let mut couch = Couch::new("big-back");
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::General);
        couch.frame();
        assert_eq!(
            couch.app.mobile.nav.settings_page(),
            Some(SettingsTab::General)
        );
        couch.press(PadButton::East);
        assert_eq!(
            couch.app.mobile.nav.settings_page(),
            None,
            "back leaves the page"
        );
        assert_eq!(couch.app.mobile.nav.place(), Place::Settings);
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
        for _ in 0..16 {
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
    fn left_and_right_change_a_setting_in_place() {
        let mut couch = Couch::new("big-stepper");
        couch.app.go_place(Place::Settings);
        couch.app.mobile.nav.open_settings(SettingsTab::General);
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
    fn the_screen_moves_by_drag_and_by_pad_and_centres_again() {
        let mut couch = Couch::playing("big-move");
        couch.app.emulator.clear_frame(&couch.ctx);
        let middle = picture(&couch.showing());
        couch.drag(middle.center(), egui::vec2(-200.0, 0.0));
        let dragged = picture(&couch.showing());
        assert_eq!(dragged, middle.translate(egui::vec2(-200.0, 0.0)));

        couch.press(PadButton::Mode);
        let output = couch.showing();
        let entry = crate::app::tests::text_rect(&output, "Move screen").expect("a menu entry");
        couch.frame_with(crate::app::tests::tap(entry.center()));
        couch.frame();
        assert!(!couch.app.mobile.nav.more_open(), "the menu closes");
        assert!(
            !couch.app.emulator.running,
            "the game waits while the screen moves"
        );
        couch.press(PadButton::DPadRight);
        let stepped = picture(&couch.showing());
        assert!(
            stepped.left() > dragged.left(),
            "{stepped:?} after {dragged:?}"
        );
        couch.press(PadButton::North);
        assert_eq!(
            picture(&couch.showing()),
            middle,
            "North centres the screen"
        );
        couch.press(PadButton::East);
        assert!(couch.app.emulator.running, "Back goes on with the game");
        assert!(couch.app.ui.settings.dirty, "the place is saved");
    }
}
