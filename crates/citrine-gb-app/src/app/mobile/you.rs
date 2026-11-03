use super::kit::{self, Row, SwitchRow};
use super::nav::Place;
use super::sheet;
use crate::app::Citrine;
use crate::app::tabs::{Tab, stats};
use crate::app::widgets::avatar;
use crate::app::widgets::settings::online::{self, connection_line, my_seen};
use crate::icons;
use egui::{Context, Id, RichText, Ui};
use web_time::Instant;

const FACE: f32 = 88.0;
const KEY_NOTE: &str =
    "Use it to sign in on another device. Don't share it, since anyone with it can sign in as you.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YouSheet {
    DisplayName,
    SignInKey,
    TwoFactor,
    Devices,
    Accounts,
    AddAccount,
}

pub fn stat_rows(overview: &crate::stats::Overview) -> Vec<(&'static str, &'static str, String)> {
    let mut rows = vec![(
        icons::CHART_BAR,
        "This week",
        stats::play_time(overview.week_ms),
    )];
    if let Some(game) = overview.most_played() {
        rows.push((icons::GAME_CONTROLLER, "Most played", game.name.clone()));
    }
    rows
}

impl Citrine {
    pub(super) fn you_place(&mut self, ui: &mut Ui) {
        let mut open_stats = false;
        let overview = self.stats.book().overview(crate::stats::today());
        let now = Instant::now();
        let account = online::joined_account(&self.ui.settings, &self.online);
        if account.is_some() {
            online::track(
                &mut self.ui.settings,
                &mut self.online,
                &mut self.ui.join,
                now,
            );
        }
        kit::place(ui, "you_place", |ui| {
            kit::place_title(ui, Place::You.label(), |_| {});
            if account.is_some() {
                self.you_face(ui);
            }
            kit::section_label(ui, "Stats");
            let rows = stat_rows(&overview)
                .into_iter()
                .map(|(icon, label, value)| kit::Row::new(label).icon(icon).value(value))
                .collect();
            open_stats = kit::rows(ui, rows).is_some();
            match &account {
                Some(account) => self.account_card(ui, account, now),
                None => {
                    kit::section_label(ui, "Account");
                    self.signed_out_rows(ui, true);
                }
            }
        });
        if open_stats {
            self.mobile.open(Tab::Stats);
            self.release_touch();
        }
        if account.is_some() {
            self.you_sheet(ui.ctx(), now);
            online::account_dialogs(
                ui,
                &mut self.ui.settings,
                &mut self.online,
                &mut self.ui.join,
            );
        } else {
            self.mobile.you_sheet = None;
            self.signed_out_sheet(ui);
        }
    }

    pub(super) fn signed_out_rows(&mut self, ui: &mut Ui, account_rows: bool) {
        let now = Instant::now();
        let s = &mut self.ui.settings;
        let form = &mut self.ui.join;
        online::track(s, &mut self.online, form, now);
        if let Some(open) = online::signed_out_rows(ui, s, form, account_rows) {
            online::open_sheet(s, &mut self.online, form, open, now);
            self.mobile.join_sheet = Some(open);
            self.release_touch();
        }
    }

    pub(super) fn signed_out_sheet(&mut self, ui: &mut Ui) {
        let s = &mut self.ui.settings;
        let form = &mut self.ui.join;
        let now = Instant::now();
        if let Some(open) = self.mobile.join_sheet
            && online::join_sheet(ui.ctx(), s, &mut self.online, form, open, now)
        {
            self.mobile.join_sheet = None;
            online::back_out(s, &mut self.online, form, now);
        }
        online::join_dialogs(ui, s, &mut self.online, form);
    }

    fn account_card(&mut self, ui: &mut Ui, account: &online::Account, now: Instant) {
        let s = &mut self.ui.settings;
        let client = &mut self.online;
        let form = &mut self.ui.join;
        online::problems(ui, s, client, form, now);
        kit::section_label(ui, "Account");
        let connected = client.is_connected();
        let copied = online::just_copied(form, "online", now);
        let mut open = None;
        kit::list(ui, |list| {
            if list
                .row(
                    Row::new("Display name")
                        .icon(icons::USER)
                        .value(&account.name),
                )
                .clicked()
            {
                open = Some(YouSheet::DisplayName);
            }
            let (id_value, id_icon) = if copied {
                ("Copied".to_owned(), icons::CHECK)
            } else {
                (account.online_id.clone(), icons::COPY)
            };
            if list
                .row(
                    Row::new("Online ID")
                        .icon(icons::GLOBE)
                        .value(id_value)
                        .trailing(id_icon),
                )
                .clicked()
            {
                let ctx = list.ui().ctx().clone();
                online::copy_now(&ctx, form, "online", &account.online_id, now);
            }
            if list.row(Row::new("Sign-in key").icon(icons::KEY)).clicked() {
                open = Some(YouSheet::SignInKey);
            }
            if let Some(on) = account.two_factor
                && list
                    .row(
                        Row::new("Two-factor sign-in")
                            .icon(icons::LOCK_SIMPLE)
                            .value(if on { "On" } else { "Off" }),
                    )
                    .clicked()
            {
                open = Some(YouSheet::TwoFactor);
            }
            if account.devices > 0
                && list
                    .row(
                        Row::new("Devices")
                            .icon(icons::DEVICES)
                            .value(match account.devices {
                                1 => "1 device".to_owned(),
                                n => format!("{n} devices"),
                            }),
                    )
                    .clicked()
            {
                open = Some(YouSheet::Devices);
            }
            if list
                .row(Row::new("Switch account").icon(icons::USER_SWITCH))
                .clicked()
            {
                open = Some(YouSheet::Accounts);
            }
            if list
                .row(Row::new("Sign out").icon(icons::SIGN_OUT).no_chevron())
                .clicked()
            {
                online::ask_sign_out(form);
            }
        });
        ui.add_space(12.0);
        let mut dirty = false;
        kit::list(ui, |list| {
            dirty |= list.switch(SwitchRow::new(
                online::SHOW_PLAYING,
                &mut s.online.show_playing,
            ));
        });
        s.dirty |= dirty;
        ui.add_space(12.0);
        let delete = kit::list(ui, |list| {
            list.custom(|ui| {
                ui.add_enabled_ui(connected, |ui| {
                    Row::new("Delete account")
                        .icon(icons::TRASH)
                        .danger()
                        .no_chevron()
                        .show(ui)
                        .clicked()
                })
                .inner
            })
        });
        if delete {
            online::ask_delete(form);
        }
        if open.is_some() {
            self.mobile.you_sheet = open;
            self.release_touch();
        }
    }

    fn you_sheet(&mut self, ctx: &Context, now: Instant) {
        let Some(open) = self.mobile.you_sheet else {
            return;
        };
        let Some(account) = online::joined_account(&self.ui.settings, &self.online) else {
            self.mobile.you_sheet = None;
            return;
        };
        let id = Id::new("you_sheet");
        let mut next = None;
        let s = &self.ui.settings;
        let client = &mut self.online;
        let form = &mut self.ui.join;
        let closed = match open {
            YouSheet::DisplayName => {
                let shown = sheet::show(ctx, id, "Display name", |ui| {
                    online::name_editor(ui, client, form, &account.name)
                });
                if shown.closed && !shown.inner {
                    online::forget_name_draft(form);
                }
                shown.inner || shown.closed
            }
            YouSheet::SignInKey => {
                let copied = online::just_copied(form, "relay", now);
                let shown = sheet::show(ctx, id, "Sign-in key", |ui| {
                    sheet::note(ui, KEY_NOTE);
                    let text = if copied {
                        format!("{} Copied", icons::CHECK)
                    } else {
                        format!("{} Copy sign-in key", icons::COPY)
                    };
                    kit::primary_button(ui, &text)
                });
                if shown.inner {
                    online::copy_now(ctx, form, "relay", &account.sign_in_key, now);
                }
                shown.closed
            }
            YouSheet::TwoFactor => {
                sheet::show(ctx, id, "Two-factor sign-in", |ui| {
                    online::two_factor_phone(ui, s, client, form, now)
                })
                .closed
            }
            YouSheet::Devices => {
                sheet::show(ctx, id, "", |ui| online::devices_body(ui, client, form)).closed
            }
            YouSheet::Accounts => {
                let s = &mut self.ui.settings;
                let shown = sheet::show(ctx, id, "Accounts", |ui| {
                    online::accounts_body(ui, s, client, form, now)
                });
                if shown.inner == online::AccountsChoice::Add {
                    next = Some(YouSheet::AddAccount);
                }
                shown.closed || shown.inner != online::AccountsChoice::Stay
            }
            YouSheet::AddAccount => {
                let s = &mut self.ui.settings;
                let shown = sheet::show(ctx, id, "Add account", online::add_account_body);
                if let Some(open) = shown.inner {
                    online::start_add(s, client, form, open, now);
                    self.mobile.join_sheet = Some(open);
                }
                shown.closed || shown.inner.is_some()
            }
        };
        if closed {
            self.mobile.you_sheet = next;
        }
    }

    fn you_face(&mut self, ui: &mut Ui) {
        let name = self
            .online
            .account()
            .map(|account| (account.display_name.clone(), account.online_id.value()));
        let Some((name, seed)) = name else {
            return;
        };
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            avatar::avatar(ui, &name, seed, FACE, Some(my_seen(&self.online)));
            ui.add_space(8.0);
            ui.add(egui::Label::new(RichText::new(&name).size(20.0).strong()).truncate());
            if self.online.is_connected() {
                ui.label(RichText::new("Online").weak());
            } else {
                connection_line(ui, &mut self.online, Instant::now());
            }
        });
    }
}

#[cfg(test)]
mod tests;
