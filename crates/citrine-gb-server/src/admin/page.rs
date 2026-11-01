use crate::civil;
use citrine_gb_net::{avatar_hues, initials};
use std::fmt::Write;

pub const STYLE: &str = include_str!("style.css");
pub const SCRIPT: &str = include_str!("admin.js");

pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

pub fn icon(name: &str) -> String {
    let body = match name {
        "dashboard" => {
            r#"<rect width="7" height="9" x="3" y="3" rx="1"/><rect width="7" height="5" x="14" y="3" rx="1"/><rect width="7" height="9" x="14" y="12" rx="1"/><rect width="7" height="5" x="3" y="16" rx="1"/>"#
        }
        "users" => {
            r#"<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>"#
        }
        "settings" => {
            r#"<line x1="21" x2="14" y1="4" y2="4"/><line x1="10" x2="3" y1="4" y2="4"/><line x1="21" x2="12" y1="12" y2="12"/><line x1="8" x2="3" y1="12" y2="12"/><line x1="21" x2="16" y1="20" y2="20"/><line x1="12" x2="3" y1="20" y2="20"/><line x1="14" x2="14" y1="2" y2="6"/><line x1="8" x2="8" y1="10" y2="14"/><line x1="16" x2="16" y1="18" y2="22"/>"#
        }
        "log-out" => {
            r#"<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" x2="9" y1="12" y2="12"/>"#
        }
        "power" => r#"<path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>"#,
        "kill" => {
            r#"<polygon points="7.86 2 16.14 2 22 7.86 22 16.14 16.14 22 7.86 22 2 16.14 2 7.86 7.86 2"/><path d="m15 9-6 6"/><path d="m9 9 6 6"/>"#
        }
        "ban" => r#"<circle cx="12" cy="12" r="10"/><path d="m4.9 4.9 14.2 14.2"/>"#,
        "unplug" => {
            r#"<path d="m19 5 3-3"/><path d="m2 22 3-3"/><path d="M6.3 20.3a2.4 2.4 0 0 0 3.4 0L12 18l-6-6-2.3 2.3a2.4 2.4 0 0 0 0 3.4Z"/><path d="M7.5 13.5 10 11"/><path d="M10.5 16.5 13 14"/><path d="m12 6 6 6 2.3-2.3a2.4 2.4 0 0 0 0-3.4l-2.6-2.6a2.4 2.4 0 0 0-3.4 0Z"/>"#
        }
        "trash" => {
            r#"<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/>"#
        }
        "drive" => {
            r#"<line x1="22" x2="2" y1="12" y2="12"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><line x1="6" x2="6.01" y1="16" y2="16"/><line x1="10" x2="10.01" y1="16" y2="16"/>"#
        }
        "more" => {
            r#"<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>"#
        }
        "copy" => {
            r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#
        }
        "search" => r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#,
        "info" => r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#,
        "reset" => {
            r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>"#
        }
        "key" => {
            r#"<path d="M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z"/><circle cx="16.5" cy="7.5" r=".5"/>"#
        }
        "radio" => {
            r#"<path d="M4.9 19.1C1 15.2 1 8.8 4.9 4.9"/><path d="M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5"/><circle cx="12" cy="12" r="2"/><path d="M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5"/><path d="M19.1 4.9C23 8.8 23 15.1 19.1 19"/>"#
        }
        "globe" => {
            r#"<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>"#
        }
        "ticket" => {
            r#"<path d="M2 9a3 3 0 0 1 0 6v2a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-2a3 3 0 0 1 0-6V7a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z"/><path d="M13 5v2"/><path d="M13 17v2"/><path d="M13 11v2"/>"#
        }
        "plus" => r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#,
        "left" => r#"<path d="m15 18-6-6 6-6"/>"#,
        "right" => r#"<path d="m9 18 6-6-6-6"/>"#,
        "check" => r#"<path d="M20 6 9 17l-5-5"/>"#,
        "shield" => {
            r#"<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/><path d="m9 12 2 2 4-4"/>"#
        }
        "download" => {
            r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>"#
        }
        "user-plus" => {
            r#"<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M19 8v6"/><path d="M22 11h-6"/>"#
        }
        "alert" => {
            r#"<circle cx="12" cy="12" r="10"/><line x1="12" x2="12" y1="8" y2="12"/><line x1="12" x2="12.01" y1="16" y2="16"/>"#
        }
        _ => "",
    };
    format!(r#"<svg class="icon" viewBox="0 0 24 24" aria-hidden="true">{body}</svg>"#)
}

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{} {}", value.round(), UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min")
    } else {
        format!("{secs} s")
    }
}

pub fn ago(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..60 => "Just now".into(),
        60..3_600 => format!("{} min ago", secs / 60),
        3_600..86_400 => format!("{} h ago", secs / 3_600),
        86_400..172_800 => "Yesterday".into(),
        172_800..1_209_600 => format!("{} days ago", secs / 86_400),
        _ => {
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let (y, m, d) = civil((then / 86_400) as i64);
            format!("{d} {} {y}", MONTHS[(m - 1) as usize])
        }
    }
}

pub fn avatar(name: &str, seed: u64) -> String {
    let (hue, second) = avatar_hues(seed);
    format!(
        r#"<span class="avatar" style="background:linear-gradient(135deg,hsl({hue} 72% 58%),hsl({second} 70% 46%))" aria-hidden="true">{}</span>"#,
        esc(&initials(name))
    )
}

pub fn hint(id: &str, text: &str) -> String {
    format!(
        r#"&#8288;<button type="button" class="hint" popovertarget="{id}" aria-label="More about this">{}</button><div id="{id}" popover class="hint-pop">{}</div>"#,
        icon("info"),
        esc(text)
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Users,
    Invites,
    Settings,
}

pub struct Flash {
    pub ok: bool,
    pub text: &'static str,
}

fn head(title: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover"><meta name="color-scheme" content="light dark"><meta name="robots" content="noindex"><title>{}</title><link rel="icon" href="/admin/icon.png"><style>{STYLE}</style></head>"#,
        esc(title)
    )
}

pub fn page(server: &str, tab: Tab, flash: Option<Flash>, body: &str) -> String {
    let tabs = [
        (Tab::Dashboard, "/admin", "dashboard", "Dashboard"),
        (Tab::Users, "/admin/users", "users", "Users"),
        (Tab::Invites, "/admin/invites", "ticket", "Invites"),
        (Tab::Settings, "/admin/settings", "settings", "Settings"),
    ];
    let mut nav = String::new();
    for (t, href, ic, label) in tabs {
        let current = if t == tab {
            r#" aria-current="page""#
        } else {
            ""
        };
        let _ = write!(
            nav,
            r#"<a href="{href}"{current}>{}<span>{label}</span></a>"#,
            icon(ic)
        );
    }
    let title = tabs
        .iter()
        .find(|(t, ..)| *t == tab)
        .map(|(.., label)| *label)
        .unwrap_or("Admin");
    let flash = flash
        .map(|f| {
            format!(
                r#"<p class="flash{}" role="status">{}{}</p>"#,
                if f.ok { "" } else { " error" },
                icon(if f.ok { "check" } else { "alert" }),
                f.text
            )
        })
        .unwrap_or_default();
    format!(
        r#"{}<body><header class="top"><div class="top-inner"><a class="brand" href="/admin"><img src="/admin/icon.png" alt=""><span>{}</span></a><nav class="nav" aria-label="Admin">{nav}</nav><form method="post" action="/admin/logout"><button class="btn ghost" type="submit">{}<span>Sign out</span></button></form></div></header><main>{flash}{body}</main><nav class="bottom-nav" aria-label="Admin">{nav}</nav><script>{SCRIPT}</script></body></html>"#,
        head(&format!("{title} · {server}")),
        esc(server),
        icon("log-out"),
    )
}

fn command_row(command: &str) -> String {
    format!(
        r#"<div class="command"><code>{command}</code><button type="button" class="btn small" data-copy="{command}">{}<span>Copy</span></button></div>"#,
        icon("copy"),
    )
}

pub fn web_missing_steps(dir: &std::path::Path) -> String {
    format!(
        r#"<div class="steps"><p>This server looks for the web app in <code class="path">{}</code>, but that folder has no web app in it. In the <code>crates/citrine-gb-app</code> folder of the Citrine repository, run:</p>{}<p>With Docker, run this in <code>crates/citrine-gb-server/docker</code> instead:</p>{}</div>"#,
        esc(&dir.display().to_string()).replace('/', "/<wbr>"),
        command_row(&esc(&crate::config::web_build_command(dir))),
        command_row("docker compose up -d --build"),
    )
}

pub fn web_missing_page(dir: &std::path::Path) -> String {
    format!(
        r#"{}<body><div class="login"><div class="card wide"><img src="/admin/icon.png" alt=""><h1>The web app isn't built yet</h1>{}</div></div><script>{SCRIPT}</script></body></html>"#,
        head("Citrine server"),
        web_missing_steps(dir),
    )
}

pub fn login(server: &str, two_factor: bool, error: Option<&str>) -> String {
    let error = error
        .map(|e| {
            format!(
                r#"<p class="flash error" role="alert">{}{e}</p>"#,
                icon("alert")
            )
        })
        .unwrap_or_default();
    let code = if two_factor {
        format!(
            r#"<div class="field"><label for="code">Code from your app{}</label><input id="code" name="code" type="text" inputmode="numeric" autocomplete="one-time-code" maxlength="20" required></div>"#,
            hint(
                "hint-code",
                "The 6-digit code from your authenticator app. Lost your phone? Enter one of your recovery codes instead."
            )
        )
    } else {
        String::new()
    };
    format!(
        r#"{}<body><div class="login"><div class="card"><img src="/admin/icon.png" alt=""><h1>{}</h1>{error}<form method="post" action="/admin/login"><div class="field"><label for="password">Admin password</label><input id="password" name="password" type="password" autocomplete="current-password" required autofocus></div>{code}<button class="btn primary" type="submit">Sign in</button></form></div></div><script>{SCRIPT}</script></body></html>"#,
        head(&format!("Sign in · {server}")),
        esc(server)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html() {
        assert_eq!(
            esc(r#"<script>"a" & 'b'</script>"#),
            "&lt;script&gt;&quot;a&quot; &amp; &#39;b&#39;&lt;/script&gt;"
        );
    }

    #[test]
    fn formats_sizes_and_times() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(100 << 20), "100 MB");
        assert_eq!(duration(59), "59 s");
        assert_eq!(duration(3_660), "1 h 1 min");
        assert_eq!(duration(90_000), "1 d 1 h");
        assert_eq!(ago(1_000, 1_030), "Just now");
        assert_eq!(ago(0, 7_200), "2 h ago");
        assert_eq!(
            ago(1_761_782_400, 1_761_782_400 + 30 * 86_400),
            "30 Oct 2025"
        );
    }
}
