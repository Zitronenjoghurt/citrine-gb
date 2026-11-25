use crate::app::ds_firmware::DsFile;
use crate::app::events::AppEventQueue;
use crate::app::system_files::SystemFile;
use crate::app::ui_state::ds_user::{
    COLORS, CUSTOM_TIME, ClockPart, DsLanguage, DsUser, NICKNAME_CHARS, SYSTEM_TIME, clock_text,
    days_in, parse_clock,
};
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::field::row_field;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::{RESET_GAP, reset_width, with_reset};
use crate::app::widgets::settings::file_row::file_row;
use crate::app::widgets::settings::form::Form;
use egui::{Color32, Response, Sense, Stroke, StrokeKind, TextEdit, Ui, Widget, vec2};
use std::fmt::Display;

pub const HEADING_HINT: &str = "Games read these when they start.";
pub const FILES_HINT: &str = "DS games run on FreeBIOS, built into Citrine. A BIOS or firmware dumped from your own console replaces it.";
#[cfg(ds_wifi)]
pub const WIFI_HINT: &str =
    "Nintendo shut down online play for DS games in 2014. Kaeru WFC brings it back for most games.";
#[cfg(ds_wifi)]
pub const DNS_HINT: &str = "Your DS games look up Nintendo's servers here.";
#[cfg(ds_wifi)]
pub const DIRECT_HINT: &str = "DS games connect straight to the online service you picked, and other players can see your internet address.";
pub const IN_GAME_HINT: &str =
    "Every step you walk in HeartGold or SoulSilver counts on your Pokéwalker.";
#[cfg(target_os = "android")]
pub const PHONE_HINT: &str = "Steps your phone counts go to your Pokéwalker.";
#[cfg(target_os = "android")]
pub const PHONE_DENIED: &str = "Allow physical activity for Citrine in the phone's settings";
pub const KEEP_HINT: &str = "Use the nickname, birthday, language and colour saved in your firmware file instead of the ones above.";
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const SWATCH_COLUMNS: usize = 8;

#[derive(Clone, Copy)]
pub struct DsState<'a> {
    pub account: Option<&'a str>,
    pub files: [Option<&'a str>; 3],
    pub mic: Option<crate::audio::mic::MicTest>,
}

#[derive(Clone, Copy, PartialEq)]
struct Month(u8, bool);

#[derive(Clone, Copy, PartialEq)]
pub enum ClockChoice {
    System,
    Custom,
}

impl ClockChoice {
    pub const ALL: [Self; 2] = [Self::System, Self::Custom];

    pub fn of(clock: Option<i64>) -> Self {
        if clock.is_some() {
            Self::Custom
        } else {
            Self::System
        }
    }

    pub fn offset(self) -> Option<i64> {
        (self == Self::Custom).then_some(0)
    }
}

impl Display for ClockChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::System => SYSTEM_TIME,
            Self::Custom => CUSTOM_TIME,
        })
    }
}

pub fn clock_field(
    ui: &mut Ui,
    offset: i64,
    now: u64,
    part: ClockPart,
    add: impl FnOnce(&mut Ui, TextEdit<'_>) -> Response,
) -> Option<i64> {
    let id = egui::Id::new(("ds_clock", part));
    let shown = clock_text(offset, now, part);
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| shown.clone());
    let wrong = parse_clock(offset, now, part, &text).is_none();
    let mut edit = TextEdit::singleline(&mut text)
        .id(id.with("field"))
        .hint_text(part.hint());
    if wrong {
        edit = edit.text_color(ui.visuals().error_fg_color);
    }
    let response = add(ui, edit);
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs(1));
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    if response.lost_focus() && text != shown {
        parse_clock(offset, now, part, &text)
    } else {
        None
    }
}

impl Display for Month {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = MONTHS[usize::from(self.0 - 1)];
        f.write_str(if self.1 { name } else { &name[..3] })
    }
}

pub fn user_row(form: &mut Form, s: &mut Settings, state: DsState, row: RowId) {
    let keep = s.ds_user.keep_firmware_settings && state.files[2].is_some();
    match row {
        RowId::Clock => clock_rows(form, s),
        _ => user_rows(form, s, state.account, !keep, row),
    }
}

pub fn system_row(
    form: &mut Form,
    s: &mut Settings,
    state: DsState,
    events: &mut Option<&mut AppEventQueue>,
    row: RowId,
) {
    match row {
        RowId::Arm9Bios | RowId::Arm7Bios | RowId::Firmware => {
            let file = match row {
                RowId::Arm9Bios => DsFile::Arm9Bios,
                RowId::Arm7Bios => DsFile::Arm7Bios,
                _ => DsFile::Firmware,
            };
            file_row(
                form,
                SystemFile::Ds(file),
                state.files[file.index()],
                events,
            );
        }
        RowId::KeepFirmware if state.files[2].is_some() => {
            s.dirty |=
                form.toggle_with_hint(label(row), KEEP_HINT, &mut s.ds_user.keep_firmware_settings);
        }
        #[cfg(ds_wifi)]
        RowId::OnlineService | RowId::DsDirect | RowId::Dns => wifi_row(form, s, row),
        _ => {}
    }
}

pub fn walker_row(form: &mut Form, s: &mut Settings, row: RowId) {
    if row == RowId::StepsInGame {
        s.dirty |= form.toggle_with_hint(label(row), IN_GAME_HINT, &mut s.walker.in_game);
    }
    #[cfg(target_os = "android")]
    if row == RowId::PhoneSteps {
        let was = s.walker.phone;
        s.dirty |= form.toggle_with_hint(label(row), PHONE_HINT, &mut s.walker.phone);
        if s.walker.phone && !was {
            s.walker_asked = true;
        }
        if s.walker.phone && crate::platform::android::steps::denied() {
            form.hint(PHONE_DENIED);
        }
    }
}

fn label(row: RowId) -> &'static str {
    row.label(Surface::Desktop)
}

#[cfg(ds_wifi)]
fn wifi_row(form: &mut Form, s: &mut Settings, row: RowId) {
    use crate::app::ui_state::ds_online::OnlineService;
    let online = &mut s.ds_online;
    let mut dirty = false;
    if row == RowId::OnlineService {
        form.row_with_hint(label(row), WIFI_HINT, |ui| {
            let mut service = online.service;
            if GenericSelect::from_enum(&mut service, "enum_select_ds_wifi")
                .default_value(OnlineService::None)
                .ui(ui)
                .changed()
            {
                online.set_service(service);
                dirty = true;
            }
        });
    }
    if row == RowId::DsDirect && online.service != OnlineService::None {
        dirty |= form.toggle_with_hint(label(row), DIRECT_HINT, &mut online.direct);
    }
    if row == RowId::Dns && online.dns_needed() {
        form.row_with_hint(label(row), DNS_HINT, |ui| {
            let preset = online.service.preset_dns().unwrap_or_default();
            let kaeru = OnlineService::Kaeru.preset_dns().unwrap_or_default();
            let mut text = online.dns.clone().unwrap_or_default();
            let wrong = online.dns().is_none();
            let reset = with_reset(ui, online.dns.is_some(), kaeru, |ui| {
                let mut field = row_field(ui, &mut text)
                    .hint_text(preset)
                    .desired_width(f32::INFINITY);
                if wrong {
                    field = field.text_color(ui.visuals().error_fg_color);
                }
                if ui.add(field).changed() {
                    online.set_dns(&text);
                    dirty = true;
                }
            });
            if reset {
                online.reset_dns();
                dirty = true;
            }
        });
    }
    s.dirty |= dirty;
}

fn user_rows(form: &mut Form, s: &mut Settings, account: Option<&str>, enabled: bool, row: RowId) {
    let user = &mut s.ds_user;
    let mut dirty = false;
    if row == RowId::Nickname {
        form.row(label(row), |ui| {
            let mut text = user.nickname.clone().unwrap_or_default();
            let hint = user.nickname(account);
            let reset = ui
                .add_enabled_ui(enabled, |ui| {
                    with_reset(ui, user.nickname.is_some(), &hint, |ui| {
                        let field = row_field(ui, &mut text)
                            .hint_text(&hint)
                            .char_limit(NICKNAME_CHARS)
                            .desired_width(f32::INFINITY);
                        if ui.add(field).changed() {
                            user.set_nickname(&text);
                            dirty = true;
                        }
                    })
                })
                .inner;
            if reset {
                user.nickname = None;
                dirty = true;
            }
        });
    }
    if row == RowId::Birthday {
        form.row(label(row), |ui| {
            let spacing = ui.spacing();
            let chrome = 2.0 * spacing.button_padding.x + spacing.icon_width + spacing.icon_spacing;
            let gap = spacing.item_spacing.x;
            let font = egui::TextStyle::Button.resolve(ui.style());
            let text_width = |ui: &Ui, text: String| {
                ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(text, font.clone(), Color32::WHITE)
                        .size()
                        .x
                })
            };
            let day_width = text_width(ui, "31".to_owned()) + chrome;
            let month_width = text_width(ui, Month(user.birthday.0, false).to_string()) + chrome;
            let whole = text_width(ui, Month(user.birthday.0, true).to_string()) + chrome;
            let full =
                ui.available_width() - RESET_GAP - reset_width(ui) - gap - day_width >= whole;
            let default = DsUser::default().birthday;
            let reset = ui
                .add_enabled_ui(enabled, |ui| {
                    let shown = format!("{} {}", Month(default.0, full), default.1);
                    with_reset(ui, user.birthday != default, shown, |ui| {
                        ui.horizontal(|ui| {
                            let (mut month, mut day) =
                                (Month(user.birthday.0, full), user.birthday.1);
                            let months = (1..=12).map(|month| Month(month, full));
                            let days = 1..=days_in(month.0);
                            let size = vec2(
                                (ui.available_width() - gap - day_width).max(0.0),
                                ui.spacing().interact_size.y,
                            );
                            let changed = ui
                                .allocate_ui(size, |ui| {
                                    GenericSelect::new(&mut month, months, "enum_select_ds_month")
                                        .ui(ui)
                                        .changed()
                                })
                                .inner
                                | GenericSelect::new(&mut day, days, "enum_select_ds_day")
                                    .ui(ui)
                                    .changed();
                            if changed {
                                user.set_birthday(month.0, day);
                                dirty = true;
                            }
                        });
                    })
                })
                .inner;
            if reset {
                user.birthday = default;
                dirty = true;
            }
            super::form::select_need(
                ui,
                month_width + gap + day_width + RESET_GAP + reset_width(ui),
            );
        });
    }
    if row == RowId::Language {
        form.row(label(row), |ui| {
            let app = DsLanguage::of_app();
            let mut language = user.language();
            let changed = ui
                .add_enabled_ui(enabled, |ui| {
                    GenericSelect::from_enum(&mut language, "enum_select_ds_language")
                        .default_value(app)
                        .ui(ui)
                        .changed()
                })
                .inner;
            if changed {
                user.language = (language != app).then_some(language);
                dirty = true;
            }
        });
    }
    if row == RowId::Colour {
        form.row(label(row), |ui| {
            let default = DsUser::default().favorite_color;
            let reset = ui
                .add_enabled_ui(enabled, |ui| {
                    let shown = COLORS[usize::from(default)].1;
                    with_reset(ui, user.favorite_color != default, shown, |ui| {
                        let side = ui.spacing().interact_size.y;
                        if let Some(color) = swatches(ui, user.favorite_color, SWATCH_COLUMNS, side)
                        {
                            user.favorite_color = color;
                            dirty = true;
                        }
                    })
                })
                .inner;
            if reset {
                user.favorite_color = default;
                dirty = true;
            }
        });
    }
    s.dirty |= dirty;
}

fn clock_rows(form: &mut Form, s: &mut Settings) {
    let now = crate::storage::now_unix();
    let user = &mut s.ds_user;
    let mut dirty = false;
    form.row(label(RowId::Clock), |ui| {
        let mut choice = ClockChoice::of(user.clock);
        if GenericSelect::new(&mut choice, ClockChoice::ALL, "enum_select_ds_clock")
            .default_value(ClockChoice::System)
            .ui(ui)
            .changed()
        {
            user.clock = choice.offset();
            dirty = true;
        }
    });
    if let Some(offset) = user.clock {
        for part in [ClockPart::Date, ClockPart::Time] {
            form.row(part.label(), |ui| {
                let width = (ui.available_width() - reset_width(ui) - RESET_GAP).max(0.0);
                let size = vec2(width, ui.spacing().interact_size.y);
                let layout = egui::Layout::top_down_justified(egui::Align::Min);
                let set = ui
                    .allocate_ui_with_layout(size, layout, |ui| {
                        clock_field(ui, offset, now, part, |ui, edit| {
                            ui.add(
                                edit.min_size(vec2(0.0, ui.spacing().interact_size.y))
                                    .vertical_align(egui::Align::Center)
                                    .desired_width(f32::INFINITY),
                            )
                        })
                    })
                    .inner;
                if let Some(set) = set {
                    user.clock = Some(set);
                    dirty = true;
                }
            });
        }
    }
    s.dirty |= dirty;
}

pub fn swatches(ui: &mut Ui, selected: u8, per_row: usize, max_side: f32) -> Option<u8> {
    ui.vertical(|ui| swatch_rows(ui, selected, per_row, max_side))
        .inner
}

fn swatch_rows(ui: &mut Ui, selected: u8, per_row: usize, max_side: f32) -> Option<u8> {
    let columns = per_row as f32;
    let fit = |gap: f32| (ui.available_width() - gap * (columns - 1.0)) / columns;
    let roomy = fit(ui.spacing().item_spacing.x / 2.0) >= max_side;
    let (gap, inset) = if roomy {
        (ui.spacing().item_spacing.x / 2.0, 2.0)
    } else {
        (ui.spacing().item_spacing.x / 4.0, 1.0)
    };
    let side = fit(gap).min(max_side);
    let ring = ui.visuals().strong_text_color();
    let mut picked = None;
    ui.spacing_mut().item_spacing = vec2(gap, gap);
    for row in COLORS.chunks(per_row).enumerate() {
        let (row, colors) = row;
        ui.horizontal(|ui| {
            for (column, &(rgb, name)) in colors.iter().enumerate() {
                let index = (row * per_row + column) as u8;
                let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
                let [_, r, g, b] = rgb.to_be_bytes();
                let radius = ui.visuals().widgets.inactive.corner_radius;
                let painter = ui.painter();
                painter.rect_filled(rect.shrink(inset), radius, Color32::from_rgb(r, g, b));
                if index == selected {
                    painter.rect_stroke(
                        rect,
                        radius,
                        Stroke::new(2.0_f32, ring),
                        StrokeKind::Inside,
                    );
                } else if response.hovered() {
                    painter.rect_stroke(
                        rect,
                        radius,
                        Stroke::new(1.0_f32, ring.gamma_multiply(0.5)),
                        StrokeKind::Inside,
                    );
                }
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::RadioButton,
                        ui.is_enabled(),
                        index == selected,
                        name,
                    )
                });
                if response.on_hover_text(name).clicked() {
                    picked = Some(index);
                }
            }
        });
    }
    picked.filter(|&index| index != selected)
}
