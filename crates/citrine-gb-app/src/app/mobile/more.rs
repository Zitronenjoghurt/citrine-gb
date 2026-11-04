use super::kit;
use crate::app::Citrine;
use crate::app::events::AppEvent;
use crate::app::tabs::stats::Page;
use crate::app::tabs::{Tab, playing};
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::settings::{console_name, other_console, plays_on_both};
use crate::app::widgets::avatar;
use crate::icons;
use crate::online::Seen;
use crate::play::Stage;
use citrine_gb::gb::GbModel;
use citrine_gb_net::OnlineId;
use egui::{Align2, CornerRadius, Frame, Id, Margin, Order, Rect, RichText, Sense, Ui, pos2, vec2};

const SHEET_MAX_WIDTH: f32 = 560.0;
const CLOSE_DRAG: f32 = 70.0;
const TILE_HEIGHT: f32 = 76.0;
const COLUMNS: usize = 4;
const SHOWN_STATES: usize = 3;

#[derive(Debug, Default)]
pub struct MoreState {
    friends: bool,
    drag: f32,
    loading: Option<usize>,
    content: f32,
    chrome: f32,
}

#[derive(Debug, Clone, PartialEq)]
enum MoreAction {
    Close,
    Together(playing::BarAction),
    FriendSound(playing::FriendSound),
    LinkEvents,
    Unlink,
    SaveNow,
    Load(usize),
    AllStates,
    Screenshot,
    Record,
    Reset,
    PlayOn(GbModel),
    LinkGame,
    PlayTogether,
    Invite(OnlineId),
    BackToActions,
    GameInfo,
    Stats,
    FullScreen,
    CloseGame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Screenshot,
    Record,
    Reset,
    PlayOn(GbModel),
    LinkGame,
    PlayTogether,
    GameInfo,
    Stats,
    FullScreen,
    CloseGame,
}

pub struct TileFacts {
    pub session: bool,
    pub alone: bool,
    pub has_rom: bool,
    pub other_console: Option<GbModel>,
    pub joined: bool,
    pub recognised: bool,
    pub web: bool,
}

pub fn tiles(facts: &TileFacts) -> Vec<Tile> {
    let mut tiles = vec![Tile::Screenshot, Tile::Record];
    if facts.recognised {
        tiles.push(Tile::GameInfo);
    }
    if !facts.session && facts.joined {
        tiles.push(Tile::PlayTogether);
    }
    if !facts.session && facts.has_rom {
        tiles.push(Tile::Reset);
    }
    if let Some(model) = facts.other_console.filter(|_| !facts.session) {
        tiles.push(Tile::PlayOn(model));
    }
    if facts.alone && facts.has_rom {
        tiles.push(Tile::LinkGame);
    }
    if facts.has_rom {
        tiles.push(Tile::Stats);
    }
    if facts.web {
        tiles.push(Tile::FullScreen);
    }
    if !facts.session {
        tiles.push(Tile::CloseGame);
    }
    tiles
}

fn tile_face(tile: Tile, recording: bool) -> (&'static str, String) {
    match tile {
        Tile::Screenshot => (icons::CAMERA, "Screenshot".into()),
        Tile::Record if recording => (icons::STOP, "Stop recording".into()),
        Tile::Record => (icons::RECORD, "Record video".into()),
        Tile::Reset => (icons::ARROW_COUNTER_CLOCKWISE, "Reset".into()),
        Tile::PlayOn(model) => (icons::SWAP, format!("Play on\n{}", console_name(model))),
        Tile::LinkGame => (icons::LINK, "Link a game".into()),
        Tile::PlayTogether => (icons::USERS, "Play together".into()),
        Tile::GameInfo => (icons::IDENTIFICATION_CARD, "Game info".into()),
        Tile::Stats => (icons::CHART_BAR, "Stats".into()),
        Tile::FullScreen => (icons::FRAME_CORNERS, "Full screen".into()),
        Tile::CloseGame => (icons::X, "Close game".into()),
    }
}

fn tile_action(tile: Tile) -> MoreAction {
    match tile {
        Tile::Screenshot => MoreAction::Screenshot,
        Tile::Record => MoreAction::Record,
        Tile::Reset => MoreAction::Reset,
        Tile::PlayOn(model) => MoreAction::PlayOn(model),
        Tile::LinkGame => MoreAction::LinkGame,
        Tile::PlayTogether => MoreAction::PlayTogether,
        Tile::GameInfo => MoreAction::GameInfo,
        Tile::Stats => MoreAction::Stats,
        Tile::FullScreen => MoreAction::FullScreen,
        Tile::CloseGame => MoreAction::CloseGame,
    }
}

impl Citrine {
    pub(super) fn open_more(&mut self) {
        self.mobile.more = MoreState::default();
        self.mobile.nav.set_more(true);
        self.ui.snapshots.invalidate();
    }

    fn leave_more(&mut self) {
        self.mobile.nav.set_more(false);
        self.release_phone_pause();
    }

    pub(super) fn save_state_with_undo(&mut self) {
        let Some(key) = self.emulator.rom_key().map(str::to_owned) else {
            return;
        };
        let slot = self.emulator.store.next_free_slot(&key);
        match self.emulator.save_snapshot(slot) {
            Ok(()) => {
                self.toasts
                    .success("State saved")
                    .button("Undo", AppEvent::UndoSnapshot { rom_key: key, slot });
            }
            Err(err) => {
                self.toasts
                    .error(format!("Couldn't save the state: {err:?}"));
            }
        }
        self.ui.snapshots.invalidate();
    }

    pub(in crate::app) fn undo_snapshot(&mut self, rom_key: &str, slot: usize) {
        if self.emulator.rom_key() != Some(rom_key) {
            return;
        }
        match self.emulator.delete_snapshot(slot) {
            Ok(()) => self.toasts.info("State removed"),
            Err(err) => self
                .toasts
                .error(format!("Couldn't remove the state: {err:?}")),
        };
        self.ui.snapshots.invalidate();
    }

    fn tile_facts(&self) -> TileFacts {
        let session = self.play.session().is_some();
        let both = self
            .emulator
            .as_gb()
            .is_some_and(|gb| plays_on_both(gb.cartridge.header.cgb_mode));
        let has_rom = self.emulator.rom_key().is_some();
        TileFacts {
            session,
            alone: !session && self.emulator.link.is_none() && self.emulator.partner.is_none(),
            has_rom,
            other_console: (both && has_rom).then(|| other_console(self.emulator.console())),
            joined: self.ui.settings.online.is_joined(),
            recognised: self.game_info.recognised(),
            web: cfg!(target_arch = "wasm32"),
        }
    }

    pub(super) fn more_sheet(&mut self, ctx: &egui::Context) {
        if !self.mobile.nav.more_open() {
            return;
        }
        if let Some(key) = self.emulator.rom_key().map(str::to_owned) {
            self.ui.snapshots.sync(ctx, &self.emulator.store, &key);
        }
        let screen = super::sheet::area(ctx).unwrap_or(ctx.content_rect());
        let dim = egui::Area::new(Id::new("more_scrim"))
            .order(Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, kit::scrim(ui.visuals()));
                response.clicked()
            })
            .inner;

        let width = screen.width().min(SHEET_MAX_WIDTH);
        let fill = kit::card_fill(&ctx.style().visuals);
        let drag = self.mobile.more.drag;
        let mut action = None;
        let most = screen.height() * 0.8;
        let scroll_height = match self.mobile.more.content {
            content if content > 0.0 => content.min(most),
            _ => most,
        };
        let chrome = match self.mobile.more.chrome {
            chrome if chrome > 0.0 => chrome,
            _ => 40.0,
        };
        let top = screen.bottom() - scroll_height - chrome + drag;
        let mut scrolled = None;
        let shown = egui::Area::new(Id::new("more_sheet"))
            .order(Order::Foreground)
            .fixed_pos(pos2(screen.center().x - width / 2.0, top))
            .show(ctx, |ui| {
                ui.set_max_height(scroll_height + chrome);
                Frame::new()
                    .fill(fill)
                    .corner_radius(CornerRadius {
                        nw: 20,
                        ne: 20,
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 0,
                        bottom: 27,
                    })
                    .show(ui, |ui| {
                        ui.set_width(width - 28.0);
                        self.grab_handle(ui);
                        let output = egui::ScrollArea::vertical()
                            .id_salt("more_scroll")
                            .max_height(scroll_height)
                            .show(ui, |ui| {
                                action = if self.mobile.more.friends {
                                    self.more_friends(ui)
                                } else {
                                    self.more_body(ui)
                                };
                            });
                        scrolled = Some((output.content_size.y, output.inner_rect.height()));
                    });
            });
        if let Some((content, viewport)) = scrolled {
            let chrome = shown.response.rect.height() - viewport;
            let more = &mut self.mobile.more;
            if (content - more.content).abs() > 0.5 || (chrome - more.chrome).abs() > 0.5 {
                more.content = content;
                more.chrome = chrome;
                ctx.request_repaint();
            }
        }
        if self.mobile.more.drag > CLOSE_DRAG || dim {
            action = Some(MoreAction::Close);
        }
        if let Some(action) = action {
            self.apply_more(action);
        }
    }

    fn grab_handle(&mut self, ui: &mut Ui) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::drag());
        let bar = Rect::from_center_size(rect.center(), vec2(36.0, 4.0));
        ui.painter().rect_filled(
            bar,
            CornerRadius::same(2),
            ui.visuals().weak_text_color().gamma_multiply(0.6),
        );
        if response.dragged() {
            self.mobile.more.drag = (self.mobile.more.drag + response.drag_delta().y).max(0.0);
        } else if response.drag_stopped() && self.mobile.more.drag <= CLOSE_DRAG {
            self.mobile.more.drag = 0.0;
        }
    }

    fn more_body(&mut self, ui: &mut Ui) -> Option<MoreAction> {
        let mut action = None;
        if self.play.session().is_some() {
            action = action.or(self.more_together(ui));
        } else if self.emulator.link.is_some() {
            kit::section_label(ui, "Linked games");
            let picked = kit::rows(
                ui,
                vec![
                    kit::Row::new("Link events").icon(icons::LIST_BULLETS),
                    kit::Row::new("Unlink").icon(icons::LINK_BREAK).no_chevron(),
                ],
            );
            action = action.or(match picked {
                Some(0) => Some(MoreAction::LinkEvents),
                Some(_) => Some(MoreAction::Unlink),
                None => None,
            });
        }
        let watching_only = self.play.session().is_some_and(|s| s.watching_only());
        if self.emulator.rom_key().is_some() && !watching_only && self.emulator.partner.is_none() {
            action = action.or(self.more_states(ui));
        }
        kit::section_label(ui, "This game");
        action.or(self.more_tiles(ui))
    }

    fn more_together(&mut self, ui: &mut Ui) -> Option<MoreAction> {
        let session = self.play.session()?;
        let friend = session.friend();
        let name = playing::member_name(&self.play, &self.online, friend);
        let room = self.play.room().is_some_and(|r| r.members().len() > 1);
        let mut action = None;
        if room {
            kit::section_label(ui, "Room");
        } else {
            kit::section_label(ui, &format!("With {name}"));
        }
        let switches = playing::bar_switches(session, &name);
        let sound = playing::FriendSound::of(&self.ui.settings.online);
        let hears_friend = playing::watching(session) && !session.watching_only();
        let leave = if session.stage() == Stage::Inviting {
            "Take back the invite"
        } else if room {
            "Leave the room"
        } else if session.watching_only() {
            "Stop watching"
        } else {
            "Stop playing together"
        };
        let can_change = session.stage() != Stage::Inviting && !session.watching_only();
        kit::card(ui, |ui| {
            for item in &switches {
                let mut on = item.on;
                let missing = item.switch == playing::Switch::Watch
                    && session.stage() == Stage::Playing
                    && session.console(false).is_none();
                let changed = kit::SwitchRow::new(&item.label, &mut on)
                    .detail(item.why_off.as_deref())
                    .enabled(!missing)
                    .show(ui);
                if changed {
                    action = Some(MoreAction::Together(match item.switch {
                        playing::Switch::Show => playing::BarAction::Show(on),
                        playing::Switch::Watch => playing::BarAction::Watch(on),
                        playing::Switch::Cable => playing::BarAction::Cable(on),
                    }));
                }
                kit::divider(ui);
            }
            if hears_friend {
                kit::padded(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(changed) = playing::friend_sound(ui, sound) {
                            action = Some(MoreAction::FriendSound(changed));
                        }
                        ui.label(format!("{name}'s sound"));
                    });
                });
                kit::divider(ui);
            }
            if can_change
                && kit::Row::new("Change game")
                    .icon(icons::SIM_CARD)
                    .show(ui)
                    .clicked()
            {
                action = Some(MoreAction::Together(playing::BarAction::ChangeGame));
            }
            if can_change {
                kit::divider(ui);
            }
            if kit::Row::new(leave)
                .icon(icons::SIGN_OUT)
                .no_chevron()
                .danger()
                .show(ui)
                .clicked()
            {
                action = Some(MoreAction::Together(playing::BarAction::Leave));
            }
        });
        action
    }

    fn more_states(&mut self, ui: &mut Ui) -> Option<MoreAction> {
        let mut action = None;
        if kit::section_label_button(ui, "States", "All", "Saves and every state") {
            action = Some(MoreAction::AllStates);
        }
        let gap = 8.0;
        let width = ((ui.available_width() - gap * 3.0) / 4.0).floor();
        let picture = vec2(width, width * self.emulator.video().aspect());
        let mut slots: Vec<_> = self.ui.snapshots.slots().iter().collect();
        slots.sort_by_key(|slot| std::cmp::Reverse(slot.saved_at));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let (rect, response) =
                ui.allocate_exact_size(vec2(width, picture.y + 22.0), Sense::click());
            let frame = Rect::from_min_size(rect.min, picture);
            ui.painter().rect_stroke(
                frame,
                CornerRadius::same(8),
                kit::hairline(ui.visuals()),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                frame.center(),
                Align2::CENTER_CENTER,
                icons::PLUS,
                egui::FontId::proportional(22.0),
                ui.visuals().text_color(),
            );
            caption(ui, rect, "Save now");
            if response.on_hover_text("Save the state now").clicked() {
                action = Some(MoreAction::SaveNow);
            }
            for slot in slots.into_iter().take(SHOWN_STATES) {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(width, picture.y + 22.0), Sense::click());
                let frame = Rect::from_min_size(rect.min, picture);
                egui::Image::new((slot.texture.id(), picture))
                    .corner_radius(CornerRadius::same(8))
                    .paint_at(ui, frame);
                let when = crate::app::widgets::snapshots::relative_time(slot.saved_at);
                caption(ui, rect, &when);
                if response.on_hover_text(slot.display_name()).clicked() {
                    action = Some(MoreAction::Load(slot.slot));
                }
            }
        });
        action
    }

    fn more_tiles(&mut self, ui: &mut Ui) -> Option<MoreAction> {
        let recording = self.emulator.screen_recorder.is_recording();
        let tiles = tiles(&self.tile_facts());
        let mut action = None;
        let gap = 6.0;
        let width =
            ((ui.available_width() - gap * (COLUMNS as f32 - 1.0)) / COLUMNS as f32).floor();
        for row in tiles.chunks(COLUMNS) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for tile in row {
                    let (icon, label) = tile_face(*tile, recording);
                    if action_tile(ui, vec2(width, TILE_HEIGHT), icon, &label) {
                        action = Some(tile_action(*tile));
                    }
                }
            });
        }
        action
    }

    fn more_friends(&mut self, ui: &mut Ui) -> Option<MoreAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            let side = ui.spacing().interact_size.y;
            if ui
                .add(kit::header_button(icons::ARROW_LEFT, side))
                .on_hover_text("Back")
                .clicked()
            {
                action = Some(MoreAction::BackToActions);
            }
            ui.label(RichText::new("Play together with").size(18.0).strong());
        });
        let mut people: Vec<_> = self
            .online
            .people()
            .map(|friend| {
                (
                    friend.online_id,
                    friend.display_name.clone(),
                    self.online.presence(friend),
                )
            })
            .collect();
        people.sort_by_key(|(_, name, seen)| (!matches!(seen, Some(Seen::Online)), name.clone()));
        if people.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("No friends yet").weak());
            return action;
        }
        kit::card(ui, |ui| {
            let count = people.len();
            for (index, (id, name, seen)) in people.into_iter().enumerate() {
                let detail = match seen {
                    Some(Seen::Online) => "Online",
                    Some(Seen::Away) => "Away",
                    _ => "Offline",
                };
                let response = ui
                    .horizontal(|ui| {
                        ui.set_min_height(kit::ROW_HEIGHT);
                        ui.add_space(12.0);
                        avatar::avatar(ui, &name, id.value(), 32.0, seen);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&name).size(16.0).strong());
                            ui.label(RichText::new(detail).size(13.0).weak());
                        });
                    })
                    .response
                    .interact(Sense::click());
                if response.clicked() {
                    action = Some(MoreAction::Invite(id));
                }
                if index + 1 < count {
                    kit::divider(ui);
                }
            }
        });
        action
    }

    fn apply_more(&mut self, action: MoreAction) {
        match action {
            MoreAction::Close => self.leave_more(),
            MoreAction::BackToActions => self.mobile.more.friends = false,
            MoreAction::PlayTogether => self.mobile.more.friends = true,
            MoreAction::Together(bar) => {
                let leaving = bar == playing::BarAction::Leave;
                playing::settle_bar(bar, &mut self.play, &mut self.online, &mut self.ui.playing);
                if leaving || bar == playing::BarAction::ChangeGame {
                    self.leave_more();
                }
            }
            MoreAction::FriendSound(sound) => {
                sound.store(&mut self.ui.settings.online);
                self.ui.settings.dirty = true;
            }
            MoreAction::LinkEvents => {
                self.leave_more();
                self.ui.linked.listing = true;
                self.mobile.open(Tab::Playing);
            }
            MoreAction::Unlink => {
                self.leave_more();
                self.events.push(AppEvent::EndLink);
            }
            MoreAction::SaveNow => {
                self.save_state_with_undo();
                self.leave_more();
            }
            MoreAction::Load(slot) => {
                self.leave_more();
                self.mobile.more.loading = Some(slot);
            }
            MoreAction::AllStates => {
                self.leave_more();
                self.mobile.open(Tab::Saves);
            }
            MoreAction::Screenshot => {
                self.leave_more();
                self.run_hotkey(HotkeyAction::Screenshot);
            }
            MoreAction::Record => {
                self.leave_more();
                self.emulator.fast_forward = false;
                self.run_hotkey(HotkeyAction::ToggleRecording);
            }
            MoreAction::Reset => {
                self.leave_more();
                self.run_hotkey(HotkeyAction::SoftReset);
            }
            MoreAction::PlayOn(model) => {
                self.leave_more();
                self.ask_play_on(model);
            }
            MoreAction::LinkGame => {
                self.leave_more();
                self.pick_second_game(true);
            }
            MoreAction::Invite(friend) => {
                self.leave_more();
                self.play_again(friend);
            }
            MoreAction::GameInfo => {
                self.leave_more();
                self.mobile.open(Tab::GameInfo);
            }
            MoreAction::Stats => {
                self.leave_more();
                if let Some(rom) = self.emulator.rom_key().map(str::to_owned) {
                    self.ui.stats.page = Page::Game {
                        rom,
                        name: self.emulator.game_name().to_owned(),
                    };
                }
                self.mobile.open(Tab::Stats);
            }
            MoreAction::FullScreen => {
                self.leave_more();
                #[cfg(target_arch = "wasm32")]
                super::game::toggle_fullscreen();
            }
            MoreAction::CloseGame => {
                self.leave_more();
                self.close_game();
            }
        }
        self.release_touch();
    }

    pub(super) fn close_game(&mut self) {
        if let Err(err) = self.emulator.flush_save() {
            self.toasts
                .error(format!("Couldn't save the game: {err:?}"));
            return;
        }
        if let Err(err) = self.emulator.autosave_if_played() {
            log::error!("could not autosave before closing the game: {err:?}");
        }
        self.end_link();
        self.emulator.unload();
        self.mobile.nav.leave_game();
        self.ui.snapshots.invalidate();
    }

    pub(in crate::app) fn load_state_confirm(&mut self, ctx: &egui::Context) {
        let Some(slot) = self.mobile.more.loading else {
            return;
        };
        let name = self.emulator.snapshot_name(slot);
        let partners = crate::app::tabs::playing::group_partners(&self.play, &self.online);
        let question = match &partners {
            Some(partners) => format!("Load this state? You'll unlink from {partners}."),
            None => format!("Load {name}?"),
        };
        let choice = crate::app::widgets::confirm::confirm_with_detail(
            ctx,
            "load_state_confirm",
            &question,
            Some("Unsaved progress is lost."),
            "Load",
        );
        match choice {
            Some(true) => {
                self.mobile.more.loading = None;
                crate::app::tabs::playing::unlink_group(&mut self.play, &mut self.emulator);
                match self.emulator.load_snapshot(slot) {
                    Ok(_) => self.toasts.success(format!("Loaded {name}")),
                    Err(err) => self
                        .toasts
                        .error(format!("Couldn't load the state: {err:?}")),
                };
            }
            Some(false) => self.mobile.more.loading = None,
            None => {}
        }
    }
}

fn caption(ui: &Ui, rect: Rect, text: &str) {
    let galley = kit::one_line(ui, text, 12.0, ui.visuals().weak_text_color(), rect.width());
    let pos = pos2(
        rect.center().x - galley.size().x / 2.0,
        rect.bottom() - 18.0,
    );
    ui.painter()
        .galley(pos, galley, ui.visuals().weak_text_color());
}

fn action_tile(ui: &mut Ui, size: egui::Vec2, icon: &str, label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        if response.is_pointer_button_down_on() {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(12),
                visuals.widgets.hovered.weak_bg_fill,
            );
        }
        let circle = pos2(rect.center().x, rect.top() + 24.0);
        ui.painter()
            .circle_filled(circle, 20.0, visuals.widgets.inactive.weak_bg_fill);
        ui.painter().text(
            circle,
            Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(20.0),
            visuals.strong_text_color(),
        );
        let mut job = egui::text::LayoutJob::simple(
            label.to_owned(),
            egui::FontId::proportional(12.0),
            visuals.text_color(),
            rect.width() - 2.0,
        );
        job.halign = egui::Align::Center;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let pos = pos2(rect.center().x, rect.top() + 48.0);
        ui.painter().galley(pos, galley, visuals.text_color());
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.replace('\n', " "))
    });
    response.clicked()
}

#[cfg(test)]
mod tests;
