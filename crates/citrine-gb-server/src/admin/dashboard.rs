use super::page;
use super::page::{Tab, bytes, duration, esc, hint, icon, web_missing_steps};
use super::two_factor;
use super::{Done, back, confirm, flash, guard, signed_in};
use crate::stats::{Refused, RefusedCounts};
use crate::storage::Storage;
use crate::{App, settings};
use axum::extract::{Form, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::response::{Html, Response};
use citrine_gb_net::Notice;
use serde::Deserialize;
use std::fmt::Write;
use std::sync::Arc;

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
    database: u64,
    waiting: u64,
    waiting_bytes: u64,
    refused: RefusedCounts,
}

async fn database_size(app: &App) -> u64 {
    let wal = app.config.data_dir.join("citrine.db-wal");
    let wal = tokio::fs::metadata(wal).await.map_or(0, |m| m.len());
    app.db.size().await.unwrap_or(0) + wal
}

async fn numbers(app: &App) -> Numbers {
    let sessions = app.hub.session_count();
    let storage_used = Storage::used(&app.db).await.unwrap_or(0);
    let cap = app.settings.get().storage_total_bytes;
    let storage_total = app
        .storage
        .disk_room()
        .map_or(cap, |room| cap.min(storage_used.saturating_add(room)));
    let (waiting, waiting_bytes) = app.db.waiting_messages().await.unwrap_or((0, 0));
    Numbers {
        online: app.hub.online_count(),
        peak: app.stats.peak_today(),
        accounts: app.db.count_accounts().await.unwrap_or(0),
        sessions,
        messages: app.stats.messages_today(),
        storage_used,
        storage_total,
        bytes_in: app.stats.bytes_in(),
        bytes_out: app.stats.bytes_out(),
        uptime: app.stats.uptime_secs(),
        database: database_size(app).await,
        waiting,
        waiting_bytes,
        refused: app.stats.refused_last_hour(),
    }
}

pub(super) async fn stats(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !signed_in(&app, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let n = numbers(&app).await;
    let refused: serde_json::Map<String, serde_json::Value> = Refused::ALL
        .iter()
        .zip(n.refused)
        .map(|(kind, count)| (kind.key().to_string(), count.into()))
        .collect();
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
        "database": n.database,
        "waiting": n.waiting,
        "waiting_bytes": n.waiting_bytes,
        "refused": refused,
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

pub(super) async fn dashboard(
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
    let sign_ups_refused = n.refused[Refused::SignUps.index()];
    if s.sign_ups && sign_ups_refused > 0 {
        let _ = write!(
            body,
            r#"<div class="banner">{}<span>{} in the last hour.</span><form method="post" action="/admin/sign-ups"><input type="hidden" name="on" value="off"><button class="btn" type="submit">Turn off sign-ups</button></form></div>"#,
            icon("user-plus"),
            match sign_ups_refused {
                1 => "1 sign-up refused".to_string(),
                n => format!("{n} sign-ups refused"),
            }
        );
    }
    let storage_refused = n.refused[Refused::RelayStorage.index()];
    if storage_refused > 0 {
        let _ = write!(
            body,
            r#"<div class="banner">{}<span>The relay is out of storage and refused {} in the last hour.</span></div>"#,
            icon("drive"),
            match storage_refused {
                1 => "1 upload, save or chat message".to_string(),
                n => format!("{n} uploads, saves or chat messages"),
            }
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
    let fingerprint = app.relay_key().fingerprint();
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
    let with_key = app
        .config
        .public_url
        .as_deref()
        .map(|url| format!("{}#{}", url.trim_end_matches('/'), fingerprint.compact()))
        .unwrap_or_else(|| fingerprint.to_string());
    let address = format!(
        r#"{address}<div class="address"><span class="muted">Relay key</span><span class="mono">{fingerprint}</span><button type="button" class="btn small" data-copy="{}">{}<span>{}</span></button></div>"#,
        esc(&with_key),
        icon("copy"),
        if app.config.public_url.is_some() {
            "Copy with address"
        } else {
            "Copy"
        }
    );
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
        r#"<section class="card"><div class="status"><div class="chips">{}{}<span class="chip{}">{}{relay_label}</span><span class="chip">{}Sign-ups {}</span>{}</div>{address}</div></section>"#,
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
        match (app.config.game_sharing, s.game_sharing) {
            (false, _) => String::new(),
            (true, true) => chip(true, "Game sharing", "radio"),
            (true, false) => format!(
                r#"<span class="chip warn">{}Game sharing waits for invite codes</span>"#,
                icon("radio")
            ),
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
        r#"<section class="tiles" data-live>{}{}{}{}{}<div class="tile"><span class="label">Storage</span><span class="value" data-stat="storage">{}</span><span class="sub" data-stat="storage_total">of {}</span><div class="bar"><i data-stat="storage_bar" style="width:{fraction:.1}%"></i></div></div>{}{}{}{}<div class="tile"><span class="label">Uptime</span><span class="value" data-stat="uptime" data-seconds="{}">{}</span></div><div class="tile"><span class="label">Version</span><span class="value">{}</span></div></section>"#,
        tile("Online now", "online", &n.online.to_string(), None),
        tile("Peak today", "peak", &n.peak.to_string(), None),
        tile("Accounts", "accounts", &n.accounts.to_string(), None),
        tile("Play sessions", "sessions", &n.sessions.to_string(), None),
        tile("Messages today", "messages", &n.messages.to_string(), None),
        bytes(n.storage_used),
        bytes(n.storage_total),
        tile("Received", "bytes_in", &bytes(n.bytes_in), None),
        tile("Sent", "bytes_out", &bytes(n.bytes_out), None),
        tile("Database", "database", &bytes(n.database), None),
        tile(
            "Waiting messages",
            "waiting",
            &n.waiting.to_string(),
            Some(("waiting_bytes", bytes(n.waiting_bytes)))
        ),
        n.uptime,
        duration(n.uptime),
        env!("CARGO_PKG_VERSION"),
    );
    body.push_str(&refused_card(&n.refused));
    Html(page::page(&s.name, Tab::Dashboard, flash(&done), &body)).into_response()
}

fn refused_card(counts: &RefusedCounts) -> String {
    let rows: String = Refused::ALL
        .iter()
        .zip(counts)
        .map(|(kind, count)| {
            format!(
                r#"<div class="refused-row{}" data-refused="{}"><span>{}</span><span class="value" data-stat="refused_{}">{count}</span></div>"#,
                if *count == 0 { " none" } else { "" },
                kind.key(),
                kind.label(),
                kind.key(),
            )
        })
        .collect();
    format!(
        r#"<section class="card"><h2>{} Refused in the last hour{}</h2><div class="refused">{rows}</div></section>"#,
        icon("shield"),
        hint(
            "hint-refused",
            "What the relay's limits turned away. A count that jumps means someone is pushing at a limit. The server's README lists every limit and its variable."
        ),
    )
}

fn people(n: usize) -> String {
    match n {
        0 => "Nobody is".into(),
        1 => "1 person".into(),
        n => format!("{n} people"),
    }
}

#[derive(Deserialize)]
pub(super) struct OnForm {
    on: String,
}

pub(super) async fn relay(
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

pub(super) async fn kill(
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

pub(super) async fn sign_ups(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<OnForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if form.on == "on" {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if app
        .settings
        .set_flag(settings::SIGN_UPS, false)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    log::warn!("sign-ups turned off from the dashboard");
    back("/admin", "sign-ups-off")
}
