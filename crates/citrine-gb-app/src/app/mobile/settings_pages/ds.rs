use super::{Sheet, pick_from};
use crate::app::Citrine;
use crate::app::ds_firmware::DsFile;
use crate::app::gba_bios::BiosChoice;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet;
use crate::app::ui_state::ds_user::{
    COLORS, ClockPart, DsLanguage, NICKNAME_CHARS, clock_label, days_in,
};
use crate::app::widgets::settings::ds::{
    ClockChoice, FILES_HINT, HEADING_HINT, IN_GAME_HINT, KEEP_HINT, MONTHS, clock_field, swatches,
};
use crate::app::widgets::settings::page::{Heading, Page};
#[cfg(ds_wifi)]
use crate::app::{
    ui_state::ds_online::{DsOnline, OnlineService},
    widgets::settings::ds::{DNS_HINT, WIFI_HINT},
};
use crate::icons;
use egui::Context;
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

    pub(super) fn ds_rows(&mut self, page: &mut Page) -> Option<Sheet> {
        let account = self.ds_account();
        let files = DsFile::iter()
            .map(|file| {
                (
                    file,
                    BiosChoice::current(self.emulator.ds_file_name(file)).to_string(),
                )
            })
            .collect::<Vec<_>>();
        let firmware = self.emulator.ds_file_name(DsFile::Firmware).is_some();
        let s = &mut self.ui.settings;
        let mut open = None;
        page.section(
            Heading::new(icons::DEVICE_MOBILE, "Nintendo DS").hint(HEADING_HINT),
            |ui| {
                if s.ds_user.clock.is_some() {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs(1));
                }
                kit::list(ui, |list| {
                    let user = &s.ds_user;
                    let enabled = !(firmware && user.keep_firmware_settings);
                    let (month, day) = user.birthday;
                    let rows = [
                        (
                            Row::new("Nickname").value(user.nickname(account.as_deref())),
                            Sheet::DsNickname,
                        ),
                        (
                            Row::new("Birthday").value(format!("{} {day}", month_name(month))),
                            Sheet::DsBirthday,
                        ),
                        (
                            Row::new("Language").value(user.language().to_string()),
                            Sheet::DsLanguage,
                        ),
                        (
                            Row::new("Colour").value(COLORS[usize::from(user.favorite_color)].1),
                            Sheet::DsColor,
                        ),
                    ];
                    for (row, sheet) in rows {
                        if list.row(row.enabled(enabled)).clicked() {
                            open = Some(sheet);
                        }
                    }
                    let now = crate::storage::now_unix();
                    if list
                        .row(Row::new("Clock").value(clock_label(user.clock, now)))
                        .clicked()
                    {
                        open = Some(Sheet::DsClock);
                    }
                    for (file, choice) in &files {
                        let response = list.row(Row::new(file.label()).value(choice));
                        if *file == DsFile::Arm7Bios && std::mem::take(&mut s.find_ds_bios) {
                            response.scroll_to_me(Some(egui::Align::Center));
                            response.request_focus();
                        }
                        if response.clicked() {
                            open = Some(Sheet::DsFile(*file));
                        }
                    }
                    if firmware {
                        s.dirty |= list.switch(
                            SwitchRow::new(
                                "Keep its own settings",
                                &mut s.ds_user.keep_firmware_settings,
                            )
                            .info(KEEP_HINT),
                        );
                    }
                    #[cfg(ds_wifi)]
                    if let Some(sheet) = wifi_rows(
                        list,
                        &mut s.ds_online,
                        &mut s.dirty,
                        std::mem::take(&mut s.find_ds_direct),
                    ) {
                        open = Some(sheet);
                    }
                })
            },
        );
        walker_rows(page, s);
        open
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
                let shown = sheet::show(ctx, super::sheet_id(), "Nickname", |ui| {
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
                let shown = sheet::show(ctx, super::sheet_id(), "Birthday", |ui| {
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
                    "Birthday",
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
                    "Language",
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
                let shown = sheet::show(ctx, super::sheet_id(), "Clock", |ui| {
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
                let shown = sheet::show(ctx, super::sheet_id(), "Colour", |ui| {
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
            Sheet::DsFile(file) => {
                let name = self.emulator.ds_file_name(file);
                let mut choice = BiosChoice::current(name);
                let (changed, done) = pick_from(
                    ctx,
                    file.label(),
                    Some(FILES_HINT),
                    BiosChoice::all(name),
                    &mut choice,
                    |choice| choice.to_string(),
                );
                if changed {
                    match choice {
                        BiosChoice::BuiltIn => self.events.remove_ds_file(file),
                        BiosChoice::Pick { .. } => self.events.pick_ds_file(file),
                        BiosChoice::File(_) => {}
                    }
                }
                return done;
            }
            #[cfg(ds_wifi)]
            Sheet::DsWifi => {
                let online = &mut self.ui.settings.ds_online;
                let mut service = online.service;
                let (changed, done) = pick_from(
                    ctx,
                    "Online service",
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
                let shown = sheet::show(ctx, super::sheet_id(), "DNS server", |ui| {
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
            _ => return true,
        };
        self.ui.settings.dirty |= changed;
        done
    }
}

#[cfg(ds_wifi)]
fn wifi_rows(
    list: &mut kit::List<'_>,
    online: &mut DsOnline,
    dirty: &mut bool,
    find: bool,
) -> Option<Sheet> {
    use crate::app::widgets::settings::ds::DIRECT_HINT;
    let mut open = None;
    if list
        .row(Row::new("Online service").value(online.service.to_string()))
        .clicked()
    {
        open = Some(Sheet::DsWifi);
    }
    if online.service != OnlineService::None {
        *dirty |=
            list.switch(SwitchRow::new("Direct connection", &mut online.direct).info(DIRECT_HINT));
        if find {
            list.ui().scroll_to_cursor(Some(egui::Align::Center));
        }
    }
    if online.dns_needed()
        && list
            .row(Row::new("DNS server").value(online.dns_text()))
            .clicked()
    {
        open = Some(Sheet::DsDns);
    }
    open
}

fn walker_rows(page: &mut Page, s: &mut crate::app::ui_state::settings::Settings) {
    page.section(
        Heading::new(icons::PERSON_SIMPLE_WALK, "Pokéwalker"),
        |ui| {
            kit::list(ui, |list| {
                s.dirty |= list.switch(
                    SwitchRow::new("Steps in-game", &mut s.walker.in_game).info(IN_GAME_HINT),
                );
                #[cfg(target_os = "android")]
                {
                    use crate::app::widgets::settings::ds::{PHONE_DENIED, PHONE_HINT};
                    let denied = s.walker.phone && crate::platform::android::steps::denied();
                    let was = s.walker.phone;
                    s.dirty |= list.switch(
                        SwitchRow::new("Phone steps", &mut s.walker.phone)
                            .info(PHONE_HINT)
                            .detail(denied.then_some(PHONE_DENIED)),
                    );
                    if s.walker.phone && !was {
                        s.walker_asked = true;
                    }
                }
            })
        },
    );
}
