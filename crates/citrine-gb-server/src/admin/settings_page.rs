use super::invites;
use super::page;
use super::page::{Tab, esc, hint, icon};
use super::two_factor::two_factor_card;
use super::{Done, back, flash, guard, no_store};
use crate::settings::Source;
use crate::{App, settings};
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::response::{Html, Response};
use citrine_gb_net::Notice;
use serde::Deserialize;
use std::fmt::Write;
use std::sync::Arc;

pub(super) fn source_chip(app: &App, key: &str) -> String {
    match app.settings.source(key) {
        Source::Default => r#"<span class="chip">Default</span>"#.to_string(),
        Source::Env => r#"<span class="chip">From the environment</span>"#.to_string(),
        Source::Panel => format!(
            r#"<span class="chip panel">Set here</span><button type="submit" form="reset" name="key" value="{key}" class="btn ghost small">{}Reset</button>"#,
            icon("reset")
        ),
    }
}

pub(super) fn row(label_html: &str, chip: &str, control: &str) -> String {
    format!(
        r#"<div class="field-row"><div><div class="field"><span class="label">{label_html}</span></div><div class="source">{chip}</div></div>{control}</div>"#
    )
}

pub(super) async fn settings_page(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(done): Query<Done>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let s = app.settings.get();
    let mut form = String::new();
    form.push_str(&row(
        r#"<label for="name">Server name</label>"#,
        &source_chip(&app, settings::NAME),
        &format!(
            r#"<input id="name" name="name" type="text" maxlength="40" value="{}" required>"#,
            esc(&s.name)
        ),
    ));
    form.push_str(&row(
        &format!(
            r#"<label for="sign_ups">New accounts</label>{}"#,
            hint(
                "hint-signups",
                "When off, nobody can make a new account. Existing accounts still work."
            )
        ),
        &source_chip(&app, settings::SIGN_UPS),
        &format!(
            r#"<input id="sign_ups" name="sign_ups" type="checkbox" class="switch" value="on"{}>"#,
            if s.sign_ups { " checked" } else { "" }
        ),
    ));
    form.push_str(&invites::setting_row(&app, &s));
    form.push_str(&row(
        &format!(
            r#"<label for="delete_empty">Delete empty accounts</label>{}"#,
            hint(
                "hint-empty",
                "Once an hour, deletes accounts nobody signed in to for 30 days that have no friends, files or cloud saves. Banned accounts stay."
            )
        ),
        &source_chip(&app, settings::DELETE_EMPTY),
        &format!(
            r#"<input id="delete_empty" name="delete_empty" type="checkbox" class="switch" value="on"{}>"#,
            if s.delete_empty { " checked" } else { "" }
        ),
    ));
    if app.config.game_sharing {
        form.push_str(&row(
            &format!(
                r#"<span>Game sharing</span>{}"#,
                hint(
                    "hint-sharing",
                    "Friends who play together can lend each other the game. The relay passes games on only while new accounts need an invite code, so strangers can't swap games through your server."
                )
            ),
            r#"<span class="chip">From the environment</span>"#,
            if s.game_sharing {
                r#"<span class="chip on">On</span>"#
            } else {
                r#"<span class="chip warn">Waits for invite codes</span>"#
            },
        ));
    }
    form.push_str(&row(
        &format!(
            r#"<label for="pictures">Discord screenshots</label>{}"#,
            hint(
                "hint-pictures",
                "Players who show their game on Discord can send a screenshot. This server keeps it until they stop playing. When off, Discord shows the Citrine icon."
            )
        ),
        &source_chip(&app, settings::PICTURES),
        &format!(
            r#"<input id="pictures" name="pictures" type="checkbox" class="switch" value="on"{}>"#,
            if s.pictures { " checked" } else { "" }
        ),
    ));
    form.push_str(&row(
        &format!(
            r#"<label for="quota">Storage per user, MB</label>{}"#,
            hint(
                "hint-quota",
                "Screenshots and recordings shared in chat count toward it. 0 turns file sharing off. A limit set for one user on the Users page wins over this."
            )
        ),
        &source_chip(&app, settings::QUOTA),
        &format!(
            r#"<input id="quota" name="quota_mb" type="number" min="0" step="1" value="{}" required>"#,
            s.quota_bytes >> 20
        ),
    ));
    form.push_str(&row(
        &format!(
            r#"<label for="storage_total">Total storage, MB</label>{}"#,
            hint(
                "hint-storage-total",
                &format!(
                    "Cloud saves and shared files of all users together, plus the database. New uploads stop once it's full. The relay also leaves {} MB or {}% of the disk free, whichever is more.",
                    app.config.limits.disk_floor_bytes >> 20,
                    app.config.limits.disk_floor_percent
                )
            )
        ),
        &source_chip(&app, settings::STORAGE_TOTAL),
        &format!(
            r#"<input id="storage_total" name="storage_total_mb" type="number" min="0" step="1" value="{}" required>"#,
            s.storage_total_bytes >> 20
        ),
    ));
    form.push_str(&row(
        r#"<label for="max_message">Largest chat message, KB</label>"#,
        &source_chip(&app, settings::MAX_MESSAGE),
        &format!(
            r#"<input id="max_message" name="max_message_kb" type="number" min="1" max="1000" step="1" value="{}" required>"#,
            s.max_message_bytes >> 10
        ),
    ));
    form.push_str(&row(
        r#"<label for="max_file">Largest file, MB</label>"#,
        &source_chip(&app, settings::MAX_FILE),
        &format!(
            r#"<input id="max_file" name="max_file_mb" type="number" min="1" step="1" value="{}" required>"#,
            s.max_file_bytes >> 20
        ),
    ));
    let mut body = format!(
        r#"<form id="reset" method="post" action="/admin/settings/reset"></form><section class="card"><h2>{} Relay</h2><form method="post" action="/admin/settings">{form}<div class="form-foot"><button class="btn primary" type="submit">Save</button></div></form></section>"#,
        icon("settings")
    );
    let open_option = if app.settings.relay_open_allowed() && s.password_set {
        r#"<button class="btn" type="submit" name="remove" value="yes" formnovalidate>Remove password</button>"#
    } else {
        ""
    };
    let _ = write!(
        body,
        r#"<section class="card"><h2>{} Relay password</h2><div class="source">{}</div><form method="post" action="/admin/password"><div class="pw-grid"><div class="field"><label for="pw">New password</label><input id="pw" name="password" type="password" autocomplete="new-password" required></div><div class="field"><label for="pw2">New password again</label><input id="pw2" name="repeat" type="password" autocomplete="new-password" required></div></div><label class="check"><input type="checkbox" name="sign_out" value="yes" checked>Sign out everyone who joined with the old one</label><div class="form-foot" style="gap:8px">{open_option}<button class="btn primary" type="submit">Change password</button></div></form></section>"#,
        icon("key"),
        if s.password_set {
            source_chip(&app, settings::PASSWORD)
        } else {
            r#"<span class="chip warn">No password, anyone can join</span>"#.to_string()
        }
    );
    let card_error = match done.done.as_deref() {
        Some("bad-code") => Some("Wrong code"),
        Some("too-many") => Some("Too many tries. Wait a minute and try again."),
        _ => None,
    };
    body.push_str(
        &two_factor_card(
            &app,
            &s.name,
            done.setup.as_deref() == Some("two-factor"),
            card_error,
        )
        .await,
    );
    let flash = if card_error.is_some() {
        None
    } else {
        flash(&done)
    };
    no_store(Html(page::page(&s.name, Tab::Settings, flash, &body)).into_response())
}

#[derive(Deserialize)]
pub(super) struct SettingsForm {
    name: String,
    sign_ups: Option<String>,
    invites: Option<String>,
    pictures: Option<String>,
    delete_empty: Option<String>,
    quota_mb: String,
    storage_total_mb: Option<String>,
    max_message_kb: String,
    max_file_mb: String,
}

pub(super) async fn settings_save(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<SettingsForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let name = form.name.trim();
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        return back("/admin/settings", "bad-name");
    }
    let parse = |t: &str, max: u64| t.trim().parse::<u64>().ok().filter(|v| *v <= max);
    let (Some(quota), Some(message), Some(file)) = (
        parse(&form.quota_mb, 1 << 30),
        parse(&form.max_message_kb, 1000).filter(|v| *v > 0),
        parse(&form.max_file_mb, 1 << 20).filter(|v| *v > 0),
    ) else {
        return back("/admin/settings", "bad-number");
    };
    let s = app.settings.get();
    let total = match form.storage_total_mb.as_deref() {
        None => s.storage_total_bytes >> 20,
        Some(text) => match parse(text, 1 << 30) {
            Some(total) => total,
            None => return back("/admin/settings", "bad-number"),
        },
    };
    let changes: [(&str, bool, String); 9] = [
        (settings::NAME, name != s.name, name.to_string()),
        (
            settings::SIGN_UPS,
            form.sign_ups.is_some() != s.sign_ups,
            if form.sign_ups.is_some() { "on" } else { "off" }.to_string(),
        ),
        (
            settings::INVITES,
            form.invites.is_some() != s.invites,
            if form.invites.is_some() { "on" } else { "off" }.to_string(),
        ),
        (
            settings::PICTURES,
            form.pictures.is_some() != s.pictures,
            if form.pictures.is_some() { "on" } else { "off" }.to_string(),
        ),
        (
            settings::DELETE_EMPTY,
            form.delete_empty.is_some() != s.delete_empty,
            if form.delete_empty.is_some() {
                "on"
            } else {
                "off"
            }
            .to_string(),
        ),
        (
            settings::QUOTA,
            quota << 20 != s.quota_bytes,
            quota.to_string(),
        ),
        (
            settings::STORAGE_TOTAL,
            total << 20 != s.storage_total_bytes,
            total.to_string(),
        ),
        (
            settings::MAX_MESSAGE,
            message << 10 != s.max_message_bytes,
            message.to_string(),
        ),
        (
            settings::MAX_FILE,
            file << 20 != s.max_file_bytes,
            file.to_string(),
        ),
    ];
    for (key, changed, value) in changes {
        if changed && app.settings.set(key, &value).await.is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    back("/admin/settings", "saved")
}

#[derive(Deserialize)]
pub(super) struct ResetForm {
    key: String,
}

pub(super) async fn settings_reset(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<ResetForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let allowed = [
        settings::NAME,
        settings::SIGN_UPS,
        settings::INVITES,
        settings::PICTURES,
        settings::DELETE_EMPTY,
        settings::QUOTA,
        settings::STORAGE_TOTAL,
        settings::MAX_MESSAGE,
        settings::MAX_FILE,
        settings::PASSWORD,
    ];
    if !allowed.contains(&form.key.as_str()) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if form.key == settings::PASSWORD
        && app.config.relay_password.is_none()
        && !app.settings.relay_open_allowed()
    {
        return back("/admin/settings", "needs-password");
    }
    if app.settings.reset(&form.key).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    back("/admin/settings", "reset")
}

#[derive(Deserialize)]
pub(super) struct PasswordForm {
    password: Option<String>,
    repeat: Option<String>,
    sign_out: Option<String>,
    remove: Option<String>,
}

pub(super) async fn password(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let sign_out = form.sign_out.is_some();
    if form.remove.is_some() {
        if !app.settings.relay_open_allowed() {
            return back("/admin/settings", "needs-password");
        }
        if app.settings.set_password(None).await.is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        return back("/admin/settings", "password-none");
    }
    let password = form.password.unwrap_or_default();
    if password.is_empty() {
        return back("/admin/settings", "empty");
    }
    if Some(&password) != form.repeat.as_ref() {
        return back("/admin/settings", "mismatch");
    }
    if app.settings.set_password(Some(&password)).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    log::info!("relay password changed in the admin panel");
    if sign_out {
        app.hub.close_all(Notice::PasswordChanged);
        back("/admin/settings", "password-out")
    } else {
        back("/admin/settings", "password")
    }
}
