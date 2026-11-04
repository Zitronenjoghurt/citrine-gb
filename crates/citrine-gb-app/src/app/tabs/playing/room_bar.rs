use super::bar::{BarSwitch, Switch, bar_switches, change_entry, switch_ui};
use super::lines::quality;
use super::room::{
    LinkEntry, Person, RoomFacts, Status, asks_line, member_menu, status, status_text, wait_line,
};
use super::sound::{FriendSound, friend_sound};
use super::{BarAction, STRIP_AVATAR, watching};
use crate::app::mobile::kit;
use crate::app::ui_state::ui_theme;
use crate::app::widgets::action::{action_height, main_button};
use crate::app::widgets::avatar;
use crate::icons;
use crate::play::{PlaySession, Stage};
use citrine_gb_net::OnlineId;
use egui::{Align2, Color32, FontId, Label, Rect, RichText, Sense, Stroke, Ui, vec2};

pub const PHONE_AVATAR: f32 = 30.0;
pub(super) const PHONE_LINE: f32 = 16.0;
const PILL_PAD: f32 = 3.0;
const PLUG_GAP: f32 = 3.0;
const PLUG_END: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MemberAction {
    Watch(bool),
    Link {
        target: OnlineId,
        moves_me: bool,
        theirs: bool,
    },
    Unlink,
    OpenCopy,
    Message,
    Profile,
    AddFriend,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RoomAction {
    Open(OnlineId),
    Member(OnlineId, MemberAction),
    Bar(BarAction),
    Add,
    Chat,
    TakeLink(OnlineId),
    DropAsks,
    Unlink(OnlineId),
}

pub struct MyMenu {
    pub switches: Vec<BarSwitch>,
    pub sound: Option<(FriendSound, String)>,
    pub can_change: bool,
}

impl MyMenu {
    pub fn of(session: &PlaySession, name: &str, sound: FriendSound) -> Self {
        Self {
            switches: bar_switches(session, name)
                .into_iter()
                .filter(|s| s.switch != Switch::Watch)
                .collect(),
            sound: (watching(session) && !session.watching_only())
                .then(|| (sound, format!("{name}'s sound"))),
            can_change: session.stage() != Stage::Inviting && !session.watching_only(),
        }
    }
}

fn plug_width(size: f32) -> f32 {
    size * 0.6
}

fn overlap(size: f32) -> f32 {
    (size * 0.1).round()
}

pub fn chunk_width(count: usize, grouped: bool, size: f32) -> f32 {
    let faces = size + count.saturating_sub(1) as f32 * (size - overlap(size));
    if grouped {
        PILL_PAD + faces + PLUG_GAP + plug_width(size) + PLUG_END
    } else {
        faces
    }
}

pub fn chunks(facts: &RoomFacts) -> Vec<Vec<&Person>> {
    let mut chunks: Vec<Vec<&Person>> = (0..facts.groups.len())
        .map(|g| {
            let mut people: Vec<&Person> = facts.group_people(g).collect();
            people.sort_by_key(|p| p.me);
            people
        })
        .collect();
    chunks.extend(
        facts
            .people
            .iter()
            .filter(|p| p.group.is_none() && !p.me)
            .map(|p| vec![p]),
    );
    chunks.retain(|c| !c.is_empty());
    chunks.sort_by_key(|c| !c.iter().any(|p| p.me));
    chunks
}

pub fn fitting(widths: &[f32], room: f32, gap: f32, more: f32) -> usize {
    let total: f32 = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    if total <= room {
        return widths.len();
    }
    let mut used = more + gap;
    widths
        .iter()
        .take_while(|w| {
            used += *w + gap;
            used <= room
        })
        .count()
}

pub fn person_line(ui: &Ui, person: &Person) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let weak = ui.visuals().weak_text_color();
    let format = |color: Color32| egui::TextFormat {
        font_id: font.clone(),
        color,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    if person.reconnecting {
        job.append(
            &format!("{} is reconnecting", person.short()),
            0.0,
            format(weak),
        );
        return job;
    }
    if let Some(game) = &person.game {
        job.append(game, 0.0, format(weak));
    }
    if let Some(ms) = person.ms {
        let (word, color) = quality(u32::from(ms), ui.visuals().dark_mode);
        if !job.text.is_empty() {
            job.append(" \u{b7} ", 0.0, format(weak));
        }
        job.append(word, 0.0, format(color));
        job.append(&format!(" {ms} ms"), 0.0, format(weak));
    }
    job
}

fn hover_line(person: &Person) -> String {
    if person.me {
        return "You".into();
    }
    if person.reconnecting {
        return format!("{} is reconnecting", person.short());
    }
    match person.ms {
        Some(ms) => format!("{} \u{b7} {ms} ms", person.name),
        None => person.name.clone(),
    }
}

fn paint_face(ui: &Ui, rect: Rect, person: &Person, ring: Option<Color32>) {
    avatar::paint(ui, rect, &person.name, person.who.value(), None);
    let painter = ui.painter();
    if let Some(ring) = ring {
        painter.circle_stroke(
            rect.center(),
            rect.width() / 2.0 + 0.75,
            Stroke::new(1.5_f32, ring),
        );
    }
    if person.reconnecting {
        painter.circle_filled(
            rect.center(),
            rect.width() / 2.0,
            ui.visuals().panel_fill.gamma_multiply(0.7),
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            icons::ARROWS_CLOCKWISE,
            FontId::proportional(rect.width() * 0.55),
            ui.visuals().strong_text_color(),
        );
    }
}

pub fn pill_fill(ui: &Ui) -> Color32 {
    let selection = ui_theme::brand(ui.visuals());
    let panel = ui.visuals().panel_fill;
    panel.lerp_to_gamma(selection, if ui.visuals().dark_mode { 0.3 } else { 0.35 })
}

fn chunk(
    ui: &mut Ui,
    facts: &RoomFacts,
    people: &[&Person],
    size: f32,
    phone: bool,
) -> Option<RoomAction> {
    let grouped = people[0].group.is_some();
    let width = chunk_width(people.len(), grouped, size);
    let height = if grouped { size + 2.0 * PILL_PAD } else { size };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let fill = pill_fill(ui);
    let mine = grouped && people[0].group == facts.mine;
    if grouped && ui.is_rect_visible(rect) {
        let stroke = if mine {
            Stroke::new(1.0_f32, ui_theme::brand(ui.visuals()))
        } else {
            Stroke::NONE
        };
        ui.painter().rect(
            rect,
            egui::CornerRadius::same((height / 2.0) as u8),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            egui::pos2(
                rect.right() - PLUG_END - plug_width(size) / 2.0,
                rect.center().y,
            ),
            Align2::CENTER_CENTER,
            icons::PLUGS_CONNECTED,
            FontId::proportional(plug_width(size)),
            ui.visuals().text_color(),
        );
    }
    let left = if grouped {
        rect.left() + PILL_PAD
    } else {
        rect.left()
    };
    let mut action = None;
    let row_left = ui.max_rect().left();
    for (i, person) in people.iter().enumerate().rev() {
        let face = Rect::from_min_size(
            egui::pos2(
                left + i as f32 * (size - overlap(size)),
                rect.center().y - size / 2.0,
            ),
            vec2(size, size),
        );
        let menu = member_menu(facts, person.who);
        let opens = !person.me || menu.link.is_some();
        let sense = if opens {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui
            .interact(face, ui.id().with(("member", person.who)), sense)
            .on_hover_text(hover_line(person));
        if ui.is_rect_visible(face) {
            paint_face(ui, face, person, grouped.then_some(fill));
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &person.name)
        });
        if !opens {
            continue;
        }
        if phone {
            if response.clicked() {
                action = Some(RoomAction::Open(person.who));
            }
            continue;
        }
        let under = Rect::from_min_max(egui::pos2(row_left, face.top()), face.max);
        egui::Popup::menu(&response).anchor(under).show(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.set_min_width(status_width(ui, facts, false));
            if let Some(picked) = member_menu_ui(ui, facts, person.who, false) {
                action = Some(RoomAction::Member(person.who, picked));
                ui.close();
            }
        });
    }
    action
}

fn overflow(
    ui: &mut Ui,
    facts: &RoomFacts,
    hidden: &[&Person],
    size: f32,
    phone: bool,
) -> Option<RoomAction> {
    let mut action = None;
    let button = egui::Button::new(RichText::new(format!("+{}", hidden.len())).small())
        .min_size(vec2(size, size))
        .corner_radius(size / 2.0);
    let response = ui.add(button).on_hover_text("Everyone else in the room");
    egui::Popup::menu(&response).show(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        for person in hidden {
            if phone {
                if ui.button(&person.name).clicked() {
                    action = Some(RoomAction::Open(person.who));
                    ui.close();
                }
                continue;
            }
            ui.menu_button(&person.name, |ui| {
                if let Some(picked) = member_menu_ui(ui, facts, person.who, false) {
                    action = Some(RoomAction::Member(person.who, picked));
                    ui.close();
                }
            });
        }
    });
    action
}

fn people(
    ui: &mut Ui,
    facts: &RoomFacts,
    size: f32,
    phone: bool,
    invite: bool,
) -> Option<RoomAction> {
    let gap = ui.spacing().item_spacing.x;
    let add = match (invite, phone) {
        (false, _) => -gap,
        (true, true) => phone_button_side(ui),
        (true, false) => ui.spacing().interact_size.y.max(size),
    };
    let chunks = chunks(facts);
    let widths: Vec<f32> = chunks
        .iter()
        .map(|c| chunk_width(c.len(), c[0].group.is_some(), size))
        .collect();
    let keep = usize::from(chunks.first().is_some_and(|c| c.iter().any(|p| p.me)));
    let shown = fitting(&widths, ui.available_width() - add - gap, gap, size).max(keep);
    let mut action = None;
    for people in &chunks[..shown] {
        action = chunk(ui, facts, people, size, phone).or(action);
    }
    if shown < chunks.len() {
        let hidden: Vec<&Person> = chunks[shown..].iter().flatten().copied().collect();
        action = overflow(ui, facts, &hidden, size, phone).or(action);
    }
    if invite {
        action = invite_button(ui, phone).or(action);
    }
    action
}

fn invite_button(ui: &mut Ui, phone: bool) -> Option<RoomAction> {
    let invite = icon_button(ui, icons::USER_PLUS, phone).on_hover_text("Invite");
    invite.clicked().then_some(RoomAction::Add)
}

fn phone_button_side(ui: &Ui) -> f32 {
    ui.spacing().interact_size.y.max(40.0)
}

fn icon_button(ui: &mut Ui, icon: &str, phone: bool) -> egui::Response {
    if !phone {
        return ui.button(icon);
    }
    let side = phone_button_side(ui);
    ui.add(
        egui::Button::new(RichText::new(icon).size(21.0))
            .min_size(vec2(side, side))
            .corner_radius(side / 2.0)
            .frame(false),
    )
}

fn switch_note(ui: &mut Ui, text: &str) {
    let indent = ui.spacing().icon_width + ui.spacing().icon_spacing;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.label(RichText::new(text).weak());
    });
}

fn my_menu_ui(ui: &mut Ui, mine: &MyMenu) -> Option<BarAction> {
    let mut action = None;
    for item in &mine.switches {
        if let Some(picked) = switch_ui(ui, item, false) {
            action = Some(picked);
        }
        if let Some(why) = &item.why_off {
            switch_note(ui, why);
        }
    }
    if let Some((sound, label)) = &mine.sound {
        ui.horizontal(|ui| {
            if let Some(sound) = friend_sound(ui, *sound) {
                action = Some(BarAction::FriendSound(sound));
            }
            sound_label(ui, label);
        });
    }
    if mine.can_change && change_entry(ui) {
        action = Some(BarAction::ChangeGame);
        ui.close();
    }
    action
}

fn sound_label(ui: &mut Ui, label: &str) {
    let pad = ui.spacing().button_padding.x;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let content = galley.size().x + pad;
    let width = if ui.is_sizing_pass() {
        content
    } else {
        ui.available_width().max(content)
    };
    let height = ui.spacing().interact_size.y.max(galley.size().y);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
    let pos = egui::pos2(
        rect.right() - pad - galley.size().x,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(pos, galley, ui.visuals().text_color());
}

pub fn member_row(
    ui: &mut Ui,
    facts: &RoomFacts,
    mine: &MyMenu,
    phone: bool,
) -> Option<RoomAction> {
    let size = if phone { PHONE_AVATAR } else { STRIP_AVATAR };
    let mut left = None;
    let mut right = None;
    let gap = ui.spacing().item_spacing.x;
    let room = ui.available_width();
    let faces = faces_width(ui, facts, size);
    let invite = ui.spacing().interact_size.y.max(size) + gap;
    let buttons = buttons_width(ui);
    if phone || faces + invite + buttons <= room {
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| left = people(ui, facts, size, phone, true),
            |ui| right = bar_buttons(ui, facts, mine, phone, false),
        );
        status_ui(ui, facts, phone);
        return right.or(left);
    }
    let mine_width = chunks(facts)
        .first()
        .filter(|c| c.iter().any(|p| p.me))
        .map_or(0.0, |c| chunk_width(c.len(), true, size));
    let in_menu =
        faces + invite > room && buttons + invite > room && mine_width + gap + size + invite > room;
    let invite_first = !in_menu && (faces + invite <= room || buttons + invite > room);
    let invite_below = !in_menu && !invite_first;
    ui.horizontal(|ui| left = people(ui, facts, size, phone, invite_first));
    let tools = |ui: &mut Ui| {
        let mut picked = bar_buttons(ui, facts, mine, phone, in_menu);
        if invite_below && let Some(add) = invite_button(ui, phone) {
            picked = Some(add);
        }
        picked
    };
    let tools_width = buttons + if invite_below { invite } else { 0.0 };
    if status_width(ui, facts, phone) + gap + tools_width <= room {
        egui::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| status_ui(ui, facts, phone),
            |ui| right = tools(ui),
        );
    } else {
        let height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            vec2(room, height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| right = tools(ui),
        );
        status_ui(ui, facts, phone);
    }
    right.or(left)
}

fn faces_width(ui: &Ui, facts: &RoomFacts, size: f32) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    chunks(facts)
        .iter()
        .map(|c| chunk_width(c.len(), c[0].group.is_some(), size) + gap)
        .sum::<f32>()
        - gap
}

fn buttons_width(ui: &Ui) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let spacing = ui.spacing();
    [icons::SIGN_OUT, icons::DOTS_THREE, icons::CHAT_CIRCLE]
        .iter()
        .map(|icon| {
            let text = ui
                .painter()
                .layout_no_wrap(icon.to_string(), font.clone(), Color32::PLACEHOLDER)
                .size()
                .x;
            text + spacing.button_padding.x * 2.0 + spacing.item_spacing.x
        })
        .sum()
}

fn bar_buttons(
    ui: &mut Ui,
    facts: &RoomFacts,
    mine: &MyMenu,
    phone: bool,
    invite_in_menu: bool,
) -> Option<RoomAction> {
    let mut action = None;
    if phone {
        ui.spacing_mut().item_spacing.x = 0.0;
    }
    if icon_button(ui, icons::SIGN_OUT, phone)
        .on_hover_text("Leave")
        .clicked()
    {
        action = Some(RoomAction::Bar(BarAction::Leave));
    }
    if !phone {
        ui.menu_button(icons::DOTS_THREE, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            if invite_in_menu && entry(ui, icons::USER_PLUS, "Invite", None, false) {
                action = Some(RoomAction::Add);
                ui.close();
            }
            if let Some(picked) = my_menu_ui(ui, mine) {
                action = Some(RoomAction::Bar(picked));
            }
        });
    }
    let chat = icon_button(ui, icons::CHAT_CIRCLE, phone).on_hover_text("Room chat");
    if facts.unread > 0 {
        avatar::status_dot(ui, chat.rect.shrink(if phone { 6.0 } else { 0.0 }));
    }
    if chat.clicked() {
        action = Some(RoomAction::Chat);
    }
    action
}

fn status_width(ui: &Ui, facts: &RoomFacts, phone: bool) -> f32 {
    status_job(ui, facts, phone, 0).map_or(0.0, |job| ui.painter().layout_job(job).size().x)
}

fn status_ui(ui: &mut Ui, facts: &RoomFacts, phone: bool) {
    let room = ui.available_width();
    let fits = (0..3)
        .filter_map(|level| status_job(ui, facts, phone, level))
        .find(|job| ui.painter().layout_job(job.clone()).size().x <= room);
    if let Some(job) = fits.or_else(|| status_job(ui, facts, phone, 2)) {
        ui.add(Label::new(job).truncate());
        if phone {
            ui.add_space(5.0);
        }
    }
}

fn status_job(ui: &Ui, facts: &RoomFacts, phone: bool, level: u8) -> Option<egui::text::LayoutJob> {
    let status = status(facts)?;
    if phone && matches!(status, Status::Linked(..)) {
        return None;
    }
    let font = if phone {
        FontId::proportional(PHONE_LINE)
    } else {
        egui::TextStyle::Small.resolve(ui.style())
    };
    let ink = ui.visuals().text_color();
    let format = |color: Color32| egui::TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    let text = status_text(&status);
    let text = match status {
        Status::Linked(_, Some(_)) if level >= 2 => text
            .strip_prefix("Linked with ")
            .map_or(text.clone(), str::to_owned),
        _ => text,
    };
    job.append(&text, 0.0, format(ink));
    if let Status::Linked(_, Some(ms)) = status {
        let (word, color) = quality(u32::from(ms), ui.visuals().dark_mode);
        job.append(" \u{b7} ", 0.0, format(ink));
        if level == 0 {
            job.append(&format!("{word} link"), 0.0, format(color));
            job.append(&format!(" \u{b7} {ms} ms"), 0.0, format(ink));
        } else {
            job.append(word, 0.0, format(color));
        }
    }
    Some(job)
}

fn card_gap(phone: bool) -> f32 {
    if phone { 1.0 } else { 4.0 }
}

fn card_frame(ui: &mut Ui, phone: bool, add: impl FnOnce(&mut Ui)) {
    card_frame_sized(ui, phone, true, add);
}

fn card_frame_sized(ui: &mut Ui, phone: bool, full: bool, add: impl FnOnce(&mut Ui)) {
    let radius = if phone { kit::RADIUS } else { 8 };
    let fill = if phone {
        kit::card_fill(ui.visuals())
    } else {
        ui.visuals().faint_bg_color
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, ui_theme::brand(ui.visuals())))
        .corner_radius(radius)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            if full {
                ui.set_width(ui.available_width());
            }
            add(ui);
        });
}

fn card_row(ui: &Ui, phone: bool) -> egui::Sides {
    let height = if phone {
        ui.spacing().interact_size.y.max(36.0)
    } else {
        action_height(ui)
    };
    egui::Sides::new().height(height).shrink_left().truncate()
}

fn card_line(ui: &mut Ui, icon: &str, line: &str, phone: bool) {
    ui.label(icon);
    let text = if phone {
        RichText::new(line)
            .size(PHONE_LINE)
            .color(ui.visuals().strong_text_color())
    } else {
        RichText::new(line)
    };
    ui.add(Label::new(text).truncate());
}

fn wait_card(ui: &mut Ui, facts: &RoomFacts, phone: bool) -> Option<RoomAction> {
    let (who, line, unlink) = wait_line(facts)?;
    ui.add_space(card_gap(phone));
    let mut action = None;
    if !phone {
        card_frame_sized(ui, phone, false, |ui| {
            let height = action_height(ui);
            let button = egui::Button::new(&unlink).min_size(vec2(height * 1.8, height));
            let spacing = ui.spacing().clone();
            let text = egui::WidgetText::from(unlink.as_str())
                .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button)
                .size()
                .x;
            let gap = spacing.item_spacing.x * 2.0;
            let reserved = (text + 2.0 * spacing.button_padding.x).max(height * 1.8)
                + gap
                + 2.0 * spacing.item_spacing.x;
            ui.horizontal(|ui| {
                ui.set_min_height(height);
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - reserved).max(0.0));
                    card_line(ui, icons::HOURGLASS, &line, phone);
                });
                ui.add_space(gap);
                if ui.add(button).clicked() {
                    action = Some(RoomAction::Unlink(who));
                }
            });
        });
        return action;
    }
    card_frame(ui, phone, |ui| {
        card_row(ui, phone).show(
            ui,
            |ui| card_line(ui, icons::HOURGLASS, &line, phone),
            |ui| {
                if kit::pill(ui, &unlink, false).clicked() {
                    action = Some(RoomAction::Unlink(who));
                }
            },
        );
    });
    action
}

pub fn request_card(ui: &mut Ui, facts: &RoomFacts, phone: bool) -> Option<RoomAction> {
    let top = ui.cursor().top();
    let action = cards(ui, facts, phone);
    if ui.cursor().top() > top {
        ui.add_space(4.0);
    }
    action
}

fn cards(ui: &mut Ui, facts: &RoomFacts, phone: bool) -> Option<RoomAction> {
    let waited = wait_card(ui, facts, phone);
    let Some(line) = asks_line(facts) else {
        return waited;
    };
    ui.add_space(card_gap(phone));
    let askers: Vec<&Person> = facts
        .asking
        .iter()
        .filter_map(|who| facts.person(*who))
        .collect();
    let mut action = None;
    card_frame(ui, phone, |ui| {
        let mut top = None;
        let single = (askers.len() == 1).then(|| askers[0].who);
        let ask_buttons = |ui: &mut Ui, top: &mut Option<RoomAction>, link_first: bool| {
            let link = |ui: &mut Ui, top: &mut Option<RoomAction>| {
                if let Some(who) = single
                    && link_button(ui, phone)
                {
                    *top = Some(RoomAction::TakeLink(who));
                }
            };
            if link_first {
                link(ui, top);
            }
            if not_now_button(ui, phone).on_hover_text("Not now").clicked() {
                *top = Some(RoomAction::DropAsks);
            }
            if !link_first {
                link(ui, top);
            }
        };
        if phone || line_fits(ui, &line, single.is_some()) {
            card_row(ui, phone).show(
                ui,
                |ui| card_line(ui, icons::PLUGS, &line, phone),
                |ui| ask_buttons(ui, &mut top, false),
            );
        } else {
            let text = ui
                .horizontal(|ui| {
                    ui.label(icons::PLUGS);
                    ui.add(Label::new(line).truncate()).rect.left()
                })
                .inner;
            ui.horizontal(|ui| {
                let height = action_height(ui);
                let buttons = if single.is_some() {
                    height * 2.8 + ui.spacing().item_spacing.x
                } else {
                    height
                };
                let room = ui.available_width() - buttons;
                ui.add_space((text - ui.cursor().left()).min(room).max(0.0));
                ask_buttons(ui, &mut top, true);
            });
        }
        action = top;
        if askers.len() < 2 {
            return;
        }
        for person in &askers {
            let mut row = None;
            card_row(ui, phone).show(
                ui,
                |ui| {
                    avatar::avatar(ui, &person.name, person.who.value(), STRIP_AVATAR, None);
                    ui.add(Label::new(person.short()).truncate());
                },
                |ui| {
                    if link_button(ui, phone) {
                        row = Some(RoomAction::TakeLink(person.who));
                    }
                },
            );
            action = row.or(action);
        }
    });
    action.or(waited)
}

fn link_button(ui: &mut Ui, phone: bool) -> bool {
    if phone {
        return kit::pill(ui, "Link", true).clicked();
    }
    let height = action_height(ui);
    ui.add(main_button(ui, "Link").min_size(vec2(height * 1.8, height)))
        .clicked()
}

fn not_now_button(ui: &mut Ui, phone: bool) -> egui::Response {
    if phone {
        return icon_button(ui, icons::X, phone);
    }
    let side = action_height(ui);
    ui.add(egui::Button::new(icons::X).min_size(vec2(side, side)))
}

fn line_fits(ui: &Ui, line: &str, link: bool) -> bool {
    let spacing = ui.spacing();
    let width = |text: &str, style: egui::TextStyle| {
        ui.painter()
            .layout_no_wrap(
                text.to_string(),
                style.resolve(ui.style()),
                Color32::PLACEHOLDER,
            )
            .size()
            .x
    };
    let height = action_height(ui);
    let button = |text: &str, least: f32| {
        (width(text, egui::TextStyle::Button) + spacing.button_padding.x * 2.0).max(least)
    };
    let buttons = button(icons::X, height)
        + if link {
            button("Link", height * 1.8) + spacing.item_spacing.x
        } else {
            0.0
        };
    let text = width(icons::PLUGS, egui::TextStyle::Body)
        + spacing.item_spacing.x
        + width(line, egui::TextStyle::Body);
    text + spacing.item_spacing.x * 2.0 + buttons <= ui.available_width()
}

pub fn header_ui(ui: &mut Ui, person: &Person, size: f32, touch: bool) {
    ui.horizontal(|ui| {
        avatar::avatar(ui, &person.name, person.who.value(), size, None);
        ui.vertical(|ui| {
            let name = RichText::new(&person.name).strong();
            let name = if touch { name.size(18.0) } else { name };
            ui.add(Label::new(name).truncate());
            if touch {
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Small, FontId::proportional(13.0));
            }
            let line = person_line(ui, person);
            if !line.text.is_empty() {
                ui.add(Label::new(line).truncate());
            }
        });
    });
}

fn link_label(entry: &LinkEntry) -> (&'static str, String, Option<String>) {
    match entry {
        LinkEntry::Link => (icons::PLUGS, "Link".into(), None),
        LinkEntry::Play { game, with } => (icons::PLUGS, format!("Play {game} with {with}"), None),
        LinkEntry::Needs(game) => (icons::PLUGS, format!("Needs {game}"), None),
        LinkEntry::Join(with) => (
            icons::PLUGS,
            "Join their link".into(),
            Some(format!("with {with}")),
        ),
        LinkEntry::Next(with) => (
            icons::PLUGS,
            "Next to link".into(),
            Some(format!("with {with}")),
        ),
        LinkEntry::Unlink => (icons::LINK_BREAK, "Unlink".into(), None),
    }
}

fn entry(ui: &mut Ui, icon: &str, label: &str, detail: Option<&str>, touch: bool) -> bool {
    if touch {
        let mut row = kit::Row::new(label).icon(icon).no_chevron();
        if let Some(detail) = detail {
            row = row.detail(detail);
        }
        return row.show(ui).clicked();
    }
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &format!("{icon}  {label}"),
        0.0,
        egui::TextFormat::simple(font.clone(), ui.visuals().text_color()),
    );
    if let Some(detail) = detail {
        job.append(
            &format!("  {detail}"),
            0.0,
            egui::TextFormat::simple(
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            ),
        );
    }
    crate::app::widgets::menu_row::menu_row(ui, job, None).clicked()
}

pub fn member_menu_ui(
    ui: &mut Ui,
    facts: &RoomFacts,
    who: OnlineId,
    touch: bool,
) -> Option<MemberAction> {
    let person = facts.person(who)?;
    let menu = member_menu(facts, who);
    let mut action = None;
    if !touch {
        header_ui(ui, person, STRIP_AVATAR + 6.0, false);
        ui.separator();
    }
    if let Some(watch) = &menu.watch {
        let mut on = watch.on;
        let label = format!("Watch {}", person.short());
        if touch {
            let changed = kit::SwitchRow::new(&label, &mut on)
                .detail(watch.why_off.as_deref())
                .enabled(!watch.missing)
                .show(ui);
            if changed {
                action = Some(MemberAction::Watch(on));
            }
        } else {
            let item = BarSwitch {
                switch: Switch::Watch,
                label,
                on,
                why_off: watch.why_off.clone(),
            };
            if let Some(BarAction::Watch(on)) = switch_ui(ui, &item, watch.missing) {
                action = Some(MemberAction::Watch(on));
            }
            if let Some(why) = &watch.why_off {
                switch_note(ui, why);
            }
        }
        if watch.copy && entry(ui, icons::FOLDER_OPEN, "Open my copy", None, touch) {
            action = Some(MemberAction::OpenCopy);
        }
    }
    if let Some((link, target)) = &menu.link {
        let (icon, label, detail) = link_label(link);
        let needs = matches!(link, LinkEntry::Needs(_));
        let picked = ui
            .add_enabled_ui(!needs, |ui| {
                entry(ui, icon, &label, detail.as_deref(), touch)
            })
            .inner;
        if picked {
            action = Some(match link {
                LinkEntry::Unlink => MemberAction::Unlink,
                _ => MemberAction::Link {
                    target: *target,
                    moves_me: menu.moves_me,
                    theirs: matches!(link, LinkEntry::Play { .. }),
                },
            });
        }
    }
    if menu.message && entry(ui, icons::CHAT_CIRCLE, "Message", None, touch) {
        action = Some(MemberAction::Message);
    }
    if menu.profile && entry(ui, icons::USER, "Profile", None, touch) {
        action = Some(MemberAction::Profile);
    }
    if menu.add_friend && entry(ui, icons::USER_PLUS, "Add friend", None, touch) {
        action = Some(MemberAction::AddFriend);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::super::room::{
        GroupFacts, LinkEntry, Waiting, WatchFacts, member_menu, note_for, status,
    };
    use super::*;
    use citrine_gb_net::{Console, ErrorCode, FriendState};
    use egui::{Pos2, Shape, epaint::ClippedShape};

    fn id(n: u64) -> OnlineId {
        OnlineId::new(n).unwrap()
    }

    const NAMES: [&str; 8] = [
        "Clara",
        "Mia Sommer",
        "Jonas Weber",
        "Lena Hoffmann",
        "Ana Costa",
        "Tom Becker",
        "Ben Ito",
        "Ida Lund",
    ];

    fn person(n: u64, group: Option<usize>) -> Person {
        Person {
            who: id(n),
            name: NAMES[n as usize - 1].into(),
            me: n == 1,
            group,
            reconnecting: false,
            away: false,
            paused: false,
            ms: (n != 1).then_some(if n == 4 { 214 } else { 38 }),
            playing: true,
            game: Some("Snake".into()),
            console: Some(Console::Gb),
            can_play_theirs: true,
            friend: match n {
                1 => None,
                6 => None,
                _ => Some(FriendState::Friend),
            },
        }
    }

    fn room(count: u64) -> RoomFacts {
        let groups = vec![
            GroupFacts {
                authority: id(1),
                full: true,
            },
            GroupFacts {
                authority: id(3),
                full: true,
            },
        ];
        let people: Vec<Person> = (1..=count)
            .map(|n| {
                let group = match n {
                    1 | 2 => Some(0),
                    3 | 4 if count >= 4 => Some(1),
                    _ => None,
                };
                person(n, group)
            })
            .collect();
        let used = if count >= 4 { 2 } else { 1 };
        RoomFacts {
            people,
            groups: groups.into_iter().take(used).collect(),
            mine: Some(0),
            asking: Vec::new(),
            i_play: true,
            watch: Some(WatchFacts {
                who: id(2),
                on: false,
                why_off: None,
                missing: false,
                copy: false,
            }),
            rejoining: false,
            unread: 0,
            waiting: None,
            ..RoomFacts::default()
        }
    }

    fn mine() -> MyMenu {
        MyMenu {
            switches: Vec::new(),
            sound: None,
            can_change: true,
        }
    }

    const ZOOMS: [(f32, f32, bool); 4] = [
        (1.5, 1280.0 / 1.5 * 0.55, false),
        (2.2, 1280.0 / 2.2 * 0.55, false),
        (2.2, 1280.0 / 2.2 * 0.29, false),
        (2.625, 412.0 - 12.0, true),
    ];

    fn frame(zoom: f32, width: f32, mut add: impl FnMut(&mut Ui)) -> (Vec<ClippedShape>, Rect) {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(zoom);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width + 40.0, 700.0))),
            ..Default::default()
        };
        let mut panel = Rect::NOTHING;
        let mut shapes = Vec::new();
        for _ in 0..3 {
            shapes = ctx
                .run(input.clone(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        panel = Rect::from_min_size(ui.max_rect().min, vec2(width, 600.0));
                        ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| add(ui));
                    });
                })
                .shapes;
        }
        (shapes, panel)
    }

    fn texts(shapes: &[ClippedShape]) -> Vec<(String, Rect)> {
        shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect()
    }

    fn has(texts: &[(String, Rect)], wanted: &str) -> bool {
        texts.iter().any(|(t, _)| t == wanted)
    }

    fn row_texts(
        facts: &RoomFacts,
        zoom: f32,
        width: f32,
        phone: bool,
    ) -> (Vec<(String, Rect)>, Rect) {
        let menu = mine();
        let (shapes, panel) = frame(zoom, width, |ui| {
            member_row(ui, facts, &menu, phone);
            request_card(ui, facts, phone);
        });
        (texts(&shapes), panel)
    }

    fn assert_fits(facts: &RoomFacts, zoom: f32, width: f32, phone: bool) -> Vec<(String, Rect)> {
        let (texts, panel) = row_texts(facts, zoom, width, phone);
        let buttons: Vec<Rect> = texts
            .iter()
            .filter(|(t, _)| {
                [icons::SIGN_OUT, icons::DOTS_THREE, icons::CHAT_CIRCLE].contains(&t.as_str())
            })
            .map(|(_, r)| *r)
            .collect();
        assert_eq!(
            buttons.len(),
            if phone { 2 } else { 3 },
            "{zoom} {width}: chat and leave show, and the menu on desktop"
        );
        let first_button = |face: &Rect| {
            buttons
                .iter()
                .filter(|b| b.top() < face.bottom() && face.top() < b.bottom())
                .map(|r| r.left())
                .fold(f32::INFINITY, f32::min)
        };
        for (text, rect) in &texts {
            assert!(
                rect.left() >= panel.left() - 0.5,
                "{zoom} {width}: {text} starts at {} before {}",
                rect.left(),
                panel.left()
            );
            assert!(
                rect.right() <= panel.right() + 0.5,
                "{zoom} {width}: {text:?} ends at {} past {}",
                rect.right(),
                panel.right()
            );
        }
        for person in &facts.people {
            let initials = citrine_gb_net::initials(&person.name);
            if let Some((_, rect)) = texts.iter().find(|(t, _)| *t == initials) {
                assert!(
                    rect.right() < first_button(rect),
                    "{zoom} {width}: {} runs into the buttons",
                    person.name
                );
            }
        }
        texts
    }

    #[test]
    fn a_stalled_link_waits_on_one_line_then_offers_to_unlink_them() {
        let mut facts = room(3);
        facts.waiting = Some(Waiting {
            who: id(2),
            unlink: false,
        });
        assert_eq!(
            status(&facts).map(|s| status_text(&s)).as_deref(),
            Some("Waiting for Mia")
        );
        assert!(wait_line(&facts).is_none());
        facts.waiting = Some(Waiting {
            who: id(2),
            unlink: true,
        });
        let (who, line, button) = wait_line(&facts).unwrap();
        assert_eq!(
            (who, line.as_str(), button.as_str()),
            (id(2), "Waiting for Mia", "Unlink Mia")
        );
        assert!(status(&facts).is_none_or(|s| !status_text(&s).contains("Waiting")));
        facts.people[1].paused = true;
        assert_eq!(wait_line(&facts).unwrap().1, "Mia paused");
        for (zoom, width, phone) in ZOOMS {
            let texts = assert_fits(&facts, zoom, width, phone);
            let lines = texts
                .iter()
                .filter(|(t, _)| t.contains("Mia paused"))
                .count();
            assert_eq!(lines, 1, "{zoom} {width}: the line shows once");
            assert!(
                has(&texts, "Unlink Mia"),
                "{zoom} {width}: the button shows"
            );
        }
    }

    #[test]
    fn a_paused_or_away_partner_shows_on_the_status_line() {
        let mut facts = room(3);
        facts.people[1].paused = true;
        assert_eq!(
            status(&facts).map(|s| status_text(&s)).as_deref(),
            Some("Mia paused")
        );
        facts.people[1].away = true;
        assert_eq!(
            status(&facts).map(|s| status_text(&s)).as_deref(),
            Some("Mia is away")
        );
    }

    #[test]
    fn a_room_of_three_shows_the_linked_pair_then_the_third_and_fits_every_width() {
        let facts = room(3);
        for (zoom, width, phone) in ZOOMS {
            let texts = assert_fits(&facts, zoom, width, phone);
            for name in &NAMES[..3] {
                assert!(
                    has(&texts, &citrine_gb_net::initials(name)),
                    "{zoom}: {name} shows"
                );
            }
            assert!(
                has(&texts, icons::PLUGS_CONNECTED),
                "{zoom}: the cable mark shows"
            );
            assert!(has(&texts, icons::USER_PLUS), "{zoom}: invite shows");
            if !phone {
                assert!(
                    texts.iter().any(|(t, _)| t.starts_with("Linked with Mia")),
                    "{zoom}: the status line names the partner"
                );
            }
        }
    }

    #[test]
    fn a_room_of_five_shows_two_link_groups_and_everyone() {
        let facts = room(5);
        for (zoom, width, phone) in ZOOMS {
            let texts = assert_fits(&facts, zoom, width, phone);
            for name in &NAMES[..5] {
                assert!(
                    has(&texts, &citrine_gb_net::initials(name)),
                    "{zoom}: {name} shows"
                );
            }
            let marks = texts
                .iter()
                .filter(|(t, _)| t == icons::PLUGS_CONNECTED)
                .count();
            assert_eq!(marks, 2, "{zoom}: one cable mark per link group");
            assert!(
                !texts.iter().any(|(t, _)| t.starts_with('+')),
                "{zoom}: nobody is folded away"
            );
        }
    }

    #[test]
    fn the_narrowest_panel_keeps_my_link_and_every_button_inside() {
        let mut facts = room(4);
        facts.asking = vec![id(3)];
        let width = 1280.0 / 2.2 * 0.18;
        let texts = assert_fits(&facts, 2.2, width, false);
        for name in &NAMES[..2] {
            assert!(
                has(&texts, &citrine_gb_net::initials(name)),
                "{name} shows in my link"
            );
        }
        assert!(has(&texts, icons::PLUGS_CONNECTED), "my cable mark shows");
        assert!(
            texts.iter().any(|(t, _)| t.starts_with('+')),
            "the others fold behind a count"
        );
        assert!(
            texts.iter().any(|(t, _)| t.starts_with("Linked with Mia")),
            "the status line names the partner"
        );
        assert!(has(&texts, "Jonas wants to link"), "the asker shows whole");
    }

    #[test]
    fn a_full_row_folds_the_rest_behind_a_count() {
        let facts = room(8);
        let texts = assert_fits(&facts, 2.2, 240.0, false);
        let more = texts.iter().find(|(t, _)| t.starts_with('+'));
        let shown = NAMES
            .iter()
            .filter(|n| has(&texts, &citrine_gb_net::initials(n)))
            .count();
        let more = more.expect("a count for the people that don't fit");
        assert_eq!(more.0, format!("+{}", 8 - shown));
    }

    #[test]
    fn a_member_who_reconnects_shows_on_the_bar() {
        let mut facts = room(5);
        facts.people[4].reconnecting = true;
        assert_eq!(
            status(&facts),
            Some(super::super::room::Status::Away(
                "Ana is reconnecting".into()
            ))
        );
        for (zoom, width, phone) in ZOOMS {
            let texts = assert_fits(&facts, zoom, width, phone);
            assert!(has(&texts, "Ana is reconnecting"), "{zoom}: the line shows");
            assert!(
                has(&texts, icons::ARROWS_CLOCKWISE),
                "{zoom}: the avatar shows it"
            );
        }
    }

    #[test]
    fn two_requests_show_as_one_card_with_link_beside_each_name() {
        let mut facts = room(6);
        facts.asking = vec![id(5), id(6)];
        for (zoom, width, phone) in ZOOMS {
            let texts = assert_fits(&facts, zoom, width, phone);
            assert!(
                has(&texts, "Ana and Tom want to link"),
                "{zoom}: one card names both"
            );
            let links = texts.iter().filter(|(t, _)| t == "Link").count();
            assert_eq!(links, 2, "{zoom}: Link beside each name");
        }
        facts.asking = vec![id(5)];
        for (zoom, width, phone) in ZOOMS {
            let texts = assert_fits(&facts, zoom, width, phone);
            assert!(
                has(&texts, "Ana wants to link"),
                "{zoom} {width}: the asker shows whole"
            );
            assert_eq!(texts.iter().filter(|(t, _)| t == "Link").count(), 1);
            let rect = |wanted: &str| texts.iter().find(|(t, _)| t == wanted).map(|(_, r)| *r);
            let (line, link) = (rect("Ana wants to link").unwrap(), rect("Link").unwrap());
            assert!(
                link.bottom() - line.top() < 60.0,
                "{zoom} {width}: the card takes two lines at most"
            );
        }
    }

    fn menu_texts(
        facts: &RoomFacts,
        who: OnlineId,
        zoom: f32,
        width: f32,
        touch: bool,
    ) -> Vec<(String, Rect)> {
        let (shapes, _) = frame(zoom, width, |ui| {
            member_menu_ui(ui, facts, who, touch);
        });
        texts(&shapes)
    }

    #[test]
    fn the_menu_of_someone_in_a_full_link_says_next_to_link_and_leaves_watch_out() {
        let facts = room(5);
        let lena = id(4);
        let menu = member_menu(&facts, lena);
        assert_eq!(menu.link, Some((LinkEntry::Next("Jonas".into()), id(3))));
        assert!(menu.moves_me);
        assert!(menu.watch.is_none());
        for (zoom, width, touch) in ZOOMS {
            let width = if touch { width } else { 260.0 };
            let texts = menu_texts(&facts, lena, zoom, width, touch);
            assert!(
                texts.iter().any(|(t, _)| t.contains("Next to link")),
                "{zoom}: Next to link shows"
            );
            assert!(
                texts.iter().any(|(t, _)| t.contains("with Jonas")),
                "{zoom}"
            );
            assert!(
                !texts.iter().any(|(t, _)| t.contains("Watch")),
                "{zoom}: no Watch"
            );
            assert!(texts.iter().any(|(t, _)| t.contains("Message")), "{zoom}");
            assert!(texts.iter().any(|(t, _)| t.contains("Profile")), "{zoom}");
        }
    }

    #[test]
    fn a_member_of_another_link_group_of_three_can_be_watched() {
        let mut facts = room(6);
        facts.people[4].group = Some(1);
        facts.groups[1].full = false;
        let jonas = id(3);
        let watch = member_menu(&facts, jonas).watch.unwrap();
        assert_eq!(watch.who, jonas);
        assert!(!watch.on);
        facts.watching = Some(jonas);
        assert!(member_menu(&facts, jonas).watch.unwrap().on);
        facts.hidden = vec![jonas];
        let hidden = member_menu(&facts, jonas).watch.unwrap();
        assert!(!hidden.on);
        assert_eq!(
            hidden.why_off.as_deref(),
            Some("Jonas isn't showing their screen")
        );
        for (zoom, width, touch) in ZOOMS {
            let width = if touch { width } else { 260.0 };
            let texts = menu_texts(&facts, jonas, zoom, width, touch);
            assert!(
                texts.iter().any(|(t, _)| t.contains("Watch Jonas")),
                "{zoom}: Watch shows"
            );
        }
    }

    #[test]
    fn each_member_gets_the_link_entry_their_place_allows() {
        let mut facts = room(6);
        assert_eq!(
            member_menu(&facts, id(2)).link,
            Some((LinkEntry::Unlink, id(2)))
        );
        assert!(member_menu(&facts, id(2)).watch.is_some());
        assert_eq!(
            member_menu(&facts, id(5)).link,
            Some((LinkEntry::Link, id(5)))
        );
        assert!(member_menu(&facts, id(5)).moves_me);
        facts.groups[1].full = false;
        assert_eq!(
            member_menu(&facts, id(3)).link,
            Some((LinkEntry::Join("Lena".into()), id(3)))
        );
        let tom = member_menu(&facts, id(6));
        assert!(tom.add_friend && !tom.message && !tom.profile);
        facts.mine = None;
        facts.groups.remove(0);
        for person in &mut facts.people {
            person.group = match person.group {
                Some(0) => None,
                Some(_) => Some(0),
                None => None,
            };
        }
        assert!(!member_menu(&facts, id(5)).moves_me);
        facts.people[4].playing = false;
        assert_eq!(member_menu(&facts, id(5)).link, None);
    }

    #[test]
    fn a_member_on_the_other_console_offers_their_game() {
        let mut facts = room(6);
        facts.mine = None;
        for i in [2, 3, 4] {
            facts.people[i].console = Some(Console::Gba);
            facts.people[i].game = Some("Emerald".into());
        }
        facts.groups[1].full = false;
        let play = |game: &str, with: &str| LinkEntry::Play {
            game: game.into(),
            with: with.into(),
        };
        assert_eq!(
            member_menu(&facts, id(5)).link,
            Some((play("Emerald", "Ana"), id(5)))
        );
        assert_eq!(
            member_menu(&facts, id(4)).link,
            Some((play("Emerald", "Jonas"), id(3)))
        );
        facts.people[4].can_play_theirs = false;
        let ana = member_menu(&facts, id(5));
        assert_eq!(ana.link, Some((LinkEntry::Needs("Emerald".into()), id(5))));
        assert!(!ana.moves_me);
        assert_eq!(link_label(&play("Snake", "Mia")).1, "Play Snake with Mia");
    }

    #[test]
    fn each_invite_error_has_its_line() {
        assert_eq!(
            note_for(ErrorCode::RoomFull, "Mia Sommer", false).as_deref(),
            Some("Room is full")
        );
        assert_eq!(
            note_for(ErrorCode::NeedsUpdate, "Mia Sommer", false).as_deref(),
            Some("Can't invite until Mia updates Citrine")
        );
        assert_eq!(
            note_for(ErrorCode::CantJoin, "Mia Sommer", true).as_deref(),
            Some("Couldn't invite")
        );
        assert_eq!(note_for(ErrorCode::CantJoin, "Mia Sommer", false), None);
    }

    fn hover_widths(add: impl Fn(&mut Ui), labels: &[&str]) -> Vec<f32> {
        labels
            .iter()
            .map(|label| {
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(1.5);
                let screen = Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0));
                let mut pointer: Option<Pos2> = None;
                let mut width = None;
                for pass in 0..8 {
                    let mut input = egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    };
                    if let Some(at) = pointer {
                        input.events.push(egui::Event::PointerMoved(at));
                    }
                    let shapes = ctx
                        .run(input, |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                let response = ui.button("open");
                                egui::Popup::menu(&response).open(true).show(|ui| add(ui));
                            });
                        })
                        .shapes;
                    if pass < 3 {
                        continue;
                    }
                    let Some(at) = pointer else {
                        pointer = texts(&shapes)
                            .iter()
                            .find(|(t, _)| t.contains(label))
                            .map(|(_, r)| r.center());
                        continue;
                    };
                    let hovered = ctx.style().visuals.widgets.hovered.weak_bg_fill;
                    width = shapes.iter().find_map(|clipped| match &clipped.shape {
                        Shape::Rect(r) if r.fill == hovered && r.rect.contains(at) => {
                            Some(r.rect.width())
                        }
                        _ => None,
                    });
                }
                width.unwrap_or_else(|| panic!("{label}: no hover fill"))
            })
            .collect()
    }

    fn assert_one_width(widths: &[f32], labels: &[&str]) {
        for (w, label) in widths.iter().zip(labels) {
            assert!(
                (w - widths[0]).abs() < 0.5,
                "{label} is {w} wide, {} is {}",
                labels[0],
                widths[0]
            );
        }
    }

    #[test]
    fn every_entry_of_the_member_menu_has_the_menu_width() {
        let mut facts = room(3);
        if let Some(watch) = &mut facts.watch {
            watch.who = id(3);
        }
        let labels = ["Watch Jonas", "Message", "Profile"];
        let widths = hover_widths(
            |ui| {
                member_menu_ui(ui, &facts, id(3), false);
            },
            &labels,
        );
        assert_one_width(&widths, &labels);
    }

    #[test]
    fn the_watch_row_lines_up_with_the_other_entries() {
        let mut facts = room(3);
        if let Some(watch) = &mut facts.watch {
            watch.who = id(3);
        }
        let (shapes, _) = frame(1.0, 400.0, |ui| {
            member_menu_ui(ui, &facts, id(3), false);
        });
        let lefts: Vec<f32> = ["Watch Jonas", "Message", "Profile"]
            .iter()
            .map(|label| {
                shapes
                    .iter()
                    .find_map(|clipped| match &clipped.shape {
                        Shape::Text(text) if text.galley.text().contains(label) => Some(text.pos.x),
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("no {label}"))
            })
            .collect();
        for left in &lefts {
            assert!((left - lefts[0]).abs() < 0.5, "{lefts:?}");
        }
    }

    #[test]
    fn every_entry_of_my_menu_has_the_menu_width() {
        let menu = MyMenu {
            switches: vec![BarSwitch {
                switch: Switch::Show,
                label: "Show my game".into(),
                on: false,
                why_off: None,
            }],
            sound: None,
            can_change: true,
        };
        let labels = ["Show my game", "Change game"];
        let widths = hover_widths(
            |ui| {
                my_menu_ui(ui, &menu);
            },
            &labels,
        );
        assert_one_width(&widths, &labels);
    }

    #[test]
    fn plain_buttons_in_a_menu_take_the_menu_width() {
        let labels = ["Load ROM", "Load boot ROM and more"];
        let widths = hover_widths(
            |ui| {
                let _ = ui.button(labels[0]);
                let _ = ui.button(labels[1]);
            },
            &labels,
        );
        assert_one_width(&widths, &labels);
    }

    #[test]
    fn no_avatar_covers_the_initials_of_another() {
        let mut facts = room(5);
        for person in &mut facts.people {
            person.name = match person.name.as_str() {
                "Mia Sommer" => "Mia Wendt".into(),
                "Jonas Weber" => "Willa Mott".into(),
                other => other.into(),
            };
        }
        for (zoom, width, phone) in ZOOMS {
            let menu = mine();
            let (shapes, _) = frame(zoom, width, |ui| {
                member_row(ui, &facts, &menu, phone);
            });
            let rings: Vec<(usize, Pos2, f32)> = shapes
                .iter()
                .enumerate()
                .filter_map(|(i, clipped)| match &clipped.shape {
                    Shape::Circle(c) if c.stroke.width == 1.5 => {
                        Some((i, c.center, c.radius + c.stroke.width / 2.0))
                    }
                    _ => None,
                })
                .collect();
            assert!(rings.len() >= 4, "{zoom}: the linked faces have rings");
            for person in &facts.people {
                let initials = citrine_gb_net::initials(&person.name);
                let Some((at, rect)) =
                    shapes
                        .iter()
                        .enumerate()
                        .find_map(|(i, clipped)| match &clipped.shape {
                            Shape::Text(text) if text.galley.text() == initials => {
                                Some((i, text.visual_bounding_rect()))
                            }
                            _ => None,
                        })
                else {
                    continue;
                };
                for (_, center, radius) in rings.iter().filter(|(i, _, _)| *i > at) {
                    if rect.contains(*center) {
                        continue;
                    }
                    let near = rect.clamp(*center);
                    assert!(
                        near.distance(*center) > *radius,
                        "{zoom} {width}: a face covers {initials}"
                    );
                }
            }
        }
    }

    fn sound_menu() -> MyMenu {
        MyMenu {
            switches: vec![BarSwitch {
                switch: Switch::Show,
                label: "Show my screen".into(),
                on: true,
                why_off: None,
            }],
            sound: Some((
                FriendSound {
                    volume: 0.5,
                    muted: false,
                },
                "Maximilian's sound".into(),
            )),
            can_change: true,
        }
    }

    fn sound_and_switch_right(in_popup: bool, width: f32) -> (f32, f32) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0))),
            ..Default::default()
        };
        let menu = sound_menu();
        let mut shapes = Vec::new();
        for _ in 0..4 {
            shapes = ctx
                .run(input.clone(), |ctx| {
                    if in_popup {
                        egui::Area::new(egui::Id::new("menu"))
                            .fixed_pos(egui::pos2(20.0, 20.0))
                            .show(ctx, |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    my_menu_ui(ui, &menu);
                                });
                            });
                    } else {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_max_width(width);
                            my_menu_ui(ui, &menu);
                        });
                    }
                })
                .shapes;
        }
        let label = texts(&shapes)
            .into_iter()
            .find(|(t, _)| t == "Maximilian's sound")
            .map(|(_, r)| r.right())
            .expect("no sound label");
        let switch = shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Rect(rect)
                    if rect.rect.height() > 4.0
                        && (rect.rect.width() / rect.rect.height() - 1.7).abs() < 0.05
                        && f32::from(rect.corner_radius.nw) >= rect.rect.height() / 2.0 - 1.0 =>
                {
                    Some(rect.rect.right())
                }
                _ => None,
            })
            .expect("no switch");
        (label, switch)
    }

    #[test]
    fn the_sound_label_ends_on_the_switch_column() {
        for (in_popup, width) in [(true, 0.0), (false, 400.0), (false, 260.0)] {
            let (label, switch) = sound_and_switch_right(in_popup, width);
            assert!(
                (label - switch).abs() < 1.5,
                "popup {in_popup} width {width}: label ends at {label}, switch at {switch}"
            );
        }
    }

    #[test]
    fn the_phone_status_line_has_the_card_size() {
        let mut facts = room(3);
        facts.waiting = Some(Waiting {
            who: id(2),
            unlink: false,
        });
        let menu = mine();
        let (shapes, _) = frame(2.625, 400.0, |ui| {
            member_row(ui, &facts, &menu, true);
        });
        let size = shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Text(text) if text.galley.text() == "Waiting for Mia" => {
                    Some(text.galley.job.sections[0].format.font_id.size)
                }
                _ => None,
            })
            .expect("no status line");
        assert_eq!(size, PHONE_LINE);
    }

    #[test]
    fn the_desktop_unlink_card_is_as_wide_as_its_content() {
        let mut facts = room(3);
        facts.waiting = Some(Waiting {
            who: id(2),
            unlink: true,
        });
        let menu = mine();
        for width in [800.0, 420.0] {
            let (shapes, panel) = frame(1.5, width, |ui| {
                member_row(ui, &facts, &menu, false);
                request_card(ui, &facts, false);
            });
            let found = texts(&shapes);
            let find = |wanted: &str| {
                found
                    .iter()
                    .find(|(t, _)| t == wanted)
                    .map(|(_, r)| *r)
                    .unwrap_or_else(|| panic!("{width}: no {wanted}"))
            };
            let line = find("Waiting for Mia");
            let button = find("Unlink Mia");
            let card = shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    Shape::Rect(rect)
                        if [ui_theme::BRAND, ui_theme::BRAND_DARK].contains(&rect.stroke.color)
                            && rect.rect.contains_rect(line)
                            && rect.rect.contains_rect(button) =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{width}: no card around the line and button"));
            assert!(card.right() <= panel.right() + 0.5, "{width}: card spills");
            assert!(
                card.right() - button.right() < 24.0,
                "{width}: {} px after the button",
                card.right() - button.right()
            );
            assert!(
                button.left() - line.right() < 40.0,
                "{width}: {} px between line and button",
                button.left() - line.right()
            );
        }
    }

    #[test]
    fn the_link_line_says_what_its_time_measures() {
        let facts = room(3);
        let menu = mine();
        let (shapes, _) = frame(1.5, 800.0, |ui| {
            member_row(ui, &facts, &menu, false);
        });
        let line = texts(&shapes)
            .into_iter()
            .map(|(t, _)| t)
            .find(|t| t.starts_with("Linked with"))
            .expect("no link line");
        assert!(line.ends_with(" link \u{b7} 38 ms"), "{line}");
    }
}
