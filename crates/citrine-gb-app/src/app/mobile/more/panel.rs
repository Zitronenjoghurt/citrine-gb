use super::{MoreAction, Tile, tiles};
use crate::app::Citrine;
use crate::app::big_screen::scroll::Following;
use crate::app::focus::ScreenPlace;
use crate::app::mobile::kit::{self, Row};
use crate::app::mobile::nav::Place;
use crate::app::tabs::playing;
use crate::app::ui_state::ds_screens::ScreenContext;
use crate::icons;
use egui::{Frame, Margin, Rect, RichText, Ui, vec2};

const MENU_WIDTH: f32 = 300.0;

#[derive(Debug, Clone, PartialEq)]
enum MenuAction {
    Resume,
    More(MoreAction),
    FastForward,
    Chat,
    PlayTogether,
    MoveScreen,
    LeaveBigScreen,
}

impl Citrine {
    pub(in crate::app) fn open_game_menu(&mut self) {
        self.mobile.nav.show_game();
        self.open_more();
        self.emulator.release_all();
        self.big.focus_menu();
        self.stop_moving_screen();
    }

    pub(in crate::app) fn big_game(&mut self, ui: &mut Ui) {
        self.emulator.screen_context = ScreenContext::Landscape;
        let area = ui.max_rect();
        if !self.mobile.nav.more_open() {
            self.ui.toast_area = Some(area);
            let touch =
                self.ui.settings.layout.is_touch(ui.ctx()) && self.mobile.hud_sheet.is_none();
            let screen = self.placed_game(ui, area, touch);
            if let Some(screen) = screen
                && self.moving_screen()
            {
                self.move_hints(ui, area, screen);
            }
            return;
        }
        if let Some(key) = self.emulator.rom_key().map(str::to_owned) {
            self.ui.snapshots.sync(ui.ctx(), &self.emulator.store, &key);
        }
        let width = MENU_WIDTH.min(area.width() * 0.45);
        let menu = Rect::from_min_size(area.min, vec2(width, area.height()));
        let game = area.with_min_x(menu.right());
        ui.painter().rect_filled(game, 0.0, egui::Color32::BLACK);
        self.ui.toast_area = Some(game);
        self.paint_screen(ui, game.shrink(16.0), false, ScreenPlace::default());
        let fill = kit::card_fill(ui.visuals());
        let mut column = ui.new_child(egui::UiBuilder::new().max_rect(menu));
        let action = Frame::new()
            .fill(fill)
            .inner_margin(Margin::symmetric(12, 16))
            .show(&mut column, |ui| {
                ui.set_min_size(menu.size() - vec2(24.0, 32.0));
                egui::ScrollArea::vertical()
                    .id_salt("game_menu")
                    .auto_shrink(false)
                    .show_following(ui, |ui| self.menu_body(ui))
                    .inner
            })
            .inner;
        if let Some(action) = action {
            self.apply_menu(action);
        }
    }

    fn menu_body(&mut self, ui: &mut Ui) -> Option<MenuAction> {
        let session = self.play.session().is_some();
        ui.label(RichText::new(self.emulator.game_name()).size(18.0).strong());
        let state = match self.play.session() {
            Some(session) => format!(
                "Still running with {}",
                playing::member_name(&self.play, &self.online, session.friend())
            ),
            None => "Paused".to_owned(),
        };
        ui.label(
            RichText::new(state)
                .size(13.0)
                .color(kit::value_color(ui.visuals())),
        );
        ui.add_space(8.0);
        let facts = self.tile_facts();
        let shown = tiles(&facts);
        let has = |tile: Tile| shown.contains(&tile);
        let saved = self.ui.snapshots.slots().len();
        let fast = if self.emulator.fast_forward {
            self.emulator.fast_forward_speed.to_string()
        } else {
            "Off".to_owned()
        };
        let mut rows: Vec<(Row<'_>, MenuAction)> =
            vec![(Row::new("Resume").icon(icons::PLAY), MenuAction::Resume)];
        let watching = self.play.session().is_some_and(|s| s.watching_only());
        let states = facts.has_rom && !watching && !self.emulator.partnered();
        if session {
            rows.push((Row::new("Chat").icon(icons::CHAT_CIRCLE), MenuAction::Chat));
        }
        if states && !session {
            rows.push((
                Row::new("Save state").icon(icons::FLOPPY_DISK),
                MenuAction::More(MoreAction::SaveNow),
            ));
        }
        if states {
            let count = if saved > 0 {
                format!("{saved} saved")
            } else {
                String::new()
            };
            rows.push((
                Row::new("States").icon(icons::STACK).value(count),
                MenuAction::More(MoreAction::AllStates),
            ));
        }
        if !session {
            rows.push((
                Row::new("Fast forward")
                    .icon(icons::FAST_FORWARD)
                    .value(fast)
                    .no_chevron(),
                MenuAction::FastForward,
            ));
        }
        rows.push((
            Row::new("Screenshot").icon(icons::CAMERA).no_chevron(),
            MenuAction::More(MoreAction::Screenshot),
        ));
        rows.push((
            Row::new("Move screen")
                .icon(icons::ARROWS_OUT_CARDINAL)
                .no_chevron(),
            MenuAction::MoveScreen,
        ));
        if has(Tile::PlayTogether) {
            rows.push((
                Row::new("Play together").icon(icons::USERS),
                MenuAction::PlayTogether,
            ));
        }
        if has(Tile::GameInfo) {
            rows.push((
                Row::new("Game info").icon(icons::IDENTIFICATION_CARD),
                MenuAction::More(MoreAction::GameInfo),
            ));
        }
        let mut action = None;
        let focus_first = self.big.take_menu_focus();
        kit::list(ui, |list| {
            for (index, (row, picked)) in rows.into_iter().enumerate() {
                let response = list.row(row);
                if index == 0 && focus_first {
                    response.request_focus();
                }
                if response.clicked() {
                    action = Some(picked);
                }
            }
        });
        if session {
            action = action.or(self.more_together(ui).map(MenuAction::More));
        }
        let mut last: Vec<(Row<'_>, MenuAction)> = Vec::new();
        if has(Tile::Reset) {
            last.push((
                Row::new("Reset")
                    .icon(icons::ARROW_COUNTER_CLOCKWISE)
                    .no_chevron(),
                MenuAction::More(MoreAction::Reset),
            ));
        }
        if has(Tile::CloseGame) {
            last.push((
                Row::new("Close game").icon(icons::X).no_chevron(),
                MenuAction::More(MoreAction::CloseGame),
            ));
        }
        if !crate::platform::is_tv() {
            last.push((
                Row::new("Leave big screen")
                    .icon(icons::TELEVISION_SIMPLE)
                    .no_chevron(),
                MenuAction::LeaveBigScreen,
            ));
        }
        ui.add_space(12.0);
        kit::list(ui, |list| {
            for (row, picked) in last {
                if list.row(row).clicked() {
                    action = Some(picked);
                }
            }
        });
        action
    }

    fn apply_menu(&mut self, action: MenuAction) {
        match action {
            MenuAction::Resume => self.resume_game(),
            MenuAction::More(more) => self.apply_more(more),
            MenuAction::FastForward => {
                self.emulator.fast_forward = !self.emulator.fast_forward;
            }
            MenuAction::Chat => {
                if let Some(session) = self.play.session() {
                    let friend = session.friend();
                    self.ui.friends.chat.open(friend);
                    self.go_place(Place::Friends);
                }
            }
            MenuAction::PlayTogether => self.go_place(Place::Friends),
            MenuAction::MoveScreen => self.start_moving_screen(),
            MenuAction::LeaveBigScreen => self.set_big_screen(false),
        }
    }
}
