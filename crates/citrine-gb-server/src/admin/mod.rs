mod dashboard;
mod invites;
mod page;
mod settings_page;
mod two_factor;
mod users;

use dashboard::{dashboard, kill, relay, sign_ups, stats};
use settings_page::{password, settings_page, settings_reset, settings_save};
use two_factor::{two_factor_codes, two_factor_off, two_factor_on};
use users::{
    ban_new, unblock, user_ban, user_block, user_delete, user_device_sign_out, user_disconnect,
    user_files, user_quota, user_two_factor_off, user_unban, users,
};

use crate::settings;
use crate::settings::same_secret;
use crate::{App, now};
use axum::Router;
use axum::extract::{ConnectInfo, Form, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use page::Flash;
pub(crate) use page::web_missing_page;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

const COOKIE: &str = "citrine_admin";
const SESSION_HOURS: u64 = 12;
const PAGE_SIZE: u64 = 50;
const BLOCK_FOR: Duration = Duration::from_secs(24 * 3600);
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
        .route("/sign-ups", post(sign_ups))
        .route("/users", get(users))
        .route("/users/{id}/quota", post(user_quota))
        .route("/users/{id}/disconnect", post(user_disconnect))
        .route("/users/{id}/ban", post(user_ban))
        .route("/users/{id}/unban", post(user_unban))
        .route("/users/{id}/files", post(user_files))
        .route("/users/{id}/block", post(user_block))
        .route("/users/{id}/delete", post(user_delete))
        .route("/users/{id}/two-factor-off", post(user_two_factor_off))
        .route(
            "/users/{id}/devices/{device}/sign-out",
            post(user_device_sign_out),
        )
        .route("/users/ban-new", post(ban_new))
        .route("/blocks/unblock", post(unblock))
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
    setup: Option<String>,
}

fn flash(done: &Done) -> Option<Flash> {
    let (ok, text) = match done.done.as_deref()? {
        "relay-on" => (true, "Relay turned on"),
        "relay-off" => (true, "Relay turned off, everyone was disconnected"),
        "kill-on" => (true, "Kill switch on, everyone was disconnected"),
        "kill-off" => (true, "Kill switch off, people can connect again"),
        "sign-ups-off" => (true, "Sign-ups turned off"),
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

fn confirm(id: &str, title: &str, text: &str, action: &str, button: &str, fields: &str) -> String {
    format!(
        r#"<div id="{id}" popover><h3>{title}</h3><p>{text}</p><form method="post" action="{action}" class="buttons">{fields}<button type="button" class="btn" popovertarget="{id}" popovertargetaction="hide">Cancel</button><button type="submit" class="btn solid-danger">{button}</button></form></div>"#
    )
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
