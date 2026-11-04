use super::lines::{card_name, member_name};
use super::{PickFor, PlayingUi};
use crate::app::ui_state::recent::RecentRoms;
use crate::app::widgets::action::main_button;
use crate::icons;
use crate::online::Online;
use crate::play::Play;
use egui::{Label, RichText, Ui};

const PICKER_WIDTH: f32 = 420.0;

pub enum Pick {
    Homebrew(&'static crate::homebrew::HomebrewGame),
    #[cfg(not(target_arch = "wasm32"))]
    Recent(std::path::PathBuf),
    File,
    Theirs,
    Nothing,
}

pub fn picker(
    ctx: &egui::Context,
    playing: &mut PlayingUi,
    online: &Online,
    recent: &RecentRoms,
    play: &Play,
) -> Option<(PickFor, Pick)> {
    let target = playing.picking.clone()?;
    let (friend, title, theirs_card) = match &target {
        PickFor::Invite(friend) => {
            let name = member_name(play, online, *friend);
            (*friend, format!("Play with {name}"), None)
        }
        PickFor::Join(invite) => {
            let name = member_name(play, online, invite.from);
            (invite.from, format!("Join {name}"), invite.game.clone())
        }
        PickFor::Change => {
            let Some(session) = play.session().filter(|s| s.is_live()) else {
                playing.picking = None;
                return None;
            };
            (
                session.friend(),
                "Change game".to_owned(),
                session.friend_game().cloned(),
            )
        }
    };
    let name = member_name(play, online, friend);
    let theirs = theirs_card
        .as_ref()
        .map(|c| c.hash)
        .or_else(|| online.playing(friend).copied());
    let joining = matches!(target, PickFor::Join(_));
    let mut pick = None;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::new("play_picker")).show(ctx, |ui| {
        ui.set_width(PICKER_WIDTH.min(ctx.content_rect().width() - 32.0));
        ui.label(RichText::new(title).strong().size(16.0));
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut playing.pick_query)
                .hint_text(format!("{}  Search", icons::MAGNIFYING_GLASS))
                .desired_width(f32::INFINITY),
        );
        let query = playing.pick_query.trim().to_lowercase();
        let fits = |title: &str, detail: &str| {
            query.is_empty()
                || title.to_lowercase().contains(&query)
                || detail.to_lowercase().contains(&query)
        };
        ui.add_space(4.0);
        let height = ctx.content_rect().height() * 0.6;
        let mut scroll = egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, true]);
        if !playing.pick_shown {
            scroll = scroll.vertical_scroll_offset(0.0);
            playing.pick_shown = true;
        }
        scroll.show(ui, |ui| {
            let homebrew = crate::homebrew::HOMEBREW_GAMES
                .iter()
                .filter(|g| g.console == crate::homebrew::ShelfConsole::GameBoy);
            let playing_it = format!("{name} is playing it");
            if let Some(game) = homebrew.clone().find(|g| Some(g.sha1) == theirs) {
                if fits(game.title, game.author) && pick_row(ui, game.title, &playing_it, true) {
                    pick = Some(if joining {
                        Pick::Theirs
                    } else {
                        Pick::Homebrew(game)
                    });
                }
            } else if joining
                && let Some(card) = &theirs_card
                && fits(card_name(card), "")
                && pick_row(ui, card_name(card), &playing_it, true)
            {
                pick = Some(Pick::Theirs);
            }
            if !matches!(target, PickFor::Invite(_)) {
                let line = format!("Watch {name}");
                if fits("No game", &line) && pick_row_as(ui, "No game", &line, false, "Watch") {
                    pick = Some(Pick::Nothing);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !recent.is_empty() {
                pick_heading(ui, "Recent");
                for entry in recent.entries() {
                    let folder = entry
                        .path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let title = entry.display_name();
                    if fits(&title, &folder) && pick_row(ui, &title, &folder, false) {
                        pick = Some(Pick::Recent(entry.path.clone()));
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = recent;
            pick_heading(ui, "Homebrew");
            for game in homebrew.filter(|g| Some(g.sha1) != theirs && fits(g.title, g.author)) {
                if pick_row(ui, game.title, game.author, false) {
                    pick = Some(Pick::Homebrew(game));
                }
            }
        });
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let size = egui::vec2(columns[0].available_width(), 32.0);
            let open = egui::Button::new(format!("{}  Open a game file", icons::FOLDER_OPEN));
            if columns[0].add_sized(size, open).clicked() {
                pick = Some(Pick::File);
            }
            if columns[1]
                .add_sized(size, egui::Button::new("Cancel"))
                .clicked()
            {
                cancel = true;
            }
        });
    });
    if cancel || pick.is_some() || modal.should_close() {
        playing.picking = None;
        playing.pick_query.clear();
        playing.pick_shown = false;
    }
    pick.map(|pick| (target, pick))
}

fn pick_heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).weak());
}

fn pick_row(ui: &mut Ui, title: &str, detail: &str, main: bool) -> bool {
    pick_row_as(ui, title, detail, main, "Play")
}

fn pick_row_as(ui: &mut Ui, title: &str, detail: &str, main: bool, action: &str) -> bool {
    let mut picked = false;
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(Label::new(title).truncate());
                ui.add(Label::new(RichText::new(detail).small().weak()).truncate());
            });
        },
        |ui| {
            let button = if main {
                main_button(ui, action)
            } else {
                egui::Button::new(action)
            };
            picked = ui.add(button.min_size(egui::vec2(56.0, 32.0))).clicked();
        },
    );
    ui.separator();
    picked
}
