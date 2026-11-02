use super::page::{self, Flash, Tab, ago, avatar, esc, hint, icon};
use super::settings_page::{row, source_chip};
use super::{PAGE_SIZE, confirm, guard};
use crate::db::{InviteFilter, InviteRow};
use crate::settings::{self, Effective};
use crate::{App, now};
use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use std::fmt::Write;
use std::sync::Arc;

pub const NOTE_MAX_CHARS: usize = 80;

#[derive(Deserialize, Default)]
pub struct InvitesQuery {
    done: Option<String>,
    q: Option<String>,
    page: Option<u64>,
    show: Option<String>,
    new: Option<i64>,
}

fn filter(show: Option<&str>) -> InviteFilter {
    match show {
        Some("unused") => InviteFilter::Unused,
        Some("used") => InviteFilter::Used,
        _ => InviteFilter::All,
    }
}

fn show_key(filter: InviteFilter) -> &'static str {
    match filter {
        InviteFilter::All => "all",
        InviteFilter::Unused => "unused",
        InviteFilter::Used => "used",
    }
}

fn flash(done: Option<&str>) -> Option<Flash> {
    let (ok, text) = match done? {
        "made" => (true, "Invite code made"),
        "revoked" => (true, "Invite code revoked"),
        "was-used" => (false, "That code was used or revoked already"),
        "no-note" => (false, "Write who the code is for"),
        "long-note" => (false, "Keep the note to 80 characters"),
        _ => return None,
    };
    Some(Flash { ok, text })
}

fn url_part(text: &str) -> String {
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

pub fn setting_row(app: &App, s: &Effective) -> String {
    row(
        &format!(
            r#"<label for="invites">New accounts need an invite code</label>{}"#,
            hint(
                "hint-invites",
                "Only people with a code from you can make an account. Make codes on the Invites page. Existing accounts still work."
            )
        ),
        &source_chip(app, settings::INVITES),
        &format!(
            r#"<input id="invites" name="invites" type="checkbox" class="switch" value="on"{}>"#,
            if s.invites { " checked" } else { "" }
        ),
    )
}

fn status(s: &Effective) -> String {
    let link = |label: &str| {
        format!(
            r#"<a class="btn ghost small" href="/admin/settings">{}{label}</a>"#,
            icon("settings")
        )
    };
    match (s.sign_ups, s.invites) {
        (false, _) => format!(
            r#"<div class="source"><span class="chip warn">New accounts are off</span>{}</div>"#,
            link("Settings")
        ),
        (true, true) => format!(
            r#"<div class="source"><span class="chip on">{}New accounts need a code</span></div>"#,
            icon("check")
        ),
        (true, false) => format!(
            r#"<div class="source"><span class="chip">Codes aren't needed now</span>{}</div>"#,
            link("Turn on in Settings")
        ),
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => first.to_lowercase().chain(chars).collect(),
        _ => text.to_string(),
    }
}

fn invite_row(row: &InviteRow, fresh: bool, back: &str, now: u64) -> String {
    let id = row.id;
    let code = row.code.to_string();
    let note = esc(&row.note);
    let used_by = match &row.used {
        None => r#"<span class="muted">Not used yet</span>"#.to_string(),
        Some(used) => {
            let when = ago(used.at, now);
            match &used.by {
                Some((online_id, name)) => format!(
                    r#"<a class="who" href="/admin/users?q={oid}">{avatar}<div style="min-width:0"><div class="name">{name}</div><span class="mono muted">{oid} · {when}</span></div></a>"#,
                    avatar = avatar(name, online_id.value()),
                    name = esc(name),
                    oid = online_id,
                ),
                None => format!(r#"<span class="muted">Deleted account · {when}</span>"#),
            }
        }
    };
    let actions = if row.used.is_some() {
        String::new()
    } else {
        format!(
            r#"<button type="button" class="btn small" data-copy="{code}">{}<span>Copy</span></button><button type="button" class="btn small danger" popovertarget="revoke-{id}">{}<span>Revoke</span></button>{}"#,
            icon("copy"),
            icon("trash"),
            confirm(
                &format!("revoke-{id}"),
                &format!("Revoke the code for {note}?"),
                &format!("Nobody can make an account with {code} any more."),
                &format!("/admin/invites/{id}/revoke"),
                "Revoke",
                &format!(r#"<input type="hidden" name="back" value="{}">"#, esc(back)),
            )
        )
    };
    let made = ago(row.created_at, now);
    format!(
        r#"<div class="invite-row{fresh}{used}"><span class="mono code">{code}</span><span class="invite-note" title="{note}">{note}</span><span class="made muted"><span class="wide">{made}</span><span class="narrow">Made {made_lower}</span></span><div class="used-by">{used_by}</div><div class="actions">{actions}</div></div>"#,
        fresh = if fresh { " fresh" } else { "" },
        used = if row.used.is_some() { " used" } else { "" },
        made_lower = lower_first(&made),
    )
}

pub async fn list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<InvitesQuery>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let s = app.settings.get();
    let search = query.q.clone().unwrap_or_default();
    let search = search.trim();
    let shown = filter(query.show.as_deref());
    let page_no = query.page.unwrap_or(1).max(1);
    let (Ok((rows, total)), Ok((unused, used))) = (
        app.db
            .invites(search, shown, (page_no - 1) * PAGE_SIZE, PAGE_SIZE)
            .await,
        app.db.invite_counts().await,
    ) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let now = now();
    let mut body = format!(
        r#"<section class="card"><h2>{} New invite</h2>{}<form method="post" action="/admin/invites" class="invite-new"><div class="field"><label for="note">Who it's for</label><input id="note" name="note" type="text" maxlength="{NOTE_MAX_CHARS}" required autocomplete="off"></div><button class="btn primary" type="submit">{}Make code</button></form></section>"#,
        icon("ticket"),
        status(&s),
        icon("plus"),
    );
    let link = |f: InviteFilter, p: u64| {
        format!(
            "/admin/invites?q={}&show={}&page={p}",
            url_part(search),
            show_key(f)
        )
    };
    let here = link(shown, page_no);
    let _ = write!(
        body,
        r#"<div class="toolbar"><form method="get" action="/admin/invites" role="search"><input type="hidden" name="show" value="{}"><div class="search">{}<input type="search" name="q" value="{}" placeholder="Note, code or account" aria-label="Search invites"></div><button class="btn" type="submit">Search</button></form></div>"#,
        show_key(shown),
        icon("search"),
        esc(search),
    );
    let mut tabs = String::new();
    for (f, label, n) in [
        (InviteFilter::All, "All", unused + used),
        (InviteFilter::Unused, "Unused", unused),
        (InviteFilter::Used, "Used", used),
    ] {
        let _ = write!(
            tabs,
            r#"<a class="btn" href="{}" aria-pressed="{}">{label} <span class="muted">{n}</span></a>"#,
            esc(&link(f, 1)),
            f == shown,
        );
    }
    let mut list = format!(
        r#"<section class="card invites"><div class="invites-head"><nav class="segmented" aria-label="Show">{tabs}</nav></div><div class="invite-row head"><span>Code</span><span>Note</span><span>Made</span><span>Used by</span><span></span></div>"#
    );
    if rows.is_empty() {
        list.push_str(match (search.is_empty(), shown, unused + used) {
            (true, _, 0) => r#"<p class="empty">No invite codes yet</p>"#,
            (true, InviteFilter::Unused, _) => r#"<p class="empty">Every code was used</p>"#,
            (true, InviteFilter::Used, _) => r#"<p class="empty">No code was used yet</p>"#,
            _ => r#"<p class="empty">No code matches</p>"#,
        });
    }
    for r in &rows {
        list.push_str(&invite_row(r, query.new == Some(r.id), &here, now));
    }
    list.push_str("</section>");
    body.push_str(&list);
    let pages = total.div_ceil(PAGE_SIZE).max(1);
    if pages > 1 {
        let nav = |p: u64, label: &str, ic: &str, enabled: bool| {
            if enabled {
                format!(
                    r#"<a class="btn" href="{}">{}{label}</a>"#,
                    esc(&link(shown, p)),
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
            nav(page_no - 1, "Previous", "left", page_no > 1),
            nav(page_no + 1, "Next", "right", page_no < pages),
        );
    }
    Html(page::page(
        &s.name,
        Tab::Invites,
        flash(query.done.as_deref()),
        &body,
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct NewInvite {
    note: String,
}

pub async fn create(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<NewInvite>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let note: String = form
        .note
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if note.is_empty() {
        return Redirect::to("/admin/invites?done=no-note").into_response();
    }
    if note.chars().count() > NOTE_MAX_CHARS {
        return Redirect::to("/admin/invites?done=long-note").into_response();
    }
    match app.db.create_invite(&note, now()).await {
        Ok(invite) => {
            Redirect::to(&format!("/admin/invites?done=made&new={}", invite.id)).into_response()
        }
        Err(e) => {
            log::error!("database: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct RevokeForm {
    back: Option<String>,
}

pub async fn revoke(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<RevokeForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let done = match app.db.revoke_invite(id).await {
        Ok(true) => "revoked",
        Ok(false) => "was-used",
        Err(e) => {
            log::error!("database: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let to = form
        .back
        .as_deref()
        .filter(|b| b.starts_with("/admin/invites") && !b.contains("//"))
        .unwrap_or("/admin/invites");
    let to = to.split("&done=").next().unwrap_or(to);
    let sep = if to.contains('?') { '&' } else { '?' };
    Redirect::to(&format!("{to}{sep}done={done}")).into_response()
}
