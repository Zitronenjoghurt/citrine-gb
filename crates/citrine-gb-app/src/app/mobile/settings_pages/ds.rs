use super::{Sheet, label, opens, pick_from};
use crate::app::Citrine;
use crate::app::ds_firmware::DsFile;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet;
use crate::app::system_files::SystemFile;
use crate::app::ui_state::ds_screens::ScreenContext;
use crate::app::ui_state::ds_user::{
    COLORS, ClockPart, DsLanguage, NICKNAME_CHARS, clock_label, days_in,
};
use crate::app::ui_state::layout_picks::Place;
use crate::app::ui_state::screen_layouts::LayoutConsole;
use crate::app::ui_state::settings;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::widgets::mic_level::MicLevel;
use crate::app::widgets::settings::ds::{
    ClockChoice, HEADING_HINT, IN_GAME_HINT, KEEP_HINT, MONTHS, clock_field, swatches,
};
#[cfg(ds_wifi)]
use crate::app::{
    ui_state::ds_online::OnlineService,
    widgets::settings::ds::{DNS_HINT, WIFI_HINT},
};
use crate::audio::mic::{
    MICROPHONE_HINT, MicTest, Microphone, PHONE_PUSH_TO_TALK_HINT, PUSH_TO_TALK_HINT,
    TALK_BUTTON_HINT, input_devices,
};
use egui::{Context, Ui};
use strum::IntoEnumIterator;

const SHEET_SWATCHES: usize = 8;
const SHEET_SWATCH: f32 = 48.0;

fn month_name(month: u8) -> &'static str {
    MONTHS[usize::from(month - 1)]
}

impl Citrine {
    fn ds_account(&self) -> Option<String> {
        self.online
            .account()
            .map(|account| account.display_name.clone())
    }

    pub(super) fn ds_page(&mut self, ui: &mut Ui) {
        self.tree_page(
            ui,
            SettingsTab::Ds,
            |group| (group == GroupId::DsUser).then_some(HEADING_HINT),
            Self::ds_row,
            |_| {},
        );
    }

    fn ds_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        match row.group() {
            GroupId::DsScreens => {
                let context = self.emulator.screen_context.on_phone();
                let place = match context {
                    ScreenContext::Portrait => Place::Portrait,
                    _ => Place::Landscape,
                };
                let s = &self.ui.settings;
                let placement = s.shown_layout(LayoutConsole::Ds, place).placement;
                super::ds_screens::row(list, &s.ds_screens, placement, context, row)
            }
            #[cfg(not(target_arch = "wasm32"))]
            GroupId::Ds3d => super::ds_screens::three_d_row(list, self.ui.settings.ds_3d, row),
            GroupId::DsUser => self.ds_user_row(list, row),
            GroupId::DsPerformance => {
                match row {
                    RowId::Jit => {
                        let s = &mut self.ui.settings;
                        s.dirty |= list.switch(
                            SwitchRow::new(label(row), &mut s.ds_jit).info(settings::DS_JIT_HINT),
                        );
                    }
                    RowId::CompiledCode => {
                        let kind = crate::storage::kinds::Kind::CompiledCode;
                        if let Some(clear) =
                            super::storage::storage_row(list, &self.ui.storage, kind, None, false)
                        {
                            self.ui.storage.request = Some(clear);
                        }
                    }
                    _ => {}
                }
                None
            }
            GroupId::DsSystem => self.ds_system_row(list, row),
            GroupId::DsMicrophone => mic_row(list, &mut self.ui.settings, self.mic.test(), row),
            GroupId::Walker => {
                walker_row(list, &mut self.ui.settings, row);
                None
            }
            _ => None,
        }
    }

    fn ds_user_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let account = self.ds_account();
        let firmware = self.emulator.ds_file_name(DsFile::Firmware).is_some();
        let user = &self.ui.settings.ds_user;
        let enabled = !(firmware && user.keep_firmware_settings);
        let (month, day) = user.birthday;
        let (entry, sheet) = match row {
            RowId::Nickname => (
                Row::new(label(row)).value(user.nickname(account.as_deref())),
                Sheet::DsNickname,
            ),
            RowId::Birthday => (
                Row::new(label(row)).value(format!("{} {day}", month_name(month))),
                Sheet::DsBirthday,
            ),
            RowId::Language => (
                Row::new(label(row)).value(user.language().to_string()),
                Sheet::DsLanguage,
            ),
            RowId::Colour => (
                Row::new(label(row)).value(COLORS[usize::from(user.favorite_color)].1),
                Sheet::DsColor,
            ),
            RowId::Clock => {
                if user.clock.is_some() {
                    list.ui()
                        .ctx()
                        .request_repaint_after(std::time::Duration::from_secs(1));
                }
                let now = crate::storage::now_unix();
                return opens(
                    list,
                    Row::new(label(row)).value(clock_label(user.clock, now)),
                    Sheet::DsClock,
                );
            }
            _ => return None,
        };
        opens(list, entry.enabled(enabled), sheet)
    }

    fn ds_system_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        if let Some(file) = SystemFile::of_row(row) {
            return self.file_row(list, file);
        }
        let firmware = self.emulator.ds_file_name(DsFile::Firmware).is_some();
        let s = &mut self.ui.settings;
        match row {
            RowId::KeepFirmware if firmware => {
                s.dirty |= list.switch(
                    SwitchRow::new(label(row), &mut s.ds_user.keep_firmware_settings)
                        .info(KEEP_HINT),
                );
                None
            }
            #[cfg(ds_wifi)]
            RowId::OnlineService => opens(
                list,
                Row::new(label(row)).value(s.ds_online.service.to_string()),
                Sheet::DsWifi,
            ),
            #[cfg(ds_wifi)]
            RowId::DsDirect => {
                use crate::app::widgets::settings::ds::DIRECT_HINT;
                if s.ds_online.service != OnlineService::None {
                    s.dirty |= list.switch(
                        SwitchRow::new(label(row), &mut s.ds_online.direct).info(DIRECT_HINT),
                    );
                }
                None
            }
            #[cfg(ds_wifi)]
            RowId::Dns if s.ds_online.dns_needed() => opens(
                list,
                Row::new(label(row)).value(s.ds_online.dns_text()),
                Sheet::DsDns,
            ),
            _ => None,
        }
    }

    pub(super) fn ds_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let account = self.ds_account();
        let user = &mut self.ui.settings.ds_user;
        let (month, day) = user.birthday;
        let (changed, done) = match open {
            Sheet::DsNickname => {
                let mut text = user.nickname.clone().unwrap_or_default();
                let hint = user.nickname(account.as_deref());
                let mut changed = false;
                let shown = sheet::show(ctx, super::sheet_id(), label(RowId::Nickname), |ui| {
                    kit::list(ui, |list| {
                        list.custom(|ui| {
                            kit::padded(ui, |ui| {
                                let field = egui::TextEdit::singleline(&mut text)
                                    .hint_text(hint)
                                    .char_limit(NICKNAME_CHARS);
                                changed = kit::field(ui, field).changed();
                            });
                        });
                    });
                    ui.add_space(10.0);
                    kit::primary_button(ui, "Done")
                });
                if changed {
                    user.set_nickname(&text);
                }
                (changed, shown.inner || shown.closed)
            }
            Sheet::DsBirthday => {
                let shown = sheet::show(ctx, super::sheet_id(), label(RowId::Birthday), |ui| {
                    sheet::choices(ui, 1..=12, month, |month| month_name(month).to_owned())
                });
                if let Some(picked) = shown.inner {
                    user.set_birthday(picked, day);
                    self.ui.settings.dirty = true;
                    self.mobile.sheet = Some(Sheet::DsBirthdayDay);
                    return false;
                }
                (false, shown.closed)
            }
            Sheet::DsBirthdayDay => {
                let mut picked = day;
                let changed_done = pick_from(
                    ctx,
                    label(RowId::Birthday),
                    None,
                    1..=days_in(month),
                    &mut picked,
                    |day| format!("{} {day}", month_name(month)),
                );
                if changed_done.0 {
                    user.set_birthday(month, picked);
                }
                changed_done
            }
            Sheet::DsLanguage => {
                let app = DsLanguage::of_app();
                let mut language = user.language();
                let changed_done = pick_from(
                    ctx,
                    label(RowId::Language),
                    None,
                    DsLanguage::iter(),
                    &mut language,
                    |language| language.to_string(),
                );
                if changed_done.0 {
                    user.language = (language != app).then_some(language);
                }
                changed_done
            }
            Sheet::DsClock => {
                let now = crate::storage::now_unix();
                let mut clock = user.clock;
                let shown = sheet::show(ctx, super::sheet_id(), label(RowId::Clock), |ui| {
                    let current = ClockChoice::of(clock);
                    if let Some(picked) =
                        sheet::choices(ui, ClockChoice::ALL, current, |c| c.to_string())
                        && picked != current
                    {
                        clock = picked.offset();
                    }
                    let Some(offset) = clock else {
                        return false;
                    };
                    ui.add_space(16.0);
                    for part in [ClockPart::Date, ClockPart::Time] {
                        sheet::label(ui, part.label());
                        kit::list(ui, |list| {
                            list.custom(|ui| {
                                kit::padded(ui, |ui| {
                                    if let Some(set) =
                                        clock_field(ui, offset, now, part, kit::field)
                                    {
                                        clock = Some(set);
                                    }
                                });
                            });
                        });
                        ui.add_space(10.0);
                    }
                    kit::primary_button(ui, "Done")
                });
                let changed = clock != user.clock;
                user.clock = clock;
                let closed = changed && clock.is_none();
                (changed, shown.inner || shown.closed || closed)
            }
            Sheet::DsColor => {
                let selected = user.favorite_color;
                let shown = sheet::show(ctx, super::sheet_id(), label(RowId::Colour), |ui| {
                    kit::list(ui, |list| {
                        list.custom(|ui| {
                            kit::padded(ui, |ui| {
                                swatches(ui, selected, SHEET_SWATCHES, SHEET_SWATCH)
                            })
                        })
                    })
                });
                if let Some(color) = shown.inner {
                    user.favorite_color = color;
                }
                (shown.inner.is_some(), shown.inner.is_some() || shown.closed)
            }
            #[cfg(ds_wifi)]
            Sheet::DsWifi => {
                let online = &mut self.ui.settings.ds_online;
                let mut service = online.service;
                let (changed, done) = pick_from(
                    ctx,
                    label(RowId::OnlineService),
                    Some(WIFI_HINT),
                    OnlineService::iter(),
                    &mut service,
                    |service| service.to_string(),
                );
                if changed {
                    online.set_service(service);
                }
                (changed, done)
            }
            #[cfg(ds_wifi)]
            Sheet::DsDns => {
                let online = &mut self.ui.settings.ds_online;
                let mut text = online.dns.clone().unwrap_or_default();
                let hint = online.service.preset_dns().unwrap_or_default();
                let wrong = online.dns().is_none();
                let mut changed = false;
                let shown = sheet::show(ctx, super::sheet_id(), label(RowId::Dns), |ui| {
                    sheet::note(ui, DNS_HINT);
                    kit::list(ui, |list| {
                        list.custom(|ui| {
                            kit::padded(ui, |ui| {
                                let mut field =
                                    egui::TextEdit::singleline(&mut text).hint_text(hint);
                                if wrong {
                                    field = field.text_color(ui.visuals().error_fg_color);
                                }
                                changed = kit::field(ui, field).changed();
                            });
                        });
                    });
                    ui.add_space(10.0);
                    kit::primary_button(ui, "Done")
                });
                if changed {
                    online.set_dns(&text);
                }
                (changed, shown.inner || shown.closed)
            }
            Sheet::DsCorner | Sheet::DsSmall | Sheet::DsGap | Sheet::DsRotation => {
                super::ds_screens::sheet(ctx, open, &mut self.ui.settings.ds_screens)
            }
            #[cfg(not(target_arch = "wasm32"))]
            Sheet::Ds3dResolution
            | Sheet::DsSmoothEdges
            | Sheet::DsTextureFilter
            | Sheet::DsDistantTextures => {
                super::ds_screens::three_d_sheet(ctx, open, &mut self.ui.settings.ds_3d)
            }
            Sheet::Microphone => {
                let s = &mut self.ui.settings;
                let choices = Microphone::choices(&input_devices());
                let mut index = choices
                    .iter()
                    .position(|choice| *choice == s.microphone)
                    .unwrap_or(0);
                let (changed, done) = pick_from(
                    ctx,
                    label(RowId::Microphone),
                    None,
                    0..choices.len(),
                    &mut index,
                    |index| choices[index].label().to_owned(),
                );
                if changed {
                    s.microphone = choices[index].clone();
                }
                (changed, done)
            }
            _ => return true,
        };
        self.ui.settings.dirty |= changed;
        done
    }
}

fn mic_row(
    list: &mut kit::List<'_>,
    s: &mut settings::Settings,
    test: Option<MicTest>,
    row: RowId,
) -> Option<Sheet> {
    match row {
        RowId::Microphone => {
            let open = opens(
                list,
                Row::new(label(row))
                    .info(MICROPHONE_HINT)
                    .value(s.microphone.label()),
                Sheet::Microphone,
            );
            s.mic_test_shown = true;
            if s.microphone != Microphone::Off {
                level_line(list.ui(), test.unwrap_or(MicTest::Level(0.0)));
            }
            return open;
        }
        RowId::PushToTalk if s.microphone != Microphone::Off => {
            let hint = if s.big_screen.on {
                PUSH_TO_TALK_HINT
            } else {
                PHONE_PUSH_TO_TALK_HINT
            };
            s.dirty |= list.switch(SwitchRow::new(label(row), &mut s.push_to_talk).info(hint));
        }
        RowId::TalkButton
            if s.microphone != Microphone::Off && s.push_to_talk && !s.big_screen.on =>
        {
            s.dirty |=
                list.switch(SwitchRow::new(label(row), &mut s.talk_button).info(TALK_BUTTON_HINT));
        }
        _ => {}
    }
    None
}

fn level_line(ui: &mut Ui, test: MicTest) {
    let inset = kit::SIDE;
    let rect = ui
        .available_rect_before_wrap()
        .shrink2(egui::vec2(inset, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.add(MicLevel(test));
    });
    ui.add_space(inset);
}

fn walker_row(list: &mut kit::List<'_>, s: &mut settings::Settings, row: RowId) {
    if row == RowId::StepsInGame {
        s.dirty |=
            list.switch(SwitchRow::new(label(row), &mut s.walker.in_game).info(IN_GAME_HINT));
    }
    #[cfg(target_os = "android")]
    if row == RowId::PhoneSteps {
        use crate::app::widgets::settings::ds::{PHONE_DENIED, PHONE_HINT};
        let denied = s.walker.phone && crate::platform::android::steps::denied();
        let was = s.walker.phone;
        s.dirty |= list.switch(
            SwitchRow::new(label(row), &mut s.walker.phone)
                .info(PHONE_HINT)
                .detail(denied.then_some(PHONE_DENIED)),
        );
        if s.walker.phone && !was {
            s.walker_asked = true;
        }
    }
}
