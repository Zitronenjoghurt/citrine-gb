use super::lines::{card_name, full_name, game_title, quality, short_name, watched_line};
use super::sound::{FriendSound, friend_sound};
use super::{BarAction, PickFor, PlayingUi, STRIP_AVATAR, needs_copy, watching};
use crate::app::widgets::avatar;
use crate::icons;
use crate::online::Online;
use crate::play::{Play, PlaySession, Stage};
use egui::{Color32, Label, RichText, Ui};

const BAR_FOLD_WIDTH: f32 = 640.0;

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

pub(super) struct Tail {
    pub(super) full: egui::WidgetText,
    pub(super) short: egui::WidgetText,
    pub(super) hover: Option<String>,
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

pub(super) const MIN_SHOWN: f32 = 48.0;
pub(super) const MIN_NAME: f32 = 40.0;

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

pub(super) fn fixed_label(
    ui: &mut Ui,
    text: egui::WidgetText,
    width: f32,
) -> Option<egui::Response> {
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

pub(super) fn bar_line(ui: &mut Ui, name: &str, title: Option<&str>, tail: Tail) {
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

pub fn server_latency(ui: &mut Ui, ms: u32) {
    let (word, color) = quality(ms, ui.visuals().dark_mode);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    let format = |font: &egui::FontId, color: Color32| egui::TextFormat {
        font_id: font.clone(),
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let measured = server_line(ms);
    let mut job = egui::text::LayoutJob::default();
    job.append(word, 0.0, format(&font, color));
    job.append(&format!(" \u{b7} {measured}"), 0.0, format(&font, weak));
    let width = ui.fonts_mut(|f| f.layout_job(job.clone())).size().x;
    if width <= ui.available_width() {
        ui.add(Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
        return;
    }
    ui.add(Label::new(RichText::new(word).color(color)).wrap_mode(egui::TextWrapMode::Extend))
        .on_hover_text(measured);
}

fn server_line(ms: u32) -> String {
    format!("{ms} ms to the server")
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
            Some(format!("Needs {}", game_title(session)))
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

pub(super) fn switch_ui(ui: &mut Ui, item: &BarSwitch, missing: bool) -> Option<BarAction> {
    let text = format!("{}  {}", switch_icon(item.switch), item.label);
    let response = ui
        .add_enabled_ui(!missing, |ui| {
            crate::app::widgets::menu_row::menu_row(ui, text, Some(item.on))
        })
        .inner;
    let response = match &item.why_off {
        Some(why) if missing => response.on_disabled_hover_text(why),
        Some(why) => response.on_hover_text(why),
        None => response,
    };
    if !response.clicked() {
        return None;
    }
    let on = !item.on;
    Some(match item.switch {
        Switch::Show => BarAction::Show(on),
        Switch::Watch => BarAction::Watch(on),
        Switch::Cable => BarAction::Cable(on),
    })
}

pub(super) fn switch_missing(session: &PlaySession, item: &BarSwitch) -> bool {
    item.switch == Switch::Watch
        && session.stage() == Stage::Playing
        && session.console(false).is_none()
}

pub(super) fn change_button(ui: &mut Ui) -> bool {
    ui.button(format!("{}  Change game", icons::SIM_CARD))
        .clicked()
}

pub(super) fn change_entry(ui: &mut Ui) -> bool {
    crate::app::widgets::menu_row::menu_row(ui, format!("{}  Change game", icons::SIM_CARD), None)
        .clicked()
}

struct BarButtons<'a> {
    session: &'a PlaySession,
    switches: &'a [BarSwitch],
    facts: BarFacts,
    name: &'a str,
}

impl BarButtons<'_> {
    pub(super) fn show(&self, ui: &mut Ui, compact: bool, tight: bool) -> Option<BarAction> {
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
                if change_entry(ui) {
                    action = Some(BarAction::ChangeGame);
                    ui.close();
                }
                if tight
                    && crate::app::widgets::menu_row::menu_row(
                        ui,
                        format!("{}  {leave}", icons::SIGN_OUT),
                        None,
                    )
                    .clicked()
                {
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

    pub(super) fn width(&self, ui: &mut Ui, compact: bool) -> f32 {
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
    play: &Play,
    online: &Online,
    session: &PlaySession,
    facts: BarFacts,
) -> Option<BarAction> {
    let friend = session.friend();
    let full_name = full_name(play, online, friend);
    let name = short_name(&full_name).to_owned();
    let presence = online.friend_presence(friend);
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
    if let BarAction::Show(on) = action {
        play.set_let_watch(on);
        return;
    }
    let Some(session) = play.session_mut() else {
        return;
    };
    match action {
        BarAction::ChangeGame => playing.picking = Some(PickFor::Change),
        BarAction::Show(_) => {}
        BarAction::Watch(on) => session.set_watch_friend(on),
        BarAction::Cable(on) => session.set_cable(on),
        BarAction::Leave | BarAction::Mute | BarAction::Chat | BarAction::FriendSound(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::server_line;

    #[test]
    fn the_server_time_names_the_server() {
        assert_eq!(server_line(12), "12 ms to the server");
    }
}
