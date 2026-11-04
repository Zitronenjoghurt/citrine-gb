use super::{JoinForm, LIST_AGAIN};
use crate::app::widgets::confirm;
use crate::app::widgets::settings::form::Form;
use crate::icons;
use crate::online::Online;
use citrine_gb_net::FileInfo;
use egui::{RichText, Ui};
use web_time::Instant;

pub(super) fn storage(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    if has_storage(online) {
        ui.add_space(ui.spacing().interact_size.y.max(20.0));
        storage_body(ui, online, form, now);
    }
}

pub fn has_storage(online: &Online) -> bool {
    online
        .account()
        .is_some_and(|account| account.quota_bytes > 0 || account.storage_used > 0)
}

pub fn storage_body(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let Some(account) = online.account() else {
        return;
    };
    let (used, quota) = (account.storage_used, account.quota_bytes);
    Form::show(ui, "online_storage", |f| {
        f.text_row("Storage", |ui| {
            ui.label(if quota == 0 {
                format!("{} used", crate::media::format_size(used))
            } else {
                format!(
                    "{} of {}",
                    crate::media::format_size(used),
                    crate::media::format_size(quota)
                )
            });
        });
    });
    if quota > 0 {
        let fraction = (used as f32 / quota as f32).clamp(0.0, 1.0);
        ui.scope(|ui| {
            if !ui.visuals().dark_mode {
                ui.visuals_mut().extreme_bg_color = ui.visuals().widgets.inactive.bg_fill;
            }
            ui.add(egui::ProgressBar::new(fraction).desired_height(6.0));
        });
    }
    ui.add_space(8.0);

    let files = online.shared_files().map(<[FileInfo]>::to_vec);
    let due = form
        .listed_at
        .is_none_or(|at| now.duration_since(at) >= LIST_AGAIN);
    if files.is_none() && online.is_connected() && due && online.list_files() {
        form.listed_at = Some(now);
    }
    let (title, bytes) = match &files {
        Some(files) => (
            format!("Shared files ({})", files.len()),
            Some(files.iter().map(|f| f.size).sum::<u64>()),
        ),
        None => ("Shared files".to_owned(), None),
    };
    if storage_row(ui, Some(form.show_files), &title, bytes) {
        form.show_files = !form.show_files;
    }
    if form.show_files {
        shared_files(ui, online, form, files);
    }
    cloud_saves(ui, online, form);
    if let Some(bytes) = online.account_bytes().filter(|bytes| *bytes > 0) {
        storage_row(ui, None, "Settings and stats", Some(bytes));
    }
}

pub(super) fn caret_width(ui: &Ui) -> f32 {
    egui::WidgetText::from(format!("{} ", icons::CARET_RIGHT))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

pub(super) fn item_indent(ui: &Ui) -> f32 {
    caret_width(ui) + ui.spacing().indent
}

pub(super) fn stored_item(ui: &mut Ui, enabled: bool, title: &str, facts: &str) -> bool {
    let side = ui.spacing().interact_size.y;
    let indent = item_indent(ui);
    let text = 2.0 * ui.text_style_height(&egui::TextStyle::Body) + 1.0;
    let height = side.max(text);
    let mut delete = false;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().button_padding = egui::Vec2::ZERO;
                delete = ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(trash(ui, side)).min_size(egui::vec2(side, side)),
                    )
                    .on_hover_text("Delete")
                    .clicked();
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.add_space((height - text) / 2.0);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(egui::Label::new(RichText::new(title).strong()).truncate());
                    ui.add(egui::Label::new(RichText::new(facts).weak()).truncate());
                });
            },
        );
    });
    ui.add_space(2.0);
    delete
}

pub(super) fn trash(ui: &Ui, side: f32) -> RichText {
    let body = egui::TextStyle::Body.resolve(ui.style()).size;
    RichText::new(icons::TRASH).size((side * 0.4).max(body))
}

pub(super) fn stored_note(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(item_indent(ui));
        ui.label(RichText::new(text).weak());
    });
}

pub(super) fn storage_row(
    ui: &mut Ui,
    open: Option<bool>,
    title: &str,
    bytes: Option<u64>,
) -> bool {
    let caret = |open: bool| {
        if open {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        }
    };
    let text = match open {
        Some(open) => format!("{} {title}", caret(open)),
        None => title.to_owned(),
    };
    let indent = if open.is_some() { 0.0 } else { caret_width(ui) };
    let height = ui.spacing().interact_size.y;
    let row = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            if let Some(bytes) = bytes {
                ui.label(RichText::new(crate::media::format_size(bytes)).weak());
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(indent);
                ui.add(egui::Label::new(text).truncate());
            });
        },
    );
    if open.is_none() {
        return false;
    }
    row.response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

pub(super) fn shared_files(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    files: Option<Vec<FileInfo>>,
) {
    let Some(mut files) = files else {
        stored_note(ui, "Loading…");
        ui.add_space(8.0);
        return;
    };
    if files.is_empty() {
        stored_note(ui, "Nothing shared right now");
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.created));
    for file in &files {
        let (title, game) = match online.share_of(file.id) {
            Some((friend, share)) => {
                let name = online.display_name(friend);
                (
                    format!(
                        "{} to {name}",
                        crate::app::tabs::chat::share::share_label(share.kind)
                    ),
                    Some(crate::app::tabs::media::game_name(&share.game).to_owned()),
                )
            }
            None => ("Shared file".to_owned(), None),
        };
        let mut facts = Vec::new();
        facts.extend(game);
        facts.push(crate::media::format_size(file.size));
        facts.push(crate::media::local_stamp(file.created, "%-d %b %Y"));
        if stored_item(ui, online.is_connected(), &title, &facts.join(" · ")) {
            form.delete = Some((file.id, title.clone()));
        }
    }
    ui.add_space(8.0);
    if let Some((file, title)) = form.delete.clone() {
        let question = format!("Delete \"{title}\"? Your friend can't download it after this.");
        if let Some(yes) = confirm::confirm(ui.ctx(), "online_file_delete", &question, "Delete") {
            form.delete = None;
            if yes {
                online.delete_file(file);
            }
        }
    }
}

pub(super) fn cloud_saves(ui: &mut Ui, online: &mut Online, form: &mut JoinForm) {
    if !online.cloud_enabled() || !online.cloud_listed() {
        return;
    }
    let games = online.cloud_games();
    let title = format!(
        "Cloud saves ({})",
        games.iter().map(|game| game.saves).sum::<usize>()
    );
    let bytes: u64 = games.iter().map(|game| game.bytes).sum();
    if storage_row(ui, Some(form.show_cloud), &title, Some(bytes)) {
        form.show_cloud = !form.show_cloud;
    }
    if form.show_cloud {
        cloud_items(ui, online, form, &games);
    }
    cloud_confirm(ui, online, form);
}

fn cloud_items(
    ui: &mut Ui,
    online: &Online,
    form: &mut JoinForm,
    games: &[crate::online::cloud::CloudGame],
) {
    {
        if games.is_empty() {
            stored_note(ui, "No saves in the cloud yet");
        }
        for game in games {
            let title = if game.title.is_empty() {
                "Unknown game".to_owned()
            } else {
                game.title.clone()
            };
            let saves = if game.saves == 1 {
                "1 save".to_owned()
            } else {
                format!("{} saves", game.saves)
            };
            let facts = format!("{saves} · {}", crate::media::format_size(game.bytes));
            if stored_item(ui, online.is_connected(), &title, &facts) {
                form.delete_cloud = Some((game.game, title.clone()));
            }
        }
        ui.add_space(8.0);
    }
}

fn cloud_confirm(ui: &mut Ui, online: &mut Online, form: &mut JoinForm) {
    if let Some((game, title)) = form.delete_cloud.clone() {
        let question = format!("Delete the cloud saves of {title}? The saves on this device stay.");
        if let Some(yes) = confirm::confirm(ui.ctx(), "online_cloud_delete", &question, "Delete") {
            form.delete_cloud = None;
            if yes {
                online.cloud_delete_game(game);
            }
        }
    }
}

fn counted(count: usize, one: &str, many: &str) -> String {
    let noun = if count == 1 { one } else { many };
    format!("{count} {noun}")
}

pub fn storage_list(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    use crate::app::mobile::kit::{self, Row};
    let Some(account) = online.account() else {
        return;
    };
    let (used, quota) = (account.storage_used, account.quota_bytes);
    let summary = if quota == 0 {
        format!("{} used", crate::media::format_size(used))
    } else {
        format!(
            "{} of {}",
            crate::media::format_size(used),
            crate::media::format_size(quota)
        )
    };
    let files = online.shared_files().map(<[FileInfo]>::to_vec);
    let due = form
        .listed_at
        .is_none_or(|at| now.duration_since(at) >= LIST_AGAIN);
    if files.is_none() && online.is_connected() && due && online.list_files() {
        form.listed_at = Some(now);
    }
    let caret = |open: bool| {
        if open {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        }
    };
    kit::list(ui, |list| {
        list.custom(|ui| {
            kit::padded(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Storage").size(16.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(&summary).size(15.0).weak());
                    });
                });
                if quota > 0 {
                    ui.add_space(6.0);
                    crate::app::widgets::progress::bar(
                        ui,
                        Some((used as f32 / quota as f32).clamp(0.0, 1.0)),
                    );
                    ui.add_space(4.0);
                }
            })
        });
        let (files_value, files_size) = match &files {
            Some(files) if files.is_empty() => ("None".to_owned(), String::new()),
            Some(files) => (
                counted(files.len(), "file", "files"),
                crate::media::format_size(files.iter().map(|f| f.size).sum()),
            ),
            None => (String::new(), String::new()),
        };
        let row = Row::new("Shared files")
            .detail(&files_size)
            .value(files_value)
            .trailing(caret(form.show_files));
        if list.row(row).clicked() {
            form.show_files = !form.show_files;
        }
        if form.show_files {
            list.custom(|ui| kit::padded(ui, |ui| shared_files(ui, online, form, files)));
        }
        if online.cloud_enabled() && online.cloud_listed() {
            let games = online.cloud_games();
            let saves = games.iter().map(|game| game.saves).sum::<usize>();
            let bytes = games.iter().map(|game| game.bytes).sum();
            let size = crate::media::format_size(bytes);
            let row = Row::new("Saves")
                .detail(&size)
                .value(counted(saves, "save", "saves"))
                .trailing(caret(form.show_cloud));
            if list.row(row).clicked() {
                form.show_cloud = !form.show_cloud;
            }
            if form.show_cloud {
                list.custom(|ui| kit::padded(ui, |ui| cloud_items(ui, online, form, &games)));
            }
        }
        if let Some(bytes) = online.account_bytes().filter(|bytes| *bytes > 0) {
            list.row(
                Row::new("Settings and stats")
                    .value(crate::media::format_size(bytes))
                    .no_chevron(),
            );
        }
    });
    cloud_confirm(ui, online, form);
}
