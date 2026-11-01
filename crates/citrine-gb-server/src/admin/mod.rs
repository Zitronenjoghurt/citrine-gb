mod invites;
mod page;
mod two_factor;

use crate::relay::{file_path, quota_of};
use crate::settings::{self, Source, same_secret};
use crate::{App, now};
use axum::Router;
use axum::extract::{ConnectInfo, Form, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use citrine_gb_net::Notice;
pub(crate) use page::web_missing_page;
use page::{Flash, Tab, ago, avatar, bytes, duration, esc, hint, icon, web_missing_steps};
use serde::Deserialize;
use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

const COOKIE: &str = "citrine_admin";
const SESSION_HOURS: u64 = 12;
const PAGE_SIZE: u64 = 50;
const ICON: &[u8] = include_bytes!("../../../citrine-gb-app/assets/icon-64x64.png");

pub fn router() -> Router<Arc<App>> {
    Router::new()
        .route("/", get(dashboard))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/stats", get(stats))
        .route("/icon.png", get(icon_png))
        .route("/relay", post(relay))
        .route("/kill", post(kill))
        .route("/users", get(users))
        .route("/users/{id}/quota", post(user_quota))
        .route("/users/{id}/disconnect", post(user_disconnect))
        .route("/users/{id}/ban", post(user_ban))
        .route("/users/{id}/unban", post(user_unban))
        .route("/users/{id}/files", post(user_files))
        .route("/invites", get(invites::list).post(invites::create))
        .route("/invites/{id}/revoke", post(invites::revoke))
        .route("/settings", get(settings_page).post(settings_save))
        .route("/settings/reset", post(settings_reset))
        .route("/password", post(password))
        .route("/two-factor/on", post(two_factor_on))
        .route("/two-factor/off", post(two_factor_off))
        .route("/two-factor/codes", post(two_factor_codes))
}

fn token_hash(token: &str) -> [u8; 32] {
    settings::hash(token)
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.to_string())
}

fn signed_in(app: &App, headers: &HeaderMap) -> bool {
    let Some(token) = cookie_token(headers) else {
        return false;
    };
    let mut sessions = app.admin_sessions.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    sessions.retain(|_, expires| *expires > now);
    sessions.contains_key(&token_hash(&token))
}

fn guard(app: &App, headers: &HeaderMap) -> Option<Response> {
    (!signed_in(app, headers)).then(|| Redirect::to("/admin").into_response())
}

fn back(to: &str, flash: &str) -> Response {
    Redirect::to(&format!("{to}?done={flash}")).into_response()
}

#[derive(Deserialize, Default)]
struct Done {
    done: Option<String>,
    q: Option<String>,
    page: Option<u64>,
    setup: Option<String>,
}

fn flash(done: &Done) -> Option<Flash> {
    let (ok, text) = match done.done.as_deref()? {
        "relay-on" => (true, "Relay turned on"),
        "relay-off" => (true, "Relay turned off, everyone was disconnected"),
        "kill-on" => (true, "Kill switch on, everyone was disconnected"),
        "kill-off" => (true, "Kill switch off, people can connect again"),
        "saved" => (true, "Settings saved"),
        "reset" => (true, "Setting reset"),
        "password" => (true, "Relay password changed"),
        "password-out" => (true, "Relay password changed, everyone was signed out"),
        "password-none" => (true, "Relay password removed"),
        "quota" => (true, "Storage limit changed"),
        "disconnected" => (true, "Disconnected"),
        "banned" => (true, "Banned and disconnected"),
        "unbanned" => (true, "Unbanned"),
        "files" => (true, "Files deleted"),
        "bad-number" => (false, "Sizes must be whole numbers"),
        "bad-name" => (false, "The server name must be 1 to 40 characters"),
        "mismatch" => (false, "The two passwords don't match"),
        "empty" => (false, "Enter a password"),
        "needs-password" => (false, "Set a relay password before turning the relay on"),
        "2fa-off" => (true, "Two-factor sign-in turned off"),
        "bad-code" => (false, "Wrong code"),
        "too-many" => (false, "Too many tries. Wait a minute and try again."),
        _ => return None,
    };
    Some(Flash { ok, text })
}

pub(crate) async fn icon_png() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        ICON,
    )
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
    code: Option<String>,
}

async fn code_ok(app: &Arc<App>, code: String) -> bool {
    two_factor::verify(&app.db, &code, now())
        .await
        .unwrap_or(false)
}

async fn login(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Form(form): Form<LoginForm>,
) -> Response {
    let name = app.settings.get().name;
    let Some(expected) = app.admin_hash else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ip = app.client_ip(&headers, addr);
    let second = two_factor::is_on(&app.db).await;
    if app.admin_attempts.blocked(ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Html(page::login(
                &name,
                second,
                Some("Too many tries. Wait a minute and try again."),
            )),
        )
            .into_response();
    }
    let password_ok = same_secret(&form.password, &expected);
    let ok = password_ok && (!second || code_ok(&app, form.code.unwrap_or_default()).await);
    if !ok {
        app.admin_attempts.fail(ip);
        log::warn!("admin sign-in failed from {ip}");
        let error = if second {
            "Wrong password or code"
        } else {
            "Wrong password"
        };
        return (
            StatusCode::UNAUTHORIZED,
            Html(page::login(&name, second, Some(error))),
        )
            .into_response();
    }
    app.admin_attempts.clear(ip);
    let mut raw = [0u8; 32];
    getrandom::getrandom(&mut raw).expect("no system random source");
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    app.admin_sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            token_hash(&token),
            Instant::now() + Duration::from_secs(SESSION_HOURS * 3600),
        );
    let secure = app
        .config
        .public_url
        .as_deref()
        .is_some_and(|u| u.starts_with("https://"));
    let cookie = format!(
        "{COOKIE}={token}; Path=/admin; HttpOnly; SameSite=Strict; Max-Age={}{}",
        SESSION_HOURS * 3600,
        if secure { "; Secure" } else { "" }
    );
    let mut response = Redirect::to("/admin").into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie_token(&headers) {
        app.admin_sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&token_hash(&token));
    }
    let mut response = Redirect::to("/admin").into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "citrine_admin=; Path=/admin; HttpOnly; SameSite=Strict; Max-Age=0",
        ),
    );
    response
}

struct Numbers {
    online: usize,
    peak: u64,
    accounts: u64,
    sessions: usize,
    messages: u64,
    storage_used: u64,
    storage_total: u64,
    bytes_in: u64,
    bytes_out: u64,
    uptime: u64,
}

async fn numbers(app: &App) -> Numbers {
    let sessions = app.hub.session_count();
    let storage_used = app.db.total_storage().await.unwrap_or(0);
    let free = fs4::available_space(&app.config.data_dir).unwrap_or(0);
    Numbers {
        online: app.hub.online_count(),
        peak: app.stats.peak_today(),
        accounts: app.db.count_accounts().await.unwrap_or(0),
        sessions,
        messages: app.stats.messages_today(),
        storage_used,
        storage_total: storage_used + free,
        bytes_in: app.stats.bytes_in(),
        bytes_out: app.stats.bytes_out(),
        uptime: app.stats.uptime_secs(),
    }
}

async fn stats(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !signed_in(&app, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let n = numbers(&app).await;
    axum::Json(serde_json::json!({
        "online": n.online,
        "peak_today": n.peak,
        "accounts": n.accounts,
        "sessions": n.sessions,
        "messages_today": n.messages,
        "storage_used": n.storage_used,
        "storage_total": n.storage_total,
        "bytes_in": n.bytes_in,
        "bytes_out": n.bytes_out,
        "uptime": n.uptime,
    }))
    .into_response()
}

fn tile(label: &str, key: &str, value: &str, sub: Option<(&str, String)>) -> String {
    let sub = sub
        .map(|(k, s)| format!(r#"<span class="sub" data-stat="{k}">{s}</span>"#))
        .unwrap_or_default();
    format!(
        r#"<div class="tile"><span class="label">{label}</span><span class="value" data-stat="{key}">{value}</span>{sub}</div>"#
    )
}

fn confirm(id: &str, title: &str, text: &str, action: &str, button: &str, fields: &str) -> String {
    format!(
        r#"<div id="{id}" popover><h3>{title}</h3><p>{text}</p><form method="post" action="{action}" class="buttons">{fields}<button type="button" class="btn" popovertarget="{id}" popovertargetaction="hide">Cancel</button><button type="submit" class="btn solid-danger">{button}</button></form></div>"#
    )
}

async fn dashboard(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(done): Query<Done>,
) -> Response {
    let s = app.settings.get();
    if !signed_in(&app, &headers) {
        return Html(page::login(&s.name, two_factor::is_on(&app.db).await, None)).into_response();
    }
    let n = numbers(&app).await;
    let mut body = String::new();
    if s.kill {
        let _ = write!(
            body,
            r#"<div class="banner">{}<span>Kill switch is on. Nobody can connect.</span><form method="post" action="/admin/kill"><input type="hidden" name="on" value="off"><button class="btn" type="submit">Turn off</button></form></div>"#,
            icon("kill")
        );
    }
    let chip = |on: bool, label: &str, ic: &str| {
        format!(
            r#"<span class="chip{}">{}{label} {}</span>"#,
            if on { " on" } else { "" },
            icon(ic),
            if on { "on" } else { "off" }
        )
    };
    let address = app
        .config
        .public_url
        .as_deref()
        .map(|url| {
            format!(
                r#"<div class="address"><span class="mono">{}</span><button type="button" class="btn small" data-copy="{}">{}<span>Copy</span></button></div>"#,
                esc(url),
                esc(url),
                icon("copy")
            )
        })
        .unwrap_or_default();
    let web_missing = app.config.missing_web_files();
    if let Some(dir) = &web_missing {
        let _ = write!(
            body,
            r#"<div class="note" style="margin:0">{}{}</div>"#,
            icon("alert"),
            web_missing_steps(dir),
        );
    }
    let relay_on = s.relay && !s.kill;
    let relay_label = if relay_on {
        "Relay on"
    } else if s.kill {
        "Relay stopped by the kill switch"
    } else {
        "Relay off"
    };
    let can_turn_on = s.password_set || app.settings.relay_open_allowed();
    let _ = write!(
        body,
        r#"<section class="card"><div class="status"><div class="chips">{}{}<span class="chip{}">{}{relay_label}</span><span class="chip">{}Sign-ups {}</span></div>{address}</div></section>"#,
        if web_missing.is_some() {
            format!(
                r#"<span class="chip warn">{}Web app on</span>"#,
                icon("globe")
            )
        } else {
            chip(app.config.web, "Web app", "globe")
        },
        if s.password_set {
            String::new()
        } else if s.relay {
            r#"<span class="chip warn">Open relay, no password</span>"#.to_string()
        } else {
            String::new()
        },
        if relay_on { " on" } else { "" },
        icon("radio"),
        icon("user-plus"),
        match (s.sign_ups, s.invites) {
            (false, _) => "closed",
            (true, false) => "open",
            (true, true) => "by invite",
        },
    );
    let relay_toggle = if s.relay {
        format!(
            r#"<div class="segmented" role="group" aria-label="Relay"><button type="button" class="btn" aria-pressed="true">{}On</button><button type="button" class="btn" popovertarget="relay-off" aria-pressed="false">Off</button></div>{}"#,
            icon("power"),
            confirm(
                "relay-off",
                "Turn the relay off?",
                &format!(
                    "{} online now will be disconnected and told the relay is off.",
                    people(n.online)
                ),
                "/admin/relay",
                "Turn off",
                r#"<input type="hidden" name="on" value="off">"#
            )
        )
    } else {
        format!(
            r#"<form method="post" action="/admin/relay" class="segmented" role="group" aria-label="Relay"><input type="hidden" name="on" value="on"><button type="submit" class="btn" aria-pressed="false"{}>{}On</button><button type="button" class="btn" aria-pressed="true">Off</button></form>"#,
            if can_turn_on { "" } else { " disabled" },
            icon("power")
        )
    };
    let kill = if s.kill {
        String::new()
    } else {
        format!(
            r#"<button type="button" class="btn danger" popovertarget="kill-on">{}Kill switch</button>{}"#,
            icon("kill"),
            confirm(
                "kill-on",
                "Turn on the kill switch?",
                "Everyone is disconnected and new connections are refused until you turn it off here.",
                "/admin/kill",
                "Disconnect everyone",
                r#"<input type="hidden" name="on" value="on">"#
            )
        )
    };
    let _ = write!(
        body,
        r#"<section class="card"><h2>{} Relay</h2><div class="controls">{relay_toggle}{kill}</div></section>"#,
        icon("radio")
    );
    let fraction = if n.storage_total > 0 {
        (n.storage_used as f64 / n.storage_total as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let _ = write!(
        body,
        r#"<section class="tiles" data-live>{}{}{}{}{}<div class="tile"><span class="label">Storage</span><span class="value" data-stat="storage">{}</span><span class="sub" data-stat="storage_total">of {}</span><div class="bar"><i data-stat="storage_bar" style="width:{fraction:.1}%"></i></div></div>{}{}<div class="tile"><span class="label">Uptime</span><span class="value" data-stat="uptime" data-seconds="{}">{}</span></div><div class="tile"><span class="label">Version</span><span class="value">{}</span></div></section>"#,
        tile("Online now", "online", &n.online.to_string(), None),
        tile("Peak today", "peak", &n.peak.to_string(), None),
        tile("Accounts", "accounts", &n.accounts.to_string(), None),
        tile("Play sessions", "sessions", &n.sessions.to_string(), None),
        tile("Messages today", "messages", &n.messages.to_string(), None),
        bytes(n.storage_used),
        bytes(n.storage_total),
        tile("Received", "bytes_in", &bytes(n.bytes_in), None),
        tile("Sent", "bytes_out", &bytes(n.bytes_out), None),
        n.uptime,
        duration(n.uptime),
        env!("CARGO_PKG_VERSION"),
    );
    Html(page::page(&s.name, Tab::Dashboard, flash(&done), &body)).into_response()
}

fn people(n: usize) -> String {
    match n {
        0 => "Nobody is".into(),
        1 => "1 person".into(),
        n => format!("{n} people"),
    }
}

#[derive(Deserialize)]
struct OnForm {
    on: String,
}

async fn relay(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<OnForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let on = form.on == "on";
    if on && !app.settings.get().password_set && !app.settings.relay_open_allowed() {
        return back("/admin", "needs-password");
    }
    if app.settings.set_flag(settings::RELAY, on).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if !on {
        app.hub.close_all(Notice::RelayOff);
    }
    log::info!(
        "relay turned {} in the admin panel",
        if on { "on" } else { "off" }
    );
    back("/admin", if on { "relay-on" } else { "relay-off" })
}

async fn kill(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<OnForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let on = form.on == "on";
    let result = if on {
        app.settings.set_flag(settings::KILL, true).await
    } else {
        app.settings.reset(settings::KILL).await
    };
    if result.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if on {
        app.hub.close_all(Notice::KillSwitch);
    }
    log::warn!("kill switch turned {}", if on { "on" } else { "off" });
    back("/admin", if on { "kill-on" } else { "kill-off" })
}

async fn users(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<Done>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let s = app.settings.get();
    let search = query.q.clone().unwrap_or_default();
    let search = search.trim();
    let page_no = query.page.unwrap_or(1).max(1);
    let Ok((rows, total)) = app
        .db
        .users(search, (page_no - 1) * PAGE_SIZE, PAGE_SIZE)
        .await
    else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let online = app.hub.online();
    let now = now();
    let mut body = String::new();
    let _ = write!(
        body,
        r#"<div class="toolbar"><form method="get" action="/admin/users" role="search"><div class="search">{}<input type="search" name="q" value="{}" placeholder="Name or online ID" aria-label="Search users"></div><button class="btn" type="submit">Search</button></form><span class="count">{}</span></div>"#,
        icon("search"),
        esc(search),
        match total {
            1 => "1 account".to_string(),
            n => format!("{n} accounts"),
        }
    );
    let mut list = String::from(
        r#"<section class="card users"><div class="user-row head"><span>Name</span><span>Online ID</span><span>Last seen</span><span>Storage</span><span></span></div>"#,
    );
    if rows.is_empty() {
        list.push_str(if search.is_empty() {
            r#"<p class="empty">No accounts yet</p>"#
        } else {
            r#"<p class="empty">No account matches</p>"#
        });
    }
    let here = format!(
        "/admin/users?q={}&page={page_no}",
        search
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b == b'-' {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            })
            .collect::<String>()
    );
    for row in rows {
        let a = &row.account;
        let id = a.id;
        let name = esc(&a.display_name);
        let oid = a.online_id.to_string();
        let is_online = online.contains(&id);
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
        let seen = if is_online {
            r#"<span class="seen online"><span class="dot"></span>Online</span>"#.to_string()
        } else {
            format!(
                r#"<span class="seen muted">{}</span>"#,
                ago(a.last_seen, now)
            )
        };
        let quota_text = if quota == 0 {
            "no file sharing".to_string()
        } else {
            bytes(quota)
        };
        let ret = esc(&here);
        let ban = if a.banned {
            format!(
                r#"<form method="post" action="/admin/users/{id}/unban"><input type="hidden" name="back" value="{ret}"><button class="btn" type="submit">{}Unban</button></form>"#,
                icon("ban")
            )
        } else {
            format!(
                r#"<button type="button" class="btn danger" popovertarget="ban-{id}">{}Ban</button>"#,
                icon("ban")
            )
        };
        let files_label = match row.files {
            0 => "Delete files".to_string(),
            1 => format!("Delete 1 file, {}", bytes(row.storage_used)),
            n => format!("Delete {n} files, {}", bytes(row.storage_used)),
        };
        let _ = write!(
            list,
            r#"<div class="user-row"><div class="who">{avatar}<div style="min-width:0"><div class="name">{name} {banned}</div><span class="mono muted">{oid}</span></div></div><span class="mono oid">{oid}</span>{seen}<div class="storage">{used} of {quota_text}<div class="bar"><i style="width:{fraction:.1}%"></i></div></div><div class="actions"><button type="button" class="btn small" popovertarget="manage-{id}">{more}<span>Manage</span></button></div></div>
<div id="manage-{id}" popover><div class="who">{avatar}<div style="min-width:0"><div class="name">{name} {banned}</div><span class="mono muted">{oid}</span></div></div><div class="menu"><form method="post" action="/admin/users/{id}/quota" class="inline"><input type="hidden" name="back" value="{ret}"><input type="number" name="quota_mb" min="0" step="1" value="{quota_value}" placeholder="{default_mb} MB (default)" aria-label="Storage limit in MB"><button class="btn" type="submit">{drive}Set limit</button></form><form method="post" action="/admin/users/{id}/disconnect"><input type="hidden" name="back" value="{ret}"><button class="btn" type="submit"{offline}>{unplug}Disconnect</button></form>{ban}<button type="button" class="btn danger" popovertarget="files-{id}"{no_files}>{trash}{files_label}</button></div></div>
{ban_confirm}{files_confirm}"#,
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
            offline = if is_online { "" } else { " disabled" },
            no_files = if row.files == 0 { " disabled" } else { "" },
            ban_confirm = if a.banned {
                String::new()
            } else {
                confirm(
                    &format!("ban-{id}"),
                    &format!("Ban {name}?"),
                    "They're disconnected now and can't sign in until you unban them.",
                    &format!("/admin/users/{id}/ban"),
                    "Ban",
                    &format!(r#"<input type="hidden" name="back" value="{ret}">"#),
                )
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
                &format!(r#"<input type="hidden" name="back" value="{ret}">"#),
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
                    r#"<a class="btn" href="/admin/users?q={}&page={p}">{}{label}</a>"#,
                    esc(search),
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
    Html(page::page(&s.name, Tab::Users, flash(&query), &body)).into_response()
}

#[derive(Deserialize)]
struct UserForm {
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

async fn user_quota(
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

async fn user_disconnect(
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

async fn user_ban(
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

async fn user_unban(
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

async fn user_files(
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

fn source_chip(app: &App, key: &str) -> String {
    match app.settings.source(key) {
        Source::Default => r#"<span class="chip">Default</span>"#.to_string(),
        Source::Env => r#"<span class="chip">From the environment</span>"#.to_string(),
        Source::Panel => format!(
            r#"<span class="chip panel">Set here</span><button type="submit" form="reset" name="key" value="{key}" class="btn ghost small">{}Reset</button>"#,
            icon("reset")
        ),
    }
}

fn row(label_html: &str, chip: &str, control: &str) -> String {
    format!(
        r#"<div class="field-row"><div><div class="field"><span class="label">{label_html}</span></div><div class="source">{chip}</div></div>{control}</div>"#
    )
}

async fn settings_page(
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

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn code_field(id: &str, label: &str, focus: bool) -> String {
    format!(
        r#"<div class="field"><label for="{id}">{label}</label><input id="{id}" name="code" type="text" inputmode="numeric" autocomplete="one-time-code" maxlength="20" required{}></div>"#,
        if focus { " autofocus" } else { "" }
    )
}

fn code_popover(
    id: &str,
    title: &str,
    text: &str,
    action: &str,
    button: &str,
    danger: bool,
) -> String {
    format!(
        r#"<div id="{id}" popover><h3>{title}</h3><p>{text}</p><form method="post" action="{action}">{}<div class="buttons"><button type="button" class="btn" popovertarget="{id}" popovertargetaction="hide">Cancel</button><button type="submit" class="btn {}">{button}</button></div></form></div>"#,
        code_field(
            &format!("{id}-code"),
            "Code from the app or a recovery code",
            true
        ),
        if danger { "solid-danger" } else { "primary" },
    )
}

async fn two_factor_card(app: &App, server: &str, setup: bool, error: Option<&str>) -> String {
    let title = format!(
        r#"<h2>{} Two-factor sign-in{}</h2>"#,
        icon("shield"),
        hint(
            "hint-2fa",
            "Signing in asks for a code from an authenticator app on your phone as well as the password."
        )
    );
    let failed = error.is_some();
    let error = error
        .map(|e| {
            format!(
                r#"<p class="note" role="alert">{}<span>{e}</span></p>"#,
                icon("alert")
            )
        })
        .unwrap_or_default();
    let reset_note = if app.config.admin_2fa_reset {
        format!(
            r#"<p class="note">{}<span>CITRINE_ADMIN_2FA_RESET is still set. Remove it, or the next start turns two-factor sign-in off.</span></p>"#,
            icon("alert")
        )
    } else {
        String::new()
    };
    if two_factor::is_on(&app.db).await {
        let left = two_factor::codes_left(&app.db).await;
        let left = match left {
            0 => r#"<span class="chip warn">No recovery codes left</span>"#.to_string(),
            1 => r#"<span class="muted">1 recovery code left</span>"#.to_string(),
            n => format!(r#"<span class="muted">{n} recovery codes left</span>"#),
        };
        return format!(
            r#"<section class="card" id="two-factor">{title}<div class="source"><span class="chip on">{}On</span>{left}</div>{reset_note}{error}<div class="controls"><button type="button" class="btn" popovertarget="tf-codes">{}New recovery codes</button><button type="button" class="btn danger" popovertarget="tf-off">{}Turn off</button></div>{}{}</section>"#,
            icon("check"),
            icon("reset"),
            icon("power"),
            code_popover(
                "tf-codes",
                "Make new recovery codes?",
                "Your old recovery codes stop working.",
                "/admin/two-factor/codes",
                "Make new codes",
                false
            ),
            code_popover(
                "tf-off",
                "Turn off two-factor sign-in?",
                "Signing in will ask for the password only.",
                "/admin/two-factor/off",
                "Turn off",
                true
            ),
        );
    }
    if !setup {
        return format!(
            r#"<section class="card" id="two-factor">{title}<div class="source"><span class="chip">Off</span></div>{reset_note}<div class="controls"><a class="btn primary" href="/admin/settings?setup=two-factor#two-factor">{}Set up</a></div></section>"#,
            icon("shield")
        );
    }
    let secret = app
        .admin_setup
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(two_factor::new_secret)
        .clone();
    let key = two_factor::key_text(&secret);
    format!(
        r#"<section class="card" id="two-factor">{title}<div class="tf-setup"><div class="qr">{}</div><div class="tf-steps"><p>Scan the QR code with your authenticator app, then enter the 6-digit code it shows.</p><div class="field"><span class="label">Setup key</span><div class="tf-key"><span class="mono" id="tf-key">{}</span><button type="button" class="btn small" data-copy="{}">{}<span>Copy</span></button></div></div><form method="post" action="/admin/two-factor/on" id="tf-on">{}</form>{error}</div></div><div class="form-foot"><a class="btn" href="/admin/settings" data-esc>Cancel</a><button class="btn primary" type="submit" form="tf-on">Turn on</button></div></section>"#,
        two_factor::qr_svg(&secret, server),
        esc(&key),
        esc(&key.replace(' ', "")),
        icon("copy"),
        code_field("tf-code", "Code from the app", failed),
    )
}

fn recovery_page(app: &App, codes: &[String], flash_text: &'static str) -> Response {
    let s = app.settings.get();
    let list: String = codes
        .iter()
        .map(|c| format!("<li>{}</li>", esc(c)))
        .collect();
    let text = format!(
        "Recovery codes for the {} admin panel. Each one signs you in once.\n\n{}\n",
        s.name,
        codes.join("\n")
    );
    let href: String = text
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'.' {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let body = format!(
        r#"<section class="card" id="two-factor"><h2>{} Recovery codes</h2><p class="lead">Each code signs you in once if you lose your phone. Keep them somewhere safe. They won't be shown again.</p><ol class="codes mono">{list}</ol><div class="form-foot"><button type="button" class="btn" data-copy="{}">{}<span>Copy</span></button><a class="btn" download="citrine-recovery-codes.txt" href="data:text/plain;charset=utf-8,{href}">{}Download</a><a class="btn primary" href="/admin/settings#two-factor">Done</a></div></section>"#,
        icon("key"),
        esc(&codes.join("\n")),
        icon("copy"),
        icon("download"),
    );
    no_store(
        Html(page::page(
            &s.name,
            Tab::Settings,
            Some(Flash {
                ok: true,
                text: flash_text,
            }),
            &body,
        ))
        .into_response(),
    )
}

#[derive(Deserialize)]
struct CodeForm {
    code: String,
}

async fn two_factor_on(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<CodeForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if two_factor::is_on(&app.db).await {
        return Redirect::to("/admin/settings#two-factor").into_response();
    }
    let Some(secret) = app
        .admin_setup
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    else {
        return Redirect::to("/admin/settings?setup=two-factor#two-factor").into_response();
    };
    let Some(step) = two_factor::step_of(&secret, &form.code, now()) else {
        return Redirect::to("/admin/settings?setup=two-factor&done=bad-code#two-factor")
            .into_response();
    };
    let (codes, hashes) = tokio::task::spawn_blocking(two_factor::new_recovery_codes)
        .await
        .unwrap_or_default();
    if hashes.len() != two_factor::RECOVERY_CODES
        || app.db.set_admin_totp(&secret, step, &hashes).await.is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    *app.admin_setup.lock().unwrap_or_else(|e| e.into_inner()) = None;
    log::info!("two-factor sign-in for the admin panel turned on");
    recovery_page(&app, &codes, "Two-factor sign-in is on")
}

async fn refuse_code(
    app: &Arc<App>,
    headers: &HeaderMap,
    addr: SocketAddr,
    code: String,
) -> Option<Response> {
    if let Some(r) = guard(app, headers) {
        return Some(r);
    }
    if !two_factor::is_on(&app.db).await {
        return Some(Redirect::to("/admin/settings#two-factor").into_response());
    }
    let ip = app.client_ip(headers, addr);
    if app.admin_attempts.blocked(ip) {
        return Some(back_to_card("too-many"));
    }
    if !code_ok(app, code).await {
        app.admin_attempts.fail(ip);
        log::warn!("wrong two-factor code in the admin panel from {ip}");
        return Some(back_to_card("bad-code"));
    }
    None
}

fn back_to_card(flash: &str) -> Response {
    Redirect::to(&format!("/admin/settings?done={flash}#two-factor")).into_response()
}

async fn two_factor_off(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Form(form): Form<CodeForm>,
) -> Response {
    if let Some(r) = refuse_code(&app, &headers, addr, form.code).await {
        return r;
    }
    if app.db.clear_admin_totp().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    log::info!("two-factor sign-in for the admin panel turned off");
    back_to_card("2fa-off")
}

async fn two_factor_codes(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Form(form): Form<CodeForm>,
) -> Response {
    if let Some(r) = refuse_code(&app, &headers, addr, form.code).await {
        return r;
    }
    let (codes, hashes) = tokio::task::spawn_blocking(two_factor::new_recovery_codes)
        .await
        .unwrap_or_default();
    if hashes.len() != two_factor::RECOVERY_CODES
        || app.db.replace_admin_recovery_codes(&hashes).await.is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    log::info!("new recovery codes made for the admin panel");
    recovery_page(
        &app,
        &codes,
        "New recovery codes made, the old ones no longer work",
    )
}

#[derive(Deserialize)]
struct SettingsForm {
    name: String,
    sign_ups: Option<String>,
    invites: Option<String>,
    pictures: Option<String>,
    quota_mb: String,
    max_message_kb: String,
    max_file_mb: String,
}

async fn settings_save(
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
    let changes: [(&str, bool, String); 7] = [
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
            settings::QUOTA,
            quota << 20 != s.quota_bytes,
            quota.to_string(),
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
struct ResetForm {
    key: String,
}

async fn settings_reset(
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
        settings::QUOTA,
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
struct PasswordForm {
    password: Option<String>,
    repeat: Option<String>,
    sign_out: Option<String>,
    remove: Option<String>,
}

async fn password(
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
