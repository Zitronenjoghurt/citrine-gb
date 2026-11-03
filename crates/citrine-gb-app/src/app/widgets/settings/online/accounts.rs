use super::JoinForm;
use crate::app::mobile::kit;
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::ui_theme;
use crate::app::widgets::{avatar, confirm};
use crate::icons;
use crate::online::settings::SavedAccount;
use crate::online::{Online, ServerAddress, Trust};
use egui::{Align2, CornerRadius, FontId, Rect, RichText, Sense, Ui, Vec2, pos2, vec2};
use web_time::Instant;

const ROW_AVATAR: f32 = 36.0;
const AVATAR_COLUMN: f32 = 50.0;
const END_COLUMN: f32 = 28.0;

#[derive(Default)]
pub struct RowAnswer {
    pub picked: bool,
    pub remove: bool,
}

#[derive(Clone, Copy)]
pub struct Look {
    pub in_card: bool,
}

impl Look {
    pub const CARD: Self = Self { in_card: true };
    pub const PAGE: Self = Self { in_card: false };

    fn pad(self) -> f32 {
        if self.in_card { 12.0 } else { 0.0 }
    }

    fn sizes(self, ui: &Ui) -> (f32, f32) {
        if self.in_card {
            (16.0, 13.0)
        } else {
            (
                egui::TextStyle::Body.resolve(ui.style()).size,
                egui::TextStyle::Small.resolve(ui.style()).size,
            )
        }
    }

    fn height(self, ui: &Ui) -> f32 {
        if self.in_card {
            (kit::ROW_HEIGHT + 8.0).max(ui.spacing().interact_size.y + 14.0)
        } else {
            (ROW_AVATAR + 10.0).max(ui.spacing().interact_size.y + 10.0)
        }
    }
}

pub fn all_saved(s: &Settings) -> Vec<(String, SavedAccount)> {
    s.online
        .relays
        .keys()
        .flat_map(|key| {
            s.online
                .accounts_on(key)
                .into_iter()
                .map(move |account| (key.clone(), account))
        })
        .collect()
}

pub fn spans_servers(saved: &[(String, SavedAccount)]) -> bool {
    saved.iter().any(|(key, _)| *key != saved[0].0)
}

pub fn detail(account: &SavedAccount, key: &str, many_servers: bool) -> String {
    match (&account.online_id, many_servers) {
        (Some(online_id), false) => online_id.clone(),
        _ => key.to_owned(),
    }
}

fn row_frame(ui: &mut Ui, look: Look, height: f32, sense: Sense) -> (Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    if sense.senses_click() && (response.is_pointer_button_down_on() || response.hovered()) {
        let side = if look.in_card { -4.0 } else { 0.0 };
        ui.painter().rect_filled(
            rect.expand2(vec2(side, -2.0)),
            CornerRadius::same(8),
            ui.visuals()
                .widgets
                .hovered
                .weak_bg_fill
                .gamma_multiply(0.6),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (rect, response)
}

pub fn account_row(
    ui: &mut Ui,
    account: &SavedAccount,
    detail: &str,
    current: bool,
    removable: bool,
    look: Look,
) -> RowAnswer {
    let mut answer = RowAnswer::default();
    let sense = if current {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, response) = row_frame(ui, look, look.height(ui), sense);
    let visuals = ui.visuals().clone();
    let left = rect.left() + look.pad();
    let face = Rect::from_center_size(
        pos2(left + ROW_AVATAR / 2.0, rect.center().y),
        Vec2::splat(ROW_AVATAR),
    );
    avatar::paint(ui, face, account.name(), account.seed(), None);
    let end = Rect::from_center_size(
        pos2(
            rect.right() - look.pad() - END_COLUMN / 2.0,
            rect.center().y,
        ),
        Vec2::splat(END_COLUMN),
    );
    if current {
        ui.painter().text(
            end.center(),
            Align2::CENTER_CENTER,
            icons::CHECK,
            FontId::proportional(18.0),
            ui_theme::brand_mark(&visuals),
        );
    }
    let text_left = left + AVATAR_COLUMN;
    let width = (end.left() - 8.0 - text_left).max(1.0);
    let name = match account.name() {
        "" => "Account",
        name => name,
    };
    let strong = visuals.strong_text_color();
    let weak = visuals.weak_text_color();
    let (name_size, detail_size) = look.sizes(ui);
    let label = kit::one_line(ui, name, name_size, strong, width);
    let second = kit::one_line(ui, detail, detail_size, weak, width);
    let top = rect.center().y - (label.size().y + second.size().y + 2.0) / 2.0;
    let second_top = top + label.size().y + 2.0;
    ui.painter().galley(pos2(text_left, top), label, strong);
    ui.painter()
        .galley(pos2(text_left, second_top), second, weak);
    if removable {
        let trash = egui::Button::new(RichText::new(icons::TRASH).size(18.0).color(weak))
            .frame(false)
            .min_size(Vec2::splat(END_COLUMN));
        if ui
            .put(end, trash)
            .on_hover_text("Remove from this device")
            .clicked()
        {
            answer.remove = true;
        }
    }
    if !answer.remove && response.clicked() {
        answer.picked = true;
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::RadioButton,
            ui.is_enabled(),
            current,
            name,
        )
    });
    answer
}

pub fn action_row(ui: &mut Ui, icon: &str, label: &str, look: Look) -> bool {
    let height = ui.spacing().interact_size.y + 12.0;
    let (rect, response) = row_frame(ui, look, height, Sense::click());
    let left = rect.left() + look.pad();
    let color = ui.visuals().strong_text_color();
    let (size, _) = look.sizes(ui);
    ui.painter().text(
        pos2(left + ROW_AVATAR / 2.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(size + 2.0),
        color,
    );
    let text = kit::one_line(ui, label, size, color, rect.width() - AVATAR_COLUMN);
    let top = rect.center().y - text.size().y / 2.0;
    ui.painter()
        .galley(pos2(left + AVATAR_COLUMN, top), text, color);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.clicked()
}

pub fn switch_to(s: &mut Settings, online: &mut Online, key: &str, relay_id: &str, now: Instant) {
    online.leave();
    if let Some(relay) = s.online.switch_to(key, relay_id) {
        online.sign_in(relay, now);
    }
    s.dirty = true;
}

pub fn add_account(s: &mut Settings, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let back_to = s
        .online
        .joined
        .clone()
        .zip(s.online.joined_entry().map(|entry| entry.relay_id.clone()));
    online.leave();
    s.online.leave();
    s.dirty = true;
    *form = JoinForm {
        new_account: true,
        back_to,
        ..JoinForm::default()
    };
    if let Ok(address) = ServerAddress::parse(&s.online.address) {
        let entry = s.online.entry(&address);
        let password = entry.map(|e| e.password.clone()).unwrap_or_default();
        let trust = Trust {
            relay_key: entry.and_then(|e| e.relay_key),
            ..Trust::default()
        };
        online.probe(address, password, trust, now);
    }
}

pub fn can_back_out(s: &Settings, form: &JoinForm) -> bool {
    form.back_to.is_some() && !s.online.is_joined()
}

pub fn back_out(s: &mut Settings, online: &mut Online, form: &mut JoinForm, now: Instant) {
    if !can_back_out(s, form) {
        return;
    }
    if let Some((key, relay_id)) = form.back_to.take() {
        *form = JoinForm::default();
        switch_to(s, online, &key, &relay_id, now);
    }
}

pub fn ask_remove(form: &mut JoinForm, key: &str, account: &SavedAccount) {
    form.forget = Some((key.to_owned(), account.relay_id.clone()));
}

pub fn remove_dialog(ui: &mut Ui, s: &mut Settings, online: &mut Online, form: &mut JoinForm) {
    let Some((key, relay_id)) = form.forget.clone() else {
        return;
    };
    let Some(account) = s
        .online
        .accounts_on(&key)
        .into_iter()
        .find(|a| a.relay_id == relay_id)
    else {
        form.forget = None;
        return;
    };
    let who = match account.name() {
        "" => "this account".to_owned(),
        name => name.to_owned(),
    };
    let question = format!(
        "Remove {who} from this device? You can sign in again only with its sign-in key from another device. Chats stay on the server."
    );
    if let Some(yes) = confirm::confirm(ui.ctx(), "online_forget", &question, "Remove") {
        form.forget = None;
        if yes {
            if let Some(online_id) = &account.online_id {
                online.forget_chats(&format!("{online_id}@{key}"));
            }
            s.online.forget_account(&key, &relay_id);
            form.pasted.clear();
            s.dirty = true;
        }
    }
}
