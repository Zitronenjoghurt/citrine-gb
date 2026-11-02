use super::page;
use super::page::{Flash, Tab, esc, hint, icon};
use super::{code_ok, guard, no_store};
use crate::db::{self, Db};
use crate::two_factor::{clean, recovery_code, step_of};
use crate::{App, now};
use axum::extract::{ConnectInfo, Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::response::{Html, Redirect, Response};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;

pub async fn is_on(db: &Db) -> bool {
    matches!(db.admin_totp().await, Ok(Some(_)))
}

pub async fn codes_left(db: &Db) -> usize {
    db.admin_recovery_codes()
        .await
        .map(|c| c.len())
        .unwrap_or(0)
}

pub async fn verify(db: &Db, code: &str, now: u64) -> db::Result<bool> {
    let Some(secret) = db.admin_totp().await? else {
        return Ok(false);
    };
    let Some(digits) = clean(code) else {
        return Ok(false);
    };
    match digits.len() {
        6 => match step_of(&secret, &digits, now) {
            Some(step) => db.use_admin_step(step).await,
            None => Ok(false),
        },
        12 => match recovery_code(db.admin_recovery_codes().await?, digits).await {
            Some(id) => db.use_admin_recovery_code(id).await,
            None => Ok(false),
        },
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::two_factor::{RECOVERY_CODES, new_recovery_codes, new_secret};

    const NOW: u64 = 1_761_782_415;

    async fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).await.unwrap();
        (dir, db)
    }

    fn code(secret: &[u8], at: u64) -> String {
        crate::two_factor::totp(secret, "").generate(at).to_string()
    }

    async fn turned_on(db: &Db) -> (Vec<u8>, Vec<String>) {
        let secret = new_secret();
        let step = step_of(&secret, &code(&secret, NOW - 60), NOW - 60).unwrap();
        let (codes, hashes) = new_recovery_codes();
        db.set_admin_totp(&secret, step, &hashes).await.unwrap();
        (secret, codes)
    }

    #[tokio::test]
    async fn one_step_of_drift_each_way() {
        let (_dir, db) = db().await;
        let (secret, _) = turned_on(&db).await;
        assert!(!verify(&db, &code(&secret, NOW - 60), NOW).await.unwrap());
        assert!(verify(&db, &code(&secret, NOW - 30), NOW).await.unwrap());
        assert!(verify(&db, &code(&secret, NOW), NOW).await.unwrap());
        assert!(verify(&db, &code(&secret, NOW + 30), NOW).await.unwrap());
        assert!(!verify(&db, &code(&secret, NOW + 60), NOW).await.unwrap());
    }

    #[tokio::test]
    async fn a_used_code_is_refused() {
        let (_dir, db) = db().await;
        let (secret, _) = turned_on(&db).await;
        let now = code(&secret, NOW);
        assert!(verify(&db, &now, NOW).await.unwrap());
        assert!(!verify(&db, &now, NOW).await.unwrap());
        assert!(!verify(&db, &now, NOW + 20).await.unwrap());
        assert!(!verify(&db, &code(&secret, NOW - 30), NOW).await.unwrap());
        let spaced = code(&secret, NOW + 30);
        let spaced = format!("{} {}", &spaced[..3], &spaced[3..]);
        assert!(verify(&db, &spaced, NOW + 30).await.unwrap());
    }

    #[tokio::test]
    async fn a_recovery_code_works_once() {
        let (_dir, db) = db().await;
        let (_, codes) = turned_on(&db).await;
        assert_eq!(codes.len(), RECOVERY_CODES);
        assert_eq!(codes_left(&db).await, RECOVERY_CODES);
        assert!(codes.iter().all(|c| c.len() == 14));
        assert!(verify(&db, &codes[3], NOW).await.unwrap());
        assert!(!verify(&db, &codes[3], NOW).await.unwrap());
        assert_eq!(codes_left(&db).await, RECOVERY_CODES - 1);
        assert!(verify(&db, &codes[4].replace('-', ""), NOW).await.unwrap());
        assert!(!verify(&db, "1234-5678-9012", NOW).await.unwrap());
        assert!(!verify(&db, "abcdef", NOW).await.unwrap());
        assert!(!verify(&db, "", NOW).await.unwrap());
    }

    #[tokio::test]
    async fn off_accepts_nothing_and_clearing_turns_it_off() {
        let (_dir, db) = db().await;
        assert!(!is_on(&db).await);
        assert!(!verify(&db, "123456", NOW).await.unwrap());
        turned_on(&db).await;
        assert!(is_on(&db).await);
        assert!(db.clear_admin_totp().await.unwrap());
        assert!(!is_on(&db).await);
        assert_eq!(codes_left(&db).await, 0);
        assert!(!db.clear_admin_totp().await.unwrap());
    }
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

pub(super) async fn two_factor_card(
    app: &App,
    server: &str,
    setup: bool,
    error: Option<&str>,
) -> String {
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
    if is_on(&app.db).await {
        let left = codes_left(&app.db).await;
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
        .get_or_insert_with(crate::two_factor::new_secret)
        .clone();
    let key = crate::two_factor::key_text(&secret);
    format!(
        r#"<section class="card" id="two-factor">{title}<div class="tf-setup"><div class="qr">{}</div><div class="tf-steps"><p>Scan the QR code with your authenticator app, then enter the 6-digit code it shows.</p><div class="field"><span class="label">Setup key</span><div class="tf-key"><span class="mono" id="tf-key">{}</span><button type="button" class="btn small" data-copy="{}">{}<span>Copy</span></button></div></div><form method="post" action="/admin/two-factor/on" id="tf-on">{}</form>{error}</div></div><div class="form-foot"><a class="btn" href="/admin/settings" data-esc>Cancel</a><button class="btn primary" type="submit" form="tf-on">Turn on</button></div></section>"#,
        crate::two_factor::qr_svg(&secret, server),
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
pub(super) struct CodeForm {
    code: String,
}

pub(super) async fn two_factor_on(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<CodeForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if is_on(&app.db).await {
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
    let Some(step) = crate::two_factor::step_of(&secret, &form.code, now()) else {
        return Redirect::to("/admin/settings?setup=two-factor&done=bad-code#two-factor")
            .into_response();
    };
    let (codes, hashes) = tokio::task::spawn_blocking(crate::two_factor::new_recovery_codes)
        .await
        .unwrap_or_default();
    if hashes.len() != crate::two_factor::RECOVERY_CODES
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
    if !is_on(&app.db).await {
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

pub(super) async fn two_factor_off(
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

pub(super) async fn two_factor_codes(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Form(form): Form<CodeForm>,
) -> Response {
    if let Some(r) = refuse_code(&app, &headers, addr, form.code).await {
        return r;
    }
    let (codes, hashes) = tokio::task::spawn_blocking(crate::two_factor::new_recovery_codes)
        .await
        .unwrap_or_default();
    if hashes.len() != crate::two_factor::RECOVERY_CODES
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
