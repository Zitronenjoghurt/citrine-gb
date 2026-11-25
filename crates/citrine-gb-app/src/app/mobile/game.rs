use super::hide;
use super::strip::{
    STRIP_HEIGHT, STRIP_INSET, plan, strip_height, strip_row_width, top_bar_height,
};
use crate::app::Citrine;
use crate::app::focus::ScreenPlace;
use crate::app::layout_edit::ResetButton;
use crate::app::screen_extras;
use crate::app::tabs::{Tab, linked, playing};
use crate::app::toasts;
use crate::app::ui_state::ds_screens::ScreenContext;
use crate::app::ui_state::layout_picks::Preset;
use crate::app::ui_state::screen_layouts::{self, Item, LayoutConsole, LayoutView};
use crate::app::ui_state::touch::TouchSettings;
use crate::app::widgets::clip_panel::{self, ClipFacts};
use crate::app::widgets::layout_editor::{EditItem, PadEdit};
use crate::app::widgets::tilt;
use crate::app::widgets::touch_controls::{TouchLayout, XY_DPAD_FLOOR};
use crate::emulator::layout::ScreenSpots;
use crate::held::InputSource;
use crate::icons;
use crate::local_link::Game;
use citrine_gb::console::{ConsoleKind, Keys, VideoSpec};
use egui::{Rect, Ui, Vec2, vec2};

const OVERLAY_COLUMN: f32 = 0.28;

const MAX_COLUMN: f32 = 0.4;

const CONTROL_BREATHING_ROOM: f32 = 1.15;

const WATCH_GAP: f32 = 8.0;
const WATCH_CAPTION: f32 = 16.0;
const WATCH_MIN_HEIGHT: f32 = 72.0;
const SLIDER_MIN: f32 = 52.0;
const SLIDER_MAX: f32 = 200.0;
const SLIDER_ROOMY: f32 = 150.0;
const BAR_SLACK: f32 = 4.0;
const OPACITY: &str = "Opacity";
const OPACITY_SIZE: f32 = 12.0;
const OPACITY_ROW: f32 = 14.0;
const BAR_KNOB: f32 = 9.0;
const EDIT_TITLE: &str = "Edit layout";

impl Citrine {
    pub(super) fn game_view(&mut self, ui: &mut Ui, landscape: bool) {
        self.emulator.screen_context = if landscape {
            ScreenContext::Landscape
        } else {
            ScreenContext::Portrait
        };
        let watching = self.play.session().is_some_and(|s| s.watching_only())
            || self.emulator.is_playing_tape();
        let hidden = self.mobile.hidden.on();
        let columns = landscape && self.ui.settings.touch.show_controls && !watching;
        let editing = self.editing_layout();
        let page = ui.available_rect_before_wrap();
        if !columns && !hidden {
            if editing {
                self.layout_bar(ui);
            } else {
                self.strip(ui, false);
            }
        }
        self.room_row(ui);
        let clip = self
            .ui
            .clip
            .as_ref()
            .and_then(|view| view.facts(&self.emulator, true))
            .filter(|_| !editing);
        let replay_bottom = if editing || clip.is_some() {
            None
        } else {
            self.record_row(ui)
        };
        self.banners(ui);
        let mut area =
            Rect::from_x_y_ranges(page.x_range(), ui.available_rect_before_wrap().y_range());
        if landscape && self.side_sheet_open() {
            area.max.x = area.max.x.min(super::sheet::side_rect(ui.ctx()).left());
        }
        if let Some(facts) = clip {
            area = self.clip_sheet(ui, area, &facts);
        }
        if let Some(bottom) = replay_bottom {
            area.min.y = bottom.min(area.min.y);
        }
        #[cfg(test)]
        {
            self.ui.replay_room = replay_bottom.map(|bottom| (bottom, area));
        }
        let (buttons, screen) = self.mobile_play(ui, area, landscape);
        self.popups(ui.ctx());
        if hidden && !editing {
            let taken: Vec<Rect> = self
                .mobile
                .hidden
                .taken
                .iter()
                .copied()
                .chain(self.ui.toast_spare)
                .collect();
            let eye = hide::eye_rect(screen, area, landscape, &taken, &self.mobile.hidden.screens);
            #[cfg(test)]
            {
                self.mobile.hidden.eye = Some(eye);
            }
            if hide::eye_button(ui, eye) {
                self.mobile.hidden.keep_shown();
                self.release_touch();
            }
        } else if editing && (hidden || buttons.is_some()) {
            let height = top_bar_height(ui);
            let bottom = buttons
                .map_or(area.top(), |b| b.bottom())
                .max(area.top() + height);
            if hidden {
                let fill = Rect::from_min_size(area.min, egui::vec2(area.width(), height));
                ui.painter().rect_filled(fill, 0.0, ui.visuals().panel_fill);
            }
            let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
                area.min,
                egui::pos2(area.right(), bottom),
            )));
            self.layout_bar(&mut bar);
        } else if let Some(buttons) = buttons {
            let mut column = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(buttons.with_min_x(buttons.left() + 8.0))
                    .layout(egui::Layout::right_to_left(egui::Align::Min).with_main_wrap(true)),
            );
            self.strip(&mut column, true);
        }
    }

    fn clip_sheet(&mut self, ui: &mut Ui, area: Rect, facts: &ClipFacts) -> Rect {
        let margin = 16.0;
        let height = clip_panel::panel_height(ui, true) + margin * 2.0;
        let sheet = Rect::from_min_max(egui::pos2(area.left(), area.bottom() - height), area.max);
        let pictures = self
            .ui
            .clip
            .as_ref()
            .map(|view| view.pictures.clone())
            .unwrap_or_default();
        let action = ui
            .scope_builder(
                egui::UiBuilder::new().max_rect(sheet.shrink(margin)),
                |ui| clip_panel::clip_panel(ui, facts, &pictures),
            )
            .inner;
        ui.painter().hline(
            sheet.x_range(),
            sheet.top(),
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        if let Some(action) = action {
            self.clip_action(action);
        }
        area.with_max_y(sheet.top())
    }

    fn layout_bar(&mut self, ui: &mut Ui) {
        let show_opacity = self.ui.settings.touch.show_controls;
        let reset = self.ui.layout_reset_button();
        let done_label = self.ui.layout_done_label();
        let mut done = false;
        let mut cancel = false;
        let mut pressed_reset = false;
        let done_text = format!("{} {done_label}", icons::CHECK);
        let reset_text = format!("{} {}", reset.icon(), reset.label());
        let percent = super::sheet::percent(self.ui.settings.touch.opacity.into());
        let small = |text: &str| {
            ui.painter()
                .layout_no_wrap(
                    text.to_owned(),
                    egui::FontId::proportional(OPACITY_SIZE),
                    egui::Color32::WHITE,
                )
                .size()
                .x
        };
        let text = |text: &str, strong: bool| {
            let rich = egui::RichText::new(text);
            let rich = if strong { rich.strong() } else { rich };
            egui::WidgetText::from(rich)
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x
        };
        let widths = BarWidths {
            gap: ui.spacing().item_spacing.x,
            done: super::kit::pill_width(ui, &done_text),
            cancel: super::kit::pill_width(ui, crate::app::layout_edit::CANCEL),
            reset_text: super::kit::pill_width(ui, &reset_text),
            reset_icon: super::kit::icon_pill_width(ui),
            percent: small("100%"),
            word: small(OPACITY),
            title: text(EDIT_TITLE, true),
        };
        let bar = bar_plan(&widths, ui.available_width(), show_opacity);
        let layout = egui::Layout::right_to_left(egui::Align::Center);
        let height = top_bar_height(ui);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), height), layout, |ui| {
            ui.set_min_height(height);
            ui.add_space(bar.margin);
            done = super::kit::pill(ui, &done_text, true).clicked();
            cancel = super::kit::pill(ui, crate::app::layout_edit::CANCEL, false).clicked();
            pressed_reset = ui
                .add_enabled_ui(reset != ResetButton::Nothing, |ui| {
                    if bar.reset_text {
                        super::kit::pill(ui, &reset_text, false)
                    } else {
                        super::kit::labeled(super::kit::icon_pill(ui, reset.icon()), reset.label())
                            .on_hover_text(reset.label())
                    }
                })
                .inner
                .clicked();
            if let Some(slider_width) = bar.slider {
                let opacity = &mut self.ui.settings.touch.opacity;
                let title = if bar.title {
                    widths.gap + widths.title
                } else {
                    0.0
                };
                let left = ui.available_width() - bar.margin - title;
                let width = slider_width.min(left - 1.0);
                let band = (height - super::kit::pill_height(ui)) / 2.0;
                let stack = egui::Layout::top_down(egui::Align::Min);
                let changed = ui
                    .allocate_ui_with_layout(vec2(width, height), stack, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add_space(band);
                        let (row, _) =
                            ui.allocate_exact_size(vec2(width, OPACITY_ROW), egui::Sense::hover());
                        let font = egui::FontId::proportional(OPACITY_SIZE);
                        ui.spacing_mut().interact_size.y = height - band - OPACITY_ROW;
                        let ink = ui.visuals().strong_text_color();
                        let painter = ui.painter();
                        let start = row.left_center() + vec2(BAR_KNOB, 0.0);
                        let word = painter.text(
                            start,
                            egui::Align2::LEFT_CENTER,
                            OPACITY,
                            font.clone(),
                            ink,
                        );
                        painter.text(
                            word.right_center() + vec2(widths.gap, 0.0),
                            egui::Align2::LEFT_CENTER,
                            &percent,
                            font,
                            ink,
                        );
                        let range = TouchSettings::MIN_OPACITY..=1.0;
                        super::sheet::slider_with_knob(ui, opacity, range, 0.05, width, BAR_KNOB)
                            .changed()
                    })
                    .inner;
                if changed {
                    self.ui.settings.dirty = true;
                }
            }
            if bar.title {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add_space(bar.margin);
                    ui.label(egui::RichText::new(EDIT_TITLE).strong());
                });
            }
        });
        if pressed_reset {
            self.ui.press_layout_reset();
        }
        if cancel {
            self.cancel_layout_edit();
        } else if done {
            self.done_layout_edit();
        }
    }

    pub(in crate::app) fn release_touch(&mut self) {
        self.mobile.held_buttons.clear();
        let stuck = self.mobile.touch.reset();
        for button in ALL_BUTTONS {
            if stuck.contains(button) {
                self.emulator.release(InputSource::Touch, button);
            }
        }
    }

    fn mobile_play(&mut self, ui: &mut Ui, area: Rect, landscape: bool) -> (Option<Rect>, Rect) {
        let settings = &self.ui.settings;
        let frame = self.emulator.layout().for_area(area.size()).size();
        let kind = self.emulator.played_kind();
        let shoulders = matches!(kind, ConsoleKind::Gba | ConsoleKind::Nds);
        let xy = kind == ConsoleKind::Nds;
        let watching = self.play.session().is_some_and(|s| s.watching_only())
            || self.emulator.is_playing_tape();
        let hidden = self.mobile.hidden.on();
        let view = if landscape {
            LayoutView::Landscape
        } else {
            LayoutView::Portrait
        };
        let spotted = self.plays_phone_default(view)
            || settings.layouts.edited(LayoutConsole::of(kind), view);
        let wanted = settings.touch.show_controls && !watching;
        let one_screen =
            !self.emulator.linked() && !self.emulator.partnered() && !self.friend_shown();
        let strip_buttons = (!hidden).then(|| plan(&self.strip_facts()).buttons.len());
        let beside_sheet = self.side_sheet_open();
        let split = (landscape && wanted).then(|| {
            if beside_sheet {
                sheet_split(area, frame, strip_buttons, xy)
            } else {
                landscape_split(area, settings.touch.landscape, frame, strip_buttons, xy)
            }
        });
        let kept = match split {
            _ if !landscape => {
                portrait_split(area, &settings.touch, one_screen, frame, shoulders, 0.0).0
            }
            Some((screen, ..)) => screen,
            None => area,
        };
        let picture = if landscape {
            hide::picture_in(kept)
        } else {
            kept
        };
        let touched = ui.input(|i| {
            i.events.iter().any(|event| {
                matches!(event, egui::Event::PointerButton { pos, pressed: true, .. } if !picture.contains(*pos))
            })
        });
        let pressed_by = self.emulator.take_pressed_by();
        self.mobile.hidden.note(pressed_by, touched);

        let mut buttons = None;
        let mut reserved = 0.0;
        let (screen, layout, free) = if !wanted {
            (area, None, area)
        } else if !self.editing_layout()
            && !self.mobile.hidden.buttons(settings.touch.hidden_buttons)
        {
            (kept, None, kept)
        } else if let Some((screen, left, right, strip)) = split {
            let right = right.with_min_y(strip.bottom().max(area.top() + STRIP_HEIGHT));
            buttons = Some(strip);
            (
                screen,
                Some(TouchLayout::columns(left, right, &settings.touch, shoulders).with_xy(xy)),
                Rect::from_min_max(
                    egui::pos2(left.right(), strip.bottom().max(area.top())),
                    right.left_bottom(),
                ),
            )
        } else {
            let place = |reserve: f32| {
                let (screen, mut controls) =
                    portrait_split(area, &settings.touch, one_screen, frame, shoulders, reserve);
                if hidden {
                    controls = hide::below_eye(
                        controls,
                        hide::eye_rect(screen, area, false, &[], &[]),
                        area.with_max_y(area.bottom() - reserve),
                    );
                }
                let layout = TouchLayout::bar(controls, &settings.touch, shoulders).with_xy(xy);
                let free = screen.with_max_y(layout.reach().top().max(screen.bottom()));
                (screen, layout, free)
            };
            let (screen, layout, free) = place(0.0);
            let picture = if one_screen {
                picture_in(screen, frame)
            } else {
                screen
            };
            let reserve = if spotted {
                self.toasts.room_needed(ui.ctx())
            } else {
                self.toasts.reserve(ui.ctx(), free, picture)
            };
            let (screen, layout, free) = match reserve {
                Some(reserve) => {
                    self.ui.toast_spare = Some(area.with_min_y(area.bottom() - reserve));
                    reserved = reserve;
                    place(reserve)
                }
                None => (screen, layout, free),
            };
            (screen, Some(layout), free)
        };
        let portrait = layout.is_some() && !landscape;
        self.follow_layout_view(view);
        let room = match buttons {
            Some(strip) => area.with_min_y(strip.bottom().clamp(area.top(), area.bottom())),
            None => area.with_max_y(area.bottom() - reserved),
        };
        let mut layout = layout;
        if let Some(layout) = &mut layout {
            for (item, icon) in self.wanted_buttons() {
                let default = layout.beside_start();
                layout.add_button(item, icon, default);
            }
        }
        let console = LayoutConsole::of(kind);
        let mut spots = self.ui.settings.layouts.spots(console, view);
        if self.plays_phone_default(view) {
            let scale = self.ui.settings.touch.clamped_scale();
            let defaults = if view == LayoutView::Landscape {
                screen_layouts::big_top_phone(scale)
            } else {
                screen_layouts::stacked_phone(scale)
            };
            spots = screen_layouts::with_defaults(spots, defaults);
        }
        let mut items = Vec::new();
        if let Some(layout) = &mut layout {
            for (item, default) in layout.groups() {
                let rect = spots
                    .get(&item)
                    .map_or(default, |spot| spot.rect(room, default.size()));
                layout.place_group(item, rect);
                items.push(EditItem::new(item, rect, default.size()));
            }
        }
        #[cfg(test)]
        if buttons.is_none()
            && let Some(layout) = &layout
        {
            self.ui.touch_bounds = Some(layout.bounds());
        }
        self.ui.toast_area = Some(free);
        if self.emulator.has_game() {
            let picture = if one_screen {
                picture_in(screen, frame)
            } else {
                screen
            };
            self.ui.toast_picture = Some(picture);
        }

        let screen_spot = |item| spots.get(&item).copied();
        self.emulator.screen_spots = Some(ScreenSpots {
            room,
            screen: screen_spot(Item::Screen),
            touch: screen_spot(Item::TouchScreen),
        });
        let painted = self.paint_screen(ui, screen, portrait, ScreenPlace::default());
        self.emulator.screen_spots = None;
        if painted.is_some() {
            let touch = self.emulator.placed_touch();
            let screens = self.emulator.placed_screens().iter().map(|rect| {
                let item = if Some(*rect) == touch {
                    Item::TouchScreen
                } else {
                    Item::Screen
                };
                EditItem::new(item, *rect, rect.size())
            });
            items.splice(0..0, screens);
        }
        let main = self
            .emulator
            .placed_screens()
            .iter()
            .copied()
            .find(|rect| Some(*rect) != self.emulator.placed_touch())
            .unwrap_or(screen);
        let taken: Vec<Rect> = items.iter().map(|item| item.rect).collect();
        let marks = screen_extras::place_marks(
            ui,
            &self.ui,
            LayoutConsole::of(kind),
            view,
            room,
            main,
            &taken,
        );
        items.extend(marks);
        let touch = self.mobile.hud_sheet.is_none();
        let keep_below = (self.editing_layout() && hidden).then(|| area.top() + top_bar_height(ui));
        self.game_hud(ui, screen, self.ui.toast_picture, touch, keep_below);

        self.mobile.hidden.screens = self.emulator.placed_screens().to_vec();
        self.mobile.hidden.taken = self
            .mobile
            .hidden
            .screens
            .iter()
            .copied()
            .chain(layout.iter().flat_map(|layout| layout.targets()))
            .collect();
        if landscape && spotted {
            let screens = &self.mobile.hidden.screens;
            let controls: Vec<Rect> = self
                .mobile
                .hidden
                .taken
                .iter()
                .chain(items.iter().map(|item| &item.rect))
                .filter(|rect| !screens.contains(rect))
                .copied()
                .chain(buttons)
                .collect();
            let taken: Vec<Rect> = controls.iter().chain(screens).copied().collect();
            let width = free.width();
            if let Some(column) = self
                .toasts
                .clear_column(ui.ctx(), area, &taken, width)
                .or_else(|| self.toasts.clear_column(ui.ctx(), area, &controls, width))
            {
                self.ui.toast_spare = Some(column);
            }
        }
        #[cfg(all(test, feature = "nds"))]
        {
            self.ui.touch_targets = layout
                .iter()
                .flat_map(|layout| layout.targets())
                .chain(buttons)
                .collect();
        }

        if self.editing_layout() {
            if let Some(layout) = &layout {
                layout.paint(
                    ui,
                    Keys::empty(),
                    &self.ui.settings.touch,
                    self.emulator.placed_screens(),
                );
            }
            self.release_touch();
            items.extend(self.ui.hud_items());
            self.ui
                .edit_layout(ui, room, screen, &items, PadEdit::default());
            return (buttons, screen);
        }

        let Some(mut layout) = layout else {
            self.release_touch();
            return (buttons, screen);
        };

        layout.claim(ui);

        let held = self.mobile.touch.update(ui, &layout);
        let moved = self.mobile.touch.transitions(held);
        self.mobile.held_buttons = layout
            .singles()
            .filter(|(_, rect)| self.mobile.touch.holds(*rect))
            .map(|(item, _)| item)
            .collect();
        layout.light(self.mobile.held_buttons.clone());

        if moved.tick && self.ui.settings.touch.haptics {
            crate::platform::haptic_tick();
        }

        for button in ALL_BUTTONS {
            if moved.pressed.contains(button) {
                self.emulator.press(InputSource::Touch, button);
            } else if moved.released.contains(button) {
                self.emulator.release(InputSource::Touch, button);
            }
        }

        layout.paint(
            ui,
            held,
            &self.ui.settings.touch,
            self.emulator.placed_screens(),
        );
        (buttons, screen)
    }

    pub(in crate::app) fn plays_phone_default(&self, view: LayoutView) -> bool {
        let (context, preset) = match view {
            LayoutView::Landscape => (ScreenContext::Landscape, Preset::BigTop),
            LayoutView::Portrait => (ScreenContext::Portrait, Preset::Stacked),
            LayoutView::Desktop | LayoutView::BigScreen => return false,
        };
        self.emulator.played_kind() == ConsoleKind::Nds
            && !self.big_screen_on()
            && self.emulator.ds_screens.placement(context) == preset.placement()
    }

    fn friend_shown(&self) -> bool {
        self.play
            .session()
            .is_some_and(|s| playing::friend_picture(&self.play, s).is_some() && !s.watching_only())
    }

    pub(in crate::app) fn paint_screen(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        portrait: bool,
        place: ScreenPlace,
    ) -> Option<(Rect, Rect)> {
        if let Some(session) = self.play.session()
            && let Some((label, progress)) = playing::transfer(
                session,
                &playing::member_name(&self.play, &self.online, session.friend()),
            )
        {
            playing::arriving(
                ui,
                area,
                playing::friend_video(&self.play, session),
                &label,
                progress,
            );
            return None;
        }
        if playing::download_screen(
            &mut self.ui.playing,
            ui,
            area,
            &self.play,
            &self.online,
            &mut self.emulator,
        ) {
            return None;
        }
        let linked = self.play.session().is_some_and(|s| s.runs_engine());
        if let Some(session) = self.play.session()
            && let Some(line) = playing::screen_note(
                &playing::member_name(&self.play, &self.online, session.friend()),
                session,
            )
        {
            let copy = playing::needs_copy(session).then_some("Open my copy");
            if playing::note_over(
                ui,
                area,
                playing::friend_video(&self.play, session),
                &line,
                copy,
            ) {
                self.ui.playing.wants_copy = true;
                self.release_touch();
            }
            return None;
        }
        if !self.emulator.has_game() && !linked {
            return None;
        }
        if self.emulator.linked() {
            return self.paint_linked(ui, area, portrait, place);
        }
        if self.emulator.partnered() && portrait {
            let (main, small) = watch_split(area, self.emulator.video());
            self.emulator.ui_at(ui, main);
            if let Some(small) = small
                && let Some(partner) = self.emulator.partner()
            {
                let sprite = linked::partner::sprite(&self.ui.linked.events, partner);
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(small.shrink2(egui::vec2(8.0, 4.0)))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                linked::partner::status(&mut child, partner, sprite.as_ref());
            }
            return None;
        }
        if portrait
            && self.friend_shown()
            && let Some(session) = self.play.session()
        {
            let (main, friend) = watch_split(area, self.emulator.video());
            let Some(friend) = friend else {
                self.emulator.ui_at(ui, main);
                return None;
            };
            let swapped = self.ui.playing.swapped;
            let (mine, theirs) = if swapped {
                (friend, main)
            } else {
                (main, friend)
            };
            self.emulator.ui_at(ui, mine);
            let watched = playing::watched_friend(&self.play, session);
            let name = playing::watched_name(&self.play, &self.online, session);
            let full = playing::full_name(&self.play, &self.online, watched);
            let own = self.online.account();
            let picture = playing::friend_picture(&self.play, session);
            let shown = playing::paint_friend_screen(&mut self.ui.playing, ui, picture, theirs)?;
            let caption = if swapped {
                playing::WatchCaption {
                    label: "You",
                    name: own.map_or("You", |a| a.display_name.as_str()),
                    seed: own.map_or(0, |a| a.online_id.value()),
                    own: true,
                }
            } else {
                playing::WatchCaption {
                    label: &name,
                    name: &full,
                    seed: watched.value(),
                    own: false,
                }
            };
            let small = if swapped { friend } else { shown };
            match playing::watch_thumbnail(ui, small, &caption) {
                Some(playing::WatchTap::Swap) => self.ui.playing.swapped = !swapped,
                Some(playing::WatchTap::Profile) => {
                    self.ui.friends.show_profile(watched);
                    self.mobile.open(Tab::Friends);
                    self.release_touch();
                }
                None => {}
            }
            return None;
        }
        let tilting = self.emulator.cart().motion();
        let room = tilt::screen_area(area, tilting);
        let screen = self
            .emulator
            .ui_placed(ui, room, |size| place.rect(room, size));
        if tilting && tilt::badge(ui, self.emulator.gimmicks.motion, screen, area) {
            self.centre_tilt();
        }
        Some((room, screen))
    }

    fn paint_linked(
        &mut self,
        ui: &mut Ui,
        area: Rect,
        portrait: bool,
        place: ScreenPlace,
    ) -> Option<(Rect, Rect)> {
        let control = self.link_control()?;
        let (main, small) = if portrait {
            watch_split(area, self.emulator.video())
        } else {
            (area, None)
        };
        let paint_first = |this: &mut Self, ui: &mut Ui, rect: Rect| {
            if small.is_some_and(|small| small == rect) {
                return this.emulator.paint_fitted(ui, rect);
            }
            this.emulator
                .ui_placed(ui, rect, |size| place.rect(rect, size))
        };
        let paint_second = |this: &mut Self, ui: &mut Ui, rect: Rect| {
            let Some(link) = this.emulator.link() else {
                return rect;
            };
            linked::paint_screen(&mut this.ui.linked, ui, &link.machine, rect, f32::MAX)
        };
        let placed = match control {
            Game::First => Some((main, paint_first(self, ui, main))),
            Game::Second => {
                paint_second(self, ui, main);
                None
            }
        };
        let Some(small) = small else {
            return placed;
        };
        let shown = match control {
            Game::First => paint_second(self, ui, small),
            Game::Second => paint_first(self, ui, small),
        };
        let caption = match control {
            Game::First => linked::title(&self.emulator, self.ui.linked.listing),
            Game::Second => self.emulator.game_name().to_owned(),
        };
        ui.painter().text(
            egui::pos2(shown.center().x, shown.bottom() + 3.0),
            egui::Align2::CENTER_TOP,
            format!("{}  {caption}", icons::HAND_TAP),
            egui::TextStyle::Small.resolve(ui.style()),
            ui.visuals().weak_text_color(),
        );
        let tapped = ui
            .interact(
                shown,
                egui::Id::new("linked_small_screen"),
                egui::Sense::click(),
            )
            .clicked();
        if tapped {
            self.release_touch();
            self.emulator.set_link_control(control.other());
        }
        placed
    }

    fn banners(&mut self, ui: &mut Ui) {
        if self.play.session().is_some() {
            return;
        }
        let inset = egui::Frame::new().inner_margin(egui::Margin::symmetric(STRIP_INSET as i8, 0));
        if let Some(link) = self.emulator.link() {
            ui.add_space(2.0);
            if inset.show(ui, |ui| linked::strip(ui, link)).inner {
                self.mobile.open(Tab::Playing);
                self.release_touch();
            }
            ui.add_space(2.0);
        } else if let Some(partner) = self.emulator.partner() {
            ui.add_space(2.0);
            if inset
                .show(ui, |ui| linked::partner_strip(ui, partner))
                .inner
            {
                self.mobile.open(Tab::Playing);
                self.release_touch();
            }
            ui.add_space(2.0);
        }
    }

    fn popups(&mut self, ctx: &egui::Context) {
        let Some(panel) = self.ui.toast_area else {
            return;
        };
        let picture = self.ui.toast_picture;
        if let Some(invite) = playing::pending_ask(&self.play) {
            let id = egui::Id::new("ask_banner");
            let (answer, rect) = toasts::banner(ctx, id, panel, picture, |ui| {
                playing::ask_banner(ui, &self.play, &self.online, &invite)
            });
            self.ui.toast_banner = Some(rect);
            match answer {
                Some(playing::Answer::Join) => self.answer_ask(&invite),
                Some(playing::Answer::Pick) => {
                    self.ui.playing.picking = Some(playing::PickFor::Join(invite));
                }
                Some(playing::Answer::Decline) => {
                    self.play.decline(&mut self.online, invite.session)
                }
                None => {}
            }
        } else if let Some(ended) = self.ui.playing.ended.clone() {
            let again =
                playing::can_play_again(&self.play, &self.online, &self.emulator, ended.friend);
            let id = egui::Id::new("ended_banner");
            let (action, rect) = toasts::banner(ctx, id, panel, picture, |ui| {
                playing::banner(ui, &ended, again)
            });
            self.ui.toast_banner = Some(rect);
            match action {
                Some(playing::BannerAction::PlayAgain) => self.play_again(ended.friend),
                Some(playing::BannerAction::Resume) => {
                    self.emulator.running = true;
                    self.ui.playing.ended = None;
                }
                Some(playing::BannerAction::Dismiss) => self.ui.playing.ended = None,
                None => {}
            }
        }
    }
}

const ALL_BUTTONS: [Keys; 10] = [
    Keys::UP,
    Keys::DOWN,
    Keys::LEFT,
    Keys::RIGHT,
    Keys::A,
    Keys::B,
    Keys::START,
    Keys::SELECT,
    Keys::L,
    Keys::R,
];

struct BarWidths {
    gap: f32,
    done: f32,
    cancel: f32,
    reset_text: f32,
    reset_icon: f32,
    percent: f32,
    word: f32,
    title: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BarPlan {
    margin: f32,
    reset_text: bool,
    slider: Option<f32>,
    title: bool,
}

fn stack_min(widths: &BarWidths) -> f32 {
    let label = widths.word + widths.gap + widths.percent;
    (BAR_KNOB + label).max(SLIDER_MIN)
}

fn bar_plan(widths: &BarWidths, width: f32, opacity: bool) -> BarPlan {
    let gap = widths.gap;
    let room = width - 2.0 * STRIP_INSET;
    let need = |reset_text: bool, slider: bool| {
        let reset = if reset_text {
            widths.reset_text
        } else {
            widths.reset_icon
        };
        let mut need = widths.done + gap + widths.cancel + gap + reset;
        if slider {
            let least = stack_min(widths);
            need += gap
                + if reset_text {
                    least.max(SLIDER_ROOMY)
                } else {
                    least
                };
        }
        need
    };
    let choices: &[(bool, bool)] = if opacity {
        &[(true, true), (false, true), (false, false)]
    } else {
        &[(true, false), (false, false)]
    };
    let (reset_text, slider) = choices
        .iter()
        .copied()
        .find(|(reset, slider)| need(*reset, *slider) + BAR_SLACK <= room)
        .unwrap_or((false, false));
    let used = need(reset_text, slider);
    let mut spare = (room - used).max(0.0);
    let slider = slider.then(|| {
        let least = stack_min(widths);
        let grow = spare.min((SLIDER_MAX - least).max(0.0));
        spare -= grow;
        least + grow
    });
    let title = spare >= widths.title + gap;
    let margin = if title {
        STRIP_INSET
    } else {
        STRIP_INSET + spare / 2.0
    };
    BarPlan {
        margin,
        reset_text,
        slider,
        title,
    }
}

fn watch_split(area: Rect, video: VideoSpec) -> (Rect, Option<Rect>) {
    let main_height = (area.width() * video.height as f32 / video.width as f32).min(area.height());
    let main = Rect::from_min_size(area.min, vec2(area.width(), main_height));
    let below = Rect::from_min_max(
        egui::pos2(area.left(), main.bottom() + WATCH_GAP),
        egui::pos2(area.right(), area.bottom() - WATCH_GAP - WATCH_CAPTION),
    );
    let friend = (below.height() >= WATCH_MIN_HEIGHT).then_some(below);
    (main, friend)
}

fn picture_in(screen: Rect, frame: Vec2) -> Rect {
    let scale = (screen.width() / frame.x).min(screen.height() / frame.y);
    Rect::from_center_size(screen.center(), frame * scale)
}

fn portrait_split(
    area: Rect,
    settings: &TouchSettings,
    one_screen: bool,
    frame: Vec2,
    shoulders: bool,
    reserve: f32,
) -> (Rect, Rect) {
    let needed = TouchLayout::bar_height(area.width(), settings, shoulders);
    let controls_height = (needed * CONTROL_BREATHING_ROOM)
        .max(needed)
        .clamp(0.0, area.height() * 0.6);
    let room = area.height() - controls_height;
    let screen_height = if one_screen {
        room.min(area.width() * frame.y / frame.x)
    } else {
        room
    };
    let spare = room - screen_height;
    let from_spare = reserve.min(spare);
    let from_slack = (reserve - from_spare).min((controls_height - needed).max(0.0));
    let controls_height = controls_height - from_slack;
    let screen_height = (screen_height - (reserve - from_spare - from_slack)).max(0.0);
    let spare = spare - from_spare;
    let screen = Rect::from_min_size(area.min, vec2(area.width(), screen_height));
    let dpad_slack = (controls_height - needed).max(0.0) / 2.0;
    let above = ((spare - dpad_slack) / 2.0).max(0.0);
    let controls = Rect::from_min_size(
        egui::pos2(area.left(), screen.bottom() + above),
        vec2(area.width(), controls_height),
    );
    (screen, controls)
}

fn landscape_split(
    area: Rect,
    mode: crate::app::ui_state::touch::LandscapeControls,
    frame: Vec2,
    strip_buttons: Option<usize>,
    xy: bool,
) -> (Rect, Rect, Rect, Rect) {
    use crate::app::ui_state::touch::LandscapeControls;

    let column = match mode {
        LandscapeControls::Overlay => area.width() * OVERLAY_COLUMN,
        LandscapeControls::Beside => {
            let wanted = area.height() / frame.y * frame.x;
            ((area.width() - wanted) / 2.0).clamp(area.width() * 0.16, area.width() * 0.32)
        }
    };
    let floor = if xy {
        let strip = match (mode, strip_buttons) {
            (LandscapeControls::Overlay, Some(n)) => strip_row_width(n) + 8.0,
            _ => 0.0,
        };
        TouchLayout::column_for_dpad(XY_DPAD_FLOOR)
            .max(strip)
            .min(area.width() * MAX_COLUMN)
    } else {
        0.0
    };
    let column = column.max(floor);

    let left = Rect::from_min_size(area.min, vec2(column, area.height()));
    let right = Rect::from_min_size(
        egui::pos2(area.right() - column, area.top()),
        vec2(column, area.height()),
    );
    let strip_width = match mode {
        LandscapeControls::Overlay => right.width(),
        LandscapeControls::Beside => right.right() - left.right(),
    };
    let band = area.top() + strip_buttons.map_or(0.0, |n| strip_height(n, strip_width - 8.0));
    let (screen, strip) = match mode {
        LandscapeControls::Overlay => (area, right.with_max_y(band)),
        LandscapeControls::Beside => (
            Rect::from_min_max(egui::pos2(left.right(), band), right.left_bottom()),
            Rect::from_min_max(
                egui::pos2(left.right(), area.top()),
                egui::pos2(right.right(), band),
            ),
        ),
    };
    (screen, left, right, strip)
}

fn sheet_split(
    area: Rect,
    frame: Vec2,
    strip_buttons: Option<usize>,
    xy: bool,
) -> (Rect, Rect, Rect, Rect) {
    use crate::app::ui_state::touch::LandscapeControls;

    let band = strip_buttons.map_or(0.0, |n| strip_height(n, area.width() - 8.0));
    let strip = area.with_max_y(area.top() + band);
    let below = area.with_min_y(strip.bottom());
    let (screen, left, right, _) =
        landscape_split(below, LandscapeControls::Overlay, frame, None, xy);
    (screen, left, right, strip)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn toggle_fullscreen() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if document.fullscreen_element().is_some() {
        document.exit_fullscreen();
    } else if let Some(root) = document.document_element() {
        let _ = root.request_fullscreen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::touch::LandscapeControls;
    use citrine_gb::console::{ConsoleKind, VideoSpec};
    use egui::pos2;

    fn frame(video: VideoSpec) -> Vec2 {
        vec2(video.width as f32, video.height as f32)
    }

    fn gb() -> Vec2 {
        frame(ConsoleKind::Gb.video())
    }

    fn gba() -> Vec2 {
        frame(ConsoleKind::Gba.video())
    }

    fn bar_widths() -> BarWidths {
        BarWidths {
            gap: 8.0,
            done: 80.0,
            cancel: 80.0,
            reset_text: 80.0,
            reset_icon: 44.0,
            percent: 35.0,
            word: 50.0,
            title: 80.0,
        }
    }

    #[test]
    fn the_opacity_slider_keeps_its_word_and_number_down_to_360() {
        let widths = bar_widths();
        let full = bar_plan(&widths, 900.0, true);
        assert!(full.reset_text && full.title, "{full:?}");
        let mid = bar_plan(&widths, 412.0, true);
        assert!(!mid.reset_text, "{mid:?}");
        assert!(mid.slider.unwrap_or_default() >= SLIDER_ROOMY, "{mid:?}");
        let phone = bar_plan(&widths, 360.0, true);
        assert!(phone.slider.is_some() && !phone.reset_text, "{phone:?}");
        for width in [300.0, 330.0, 360.0, 412.0, 600.0] {
            let plan = bar_plan(&widths, width, true);
            assert!(plan.margin >= STRIP_INSET, "{width}: {plan:?}");
            if plan.reset_text {
                assert!(
                    plan.slider.unwrap_or_default() >= SLIDER_ROOMY,
                    "{width}: {plan:?}"
                );
            }
            if width >= 360.0 {
                let slider = plan.slider.unwrap_or_default();
                assert!(slider >= stack_min(&widths), "{width}: {plan:?}");
            }
        }
    }

    #[test]
    fn portrait_bands_cover_the_area_exactly() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(390.0, 760.0));
        let (screen, controls) =
            portrait_split(area, &TouchSettings::default(), false, gb(), false, 0.0);

        assert!((screen.height() + controls.height() - area.height()).abs() < 0.01);
        assert_eq!(screen.top(), area.top());
        assert_eq!(controls.bottom(), area.bottom());
    }

    #[test]
    fn one_screen_sits_under_the_strip_with_even_space_around_the_buttons() {
        let area = Rect::from_min_size(pos2(0.0, 60.0), vec2(412.0, 700.0));
        let (screen, controls) =
            portrait_split(area, &TouchSettings::default(), true, gb(), false, 0.0);

        assert_eq!(screen.top(), area.top());
        assert!((screen.height() - 412.0 * 144.0 / 160.0).abs() < 0.01);
        let needed = TouchLayout::bar_height(area.width(), &TouchSettings::default(), false);
        let slack = (controls.height() - needed) / 2.0;
        let above = controls.top() - screen.bottom() + slack;
        let below = area.bottom() - controls.bottom();
        assert!(
            above > 0.0,
            "a tall phone leaves room between the game and the buttons"
        );
        assert!(
            (above - below).abs() < 0.01,
            "{above} above the d-pad, {below} below the pills"
        );
    }

    #[test]
    fn the_control_band_fits_the_controls_without_stranding_them() {
        for height in [600.0_f32, 760.0, 900.0, 1200.0] {
            let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, height));
            let settings = TouchSettings::default();
            let (_, controls) = portrait_split(area, &settings, false, gb(), false, 0.0);
            let needed = TouchLayout::bar_height(area.width(), &settings, false);

            assert!(
                controls.height() >= needed - 0.01,
                "controls were squeezed at height {height}: {} < {needed}",
                controls.height()
            );
            assert!(
                controls.height() <= needed * CONTROL_BREATHING_ROOM + 0.01,
                "a taller window must not strand the buttons in empty space at \
             height {height}: {} for {needed} needed",
                controls.height()
            );
        }
    }

    #[test]
    fn bigger_buttons_get_a_taller_band() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 900.0));
        let small = TouchSettings {
            scale: TouchSettings::MIN_SCALE,
            ..Default::default()
        };
        let large = TouchSettings {
            scale: TouchSettings::MAX_SCALE,
            ..Default::default()
        };

        let (_, small_band) = portrait_split(area, &small, false, gb(), false, 0.0);
        let (_, large_band) = portrait_split(area, &large, false, gb(), false, 0.0);
        assert!(large_band.height() > small_band.height());
    }

    #[test]
    fn a_short_portrait_window_still_leaves_room_for_buttons() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        let (screen, controls) =
            portrait_split(area, &TouchSettings::default(), false, gb(), false, 0.0);

        assert!(
            screen.height() > 0.0,
            "the screen never collapses to nothing"
        );
        assert!(
            controls.height() <= area.height() * 0.6 + 0.01,
            "the screen keeps at least 40% of a short window"
        );
    }

    #[test]
    fn landscape_columns_do_not_overlap_the_screen() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(844.0, 390.0));
        let (screen, left, right, _) =
            landscape_split(area, LandscapeControls::Beside, gb(), Some(5), false);

        assert!(screen.left() >= left.right());
        assert!(screen.right() <= right.left());
        assert!(screen.width() > 0.0);
    }

    #[test]
    fn the_overlay_mode_hands_the_whole_area_to_the_screen() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(844.0, 390.0));
        let (screen, left, right, _) =
            landscape_split(area, LandscapeControls::Overlay, gb(), Some(5), false);

        assert_eq!(screen, area, "the picture uses the full width");
        assert!(left.width() > 0.0 && right.width() > 0.0);
        assert!(right.right() <= area.right());
    }

    #[test]
    fn a_very_wide_window_still_keeps_the_columns_reachable() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 400.0));
        let (_, left, _, _) =
            landscape_split(area, LandscapeControls::Beside, gb(), Some(5), false);
        assert!(
            left.width() <= area.width() * 0.32,
            "columns never sprawl across half the screen"
        );
    }

    #[test]
    fn a_gba_screen_spans_the_portrait_width_and_leaves_more_room_below() {
        let area = Rect::from_min_size(pos2(0.0, 60.0), vec2(412.0, 760.0));
        let settings = TouchSettings::default();
        let (gb_screen, _) = portrait_split(area, &settings, true, gb(), false, 0.0);
        let (screen, controls) = portrait_split(area, &settings, true, gba(), true, 0.0);

        assert!((screen.height() - 412.0 * 160.0 / 240.0).abs() < 0.01);
        assert!(screen.height() < gb_screen.height());
        assert!(controls.top() >= screen.bottom());
        assert!(controls.bottom() <= area.bottom() + 0.01);
        let needed = TouchLayout::bar_height(area.width(), &settings, true);
        assert!(
            controls.height() >= needed - 0.01,
            "L and R fit in the band"
        );
        let layout = TouchLayout::bar(controls, &settings, true);
        assert!(controls.contains_rect(layout.bounds()));
    }

    #[test]
    fn a_gba_screen_in_landscape_keeps_its_shape_beside_the_columns() {
        for size in [vec2(915.0, 412.0), vec2(844.0, 390.0), vec2(740.0, 360.0)] {
            let area = Rect::from_min_size(pos2(0.0, 0.0), size);
            let (screen, left, right, _) =
                landscape_split(area, LandscapeControls::Beside, gba(), None, false);
            assert!(screen.left() >= left.right() && screen.right() <= right.left());
            let picture_width = screen.width().min(screen.height() * 1.5);
            assert!(
                picture_width >= area.height() * 1.5 * 0.85,
                "the 3:2 picture keeps most of the height at {size:?}: {picture_width}"
            );
            let layout = TouchLayout::columns(left, right, &TouchSettings::default(), true);
            assert!(left.union(right).contains_rect(layout.bounds()), "{size:?}");
        }
    }

    #[test]
    fn a_gba_screen_in_the_overlay_mode_gets_the_whole_area() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(915.0, 412.0));
        let (screen, left, right, _) =
            landscape_split(area, LandscapeControls::Overlay, gba(), None, false);
        assert_eq!(screen, area);
        let layout = TouchLayout::columns(left, right, &TouchSettings::default(), true);
        assert!(area.contains_rect(layout.bounds()));
    }

    #[test]
    fn beside_puts_the_strip_in_one_row_above_the_picture() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(794.0, 400.0));
        let (screen, _, right, strip) =
            landscape_split(area, LandscapeControls::Beside, gb(), Some(5), false);

        assert!(
            screen.top() >= strip.bottom(),
            "the picture stays under the strip"
        );
        assert_eq!(
            strip.right(),
            right.right(),
            "the strip ends at the right edge"
        );
        assert!(
            strip.width() >= 5.0 * 52.0,
            "five strip buttons fit one row, got {}",
            strip.width()
        );
    }

    #[test]
    fn with_the_strip_hidden_the_beside_picture_takes_the_full_height() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(891.0, 412.0));
        let (screen, left, right, strip) =
            landscape_split(area, LandscapeControls::Beside, gb(), None, false);
        assert_eq!(screen.top(), area.top());
        assert_eq!(screen.bottom(), area.bottom());
        assert_eq!(screen.left(), left.right());
        assert_eq!(screen.right(), right.left());
        assert_eq!(strip.height(), 0.0);
        let picture = super::hide::picture_in(screen);
        assert!((picture.height() - area.height()).abs() < 0.01);
        assert!(picture.width() <= screen.width() + 0.01);
    }

    #[test]
    fn every_ds_arrangement_keeps_a_playable_pad_in_landscape() {
        let frames = [
            ("stacked", vec2(256.0, 384.0)),
            ("side by side", vec2(512.0, 192.0)),
            ("side by side with a gap", vec2(544.0, 192.0)),
            ("big and small", vec2(384.0, 192.0)),
            ("one screen", vec2(256.0, 192.0)),
        ];
        let settings = TouchSettings::default();
        for size in [
            vec2(915.0, 412.0),
            vec2(835.0, 412.0),
            vec2(844.0, 390.0),
            vec2(740.0, 360.0),
        ] {
            let area = Rect::from_min_size(pos2(0.0, 0.0), size);
            for (name, frame) in frames {
                for mode in [LandscapeControls::Beside, LandscapeControls::Overlay] {
                    let (screen, left, right, strip) =
                        landscape_split(area, mode, frame, Some(5), true);
                    let right = right.with_min_y(strip.bottom().max(area.top() + STRIP_HEIGHT));
                    let layout = TouchLayout::columns(left, right, &settings, true).with_xy(true);
                    let at = format!("{name} at {size:?} {mode:?}");
                    assert!(
                        layout.dpad_width() >= XY_DPAD_FLOOR - 0.01,
                        "{at}: d-pad {}",
                        layout.dpad_width()
                    );
                    assert!(
                        layout.smallest_face() >= 48.0,
                        "{at}: button {}",
                        layout.smallest_face()
                    );
                    assert!(left.union(right).contains_rect(layout.bounds()), "{at}");
                    assert!(screen.width() > 0.0, "{at}");
                    if mode == LandscapeControls::Beside {
                        assert!(
                            screen.left() >= left.right() && screen.right() <= right.left(),
                            "{at}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(all(test, feature = "nds"))]
mod ds_touch {
    use crate::app::Citrine;
    use crate::app::tests::phone_app;
    use crate::app::ui_state::ds_screens::{Arrangement, Placement};
    use crate::app::ui_state::layout_picks::Preset;
    use crate::app::ui_state::touch::LandscapeControls;
    use citrine_gb::nds::Cpu;
    use egui::{Context, Event, Pos2, Rect, Vec2, vec2};

    const RCNT_EXTKEYIN: u32 = 0x0400_0134;
    const PEN_UP: u32 = 1 << 6;
    const PORTRAIT: Vec2 = vec2(412.0, 915.0);
    const LANDSCAPE: Vec2 = vec2(915.0, 412.0);

    fn ds_app(name: &str) -> Citrine {
        let mut app = phone_app(name);
        app.emulator
            .load_nds(include_bytes!(
                "../../../fixtures/nds/blocksds-console-both-screens.nds"
            ))
            .unwrap();
        app.mobile.nav.show_game();
        app
    }

    fn frame(app: &mut Citrine, ctx: &Context, size: Vec2, events: Vec<Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            super::super::touch_spacing(ctx, app.is_mobile(ctx));
            app.render(ctx);
            let mobile = app.is_mobile(ctx);
            let Citrine {
                ui,
                audio,
                emulator,
                ..
            } = app;
            ui.settings.apply(ctx, audio, emulator, mobile);
        });
        app.second_screens_input();
    }

    fn pointer(at: Pos2, pressed: bool) -> Vec<Event> {
        vec![
            Event::PointerMoved(at),
            Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }

    fn pen_down(app: &mut Citrine) -> bool {
        let nds = app.emulator.as_nds_mut().expect("ds");
        (nds.buses().peek_io(Cpu::Arm7, RCNT_EXTKEYIN) >> 16) & PEN_UP == 0
    }

    struct Case {
        name: &'static str,
        size: Vec2,
        placement: Option<Placement>,
        controls: bool,
        overlay: bool,
        hidden: bool,
    }

    impl Case {
        fn new(name: &'static str, size: Vec2) -> Self {
            Self {
                name,
                size,
                placement: None,
                controls: true,
                overlay: false,
                hidden: false,
            }
        }

        fn placed(self, arrangement: Arrangement, swapped: bool, one_screen: bool) -> Self {
            Self {
                placement: Some(Placement {
                    arrangement,
                    swapped,
                    one_screen,
                }),
                ..self
            }
        }
    }

    fn touch_reaches_the_core(case: &Case) -> Result<Rect, String> {
        let name = case.name;
        let mut app = ds_app(&format!("ds-touch-{name}"));
        let ctx = Context::default();
        app.ui.settings.touch.show_controls = case.controls;
        if case.overlay {
            app.ui.settings.touch.landscape = LandscapeControls::Overlay;
        }
        for _ in 0..2 {
            frame(&mut app, &ctx, case.size, vec![]);
        }
        if let Some(placement) = case.placement {
            let context = app.emulator.screen_context;
            *app.ui.settings.ds_screens.placement_mut(context) = placement;
        }
        if case.hidden {
            app.mobile.hidden.hide();
        }
        for _ in 0..3 {
            frame(&mut app, &ctx, case.size, vec![]);
        }
        let touch = app
            .emulator
            .placed_touch()
            .ok_or(format!("{name}: no touch screen is shown"))?;
        let covered = |at: Pos2| app.ui.touch_targets.iter().any(|t| t.contains(at));
        let at = (1..10)
            .flat_map(|y| (1..10).map(move |x| (x as f32 / 10.0, y as f32 / 10.0)))
            .map(|(x, y)| touch.lerp_inside(vec2(x, y)))
            .filter(|at| !covered(*at))
            .min_by(|a, b| {
                let centre = touch.center();
                a.distance(centre).total_cmp(&b.distance(centre))
            })
            .ok_or(format!("{name}: the buttons cover the whole touch screen"))?;
        frame(&mut app, &ctx, case.size, pointer(at, true));
        frame(&mut app, &ctx, case.size, vec![]);
        if !pen_down(&mut app) {
            let missed = format!("{name}: a press at {at:?} on {touch:?} misses the pen");
            return Err(missed);
        }
        frame(&mut app, &ctx, case.size, pointer(at, false));
        frame(&mut app, &ctx, case.size, vec![]);
        if pen_down(&mut app) {
            return Err(format!("{name}: the pen stays down after the release"));
        }
        Ok(touch)
    }

    #[test]
    fn a_press_on_the_touch_screen_reaches_the_ds_in_every_phone_layout() {
        let cases = [
            Case::new("portrait", PORTRAIT),
            Case::new("landscape", LANDSCAPE),
            Case::new("portrait-swapped", PORTRAIT).placed(Arrangement::Stacked, true, false),
            Case::new("portrait-side", PORTRAIT).placed(Arrangement::SideBySide, false, false),
            Case::new("portrait-big", PORTRAIT).placed(Arrangement::BigAndSmall, false, false),
            Case::new("portrait-big-swapped", PORTRAIT).placed(
                Arrangement::BigAndSmall,
                true,
                false,
            ),
            Case::new("portrait-bottom-only", PORTRAIT).placed(Arrangement::Stacked, true, true),
            Case::new("landscape-swapped", LANDSCAPE).placed(Arrangement::SideBySide, true, false),
            Case::new("landscape-stacked", LANDSCAPE).placed(Arrangement::Stacked, false, false),
            Case::new("landscape-big", LANDSCAPE).placed(Arrangement::BigAndSmall, false, false),
            Case::new("landscape-big-swapped", LANDSCAPE).placed(
                Arrangement::BigAndSmall,
                true,
                false,
            ),
            Case::new("landscape-bottom-only", LANDSCAPE).placed(
                Arrangement::SideBySide,
                true,
                true,
            ),
            Case {
                overlay: true,
                ..Case::new("landscape-overlay", LANDSCAPE)
            },
            Case {
                hidden: true,
                ..Case::new("portrait-hidden", PORTRAIT)
            },
            Case {
                hidden: true,
                ..Case::new("landscape-hidden", LANDSCAPE)
            },
            Case {
                controls: false,
                ..Case::new("portrait-no-buttons", PORTRAIT)
            },
            Case {
                controls: false,
                ..Case::new("landscape-no-buttons", LANDSCAPE)
            },
        ];
        let placed: Vec<Result<Rect, String>> = cases.iter().map(touch_reaches_the_core).collect();
        let mut failed: Vec<String> = placed.iter().filter_map(|r| r.clone().err()).collect();
        let default_of = |size: Vec2| {
            let index = cases
                .iter()
                .position(|c| c.size == size)
                .expect("a default case");
            placed[index].clone().ok()
        };
        let default_placement = |size: Vec2| {
            if size == LANDSCAPE {
                Preset::BigTop.placement()
            } else {
                Placement::default()
            }
        };
        for (case, touch) in cases.iter().zip(&placed) {
            if let (Some(placement), Ok(touch)) = (case.placement, touch)
                && placement != default_placement(case.size)
                && default_of(case.size) == Some(*touch)
            {
                failed.push(format!(
                    "{}: the touch screen sits where the default puts it",
                    case.name
                ));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
