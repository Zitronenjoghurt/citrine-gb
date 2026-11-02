use super::page;
use super::page::{Flash, Tab, ago, avatar, bytes, esc, icon};
use super::{BLOCK_FOR, Done, PAGE_SIZE, back, confirm, flash, guard};
use crate::connections::{address_key, address_text};
use crate::db::UserOrder;
use crate::relay::{file_path, quota_of};
use crate::{App, now};
use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::response::{Html, Redirect, Response};
use citrine_gb_net::Notice;
use serde::Deserialize;
use std::fmt::Write;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

#[derive(Deserialize, Default)]
pub(super) struct UsersQuery {
    done: Option<String>,
    q: Option<String>,
    page: Option<u64>,
    sort: Option<String>,
}

const RECENT: [(u64, &str); 3] = [
    (1, "the last hour"),
    (24, "the last 24 hours"),
    (168, "the last 7 days"),
];

pub(super) fn url_part(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn users_link(search: &str, newest: bool, page_no: u64) -> String {
    format!(
        "/admin/users?q={}{}&page={page_no}",
        url_part(search),
        if newest { "&sort=newest" } else { "" }
    )
}

fn users_flash(done: Option<&str>) -> Option<Flash> {
    let (ok, text) = match done? {
        "blocked" => (
            true,
            "Address blocked for 24 hours, everyone on it was disconnected",
        ),
        "unblocked" => (true, "Address unblocked"),
        "offline" => (false, "They went offline, so there's no address to block"),
        "deleted" => (true, "Account deleted"),
        "device-signed-out" => (true, "Device signed out"),
        "device-gone" => (false, "That device was already signed out"),
        "two-factor-off" => (
            true,
            "Two-factor sign-in is off. A new device signs in with the relay ID alone.",
        ),
        "banned-many" => (true, "Accounts banned and disconnected"),
        "banned-none" => (true, "Nobody new to ban"),
        other => {
            return flash(&Done {
                done: Some(other.to_string()),
                ..Done::default()
            });
        }
    };
    Some(Flash { ok, text })
}

pub(super) fn left(time: Duration) -> String {
    let mins = time.as_secs().div_ceil(60);
    if mins >= 60 {
        format!("{} h left", mins.div_ceil(60))
    } else {
        format!("{mins} min left")
    }
}

fn blocked_card(app: &App) -> String {
    let blocked = app.connections.blocked();
    if blocked.is_empty() {
        return String::new();
    }
    let rows: String = blocked
        .iter()
        .map(|(ip, time)| {
            let text = esc(&address_text(*ip));
            format!(
                r#"<div class="blocked-row"><span class="mono">{text}</span><span class="muted">{}</span><form method="post" action="/admin/blocks/unblock"><input type="hidden" name="address" value="{text}"><button class="btn small" type="submit">Unblock</button></form></div>"#,
                left(*time)
            )
        })
        .collect();
    format!(
        r#"<section class="card blocked"><h2>{} Blocked addresses</h2>{rows}</section>"#,
        icon("ban")
    )
}

async fn ban_new_menu(app: &App, now: u64) -> String {
    let mut buttons = String::new();
    for (hours, label) in RECENT {
        let since = now.saturating_sub(hours * 3600);
        let count = app.db.count_joined_since(since).await.unwrap_or(0);
        let _ = write!(
            buttons,
            r#"<form method="post" action="/admin/users/ban-new"><input type="hidden" name="since" value="{since}"><button class="btn danger" type="submit"{}>{}Ban {} from {label}</button></form>"#,
            if count == 0 { " disabled" } else { "" },
            icon("ban"),
            count,
        );
    }
    format!(
        r#"<button type="button" class="btn danger" popovertarget="ban-new">{}<span>Ban new accounts</span></button><div id="ban-new" popover><h3>Ban accounts that joined recently?</h3><p>They're disconnected now and can't sign in until you unban them. Accounts that are already banned stay as they are.</p><div class="menu">{buttons}</div><div class="buttons" style="margin-top:12px"><button type="button" class="btn" popovertarget="ban-new" popovertargetaction="hide">Cancel</button></div></div>"#,
        icon("ban")
    )
}

pub(super) async fn users(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<UsersQuery>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let s = app.settings.get();
    let search = query.q.clone().unwrap_or_default();
    let search = search.trim();
    let newest = query.sort.as_deref() == Some("newest");
    let order = if newest {
        UserOrder::Newest
    } else {
        UserOrder::LastSeen
    };
    let page_no = query.page.unwrap_or(1).max(1);
    let Ok((rows, total)) = app
        .db
        .users(search, order, 0, (page_no - 1) * PAGE_SIZE, PAGE_SIZE)
        .await
    else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let online = app.hub.online();
    let now = now();
    let mut body = blocked_card(&app);
    let sort_link = |to_newest: bool, label: &str| {
        format!(
            r#"<a class="btn" href="{}" aria-pressed="{}">{label}</a>"#,
            esc(&users_link(search, to_newest, 1)),
            to_newest == newest
        )
    };
    let _ = write!(
        body,
        r#"<div class="toolbar"><form method="get" action="/admin/users" role="search"><div class="search">{}<input type="search" name="q" value="{}" placeholder="Name or online ID" aria-label="Search users"></div>{}<button class="btn" type="submit">Search</button></form><div class="segmented" role="group" aria-label="Order">{}{}</div><span class="count">{}</span>{}</div>"#,
        icon("search"),
        esc(search),
        if newest {
            r#"<input type="hidden" name="sort" value="newest">"#
        } else {
            ""
        },
        sort_link(false, "Last seen"),
        sort_link(true, "Newest"),
        match total {
            1 => "1 account".to_string(),
            n => format!("{n} accounts"),
        },
        ban_new_menu(&app, now).await,
    );
    let mut list = format!(
        r#"<section class="card users{}"><div class="user-row head"><span>Name</span><span>Online ID</span><span>Joined</span><span>Last seen</span><span>Storage</span><span></span></div>"#,
        if newest { " newest" } else { "" }
    );
    if rows.is_empty() {
        list.push_str(if search.is_empty() {
            r#"<p class="empty">No accounts yet</p>"#
        } else {
            r#"<p class="empty">No account matches</p>"#
        });
    }
    let here = users_link(search, newest, page_no);
    for row in rows {
        let a = &row.account;
        let two_factor = matches!(app.db.account_totp(a.id).await, Ok(Some(_)));
        let id = a.id;
        let name = esc(&a.display_name);
        let oid = a.online_id.to_string();
        let mut keys: Vec<IpAddr> = online
            .get(&id)
            .into_iter()
            .flatten()
            .map(|ip| address_key(*ip))
            .collect();
        keys.sort();
        keys.dedup();
        let address = keys.first().map(|key| match keys.len() {
            1 => address_text(*key),
            n => format!("{} and {} more", address_text(*key), n - 1),
        });
        let live = app.hub.online_devices(id);
        let devices = app.db.devices_of(id).await.unwrap_or_default();
        let mut device_rows = String::new();
        let mut device_confirms = String::new();
        for device in &devices {
            let dev = device.id;
            let device_name = esc(if device.name.is_empty() {
                "Unnamed device"
            } else {
                &device.name
            });
            let state = if live.contains(&dev) {
                r#"<span class="state online"><span class="dot"></span>Online</span>"#.to_string()
            } else {
                format!(
                    r#"<span class="state muted">Last seen {}</span>"#,
                    ago(device.last_seen, now).to_lowercase()
                )
            };
            let _ = write!(
                device_rows,
                r#"<div class="device">{}<div class="seen"><span class="name">{device_name}</span>{state}</div><button type="button" class="btn small" popovertarget="sign-out-{id}-{dev}">{}Sign out</button></div>"#,
                icon(if citrine_gb_net::is_phone(&device.name) {
                    "smartphone"
                } else {
                    "monitor"
                }),
                icon("log-out"),
            );
            device_confirms.push_str(&confirm(
                &format!("sign-out-{id}-{dev}"),
                &format!("Sign out {device_name} for {name}?"),
                "It forgets the relay ID there.",
                &format!("/admin/users/{id}/devices/{dev}/sign-out"),
                "Sign out",
                &format!(
                    r#"<input type="hidden" name="back" value="{}">"#,
                    esc(&here)
                ),
            ));
        }
        let devices_block = if devices.is_empty() {
            String::new()
        } else {
            format!(r#"<div class="devices"><h4>Devices</h4>{device_rows}</div>"#)
        };
        let quota = quota_of(&app, a);
        let fraction = if quota > 0 {
            (row.storage_used as f64 / quota as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        let banned = if a.banned {
            format!(
                r#"<span class="badge" title="Banned" aria-label="Banned">{}</span>"#,
                icon("ban")
            )
        } else {
            String::new()
        };
        let seen = match &address {
            Some(address) => format!(
                r#"<div class="seen"><span class="state online"><span class="dot"></span>Online</span><span class="ip mono muted" title="{0}">{0}</span></div>"#,
                esc(address)
            ),
            None => format!(
                r#"<div class="seen"><span class="state muted">{}</span></div>"#,
                ago(a.last_seen, now)
            ),
        };
        let joined = format!(
            r#"<span class="joined muted">{}</span>"#,
            joined_text(row.joined, now)
        );
        let quota_text = if quota == 0 {
            "no file sharing".to_string()
        } else {
            bytes(quota)
        };
        let ret = esc(&here);
        let back_field = format!(r#"<input type="hidden" name="back" value="{ret}">"#);
        let ban = if a.banned {
            format!(
                r#"<form method="post" action="/admin/users/{id}/unban">{back_field}<button class="btn" type="submit">{}Unban</button></form>"#,
                icon("ban")
            )
        } else {
            format!(
                r#"<button type="button" class="btn danger" popovertarget="ban-{id}">{}Ban</button>"#,
                icon("ban")
            )
        };
        let block = match &address {
            Some(_) => format!(
                r#"<button type="button" class="btn danger" popovertarget="block-{id}">{}Block address for 24 h</button>"#,
                icon("unplug")
            ),
            None => String::new(),
        };
        let files_label = match row.files {
            0 => "Delete files".to_string(),
            1 => format!("Delete 1 file, {}", bytes(row.storage_used)),
            n => format!("Delete {n} files, {}", bytes(row.storage_used)),
        };
        let _ = write!(
            list,
            r#"<div class="user-row"><div class="who">{avatar}<div style="min-width:0"><div class="name">{name} {banned}</div><span class="mono muted">{oid}</span></div></div><span class="mono oid">{oid}</span>{joined}{seen}<div class="storage">{used} of {quota_text}<div class="bar"><i style="width:{fraction:.1}%"></i></div></div><div class="actions"><button type="button" class="btn small" popovertarget="manage-{id}">{more}<span>Manage</span></button></div></div>
<div id="manage-{id}" popover><div class="who">{avatar}<div style="min-width:0"><div class="name">{name} {banned}</div><span class="mono muted">{oid}</span></div></div><div class="menu"><form method="post" action="/admin/users/{id}/quota" class="inline">{back_field}<input type="number" name="quota_mb" min="0" step="1" value="{quota_value}" placeholder="{default_mb} MB (default)" aria-label="Storage limit in MB"><button class="btn" type="submit">{drive}Set limit</button></form><form method="post" action="/admin/users/{id}/disconnect">{back_field}<button class="btn" type="submit"{offline}>{unplug}Disconnect</button></form>{ban}{block}{two_factor_button}<button type="button" class="btn danger" popovertarget="files-{id}"{no_files}>{trash}{files_label}</button><button type="button" class="btn danger" popovertarget="delete-{id}">{trash}Delete account</button></div>{devices_block}</div>
{ban_confirm}{block_confirm}{two_factor_confirm}{files_confirm}{delete_confirm}{device_confirms}"#,
            avatar = avatar(&a.display_name, a.online_id.value()),
            used = bytes(row.storage_used),
            more = icon("more"),
            drive = icon("drive"),
            unplug = icon("unplug"),
            trash = icon("trash"),
            quota_value = a
                .quota_bytes
                .map(|q| (q >> 20).to_string())
                .unwrap_or_default(),
            default_mb = s.quota_bytes >> 20,
            offline = if address.is_some() { "" } else { " disabled" },
            no_files = if row.files == 0 { " disabled" } else { "" },
            two_factor_button = if two_factor {
                format!(
                    r#"<button type="button" class="btn danger" popovertarget="two-factor-{id}">{}Turn off two-factor</button>"#,
                    icon("key")
                )
            } else {
                String::new()
            },
            two_factor_confirm = if two_factor {
                confirm(
                    &format!("two-factor-{id}"),
                    &format!("Turn off two-factor sign-in for {name}?"),
                    "Do this only when they lost their phone and their recovery codes. Anyone with their relay ID can then sign in on a new device.",
                    &format!("/admin/users/{id}/two-factor-off"),
                    "Turn off",
                    &back_field,
                )
            } else {
                String::new()
            },
            ban_confirm = if a.banned {
                String::new()
            } else {
                confirm(
                    &format!("ban-{id}"),
                    &format!("Ban {name}?"),
                    "They're disconnected now and can't sign in until you unban them.",
                    &format!("/admin/users/{id}/ban"),
                    "Ban",
                    &back_field,
                )
            },
            block_confirm = match &address {
                Some(_) => confirm(
                    &format!("block-{id}"),
                    &format!(
                        "Block {}?",
                        esc(&keys
                            .iter()
                            .map(|key| address_text(*key))
                            .collect::<Vec<_>>()
                            .join(", "))
                    ),
                    "Everyone connected from this address is disconnected, and nobody can connect from it for 24 hours or until you unblock it.",
                    &format!("/admin/users/{id}/block"),
                    "Block",
                    &back_field,
                ),
                None => String::new(),
            },
            files_confirm = confirm(
                &format!("files-{id}"),
                &format!("Delete {name}'s files?"),
                &format!(
                    "{} stored for chats goes for good. Their friends can't open it any more.",
                    bytes(row.storage_used)
                ),
                &format!("/admin/users/{id}/files"),
                "Delete files",
                &back_field,
            ),
            delete_confirm = confirm(
                &format!("delete-{id}"),
                &format!("Delete {name}'s account?"),
                "Their account, friends, chats on the relay, files and cloud saves go for good. With sign-ups open, they can make a new account.",
                &format!("/admin/users/{id}/delete"),
                "Delete account",
                &back_field,
            ),
        );
    }
    list.push_str("</section>");
    body.push_str(&list);
    let pages = total.div_ceil(PAGE_SIZE).max(1);
    if pages > 1 {
        let link = |p: u64, label: &str, ic: &str, enabled: bool| {
            if enabled {
                format!(
                    r#"<a class="btn" href="{}">{}{label}</a>"#,
                    esc(&users_link(search, newest, p)),
                    icon(ic)
                )
            } else {
                format!(
                    r#"<span class="btn" aria-disabled="true" style="opacity:.45">{}{label}</span>"#,
                    icon(ic)
                )
            }
        };
        let first = (page_no - 1) * PAGE_SIZE + 1;
        let last = (page_no * PAGE_SIZE).min(total);
        let _ = write!(
            body,
            r#"<nav class="pages" aria-label="Pages">{}<span class="muted">{first} to {last} of {total}</span>{}</nav>"#,
            link(page_no - 1, "Previous", "left", page_no > 1),
            link(page_no + 1, "Next", "right", page_no < pages),
        );
    }
    Html(page::page(
        &s.name,
        Tab::Users,
        users_flash(query.done.as_deref()),
        &body,
    ))
    .into_response()
}

fn joined_text(joined: u64, now: u64) -> String {
    let when = ago(joined, now);
    let mut chars = when.chars();
    let lower = chars
        .next()
        .map(|c| c.to_lowercase().chain(chars).collect::<String>())
        .unwrap_or_default();
    format!(r#"<span class="wide">{when}</span><span class="narrow">Joined {lower}</span>"#)
}

#[derive(Deserialize)]
pub(super) struct UserForm {
    back: Option<String>,
    quota_mb: Option<String>,
}

fn user_back(form: &UserForm, flash: &str) -> Response {
    let to = form
        .back
        .as_deref()
        .filter(|b| b.starts_with("/admin/users") && !b.contains("//"))
        .unwrap_or("/admin/users");
    let to = to.split("&done=").next().unwrap_or(to);
    let sep = if to.contains('?') { '&' } else { '?' };
    Redirect::to(&format!("{to}{sep}done={flash}")).into_response()
}

pub(super) async fn user_quota(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let quota = match form.quota_mb.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(text) => match text.parse::<u64>() {
            Ok(mb) if mb < (1 << 30) => Some(mb << 20),
            _ => return user_back(&form, "bad-number"),
        },
    };
    if app.db.set_quota(id, quota).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    user_back(&form, "quota")
}

pub(super) async fn user_disconnect(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    app.hub.kick(id, Notice::Disconnected);
    user_back(&form, "disconnected")
}

pub(super) async fn user_device_sign_out(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path((id, device)): Path<(i64, i64)>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    match app.db.sign_out_device(id, device, now()).await {
        Ok(true) => {}
        Ok(false) => return user_back(&form, "device-gone"),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    app.hub.kick_device(id, device, Notice::SignedOut);
    log::warn!("a device was signed out in the admin panel");
    user_back(&form, "device-signed-out")
}

pub(super) async fn user_ban(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if app.db.set_banned(id, true).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    app.hub.kick(id, Notice::Banned);
    user_back(&form, "banned")
}

pub(super) async fn user_unban(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if app.db.set_banned(id, false).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    user_back(&form, "unbanned")
}

pub(super) async fn user_block(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let Some(ips) = app.hub.online().remove(&id) else {
        return user_back(&form, "offline");
    };
    for ip in ips {
        let key = app.connections.block(ip, BLOCK_FOR);
        app.hub.kick_address(key, Notice::Disconnected);
    }
    log::warn!("an address was blocked for 24 hours in the admin panel");
    user_back(&form, "blocked")
}

pub(super) async fn user_two_factor_off(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if app.db.clear_account_totp(id).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    log::warn!("two-factor sign-in for an account was turned off in the admin panel");
    user_back(&form, "two-factor-off")
}

pub(super) async fn user_delete(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let Ok(deleted) = app.db.delete_account(id).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    for file in deleted.unwrap_or_default() {
        let _ = tokio::fs::remove_file(file_path(&app, file)).await;
    }
    app.hub.kick(id, Notice::Disconnected);
    log::info!("an account was deleted in the admin panel");
    user_back(&form, "deleted")
}

#[derive(Deserialize)]
pub(super) struct SinceForm {
    since: u64,
}

pub(super) async fn ban_new(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<SinceForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let Ok(ids) = app.db.ban_joined_since(form.since).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    for id in &ids {
        app.hub.kick(*id, Notice::Banned);
    }
    log::warn!("{} new accounts banned in the admin panel", ids.len());
    let done = if ids.is_empty() {
        "banned-none"
    } else {
        "banned-many"
    };
    Redirect::to(&format!("/admin/users?sort=newest&done={done}")).into_response()
}

#[derive(Deserialize)]
pub(super) struct AddressForm {
    address: String,
}

pub(super) async fn unblock(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<AddressForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let text = form.address.trim().trim_end_matches("/64");
    let Ok(ip) = text.parse() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    app.connections.unblock(ip);
    back("/admin/users", "unblocked")
}

pub(super) async fn user_files(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<UserForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let Ok(files) = app.db.delete_files_of(id).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    for file in files {
        let _ = tokio::fs::remove_file(file_path(&app, file)).await;
    }
    user_back(&form, "files")
}
