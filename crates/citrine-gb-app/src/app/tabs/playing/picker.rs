use super::lines::{card_name, member_name};
use super::{PickFor, PlayingUi};
use crate::app::big_screen::scroll::Following;
use crate::app::ui_state::recent::RecentRoms;
use crate::app::widgets::action::main_button;
use crate::app::widgets::tag_chip::{Look, TagChip};
use crate::icons;
use crate::online::Online;
use crate::play::Play;
use egui::{Label, RichText, Ui};

const PICKER_WIDTH: f32 = 420.0;
const DOWNLOAD: &str = "Download";
const EXPERIMENTAL: &str = "Experimental";

pub enum Pick {
    Homebrew(&'static crate::homebrew::HomebrewGame),
    #[cfg(not(target_arch = "wasm32"))]
    Recent(std::path::PathBuf),
    File,
    Theirs,
    Download,
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
        scroll.show_following(ui, |ui| {
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
            if joining
                && let Some(card) = theirs_card.as_ref().filter(|card| card.console.is_ds())
                && fits("Download Play", card_name(card))
            {
                let sharing = crate::play::shares_games(online);
                let line = match sharing {
                    true => format!("{name} starts it from the game's menu"),
                    false => "This relay doesn't share games".to_owned(),
                };
                let title = TagChip::new(EXPERIMENTAL, Look::Genre);
                if pick_row_tagged(ui, "Download Play", title, &line, DOWNLOAD, sharing) {
                    pick = Some(Pick::Download);
                }
            }
            if !matches!(target, PickFor::Invite(_)) {
                let line = format!("Watch {name}");
                if fits("No game", &line) && pick_row_as(ui, "No game", &line, false, "Watch", true)
                {
                    pick = Some(Pick::Nothing);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if !recent.is_empty() {
                pick_heading(ui, "Recent");
                for entry in recent.entries() {
                    let folder = entry.folder();
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
    pick_row_as(ui, title, detail, main, "Play", true)
}

fn pick_row_as(
    ui: &mut Ui,
    title: &str,
    detail: &str,
    main: bool,
    action: &str,
    enabled: bool,
) -> bool {
    pick_row_with(ui, title, None, detail, main, action, enabled)
}

fn pick_row_tagged(
    ui: &mut Ui,
    title: &str,
    tag: TagChip,
    detail: &str,
    action: &str,
    enabled: bool,
) -> bool {
    pick_row_with(ui, title, Some(tag), detail, false, action, enabled)
}

fn pick_row_with(
    ui: &mut Ui,
    title: &str,
    tag: Option<TagChip>,
    detail: &str,
    main: bool,
    action: &str,
    enabled: bool,
) -> bool {
    let mut picked = false;
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                match tag {
                    Some(tag) => {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), tag.height()),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(Label::new(title).truncate());
                                tag.show(ui);
                            },
                        );
                    }
                    None => {
                        ui.add(Label::new(title).truncate());
                    }
                }
                ui.add(Label::new(RichText::new(detail).small().weak()).truncate());
            });
        },
        |ui| {
            let button = if main {
                main_button(ui, action)
            } else {
                egui::Button::new(action)
            };
            let size = egui::vec2(action_width(ui), 32.0);
            picked = ui.add_enabled(enabled, button.min_size(size)).clicked();
        },
    );
    ui.separator();
    picked
}

fn action_width(ui: &Ui) -> f32 {
    let widest = egui::WidgetText::from(DOWNLOAD).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    (widest.size().x + 2.0 * ui.spacing().button_padding.x).max(56.0)
}

#[cfg(test)]
mod tests {
    use super::{DOWNLOAD, EXPERIMENTAL, pick_row, pick_row_as, pick_row_tagged};
    use crate::app::widgets::tag_chip::{Look, TagChip};

    #[test]
    fn every_button_in_the_list_starts_at_the_same_edge() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 600.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                pick_row(ui, "mk.nds", "Clara is playing it", true);
                pick_row_tagged(
                    ui,
                    "Download Play",
                    TagChip::new(EXPERIMENTAL, Look::Genre),
                    "Clara starts it",
                    DOWNLOAD,
                    true,
                );
                pick_row_as(ui, "No game", "Watch Clara", false, "Watch", true);
                pick_row(ui, "Petris", "bbbbbr", false);
            });
        });
        let button = |label: &str| {
            let text = crate::app::tests::text_rect(&output, label).expect(label);
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) if rect.rect.contains(text.center()) => Some(rect.rect),
                    _ => None,
                })
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .expect(label)
        };
        let download = button(DOWNLOAD);
        for label in ["Play", "Watch"] {
            assert_eq!(button(label).left(), download.left(), "{label}");
            assert_eq!(button(label).right(), download.right(), "{label}");
        }
        let title = crate::app::tests::text_rect(&output, "Download Play").expect("title");
        let tag = crate::app::tests::text_rect(&output, EXPERIMENTAL).expect("tag");
        assert!(tag.left() > title.right(), "{tag:?} {title:?}");
        assert!(tag.right() < download.left(), "{tag:?} {download:?}");
        assert!(
            (tag.center().y - title.center().y).abs() < 2.0,
            "{tag:?} {title:?}"
        );
    }
}
