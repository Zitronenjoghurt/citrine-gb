use super::{MoreAction, Tile, tile_action, tile_face, tile_locked, tiles};
use crate::app::Citrine;
use crate::app::big_screen::scroll::Following;
use crate::app::focus::ScreenPlace;
use crate::app::layout_sheet::{LayoutSheet, SheetGo};
use crate::app::mobile::kit::{self, Row, SliderRow};
use crate::app::mobile::nav::Place;
use crate::app::mobile::sheet;
use crate::app::tabs::playing;
use crate::app::ui_state::ds_screens::ScreenContext;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::screens_menu;
use crate::app::widgets::settings::this_game;
use crate::icons;
use egui::{Color32, Frame, Margin, Rect, RichText, Ui, pos2, vec2};

const MENU_WIDTH: f32 = 300.0;
const GAME_INSET_Y: f32 = 4.0;
const ROW_MOST: f32 = kit::ROW_HEIGHT * 2.0;
const MENU_PAD_Y: i8 = 16;
const FOOT_ROOM: i8 = 12;

#[derive(Debug, Clone, PartialEq)]
enum MenuAction {
    Resume,
    More(MoreAction),
    FastForward,
    Chat,
    PlayTogether,
    Layout,
    Hotkey(HotkeyAction),
    Volume(f32),
    LeaveBigScreen,
}

impl Citrine {
    pub(in crate::app) fn open_game_menu(&mut self) {
        self.mobile.nav.show_game();
        self.open_more();
        self.emulator.release_all();
        self.big.focus_menu();
        self.cancel_layout_edit();
    }

    pub(in crate::app) fn big_game(&mut self, ui: &mut Ui) {
        self.emulator.screen_context = ScreenContext::Landscape;
        let area = ui.max_rect();
        if !self.mobile.nav.more_open() {
            self.ui.layout_sheet = None;
            self.ui.toast_area = Some(area);
            let touch =
                self.ui.settings.layout.is_touch(ui.ctx()) && self.mobile.hud_sheet.is_none();
            let screen = self.placed_game(ui, area, touch);
            if let Some(screen) = screen
                && self.editing_layout()
            {
                self.edit_hints(ui, area, screen);
            }
            return;
        }
        if let Some(key) = self.emulator.saves_key() {
            self.ui.snapshots.sync(ui.ctx(), &self.emulator.store, &key);
        }
        let width = MENU_WIDTH.min(area.width() * 0.45);
        let menu = Rect::from_min_size(area.min, vec2(width, area.height()));
        let game = area.with_min_x(menu.right());
        ui.painter().rect_filled(game, 0.0, egui::Color32::BLACK);
        self.ui.toast_area = Some(game);
        self.paint_screen(
            ui,
            game.shrink2(vec2(16.0, GAME_INSET_Y)),
            false,
            ScreenPlace::default(),
        );
        let fill = kit::card_fill(ui.visuals());
        let mut column = ui.new_child(egui::UiBuilder::new().max_rect(menu));
        if self.ui.layout_sheet.is_some() {
            self.layout_panel(&mut column, menu, fill);
            return;
        }
        let action = Frame::new()
            .fill(fill)
            .inner_margin(Margin {
                left: 12,
                right: 12,
                top: MENU_PAD_Y,
                bottom: MENU_PAD_Y - FOOT_ROOM,
            })
            .show(&mut column, |ui| {
                ui.set_min_size(menu.size() - vec2(24.0, f32::from(2 * MENU_PAD_Y - FOOT_ROOM)));
                crate::app::widgets::scroll::gutter(ui);
                let bar = &mut ui.spacing_mut().scroll;
                bar.dormant_handle_opacity = bar.active_handle_opacity;
                let list = egui::ScrollArea::vertical()
                    .id_salt("game_menu")
                    .auto_shrink(false)
                    .show_following(ui, |ui| {
                        let action = self.menu_body(ui);
                        ui.add_space(f32::from(FOOT_ROOM));
                        action
                    });
                fade_foot(ui, list.inner_rect, fill);
                list.inner
            })
            .inner;
        if let Some(action) = action {
            self.apply_menu(action);
        }
    }

    fn layout_panel(&mut self, column: &mut Ui, menu: Rect, fill: egui::Color32) {
        let device = self.layout_device(column.ctx());
        let facts = self.ui.layout_facts(&device);
        let sheet_menu = self.ui.layout_sheet.as_ref().and_then(|sheet| sheet.menu);
        let focus = self
            .ui
            .layout_sheet
            .as_mut()
            .is_some_and(LayoutSheet::take_first_focus);
        let shoulders = facts.game.is_some().then(|| self.shown_family());
        let picked = Frame::new()
            .fill(fill)
            .inner_margin(sheet::bar_margin(12.0, 16.0))
            .show(column, |ui| {
                ui.set_min_size(menu.size() - vec2(12.0 + sheet::BAR_EDGE, 32.0));
                egui::ScrollArea::vertical()
                    .id_salt("layout_panel")
                    .auto_shrink(false)
                    .show_following(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        sheet::gutter(ui, 12.0, |ui| {
                            crate::app::mobile::screens::sheet_body(
                                ui, &facts, sheet_menu, true, shoulders, focus,
                            )
                        })
                    })
                    .inner
            })
            .inner;
        if let Some(pick) = picked
            && let Some(go) = self.ui.pick_in_sheet(&device, pick)
        {
            if matches!(go, SheetGo::Edit(_)) {
                self.mobile.nav.show_game();
            }
            self.sheet_go(go, true);
        }
    }

    fn menu_body(&mut self, ui: &mut Ui) -> Option<MenuAction> {
        let session = self.play.session().is_some();
        let named = crate::app::big_screen::tab_names(ui.ctx(), self.emulator.game_name());
        if !named {
            ui.label(RichText::new(self.emulator.game_name()).size(18.0).strong());
        }
        let state = match self.play.session() {
            Some(session) => Some(format!(
                "Still running with {}",
                playing::member_name(&self.play, &self.online, session.friend())
            )),
            None => (!named).then(|| "Paused".to_owned()),
        };
        if let Some(state) = state {
            ui.label(
                RichText::new(state)
                    .size(13.0)
                    .color(kit::value_color(ui.visuals())),
            );
        }
        if !named || session {
            ui.add_space(8.0);
        }
        let facts = self.tile_facts();
        let recording = self.emulator.is_recording();
        let hardcore = self.emulator.hardcore();
        let (record_icon, record_label) = tile_face(Tile::Record, recording);
        let shown = tiles(&facts);
        let has = |tile: Tile| shown.contains(&tile);
        let saved = self.ui.snapshots.slots().len();
        let fast = if self.emulator.fast_forward {
            self.emulator.fast_forward_speed.to_string()
        } else {
            "Off".to_owned()
        };
        let mut rows: Vec<(Row<'_>, MenuAction)> = vec![(
            Row::new("Resume").icon(icons::PLAY).no_chevron(),
            MenuAction::Resume,
        )];
        let watching = self.play.session().is_some_and(|s| s.watching_only());
        let states = facts.has_rom && !watching && !self.emulator.partnered();
        if session {
            rows.push((Row::new("Chat").icon(icons::CHAT_CIRCLE), MenuAction::Chat));
        }
        if states && !session {
            rows.push((
                Row::new("Save state").icon(icons::SAVE_STATE).no_chevron(),
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
        if has(Tile::Record) {
            rows.push((
                Row::new(&record_label).icon(record_icon).no_chevron(),
                MenuAction::More(MoreAction::Record),
            ));
        }
        rows.push((
            Row::new(screens_menu::TITLE).icon(icons::LAYOUT),
            MenuAction::Layout,
        ));
        let mut volume = self.menu_volume();
        let silent = volume == 0.0;
        let mut after: Vec<(Row<'_>, MenuAction)> = Vec::new();
        let linked = self.emulator.linked() || self.room_linked();
        if linked {
            after.push((
                Row::new("Switch game").icon(icons::SWAP).no_chevron(),
                MenuAction::Hotkey(HotkeyAction::SwitchGame),
            ));
        }
        if self.emulator.two_screens() && !linked {
            after.push((
                Row::new("Swap screens")
                    .icon(icons::ARROWS_DOWN_UP)
                    .no_chevron(),
                MenuAction::Hotkey(HotkeyAction::SwapScreens),
            ));
        }
        if let Some(closed) = self.emulator.lid() {
            after.push((
                Row::new("Lid")
                    .icon(HotkeyAction::ToggleLid.icon(closed))
                    .value(if closed { "Closed" } else { "Open" })
                    .no_chevron(),
                MenuAction::Hotkey(HotkeyAction::ToggleLid),
            ));
        }
        if has(Tile::PlayTogether) {
            after.push((
                Row::new("Play together").icon(icons::USERS),
                MenuAction::PlayTogether,
            ));
        }
        if has(Tile::GameInfo) {
            after.push((
                Row::new("Game info").icon(icons::INFO),
                MenuAction::More(MoreAction::GameInfo),
            ));
        }
        if has(Tile::ThisGame) {
            after.push((
                Row::new(this_game::TITLE).icon(icons::GEAR_FINE),
                MenuAction::More(MoreAction::ThisGame),
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
            let slid = list.slider(
                SliderRow::new("Volume", &mut volume, 0.0..=1.0, 0.01)
                    .icon(HotkeyAction::ToggleMute.icon(silent)),
            );
            if slid.changed() {
                action = Some(MenuAction::Volume(volume));
            }
            for (row, picked) in after {
                if list.row(row).clicked() {
                    action = Some(picked);
                }
            }
        });
        if session {
            action = action.or(self.more_together(ui).map(MenuAction::More));
        }
        let listed = [
            Tile::Screenshot,
            Tile::Record,
            Tile::PlayTogether,
            Tile::GameInfo,
            Tile::ThisGame,
            Tile::Reset,
            Tile::CloseGame,
            Tile::BigScreen,
        ];
        let others: Vec<(&'static str, String, Tile)> = shown
            .iter()
            .filter(|tile| !listed.contains(tile))
            .map(|&tile| {
                let (icon, label) = tile_face(tile, recording);
                let icon = if tile_locked(tile, hardcore) {
                    icons::LOCK
                } else {
                    icon
                };
                (icon, label.replace('\n', " "), tile)
            })
            .collect();
        ui.add_space(12.0);
        kit::list(ui, |list| {
            for (icon, label, tile) in &others {
                let row = Row::new(label).icon(icon);
                let row = if matches!(tile, Tile::FullScreen) {
                    row.no_chevron()
                } else {
                    row
                };
                if list.row(row).clicked() {
                    action = Some(MenuAction::More(tile_action(*tile)));
                }
            }
        });
        let mut last: Vec<(Row<'_>, MenuAction)> = Vec::new();
        if has(Tile::Reset) {
            last.push((
                Row::new("Reset").icon(icons::POWER).no_chevron(),
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

    pub(super) fn menu_volume(&self) -> f32 {
        if self.ui.settings.muted {
            0.0
        } else {
            self.ui.settings.volume
        }
    }

    pub(super) fn set_menu_volume(&mut self, volume: f32) {
        let settings = &mut self.ui.settings;
        settings.muted = volume == 0.0;
        if volume > 0.0 {
            settings.volume = volume;
        }
        settings.dirty = true;
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
            MenuAction::Layout => self.ui.layout_sheet = Some(LayoutSheet::default()),
            MenuAction::Hotkey(hotkey) => self.run_hotkey(hotkey),
            MenuAction::Volume(volume) => self.set_menu_volume(volume),
            MenuAction::LeaveBigScreen => self.set_big_screen(false),
        }
    }
}

fn fade_foot(ui: &Ui, view: Rect, fill: Color32) {
    let view = shown(view, ui.visuals().clip_rect_margin, ui.clip_rect());
    let layer = ui.layer_id();
    let rows: Vec<Rect> = ui.ctx().viewport(|v| {
        v.this_pass
            .widgets
            .layers()
            .filter(|(id, _)| **id == layer)
            .flat_map(|(_, widgets)| widgets.iter())
            .map(|widget| widget.rect)
            .collect()
    });
    if let Some(foot) = foot_fade(&rows, view) {
        fade(ui.painter(), foot, fill);
    }
}

fn fade(painter: &egui::Painter, rect: Rect, fill: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(rect.right_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(rect.left_bottom(), fill);
    mesh.colored_vertex(rect.right_bottom(), fill);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}

fn in_column(row: &Rect, view: Rect) -> bool {
    row.height() <= ROW_MOST && row.left() >= view.left() - 1.0 && row.right() <= view.right() + 1.0
}

fn shown(view: Rect, clip_margin: f32, clip: Rect) -> Rect {
    view.with_max_y((view.bottom() + clip_margin).min(clip.bottom()))
}

fn foot_fade(rows: &[Rect], view: Rect) -> Option<Rect> {
    let top = rows
        .iter()
        .filter(|row| {
            in_column(row, view) && row.top() < view.bottom() && row.bottom() > view.bottom() + 0.5
        })
        .map(|row| row.top())
        .reduce(f32::min)?;
    let top = (top - 1.0).max(view.bottom() - crate::app::big_screen::scroll::PEEK);
    Some(Rect::from_min_max(
        pos2(view.left(), top),
        view.right_bottom(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(top: f32) -> Rect {
        Rect::from_min_size(pos2(10.0, top), vec2(200.0, kit::ROW_HEIGHT))
    }

    #[test]
    fn a_row_cut_to_a_sliver_at_the_menu_foot_fades_out() {
        let view = Rect::from_min_max(pos2(0.0, 0.0), pos2(220.0, 404.0));
        let rows = [row(300.0), row(350.0), row(400.0)];
        let foot = foot_fade(&rows, view).expect("a fade");
        assert_eq!((foot.top(), foot.bottom()), (399.0, 404.0));
    }

    #[test]
    fn a_row_starting_at_the_foot_shows_in_the_clip_margin_and_fades_out() {
        let inner = Rect::from_min_max(pos2(0.0, 0.0), pos2(220.0, 400.0));
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(240.0, 420.0));
        let view = shown(inner, 3.0, clip);
        assert_eq!(view.bottom(), 403.0);
        let rows = [row(300.0), row(350.0), row(400.0)];
        assert_eq!(foot_fade(&rows, view).map(|foot| foot.top()), Some(399.0));
    }

    #[test]
    fn a_deep_peek_fades_only_its_foot_and_the_last_whole_row_stays_clear() {
        let view = Rect::from_min_max(pos2(0.0, 0.0), pos2(220.0, 440.0));
        let rows = [row(300.0), row(350.0), row(400.0)];
        let foot = foot_fade(&rows, view).expect("a fade");
        assert_eq!(foot.top(), 440.0 - crate::app::big_screen::scroll::PEEK);
        assert!(foot.top() >= 400.0);
    }

    #[test]
    fn the_end_of_the_list_does_not_fade() {
        let view = Rect::from_min_max(pos2(0.0, 0.0), pos2(220.0, 400.0));
        assert_eq!(foot_fade(&[row(300.0), row(350.0)], view), None);
    }
}
