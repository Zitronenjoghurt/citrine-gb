use crate::app::tabs::TabViewer;
use crate::app::ui_state::recent::RecentRoms;
use crate::app::widgets::avatar;
use crate::emulator::Emulator;
use crate::icons;
use crate::online::{Online, OnlineSettings, Seen, seen};
use crate::play::{
    Ending, GameFile, IncomingInvite, MyGame, Play, PlaySession, Shelf, Stage, model_of,
};
use citrine_gb_net::{GameCard, GameHash, Mode, OnlineId, SessionId};
use egui::{Color32, Label, RichText, Ui};
use std::collections::HashMap;

const GOOD_MS: u32 = 80;
const OK_MS: u32 = 150;
const ON_DARK: [Color32; 3] = [
    Color32::from_rgb(0x40, 0xC0, 0x57),
    Color32::from_rgb(0xF2, 0xB1, 0x2E),
    Color32::from_rgb(0xE6, 0x45, 0x53),
];
const ON_LIGHT: [Color32; 3] = [
    Color32::from_rgb(0x2B, 0x8A, 0x3E),
    Color32::from_rgb(0xA8, 0x62, 0x00),
    Color32::from_rgb(0xC9, 0x2A, 0x3A),
];
const PICKER_WIDTH: f32 = 420.0;
const STRIP_AVATAR: f32 = 22.0;
const BAR_FOLD_WIDTH: f32 = 640.0;
const SCREEN_WIDTH: usize = 160;
const SCREEN_HEIGHT: usize = 144;
const MAX_SCREEN_SCALE: f32 = 3.0;

#[derive(Default)]
pub struct PlayingUi {
    friend_screen: Option<egui::TextureHandle>,
    painted_pass: u64,
    pub asking_stop: bool,
    pub ended: Option<Ended>,
    pub shown: bool,
    pub picking: Option<PickFor>,
    pub switching: Option<OnlineId>,
    pub switch_to: Option<OnlineId>,
    pub file_for: Option<PickFor>,
    pub swapped: bool,
    pub wants_copy: bool,
    pub copy_for: bool,
    pub pick_query: String,
    pick_shown: bool,
    fits: HashMap<(SessionId, Option<GameHash>), Fit>,
}

#[derive(Debug, Clone)]
pub enum PickFor {
    Invite(OnlineId),
    Join(IncomingInvite),
    Change,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub friend: OnlineId,
    pub line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Open,
    Shelf,
    Borrow,
    Cable,
    Watch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayButton {
    Ready,
    Switch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BarAction {
    Leave,
    ChangeGame,
    Show(bool),
    Watch(bool),
    Cable(bool),
    Mute,
    Chat,
    FriendSound(FriendSound),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FriendSound {
    pub volume: f32,
    pub muted: bool,
}

impl FriendSound {
    pub fn of(settings: &OnlineSettings) -> Self {
        Self {
            volume: settings.friend_volume,
            muted: settings.friend_muted,
        }
    }

    pub fn store(self, settings: &mut OnlineSettings) {
        settings.friend_volume = self.volume;
        settings.friend_muted = self.muted;
    }

    fn silent(self) -> bool {
        self.muted || self.volume == 0.0
    }

    fn toggled(self) -> Self {
        if !self.silent() {
            return Self {
                muted: true,
                ..self
            };
        }
        let volume = if self.volume == 0.0 {
            OnlineSettings::DEFAULT_FRIEND_VOLUME
        } else {
            self.volume
        };
        Self {
            volume,
            muted: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BarFacts {
    pub muted: bool,
    pub compact: bool,
    pub friend_sound: FriendSound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    Show,
    Watch,
    Cable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarSwitch {
    pub switch: Switch,
    pub label: String,
    pub on: bool,
    pub why_off: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerAction {
    PlayAgain,
    Dismiss,
}

pub fn fit(open_matches: bool, on_shelf: bool, sharing: bool, game_open: bool) -> Fit {
    if open_matches {
        Fit::Open
    } else if on_shelf {
        Fit::Shelf
    } else if sharing {
        Fit::Borrow
    } else if game_open {
        Fit::Cable
    } else {
        Fit::Watch
    }
}

pub fn quality(ms: u32, dark: bool) -> (&'static str, Color32) {
    let [good, ok, slow] = if dark { ON_DARK } else { ON_LIGHT };
    if ms < GOOD_MS {
        ("Good", good)
    } else if ms < OK_MS {
        ("OK", ok)
    } else {
        ("Slow", slow)
    }
}

pub fn short_name(display_name: &str) -> &str {
    display_name
        .split_whitespace()
        .next()
        .unwrap_or(display_name)
}

pub fn friend_name(online: &Online, friend: OnlineId) -> String {
    online
        .friend(friend)
        .map(|f| short_name(&f.display_name).to_owned())
        .unwrap_or_else(|| friend.to_string())
}

pub fn ending_line(name: &str, ending: Ending, played: bool) -> Option<String> {
    match ending {
        Ending::Left => None,
        Ending::Declined => Some(format!("{name} declined your invite.")),
        Ending::FriendLeft => Some(format!("{name} left.")),
        Ending::Dropped => Some(format!("Lost the connection to {name}.")),
        Ending::Failed if played => Some(format!("Playing with {name} stopped.")),
        Ending::Failed => Some(format!("Couldn't start playing with {name}.")),
    }
}

pub fn other_game_line(
    name: &str,
    i_hold_theirs: bool,
    my_title: &str,
    their_title: &str,
    round_trip_ms: Option<u32>,
) -> String {
    let watch = if i_hold_theirs {
        format!("{name} doesn't have {my_title}, so you can't watch each other's screens.")
    } else {
        format!("You don't have {their_title}, so you can't watch {name}'s screen.")
    };
    match round_trip_ms {
        Some(ms) => format!("{watch} Each link cable exchange takes {ms} ms."),
        None => watch,
    }
}

pub fn size_line(got: u32, total: u32) -> String {
    const MB: f64 = 1_000_000.0;
    if total as f64 >= MB {
        format!("{:.1} of {:.1} MB", got as f64 / MB, total as f64 / MB)
    } else {
        format!("{} of {} KB", got.div_ceil(1000), total.div_ceil(1000))
    }
}

pub fn play_subtitle(play: &Play, friend: OnlineId) -> Option<String> {
    if let Some(session) = play
        .session()
        .filter(|s| s.is_live() && s.friend() == friend)
    {
        let line = match session.stage() {
            Stage::Inviting => "Invite sent",
            _ if needs_copy(session) => return Some(format!("Needs {}", game_title(session))),
            _ if session.watching_only() => "You're watching",
            _ if session.friend_plays() == Some(false) => "Watching you",
            _ => "Playing with you",
        };
        return Some(line.into());
    }
    play.invites()
        .iter()
        .find(|i| i.from == friend)
        .map(|i| match &i.game {
            Some(game) => format!("Wants to play {}", card_name(game)),
            None => "Wants to join you".into(),
        })
}

pub fn play_button(play: &Play, online: &Online, friend: OnlineId) -> Option<PlayButton> {
    let person = online.friend(friend)?;
    if !online.is_connected() || matches!(seen(person.presence), Seen::Offline { .. }) {
        return None;
    }
    if play.invites().iter().any(|i| i.from == friend) {
        return None;
    }
    match play.session().filter(|s| s.is_live()) {
        Some(session) if session.friend() == friend => None,
        Some(_) => Some(PlayButton::Switch),
        None => Some(PlayButton::Ready),
    }
}

pub fn can_watch(play: &Play, online: &Online, friend: OnlineId, game_open: bool) -> bool {
    !game_open
        && !play.is_live()
        && play_button(play, online, friend).is_some()
        && online.playing(friend).is_some()
}

pub fn shelf_from(recent: &RecentRoms) -> Box<dyn Shelf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Box::new(crate::play::shelf::Files {
            paths: recent.entries().iter().map(|e| e.path.clone()).collect(),
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = recent;
        Box::new(crate::play::shelf::Homebrew)
    }
}

pub fn fit_of(
    playing: &mut PlayingUi,
    play: &mut Play,
    online: &Online,
    emulator: &Emulator,
    recent: &RecentRoms,
    invite: &IncomingInvite,
) -> Fit {
    let open = emulator.open_game();
    let key = (invite.session, open);
    if let Some(found) = playing.fits.get(&key) {
        return *found;
    }
    play.set_shelf(shelf_from(recent));
    let sharing = online.info().is_some_and(|i| i.game_sharing);
    let found = match &invite.game {
        None if open.is_some() => Fit::Open,
        None => Fit::Watch,
        Some(game) => fit(
            open == Some(game.hash),
            play.holds(&game.hash),
            sharing,
            open.is_some(),
        ),
    };
    playing.fits.insert(key, found);
    found
}

fn prepare(emulator: &mut Emulator, playing: &mut PlayingUi) {
    if let Err(err) = emulator.end_link() {
        log::error!("could not save the second game before playing together: {err:?}");
    }
    if let Err(err) = emulator.autosave_if_played() {
        log::error!("could not autosave before playing together: {err:?}");
    }
    playing.ended = None;
    playing.shown = false;
    playing.swapped = false;
}

fn tune(play: &mut Play, emulator: &Emulator) {
    let rate = emulator.gb.apu.output_sample_rate;
    if let Some(session) = play.session_mut() {
        session.set_sample_rate(rate);
    }
}

pub fn invite(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    friend: OnlineId,
) -> bool {
    if play
        .session()
        .is_some_and(|s| s.is_live() && s.friend() != friend)
    {
        playing.switching = Some(friend);
        return true;
    }
    let Some(game) = emulator.game_file() else {
        playing.picking = Some(PickFor::Invite(friend));
        return true;
    };
    send_invite(play, online, emulator, playing, recent, friend, Some(game))
}

pub fn watch(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    friend: OnlineId,
) -> bool {
    if play.is_live() {
        return false;
    }
    send_invite(play, online, emulator, playing, recent, friend, None)
}

fn send_invite(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    friend: OnlineId,
    game: Option<GameFile>,
) -> bool {
    prepare(emulator, playing);
    play.set_shelf(shelf_from(recent));
    if !play.invite(online, friend, game) {
        return false;
    }
    tune(play, emulator);
    true
}

pub fn join(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    invite: &IncomingInvite,
) -> bool {
    let fit = fit_of(playing, play, online, emulator, recent, invite);
    let game = match (fit, &invite.game) {
        (Fit::Open | Fit::Cable, _) => match emulator.game_file() {
            Some(file) => MyGame::Own(file),
            None => return false,
        },
        (Fit::Shelf, Some(card)) => match play.find(&card.hash) {
            Some(rom) => MyGame::Own(GameFile {
                save: emulator.battery_for(&rom),
                rom,
                title: card_name(card).to_string(),
                model: model_of(card.console),
                running: false,
            }),
            None => return false,
        },
        (Fit::Borrow, Some(card)) => MyGame::Borrow {
            save: emulator.borrowed_save(&card.hash),
        },
        (Fit::Watch, _) => MyGame::Nothing,
        (Fit::Shelf | Fit::Borrow, None) => return false,
    };
    join_with(play, online, emulator, playing, invite, game)
}

pub fn join_with(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    invite: &IncomingInvite,
    game: MyGame,
) -> bool {
    prepare(emulator, playing);
    if !play.accept(online, invite.session, game) {
        return false;
    }
    tune(play, emulator);
    true
}

pub fn main_button(ui: &Ui, text: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(text).fill(ui.visuals().selection.bg_fill)
}

pub fn quiet_button(ui: &Ui, text: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(text)
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(
            1.0_f32,
            ui.visuals().widgets.inactive.bg_fill,
        ))
}

fn friend_texture(
    playing: &mut PlayingUi,
    ctx: &egui::Context,
    session: &PlaySession,
) -> Option<egui::TextureId> {
    if !session.sees_friend() {
        return None;
    }
    let console = session.console(false)?;
    let pass = ctx.cumulative_pass_nr();
    if playing.painted_pass != pass || playing.friend_screen.is_none() {
        playing.painted_pass = pass;
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [SCREEN_WIDTH, SCREEN_HEIGHT],
            console.frame().as_slice(),
        );
        match &mut playing.friend_screen {
            Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
            None => {
                playing.friend_screen =
                    Some(ctx.load_texture("friend_screen", image, egui::TextureOptions::NEAREST));
            }
        }
    }
    playing.friend_screen.as_ref().map(|t| t.id())
}

pub fn paint_friend_screen(
    playing: &mut PlayingUi,
    ui: &mut Ui,
    session: &PlaySession,
    area: egui::Rect,
) -> Option<egui::Rect> {
    let texture = friend_texture(playing, ui.ctx(), session)?;
    let scale = (area.width() / SCREEN_WIDTH as f32)
        .min(area.height() / SCREEN_HEIGHT as f32)
        .min(MAX_SCREEN_SCALE);
    let size = egui::vec2(SCREEN_WIDTH as f32, SCREEN_HEIGHT as f32) * scale;
    let rect = egui::Rect::from_center_size(area.center(), size);
    egui::Image::new(egui::load::SizedTexture::new(texture, size)).paint_at(ui, rect);
    Some(rect)
}

pub fn watching(session: &PlaySession) -> bool {
    session.sees_friend()
}

pub fn card_name(card: &GameCard) -> &str {
    crate::game_name::name_or(&card.hash, &card.title)
}

pub fn game_title(session: &PlaySession) -> String {
    session
        .friend_game()
        .or_else(|| session.my_game())
        .map(|g| card_name(g).to_string())
        .unwrap_or_default()
}

struct Tail {
    full: egui::WidgetText,
    short: egui::WidgetText,
    hover: Option<String>,
}

fn connection_tail(ui: &Ui, session: &PlaySession) -> Tail {
    let Some(ms) = session.round_trip_ms() else {
        let text = RichText::new("Connecting").weak();
        return Tail {
            full: text.clone().into(),
            short: text.into(),
            hover: None,
        };
    };
    let (word, color) = quality(ms, ui.visuals().dark_mode);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    let format = |color: Color32| egui::TextFormat {
        font_id: font.clone(),
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(word, 0.0, format(color));
    job.append(&format!(" \u{b7} {ms} ms"), 0.0, format(weak));
    Tail {
        full: job.into(),
        short: RichText::new(word).color(color).into(),
        hover: Some(format!("{ms} ms")),
    }
}

fn line_galley(
    ui: &Ui,
    text: egui::WidgetText,
    wrap: egui::text::TextWrapping,
) -> std::sync::Arc<egui::Galley> {
    let job = text.into_layout_job(
        ui.style(),
        egui::FontSelection::Default,
        egui::Align::Center,
    );
    let mut job = std::sync::Arc::unwrap_or_clone(job);
    job.wrap = wrap;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn text_width(ui: &Ui, text: &egui::WidgetText) -> f32 {
    line_galley(ui, text.clone(), egui::text::TextWrapping::no_max_width())
        .size()
        .x
}

const MIN_SHOWN: f32 = 48.0;
const MIN_NAME: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineWidths {
    pub name: f32,
    pub title: f32,
    pub tail: f32,
    pub full_tail: bool,
}

pub fn line_widths(
    room: f32,
    gap: f32,
    name: f32,
    title: f32,
    full_tail: f32,
    short_tail: f32,
) -> LineWidths {
    let texts = if title > 0.0 { 2.0 } else { 1.0 };
    let least = name.min(MIN_SHOWN) + title.min(MIN_SHOWN) + texts * gap;
    let full_tail_fits = room - full_tail >= least;
    let tail = if full_tail_fits {
        full_tail
    } else {
        short_tail.min(room.max(0.0))
    };
    let mut widths = LineWidths {
        name: 0.0,
        title: 0.0,
        tail,
        full_tail: full_tail_fits,
    };
    let rest = room - tail - gap;
    if rest < MIN_NAME.min(name) {
        return widths;
    }
    if title > 0.0 && rest - gap - name.min(rest * 0.5) >= MIN_NAME.min(title) {
        let both = rest - gap;
        if name + title <= both {
            widths.name = name;
            widths.title = title;
        } else {
            widths.name = (both - title).max(both * 0.5).min(name);
            widths.title = (both - widths.name).min(title);
        }
        return widths;
    }
    widths.name = name.min(rest);
    widths
}

fn fixed_label(ui: &mut Ui, text: egui::WidgetText, width: f32) -> Option<egui::Response> {
    let wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    let galley = line_galley(ui, text, wrap);
    let only_dots = galley.elided
        && galley
            .rows
            .iter()
            .map(|row| row.glyphs.len())
            .sum::<usize>()
            < 2;
    (!only_dots).then(|| ui.add(Label::new(galley)))
}

fn bar_line(ui: &mut Ui, name: &str, title: Option<&str>, tail: Tail) {
    let gap = ui.spacing().item_spacing.x;
    let name_text: egui::WidgetText = name.into();
    let title_text: Option<egui::WidgetText> = title.map(|t| RichText::new(t).weak().into());
    let widths = line_widths(
        ui.available_width(),
        gap,
        text_width(ui, &name_text),
        title_text.as_ref().map_or(0.0, |t| text_width(ui, t)),
        text_width(ui, &tail.full),
        text_width(ui, &tail.short),
    );
    if widths.name > 0.0 {
        fixed_label(ui, name_text, widths.name);
    }
    if let Some(title_text) = title_text.filter(|_| widths.title > 0.0) {
        fixed_label(ui, title_text, widths.title);
    }
    let shown = if widths.full_tail {
        tail.full
    } else {
        tail.short
    };
    let response = fixed_label(ui, shown, widths.tail);
    if !widths.full_tail
        && let Some(response) = response
        && let Some(hover) = tail.hover
    {
        response.on_hover_text(hover);
    }
}

pub fn latency(ui: &mut Ui, ms: u32) {
    let (word, color) = quality(ms, ui.visuals().dark_mode);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    let format = |font: &egui::FontId, color: Color32| egui::TextFormat {
        font_id: font.clone(),
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(word, 0.0, format(&font, color));
    job.append(&format!(" \u{b7} {ms} ms"), 0.0, format(&font, weak));
    let width = ui.fonts_mut(|f| f.layout_job(job.clone())).size().x;
    if width <= ui.available_width() {
        ui.add(Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
        return;
    }
    ui.add(Label::new(RichText::new(word).color(color)).wrap_mode(egui::TextWrapMode::Extend))
        .on_hover_text(format!("{ms} ms"));
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let Some(session) = viewer.play.session() else {
        crate::app::tabs::linked::show(viewer, ui);
        return;
    };
    if session.stage() == Stage::Playing && watching(session) {
        let sound = FriendSound::of(&viewer.ui.settings.online);
        if let Some(sound) = friend_sound_bar(ui, sound) {
            sound.store(&mut viewer.ui.settings.online);
            viewer.ui.settings.dirty = true;
        }
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, egui::Sense::hover());
        paint_friend_screen(&mut viewer.ui.playing, ui, session, area);
        return;
    }
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let name = friend_name(viewer.online, session.friend());
    let line = friend_tab_line(&name, session);
    note_over(ui, area, &line, None);
}

pub fn friend_tab_line(name: &str, session: &PlaySession) -> String {
    if session.stage() != Stage::Playing {
        return format!("{name}'s screen shows here once the game starts.");
    }
    if session.console(false).is_none() {
        let cable = session.mode() == Some(Mode::Cable);
        return other_game_line(
            name,
            session.holds_theirs().unwrap_or(false),
            session.my_game().map(card_name).unwrap_or_default(),
            &game_title(session),
            session.round_trip_ms().filter(|_| cable),
        );
    }
    match hidden_line(name, session) {
        Some(line) => format!("{line}."),
        None => format!("{name}'s screen shows here once the game starts."),
    }
}

pub fn hidden_line(name: &str, session: &PlaySession) -> Option<String> {
    if !session.watches_friend() {
        Some(format!("You're not watching {name}"))
    } else if !session.friend_lets_me_watch() {
        Some(format!("{name} isn't showing their screen"))
    } else {
        None
    }
}

pub fn screen_note(name: &str, session: &PlaySession) -> Option<String> {
    if !session.watching_only() {
        return None;
    }
    if session.stage() != Stage::Playing {
        return Some(format!("{name}'s screen shows here once the game starts."));
    }
    if session.friend_plays() != Some(true) {
        return Some(format!("{name} isn't playing anything right now."));
    }
    if session.mode() == Some(Mode::Apart) {
        return Some(format!("You need {} to watch {name}.", game_title(session)));
    }
    if !session.friend_lets_me_watch() {
        return Some(format!("{name} isn't showing their screen."));
    }
    if !session.watches_friend() {
        return Some(format!("You're not watching {name}."));
    }
    None
}

pub fn watched_line(name: &str, session: &PlaySession) -> String {
    if session.mode() == Some(Mode::Apart) {
        format!("{name} needs {} to watch you.", game_title(session))
    } else if session.lets_friend_watch() {
        format!("{}  {name} is watching you", icons::EYE)
    } else {
        format!("{}  {name} can't see your screen", icons::EYE_SLASH)
    }
}

pub fn needs_copy(session: &PlaySession) -> bool {
    session.watching_only()
        && session.stage() == Stage::Playing
        && session.friend_plays() == Some(true)
        && session.mode() == Some(Mode::Apart)
}

pub fn note_over(ui: &mut Ui, area: egui::Rect, line: &str, button: Option<&str>) -> bool {
    let width = area
        .width()
        .min(area.height() * SCREEN_WIDTH as f32 / SCREEN_HEIGHT as f32);
    let size = egui::vec2(width, width * SCREEN_HEIGHT as f32 / SCREEN_WIDTH as f32);
    let rect = egui::Rect::from_center_size(area.center(), size);
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
    let inner = rect.shrink2(egui::vec2(width * 0.12, 0.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let lift = if button.is_some() { 30.0 } else { 12.0 };
    child.add_space(inner.height() / 2.0 - lift);
    child.add(Label::new(line).wrap());
    let Some(text) = button else {
        return false;
    };
    child.add_space(10.0);
    child
        .add(main_button(
            &child,
            format!("{}  {text}", icons::FOLDER_OPEN),
        ))
        .clicked()
}

pub fn stop_confirm(
    ctx: &egui::Context,
    play: &mut Play,
    online: &mut Online,
    playing: &mut PlayingUi,
) {
    if !playing.asking_stop {
        return;
    }
    let Some(session) = play.session().filter(|s| s.is_live()) else {
        playing.asking_stop = false;
        return;
    };
    let name = friend_name(online, session.friend());
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::new("stop_playing")).show(ctx, |ui| {
        ui.set_max_width(300.0);
        ui.label(format!("Stop playing with {name}?"));
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let size = egui::vec2(columns[0].available_width(), 32.0);
            if columns[0]
                .add_sized(size, egui::Button::new("Stop playing"))
                .clicked()
            {
                choice = Some(true);
            }
            let stay = main_button(&columns[1], "Stay");
            if columns[1].add_sized(size, stay).clicked() {
                choice = Some(false);
            }
        });
    });
    if modal.should_close() && choice.is_none() {
        choice = Some(false);
    }
    if let Some(stop) = choice {
        playing.asking_stop = false;
        if stop {
            play.leave(online);
        }
    }
}

pub enum Pick {
    Homebrew(&'static crate::homebrew::HomebrewGame),
    #[cfg(not(target_arch = "wasm32"))]
    Recent(std::path::PathBuf),
    File,
    Theirs,
    Nothing,
}

pub fn picker(
    ctx: &egui::Context,
    playing: &mut PlayingUi,
    online: &Online,
    recent: &RecentRoms,
    play: &Play,
) -> Option<(PickFor, Pick)> {
    let target = playing.picking.clone()?;
    let (friend, title, theirs_card) = match &target {
        PickFor::Invite(friend) => {
            let name = friend_name(online, *friend);
            (*friend, format!("Play with {name}"), None)
        }
        PickFor::Join(invite) => {
            let name = friend_name(online, invite.from);
            (invite.from, format!("Join {name}"), invite.game.clone())
        }
        PickFor::Change => {
            let Some(session) = play.session().filter(|s| s.is_live()) else {
                playing.picking = None;
                return None;
            };
            (
                session.friend(),
                "Change game".to_owned(),
                session.friend_game().cloned(),
            )
        }
    };
    let name = friend_name(online, friend);
    let theirs = theirs_card
        .as_ref()
        .map(|c| c.hash)
        .or_else(|| online.playing(friend).copied());
    let joining = matches!(target, PickFor::Join(_));
    let mut pick = None;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::new("play_picker")).show(ctx, |ui| {
        ui.set_width(PICKER_WIDTH.min(ctx.content_rect().width() - 32.0));
        ui.label(RichText::new(title).strong().size(16.0));
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut playing.pick_query)
                .hint_text(format!("{}  Search", icons::MAGNIFYING_GLASS))
                .desired_width(f32::INFINITY),
        );
        let query = playing.pick_query.trim().to_lowercase();
        let fits = |title: &str, detail: &str| {
            query.is_empty()
                || title.to_lowercase().contains(&query)
                || detail.to_lowercase().contains(&query)
        };
        ui.add_space(4.0);
        let height = ctx.content_rect().height() * 0.6;
        let mut scroll = egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, true]);
        if !playing.pick_shown {
            scroll = scroll.vertical_scroll_offset(0.0);
            playing.pick_shown = true;
        }
        scroll.show(ui, |ui| {
            let homebrew = crate::homebrew::HOMEBREW_GAMES.iter();
            let playing_it = format!("{name} is playing it");
            if let Some(game) = homebrew.clone().find(|g| Some(g.sha1) == theirs) {
                if fits(game.title, game.author) && pick_row(ui, game.title, &playing_it, true) {
                    pick = Some(if joining {
                        Pick::Theirs
                    } else {
                        Pick::Homebrew(game)
                    });
                }
            } else if joining
                && let Some(card) = &theirs_card
                && fits(card_name(card), "")
                && pick_row(ui, card_name(card), &playing_it, true)
            {
                pick = Some(Pick::Theirs);
            }
            if !matches!(target, PickFor::Invite(_)) {
                let line = format!("Watch {name}");
                if fits("No game", &line) && pick_row_as(ui, "No game", &line, false, "Watch") {
                    pick = Some(Pick::Nothing);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !recent.is_empty() {
                pick_heading(ui, "Recent");
                for entry in recent.entries() {
                    let folder = entry
                        .path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let title = entry.display_name();
                    if fits(&title, &folder) && pick_row(ui, &title, &folder, false) {
                        pick = Some(Pick::Recent(entry.path.clone()));
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = recent;
            pick_heading(ui, "Homebrew");
            for game in homebrew.filter(|g| Some(g.sha1) != theirs && fits(g.title, g.author)) {
                if pick_row(ui, game.title, game.author, false) {
                    pick = Some(Pick::Homebrew(game));
                }
            }
        });
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let size = egui::vec2(columns[0].available_width(), 32.0);
            let open = egui::Button::new(format!("{}  Open a game file", icons::FOLDER_OPEN));
            if columns[0].add_sized(size, open).clicked() {
                pick = Some(Pick::File);
            }
            if columns[1]
                .add_sized(size, egui::Button::new("Cancel"))
                .clicked()
            {
                cancel = true;
            }
        });
    });
    if cancel || pick.is_some() || modal.should_close() {
        playing.picking = None;
        playing.pick_query.clear();
        playing.pick_shown = false;
    }
    pick.map(|pick| (target, pick))
}

fn pick_heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).weak());
}

fn pick_row(ui: &mut Ui, title: &str, detail: &str, main: bool) -> bool {
    pick_row_as(ui, title, detail, main, "Play")
}

fn pick_row_as(ui: &mut Ui, title: &str, detail: &str, main: bool, action: &str) -> bool {
    let mut picked = false;
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(Label::new(title).truncate());
                ui.add(Label::new(RichText::new(detail).small().weak()).truncate());
            });
        },
        |ui| {
            let button = if main {
                main_button(ui, action)
            } else {
                egui::Button::new(action)
            };
            picked = ui.add(button.min_size(egui::vec2(56.0, 32.0))).clicked();
        },
    );
    ui.separator();
    picked
}

pub fn switch_confirm(
    ctx: &egui::Context,
    playing: &mut PlayingUi,
    online: &Online,
    play: &Play,
) -> Option<OnlineId> {
    let next = playing.switching?;
    let Some(session) = play.session().filter(|s| s.is_live()) else {
        playing.switching = None;
        return Some(next);
    };
    let current = friend_name(online, session.friend());
    let name = friend_name(online, next);
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::new("switch_friend")).show(ctx, |ui| {
        ui.set_max_width(320.0);
        ui.label(format!("Switch to {name}?"));
        ui.label(
            RichText::new(format!("{current} leaves your game."))
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let size = egui::vec2(columns[0].available_width(), 32.0);
            let invite = main_button(&columns[0], format!("Invite {name}"));
            if columns[0].add_sized(size, invite).clicked() {
                choice = Some(true);
            }
            if columns[1]
                .add_sized(size, egui::Button::new("Cancel"))
                .clicked()
            {
                choice = Some(false);
            }
        });
    });
    if modal.should_close() && choice.is_none() {
        choice = Some(false);
    }
    let choice = choice?;
    playing.switching = None;
    choice.then_some(next)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Join,
    Pick,
    Decline,
}

pub fn ask_line(name: &str, invite: &IncomingInvite) -> String {
    match &invite.game {
        Some(game) => format!("{name} wants to play {}", card_name(game)),
        None => format!("{name} wants to join you"),
    }
}

pub fn ask_banner(ui: &mut Ui, online: &Online, invite: &IncomingInvite) -> Option<Answer> {
    let person = online.friend(invite.from);
    let full_name = person
        .map(|p| p.display_name.clone())
        .unwrap_or_else(|| invite.from.to_string());
    let presence = person.and_then(|p| online.is_connected().then(|| seen(p.presence)));
    let line = ask_line(short_name(&full_name), invite);
    let mut answer = None;
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            avatar::avatar(ui, &full_name, invite.from.value(), STRIP_AVATAR, presence);
            ui.add(Label::new(line).truncate());
        },
        |ui| {
            if ui.add(main_button(ui, "Join")).clicked() {
                answer = Some(Answer::Join);
            }
            if ui.button("Pick another").clicked() {
                answer = Some(Answer::Pick);
            }
            if ui
                .add(egui::Button::new(icons::X).frame(false))
                .on_hover_text("Decline")
                .clicked()
            {
                answer = Some(Answer::Decline);
            }
        },
    );
    answer
}

pub fn pending_ask(play: &Play) -> Option<IncomingInvite> {
    if play.is_live() {
        return None;
    }
    play.invites().last().cloned()
}

pub fn banner(ui: &mut Ui, ended: &Ended, can_play_again: bool) -> Option<BannerAction> {
    let mut action = None;
    egui::Sides::new().shrink_left().wrap().show(
        ui,
        |ui| {
            ui.label(&ended.line);
        },
        |ui| {
            if ui
                .add(egui::Button::new(icons::X).frame(false))
                .on_hover_text("Dismiss")
                .clicked()
            {
                action = Some(BannerAction::Dismiss);
            }
            if can_play_again && ui.button("Play again").clicked() {
                action = Some(BannerAction::PlayAgain);
            }
        },
    );
    action
}

pub fn bar_switches(session: &PlaySession, name: &str) -> Vec<BarSwitch> {
    let mut switches = Vec::new();
    if session.stage() == Stage::Inviting {
        return switches;
    }
    let mine = session.my_game().is_some();
    let theirs = session.friend_plays() == Some(true);
    if mine {
        switches.push(BarSwitch {
            switch: Switch::Show,
            label: "Show my screen".into(),
            on: session.lets_friend_watch(),
            why_off: None,
        });
    }
    if theirs {
        let missing = session.stage() == Stage::Playing && session.console(false).is_none();
        let why_off = if missing && session.holds_theirs() == Some(true) {
            let title = session.my_game().map(card_name).unwrap_or_default();
            Some(format!("{name} doesn't have {title}"))
        } else if missing {
            Some(format!("You don't have {}", game_title(session)))
        } else if !session.friend_lets_me_watch() && session.stage() == Stage::Playing {
            Some(format!("{name} isn't showing their screen"))
        } else {
            None
        };
        switches.push(BarSwitch {
            switch: Switch::Watch,
            label: format!("Watch {name}"),
            on: session.watches_friend(),
            why_off,
        });
    }
    if mine && theirs {
        let why_off = (session.my_cable() && !session.friend_cable())
            .then(|| format!("{name}'s cable is out"));
        switches.push(BarSwitch {
            switch: Switch::Cable,
            label: "Link cable".into(),
            on: session.my_cable(),
            why_off,
        });
    }
    switches
}

fn switch_icon(switch: Switch) -> &'static str {
    match switch {
        Switch::Show => icons::SCREENCAST,
        Switch::Watch => icons::EYE,
        Switch::Cable => icons::PLUGS,
    }
}

fn switch_ui(ui: &mut Ui, item: &BarSwitch, missing: bool) -> Option<BarAction> {
    let mut on = item.on;
    let text = format!("{}  {}", switch_icon(item.switch), item.label);
    let response = ui.add_enabled(!missing, egui::Checkbox::new(&mut on, text));
    let response = match &item.why_off {
        Some(why) if missing => response.on_disabled_hover_text(why),
        Some(why) => response.on_hover_text(why),
        None => response,
    };
    if !response.changed() {
        return None;
    }
    Some(match item.switch {
        Switch::Show => BarAction::Show(on),
        Switch::Watch => BarAction::Watch(on),
        Switch::Cable => BarAction::Cable(on),
    })
}

fn switch_missing(session: &PlaySession, item: &BarSwitch) -> bool {
    item.switch == Switch::Watch
        && session.stage() == Stage::Playing
        && session.console(false).is_none()
}

fn change_button(ui: &mut Ui) -> bool {
    ui.button(format!("{}  Change game", icons::SIM_CARD))
        .clicked()
}

pub fn friend_sound(ui: &mut Ui, sound: FriendSound) -> Option<FriendSound> {
    let side = ui.spacing().interact_size.y;
    let (icon, hint) = if sound.silent() {
        (icons::SPEAKER_SLASH, "Sound on")
    } else {
        (icons::SPEAKER_HIGH, "Mute")
    };
    let mut changed = None;
    if ui
        .add(egui::Button::new(icon).min_size(egui::Vec2::splat(side)))
        .on_hover_text(hint)
        .clicked()
    {
        changed = Some(sound.toggled());
    }
    let mut volume = if sound.muted { 0.0 } else { sound.volume };
    let width = ui.available_width().min(ui.spacing().slider_width);
    if width < MIN_SHOWN {
        return changed;
    }
    ui.spacing_mut().slider_width = width;
    let slider = ui.add(
        egui::Slider::new(&mut volume, 0.0..=1.0)
            .show_value(false)
            .trailing_fill(true),
    );
    if slider.changed() {
        changed = Some(FriendSound {
            volume,
            muted: false,
        });
    }
    changed
}

pub fn friend_sound_bar(ui: &mut Ui, sound: FriendSound) -> Option<FriendSound> {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(ui.spacing().interact_size.y);
                friend_sound(ui, sound)
            })
            .inner
        })
        .inner
}

struct BarButtons<'a> {
    session: &'a PlaySession,
    switches: &'a [BarSwitch],
    facts: BarFacts,
    name: &'a str,
}

impl BarButtons<'_> {
    fn show(&self, ui: &mut Ui, compact: bool, tight: bool) -> Option<BarAction> {
        let session = self.session;
        let phone = self.facts.compact;
        let mut action = None;
        if session.stage() == Stage::Inviting {
            if ui
                .button(format!("{}  Take back", icons::ARROW_U_UP_LEFT))
                .clicked()
            {
                action = Some(BarAction::Leave);
            }
            return action;
        }
        let leave = if session.watching_only() {
            "Leave"
        } else {
            "Stop playing together"
        };
        if phone && session.watching_only() {
            if ui
                .button(icons::CHAT_CIRCLE)
                .on_hover_text("Message")
                .clicked()
            {
                action = Some(BarAction::Chat);
            }
            let (icon, hint) = if self.facts.muted {
                (icons::SPEAKER_SLASH, "Unmute")
            } else {
                (icons::SPEAKER_HIGH, "Mute")
            };
            if !needs_copy(session) && ui.button(icon).on_hover_text(hint).clicked() {
                action = Some(BarAction::Mute);
            }
        }
        if !tight && ui.button(icons::SIGN_OUT).on_hover_text(leave).clicked() {
            action = Some(BarAction::Leave);
        }
        if compact {
            ui.menu_button(icons::DOTS_THREE, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                for item in self.switches {
                    if let Some(picked) = switch_ui(ui, item, switch_missing(session, item)) {
                        action = Some(picked);
                    }
                    if let Some(why) = &item.why_off {
                        ui.label(RichText::new(why).small().weak());
                    }
                }
                if phone && watching(session) && !session.watching_only() {
                    ui.horizontal(|ui| {
                        if let Some(sound) = friend_sound(ui, self.facts.friend_sound) {
                            action = Some(BarAction::FriendSound(sound));
                        }
                        ui.label(format!("{}'s sound", self.name));
                    });
                }
                if change_button(ui) {
                    action = Some(BarAction::ChangeGame);
                    ui.close();
                }
                if tight && ui.button(format!("{}  {leave}", icons::SIGN_OUT)).clicked() {
                    action = Some(BarAction::Leave);
                    ui.close();
                }
            });
            return action;
        }
        for item in self.switches.iter().rev() {
            if let Some(picked) = switch_ui(ui, item, switch_missing(session, item)) {
                action = Some(picked);
            }
        }
        if change_button(ui) {
            action = Some(BarAction::ChangeGame);
        }
        action
    }

    fn width(&self, ui: &mut Ui, compact: bool) -> f32 {
        let mut measure = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("bar_measure")
                .invisible()
                .max_rect(egui::Rect::from_min_size(
                    ui.max_rect().min,
                    egui::vec2(f32::INFINITY, ui.spacing().interact_size.y),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        self.show(&mut measure, compact, false);
        measure.min_rect().width()
    }
}

pub fn bar(
    ui: &mut Ui,
    online: &Online,
    session: &PlaySession,
    facts: BarFacts,
) -> Option<BarAction> {
    let friend = session.friend();
    let person = online.friend(friend);
    let full_name = person
        .map(|p| p.display_name.clone())
        .unwrap_or_else(|| friend.to_string());
    let name = short_name(&full_name).to_owned();
    let presence = person.and_then(|p| online.is_connected().then(|| seen(p.presence)));
    let inviting = session.stage() == Stage::Inviting;
    let switches = bar_switches(session, &name);
    let buttons = BarButtons {
        session,
        switches: &switches,
        facts,
        name: &name,
    };
    let gap = ui.spacing().item_spacing.x;
    let room = ui.available_width();
    let left_least = STRIP_AVATAR + 3.0 * gap + 3.0 * MIN_SHOWN;
    let compact = facts.compact
        || room < BAR_FOLD_WIDTH
        || buttons.width(ui, false) + gap + left_least > room;
    let tight = compact
        && !facts.compact
        && !inviting
        && buttons.width(ui, true) + 2.0 * gap + STRIP_AVATAR + MIN_SHOWN > room;
    let watched = !session.watching_only() && session.friend_plays() == Some(false);
    let mut action = None;
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            if ui.available_width() >= STRIP_AVATAR + gap + MIN_NAME {
                avatar::avatar(ui, &full_name, friend.value(), STRIP_AVATAR, presence)
                    .on_hover_text(&full_name);
            }
            if watched && session.stage() == Stage::Playing {
                ui.add(Label::new(watched_line(&name, session)).truncate());
                return;
            }
            let tail = if inviting {
                let text = RichText::new("Invite sent").weak();
                Tail {
                    full: text.clone().into(),
                    short: text.into(),
                    hover: None,
                }
            } else {
                connection_tail(ui, session)
            };
            let title = (!compact && !inviting).then(|| game_title(session));
            bar_line(ui, &name, title.as_deref().filter(|t| !t.is_empty()), tail);
        },
        |ui| action = buttons.show(ui, compact, tight),
    );
    action
}

pub fn settle_bar(
    action: BarAction,
    play: &mut Play,
    online: &mut Online,
    playing: &mut PlayingUi,
) {
    if action == BarAction::Leave {
        let quick = play
            .session()
            .is_some_and(|s| s.watching_only() || s.stage() == Stage::Inviting);
        if quick {
            play.leave(online);
        } else {
            playing.asking_stop = true;
        }
        return;
    }
    let Some(session) = play.session_mut() else {
        return;
    };
    match action {
        BarAction::ChangeGame => playing.picking = Some(PickFor::Change),
        BarAction::Show(on) => session.set_let_watch(on),
        BarAction::Watch(on) => session.set_watch_friend(on),
        BarAction::Cable(on) => session.set_cable(on),
        BarAction::Leave | BarAction::Mute | BarAction::Chat | BarAction::FriendSound(_) => {}
    }
}

pub fn arriving(ui: &mut Ui, area: egui::Rect, title: &str, name: &str, got: u32, total: u32) {
    let width = area
        .width()
        .min(area.height() * SCREEN_WIDTH as f32 / SCREEN_HEIGHT as f32);
    let size = egui::vec2(width, width * SCREEN_HEIGHT as f32 / SCREEN_WIDTH as f32);
    let rect = egui::Rect::from_center_size(area.center(), size);
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
    let inner = rect.shrink2(egui::vec2(width * 0.15, 0.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.add_space(inner.height() / 2.0 - 36.0);
    child.add(
        Label::new(format!(
            "{}  Getting {title} from {name}",
            icons::DOWNLOAD_SIMPLE
        ))
        .wrap(),
    );
    child.add_space(6.0);
    let progress = if total == 0 {
        0.0
    } else {
        got as f32 / total as f32
    };
    child.add(egui::ProgressBar::new(progress).desired_height(6.0));
    child.add_space(6.0);
    child.label(RichText::new(size_line(got, total)).weak());
}

pub fn receiving(session: &PlaySession) -> Option<(u32, u32)> {
    match session.stage() {
        Stage::Receiving { got, total } => Some((got, total)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_friends_game_comes_first_then_borrowing_then_the_cable_then_watching() {
        assert_eq!(fit(true, true, true, true), Fit::Open);
        assert_eq!(fit(false, true, true, true), Fit::Shelf);
        assert_eq!(fit(false, false, true, true), Fit::Borrow);
        assert_eq!(fit(false, false, false, true), Fit::Cable);
        assert_eq!(fit(false, false, false, false), Fit::Watch);
    }

    #[test]
    fn connection_quality_has_three_words() {
        assert_eq!(quality(42, true).0, "Good");
        assert_eq!(quality(80, true).0, "OK");
        assert_eq!(quality(149, true).0, "OK");
        assert_eq!(quality(150, true).0, "Slow");
    }

    #[test]
    fn the_short_name_is_the_first_word() {
        assert_eq!(short_name("Mia Sommer"), "Mia");
        assert_eq!(short_name("Jonas"), "Jonas");
        assert_eq!(short_name(""), "");
    }

    #[test]
    fn an_ending_says_only_what_happened() {
        assert_eq!(
            ending_line("Mia", Ending::FriendLeft, true).as_deref(),
            Some("Mia left.")
        );
        assert_eq!(
            ending_line("Mia", Ending::Dropped, true).as_deref(),
            Some("Lost the connection to Mia.")
        );
        assert_eq!(
            ending_line("Mia", Ending::Failed, true).as_deref(),
            Some("Playing with Mia stopped.")
        );
        assert_eq!(
            ending_line("Mia", Ending::FriendLeft, false).as_deref(),
            Some("Mia left.")
        );
        assert_eq!(ending_line("Mia", Ending::Left, true), None);
        assert_eq!(
            ending_line("Mia", Ending::Declined, false).as_deref(),
            Some("Mia declined your invite.")
        );
    }

    #[test]
    fn the_other_game_line_gives_the_reason_and_a_number() {
        assert_eq!(
            other_game_line("Mia", false, "Petris", "Petris DX", Some(238)),
            "You don't have Petris DX, so you can't watch Mia's screen. \
             Each link cable exchange takes 238 ms."
        );
        assert_eq!(
            other_game_line("Mia", true, "Petris", "Petris DX", None),
            "Mia doesn't have Petris, so you can't watch each other's screens."
        );
    }

    fn line_right_edge(width: f32, name: &str, title: &str) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut right = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width + 100.0, 200.0),
            )),
            ..Default::default()
        };
        let mut start = 0.0;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_max_width(width);
                start = ui.min_rect().left();
                ui.horizontal(|ui| {
                    ui.set_max_width(width);
                    let tail = Tail {
                        full: "Good \u{b7} 42 ms".into(),
                        short: "Good".into(),
                        hover: Some("42 ms".into()),
                    };
                    bar_line(ui, name, Some(title), tail);
                    right = ui.min_rect().right();
                });
            });
        });
        (start, right)
    }

    #[test]
    fn the_bar_line_never_runs_past_its_room() {
        for width in [20.0, 60.0, 120.0, 200.0, 320.0, 900.0] {
            let (start, right) =
                line_right_edge(width, "Maximiliane", "Pok\u{e9}mon - Goldene Edition");
            assert!(
                right - start <= width + 0.5,
                "{width}: line ends at {}",
                right - start
            );
        }
    }

    #[test]
    fn a_shortened_name_fills_its_room_and_shows_whole_on_hover() {
        let ctx = egui::Context::default();
        let mut shown = None;
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(40.0);
                        let response = fixed_label(ui, "Maximiliane".into(), 44.0);
                        shown = response.map(|r| r.rect.width());
                        assert!(fixed_label(ui, "Maximiliane".into(), 4.0).is_none());
                    });
                });
            });
        }
        let width = shown.unwrap();
        assert!(width > 30.0 && width <= 44.0, "the name took {width}");
    }

    #[test]
    fn the_connection_word_keeps_its_room_and_names_shorten() {
        let fits = line_widths(400.0, 8.0, 60.0, 200.0, 90.0, 40.0);
        assert_eq!(
            fits,
            LineWidths {
                name: 60.0,
                title: 200.0,
                tail: 90.0,
                full_tail: true
            }
        );
        let squeezed = line_widths(216.0, 8.0, 60.0, 200.0, 90.0, 40.0);
        assert!(squeezed.full_tail);
        assert!((squeezed.name - 55.0).abs() < 1e-3);
        assert!((squeezed.title - 55.0).abs() < 1e-3);
        let narrow = line_widths(152.0, 8.0, 60.0, 200.0, 90.0, 40.0);
        assert!(!narrow.full_tail);
        assert_eq!(narrow.tail, 40.0);
        assert!(narrow.name + narrow.title + narrow.tail + 16.0 <= 152.0 + 1e-3);
        assert!(narrow.name >= MIN_NAME && narrow.title >= MIN_NAME);
        let no_title = line_widths(90.0, 8.0, 60.0, 200.0, 90.0, 40.0);
        assert_eq!((no_title.title, no_title.tail), (0.0, 40.0));
        assert!((no_title.name - 42.0).abs() < 1e-3);
        let word_only = line_widths(60.0, 8.0, 60.0, 0.0, 90.0, 40.0);
        assert_eq!((word_only.name, word_only.tail), (0.0, 40.0));
        let cut_word = line_widths(30.0, 8.0, 60.0, 0.0, 90.0, 40.0);
        assert_eq!((cut_word.name, cut_word.tail), (0.0, 30.0));
    }

    #[test]
    fn the_friends_mute_toggles_and_a_silent_slider_comes_back_at_half() {
        let playing = FriendSound {
            volume: 0.7,
            muted: false,
        };
        assert_eq!(
            playing.toggled(),
            FriendSound {
                volume: 0.7,
                muted: true
            }
        );
        assert_eq!(playing.toggled().toggled(), playing);
        let zero = FriendSound {
            volume: 0.0,
            muted: false,
        };
        assert_eq!(
            zero.toggled(),
            FriendSound {
                volume: 0.5,
                muted: false
            }
        );
    }

    #[test]
    fn sizes_read_in_kb_or_mb() {
        assert_eq!(size_line(1_600_000, 2_500_000), "1.6 of 2.5 MB");
        assert_eq!(size_line(40_000, 65_536), "40 of 66 KB");
    }
}
