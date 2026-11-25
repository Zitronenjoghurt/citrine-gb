use super::page::{self, Flash, Tab, ago, avatar, bytes, esc, icon};
use super::upload::Why;
use super::users::url_part;
use super::{PAGE_SIZE, confirm, guard};
use crate::archive::{Console, Entry};
use crate::db::UserOrder;
use crate::{App, now};
use axum::extract::{Form, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use citrine_gb_net::{ArchiveId, OnlineId};
use serde::Deserialize;
use std::fmt::Write;
use std::sync::Arc;

#[derive(Deserialize, Default)]
pub(super) struct ArchiveQuery {
    done: Option<String>,
    q: Option<String>,
    console: Option<String>,
    order: Option<String>,
    page: Option<u64>,
    added: Option<u64>,
    there: Option<u64>,
    failed: Option<u64>,
    added_game: Option<String>,
    there_game: Option<String>,
    why: Option<String>,
    failed_file: Option<String>,
}

const ROM_EXTENSIONS: [&str; 7] = ["gb", "gbc", "cgb", "gba", "agb", "nds", "dsi"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    Name,
    Newest,
    Size,
}

impl Order {
    const ALL: [Self; 3] = [Self::Name, Self::Newest, Self::Size];

    fn of(text: Option<&str>) -> Self {
        match text {
            Some("newest") => Self::Newest,
            Some("size") => Self::Size,
            _ => Self::Name,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Newest => "newest",
            Self::Size => "size",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Newest => "Newest",
            Self::Size => "ROM size",
        }
    }
}

#[derive(Clone, Copy)]
struct View<'a> {
    search: &'a str,
    console: Option<Console>,
    order: Order,
}

impl View<'_> {
    fn link(self, page_no: u64) -> String {
        let mut link = format!("/admin/archive?q={}", url_part(self.search));
        if let Some(c) = self.console {
            let _ = write!(link, "&console={}", c.slug());
        }
        if self.order != Order::Name {
            let _ = write!(link, "&order={}", self.order.slug());
        }
        let _ = write!(link, "&page={page_no}");
        link
    }

    fn shows(self, e: &Entry, needle: &str) -> bool {
        match (&e.game, self.console) {
            (Some(g), console) => {
                console.is_none_or(|want| want == g.console)
                    && (needle.is_empty() || g.name.to_lowercase().contains(needle))
            }
            (None, console) => console.is_none() && needle.is_empty(),
        }
    }

    fn sort(self, games: &mut [&Entry]) {
        fn name(e: &Entry) -> Option<String> {
            e.game.as_ref().map(|g| g.name.to_lowercase())
        }
        fn size(e: &Entry) -> u64 {
            e.game.as_ref().map_or(0, |g| g.size)
        }
        match self.order {
            Order::Name => games.sort_by_key(|e| (name(e).is_none(), name(e), e.id)),
            Order::Newest => games.sort_by_key(|e| std::cmp::Reverse((e.added_at, e.id))),
            Order::Size => games.sort_by_key(|e| (std::cmp::Reverse(size(e)), name(e), e.id)),
        }
    }

    fn toolbar(self, consoles: &[Console], add: &str) -> String {
        let tick = |on: bool| if on { icon("check") } else { String::new() };
        let console_items = |out: &mut String| {
            let all = View {
                console: None,
                ..self
            };
            let _ = write!(
                out,
                r#"<a class="btn" role="menuitemradio" aria-checked="{}" href="{}">All consoles{}</a>"#,
                self.console.is_none(),
                esc(&all.link(1)),
                tick(self.console.is_none())
            );
            for &c in consoles {
                let on = self.console == Some(c);
                let pick = View {
                    console: Some(c),
                    ..self
                };
                let _ = write!(
                    out,
                    r#"<a class="btn" role="menuitemradio" aria-checked="{on}" href="{}">{}{}</a>"#,
                    esc(&pick.link(1)),
                    c.name(),
                    tick(on)
                );
            }
        };
        let order_items = |out: &mut String, menu: bool| {
            for o in Order::ALL {
                let on = self.order == o;
                let pick = View { order: o, ..self };
                if menu {
                    let _ = write!(
                        out,
                        r#"<a class="btn" role="menuitemradio" aria-checked="{on}" href="{}">{}{}</a>"#,
                        esc(&pick.link(1)),
                        o.label(),
                        tick(on)
                    );
                } else {
                    let _ = write!(
                        out,
                        r#"<a class="btn" aria-pressed="{on}" href="{}">{}</a>"#,
                        esc(&pick.link(1)),
                        o.label()
                    );
                }
            }
        };
        let mut console_menu = String::new();
        console_items(&mut console_menu);
        let mut filter_menu = String::from(r#"<span class="menu-head">Console</span>"#);
        console_items(&mut filter_menu);
        filter_menu.push_str(r#"<span class="menu-head">Order</span>"#);
        order_items(&mut filter_menu, true);
        let mut orders = String::new();
        order_items(&mut orders, false);
        let changed = self.console.is_some() || self.order != Order::Name;
        let hidden = format!(
            "{}{}",
            self.console
                .map(|c| format!(
                    r#"<input type="hidden" name="console" value="{}">"#,
                    c.slug()
                ))
                .unwrap_or_default(),
            if self.order == Order::Name {
                String::new()
            } else {
                format!(
                    r#"<input type="hidden" name="order" value="{}">"#,
                    self.order.slug()
                )
            }
        );
        format!(
            r#"<div class="toolbar archive-toolbar"><form method="get" action="/admin/archive" role="search"><div class="search">{}<input type="search" name="q" value="{}" placeholder="Search games" aria-label="Search games">{hidden}<button type="button" class="filter-btn" popovertarget="filter-menu" data-anchor="end" aria-label="Console and order">{}{}</button></div></form><div id="filter-menu" popover class="pick-menu"><div class="menu" role="menu">{filter_menu}</div></div><button type="button" class="btn console-btn" popovertarget="console-menu" data-anchor="start">{}<span>{}</span>{}</button><div id="console-menu" popover class="pick-menu"><div class="menu" role="menu">{console_menu}</div></div><div class="segmented" role="group" aria-label="Order">{orders}</div>{add}</div>"#,
            icon("search"),
            esc(self.search),
            icon("sliders"),
            if changed {
                r#"<i class="dot"></i>"#
            } else {
                ""
            },
            icon("gamepad"),
            self.console.map_or("All consoles", Console::name),
            icon("down"),
        )
    }
}

#[derive(Deserialize)]
pub(super) struct BackForm {
    back: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct GrantForm {
    account: Option<i64>,
    who: Option<String>,
    back: Option<String>,
}

pub(super) fn archive_flash(done: Option<&str>) -> Option<Flash> {
    let (ok, text) = match done? {
        "archive-given" => (true, "Game Archive access given"),
        "archive-removed" => (true, "Game Archive access removed"),
        "archive-off" => (false, "The Game Archive is off"),
        "archive-nobody" => (false, "No account matches"),
        "archive-several" => (false, "Several accounts have that name. Use the online ID."),
        "game-deleted" => (true, "Game deleted"),
        "game-gone" => (false, "That game was already deleted"),
        _ => return None,
    };
    Some(Flash {
        ok,
        text: text.into(),
    })
}

fn back_to(back: Option<&str>, flash: &str) -> Response {
    let to = back
        .filter(|b| {
            (b.starts_with("/admin/users") || b.starts_with("/admin/archive")) && !b.contains("//")
        })
        .unwrap_or("/admin/archive");
    let to = to.split("&done=").next().unwrap_or(to);
    let sep = if to.contains('?') { '&' } else { '?' };
    Redirect::to(&format!("{to}{sep}done={flash}")).into_response()
}

fn add_games(file_bytes: u64) -> String {
    let accept: Vec<String> = ROM_EXTENSIONS.iter().map(|e| format!(".{e}")).collect();
    let consoles: serde_json::Map<String, serde_json::Value> = ROM_EXTENSIONS
        .iter()
        .map(|e| {
            (
                e.to_string(),
                Console::of(&format!("game.{e}")).label().into(),
            )
        })
        .collect();
    format!(
        r#"<button type="button" class="btn primary add-btn" data-add-games data-file-bytes="{file_bytes}" data-consoles="{}" aria-label="Add games">{}<span>Add games</span></button><input type="file" id="add-games-file" accept="{}" multiple hidden>"#,
        esc(&serde_json::Value::Object(consoles).to_string()),
        icon("plus"),
        accept.join(","),
    )
}

fn adding_row() -> String {
    format!(
        r#"<template id="adding-row"><div class="game-row adding"><span class="game"><span class="cover none">{}</span><span class="title"></span></span><span class="chip console"></span><span class="progress-words"></span><div class="actions"><button type="button" class="btn small" data-cancel aria-label="Cancel" title="Cancel">{}<span>Cancel</span></button></div><div class="row-bar"><i></i></div></div></template>"#,
        icon("gamepad"),
        icon("x"),
    )
}

fn upload_flash(app: &App, query: &ArchiveQuery, entries: &[Entry]) -> Option<Flash> {
    let name_of = |id: &Option<String>| {
        let id = id.as_deref()?;
        let entry = entries.iter().find(|e| e.id.to_string() == id)?;
        entry.game.as_ref().map(|g| esc(&g.name))
    };
    let added = query.added.unwrap_or(0);
    let there = query.there.unwrap_or(0);
    let failed = query.failed.unwrap_or(0);
    let mut parts = Vec::new();
    if failed > 0 {
        let why = query
            .why
            .as_deref()
            .and_then(Why::from_slug)
            .unwrap_or(Why::Broken)
            .words(app);
        let file = esc(query.failed_file.as_deref().unwrap_or("A file"));
        parts.push(format!("{file} wasn't added: {why}"));
        if failed > 1 {
            parts.push(format!("{} more weren't added either", failed - 1));
        }
    }
    let bare = query
        .added_game
        .as_deref()
        .and_then(ArchiveId::parse)
        .is_some_and(|id| app.archive.covers_on() && !app.archive.has_cover(id));
    if added > 0 {
        parts.push(match (added, name_of(&query.added_game)) {
            (1, Some(name)) if bare => format!("{name} added, no cover found"),
            (1, Some(name)) => format!("{name} added"),
            (1, None) => "1 game added".to_string(),
            (n, _) => format!("{n} games added"),
        });
    }
    if there > 0 {
        parts.push(match (there, name_of(&query.there_game)) {
            (1, Some(name)) => format!("{name} was already in the Game Archive"),
            (1, None) => "1 game was already in the Game Archive".to_string(),
            (n, _) => format!("{n} games were already in the Game Archive"),
        });
    }
    (!parts.is_empty()).then(|| Flash {
        ok: failed == 0,
        text: parts.join(". ").into(),
    })
}

fn off_card() -> String {
    format!(
        r#"<section class="card"><h2>{}The Game Archive is off</h2><p class="muted">Set CITRINE_ARCHIVE_KEY_FILE to a file outside the data folder and restart the relay. The relay makes the key there on its first start.</p></section>"#,
        icon("archive")
    )
}

pub(super) async fn page(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<ArchiveQuery>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let s = app.settings.get();
    let mut flash = archive_flash(query.done.as_deref());
    if !app.archive.on() {
        return Html(page::page(&s.name, Tab::Archive, flash, &off_card())).into_response();
    }
    let (Ok(entries), Ok(accounts)) = (
        app.archive.entries(&app.db).await,
        app.db.archive_accounts().await,
    ) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if query.done.as_deref() == Some("upload") {
        flash = upload_flash(&app, &query, &entries);
    }
    let now = now();
    let stored: u64 = entries.iter().map(|e| e.stored).sum();
    let cap = app.archive.total_bytes();
    let fraction = if cap > 0 {
        (stored as f64 / cap as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let mut body = format!(
        r#"<section class="tiles"><div class="tile"><span class="label">Games</span><span class="value">{}</span></div><div class="tile"><span class="label">On disk</span><span class="value">{}</span><span class="sub">of {}</span><div class="bar"><i style="width:{fraction:.1}%"></i></div></div></section>"#,
        entries.len(),
        bytes(stored),
        bytes(cap),
    );
    let search = query.q.clone().unwrap_or_default();
    let search = search.trim().to_string();
    let needle = search.to_lowercase();
    let view = View {
        search: &search,
        console: Console::ALL
            .into_iter()
            .find(|c| query.console.as_deref() == Some(c.slug())),
        order: Order::of(query.order.as_deref()),
    };
    let consoles: Vec<Console> = Console::ALL
        .into_iter()
        .filter(|c| {
            view.console == Some(*c)
                || entries
                    .iter()
                    .any(|e| e.game.as_ref().is_some_and(|g| g.console == *c))
        })
        .collect();
    let mut matching: Vec<&Entry> = entries.iter().filter(|e| view.shows(e, &needle)).collect();
    view.sort(&mut matching);
    let total = matching.len() as u64;
    let pages = total.div_ceil(PAGE_SIZE).max(1);
    let page_no = query.page.unwrap_or(1).clamp(1, pages);
    let here = esc(&view.link(page_no));
    let back_field = format!(r#"<input type="hidden" name="back" value="{here}">"#);
    body.push_str(&view.toolbar(&consoles, &add_games(app.archive.file_bytes())));
    let mut list = String::from(
        r#"<section class="card games"><div class="game-row head"><span>Game</span><span>Console</span><span>ROM size</span><span>On disk</span><span>Added</span><span></span></div>"#,
    );
    if matching.is_empty() {
        list.push_str(if search.is_empty() && view.console.is_none() {
            r#"<p class="empty">No games yet</p>"#
        } else {
            r#"<p class="empty">No game matches</p>"#
        });
    }
    let mut confirms = String::new();
    let skip = ((page_no - 1) * PAGE_SIZE) as usize;
    for e in matching.iter().skip(skip).take(PAGE_SIZE as usize) {
        let id = e.id.to_string();
        let (name, console, size) = match &e.game {
            Some(g) => (esc(&g.name), g.console.label(), bytes(g.size)),
            None => ("Locked with another key".to_string(), "", String::new()),
        };
        let cover = match app.archive.has_cover(e.id) {
            true => format!(
                r#"<img class="cover" src="/admin/archive/{id}/cover" alt="" loading="lazy" width="40" height="40">"#
            ),
            false => format!(
                r#"<span class="cover none" role="img" aria-label="No cover" title="No cover">{}</span>"#,
                icon("gamepad")
            ),
        };
        let _ = write!(
            list,
            r#"<div class="game-row"><span class="game">{cover}<span class="title" title="{name}">{name}</span></span><span class="chip console">{console}</span><span class="size rom">{size}</span><span class="size">{}</span><span class="added muted">{}</span><div class="actions"><button type="button" class="btn small danger" popovertarget="delete-{id}" aria-label="Delete" title="Delete">{}<span>Delete</span></button></div></div>"#,
            bytes(e.stored),
            ago(e.added_at, now),
            icon("trash"),
        );
        confirms.push_str(&confirm(
            &format!("delete-{id}"),
            &format!("Delete {name}?"),
            "Every player loses it, downloaded copies too. Saves stay.",
            &format!("/admin/archive/{id}/delete"),
            "Delete",
            &back_field,
        ));
    }
    list.push_str("</section>");
    body.push_str(&list);
    if pages > 1 {
        let link = |p: u64, label: &str, ic: &str, enabled: bool| {
            if enabled {
                format!(
                    r#"<a class="btn" href="{}">{}{label}</a>"#,
                    esc(&view.link(p)),
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
    let mut access = format!(
        r#"<section class="card access"><div class="access-head"><h2>{} Accounts with access</h2></div>"#,
        icon("archive")
    );
    if accounts.is_empty() {
        access.push_str(r#"<p class="empty">No account has access yet</p>"#);
    }
    for a in &accounts {
        let name = esc(&a.display_name);
        let id = a.id;
        let _ = write!(
            access,
            r#"<div class="access-row"><div class="who">{}<div style="min-width:0"><div class="name">{name}</div><span class="mono muted">{}</span></div></div><div class="actions"><button type="button" class="btn small" popovertarget="revoke-{id}">{}<span>Remove</span></button></div></div>"#,
            avatar(&a.display_name, a.online_id.value()),
            a.online_id,
            icon("x"),
        );
        confirms.push_str(&revoke_confirm(id, &name, &back_field));
    }
    let _ = write!(
        access,
        r#"<form method="post" action="/admin/archive/access" class="access-add">{back_field}<input type="search" name="who" placeholder="Name or online ID" aria-label="Account to give access" required><button class="btn" type="submit">{}Give access</button></form></section>"#,
        icon("plus"),
    );
    body.push_str(&access);
    body.push_str(&confirms);
    body.push_str(&adding_row());
    Html(page::page(&s.name, Tab::Archive, flash, &body)).into_response()
}

pub(super) fn revoke_confirm(account: i64, name: &str, back_field: &str) -> String {
    confirm(
        &format!("revoke-{account}"),
        &format!("Remove {name}'s Game Archive access?"),
        &format!("{name} loses every Game Archive game, downloaded copies too. Saves stay."),
        &format!("/admin/archive/access/{account}/remove"),
        "Remove access",
        back_field,
    )
}

pub(super) async fn cover(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let Some(png) = ArchiveId::parse(&id).map(|id| app.archive.cover(id)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match png.await {
        Some(png) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "private, max-age=86400"),
            ],
            png,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(super) async fn delete(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<BackForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    let Some(id) = ArchiveId::parse(&id) else {
        return back_to(form.back.as_deref(), "game-gone");
    };
    match app.archive.delete(&app.db, id).await {
        Ok(true) => {
            app.tell_archive_all().await;
            back_to(form.back.as_deref(), "game-deleted")
        }
        Ok(false) => back_to(form.back.as_deref(), "game-gone"),
        Err(e) => {
            log::error!("database: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub(super) async fn grant(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<GrantForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if !app.archive.on() {
        return back_to(form.back.as_deref(), "archive-off");
    }
    let account = match form.account {
        Some(id) => id,
        None => match find_account(&app, form.who.as_deref().unwrap_or("").trim()).await {
            Ok(Ok(id)) => id,
            Ok(Err(flash)) => return back_to(form.back.as_deref(), flash),
            Err(e) => {
                log::error!("database: {e}");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        },
    };
    if let Err(e) = app.db.archive_grant(account, now()).await {
        log::error!("database: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    app.tell_archive(account).await;
    back_to(form.back.as_deref(), "archive-given")
}

async fn find_account(app: &App, who: &str) -> crate::db::Result<Result<i64, &'static str>> {
    if who.is_empty() {
        return Ok(Err("archive-nobody"));
    }
    let online = who.parse::<OnlineId>().ok();
    let (rows, _) = app.db.users(who, UserOrder::LastSeen, 0, 0, 50).await?;
    let lower = who.to_lowercase();
    let hits: Vec<i64> = rows
        .iter()
        .filter(|r| {
            Some(r.account.online_id) == online || r.account.display_name.to_lowercase() == lower
        })
        .map(|r| r.account.id)
        .collect();
    Ok(match hits.as_slice() {
        [id] => Ok(*id),
        [] => Err("archive-nobody"),
        _ => Err("archive-several"),
    })
}

pub(super) async fn revoke(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(account): Path<i64>,
    Form(form): Form<BackForm>,
) -> Response {
    if let Some(r) = guard(&app, &headers) {
        return r;
    }
    if let Err(e) = app.db.archive_revoke(account).await {
        log::error!("database: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    app.tell_archive(account).await;
    back_to(form.back.as_deref(), "archive-removed")
}
