use crate::app::Citrine;
use crate::app::tabs::{CONTENT_MARGIN, Tab, TabViewer};
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::touch::TouchSettings;
use crate::app::widgets::touch_controls::{self as touch, TouchInput, TouchLayout};
use crate::held::InputSource;
use crate::icons;
use citrine_gb::gb::joypad::JoypadState;
use egui::{Context, Rect, Ui, vec2};
use egui_dock::TabViewer as _;

mod history;

const OVERLAY_COLUMN: f32 = 0.28;

const CONTROL_BREATHING_ROOM: f32 = 1.15;

pub const TOUCH_TARGET_PX: f32 = 44.0;

fn bar_icon(icon: &str) -> egui::RichText {
    egui::RichText::new(icon).size(18.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    Menu,
    Panel(Tab),
}

#[derive(Default)]
pub struct MobileState {
    pub sheet: Option<Sheet>,
    came_from: Option<Sheet>,
    touch: TouchInput,
    layer_was_open: bool,
    history: history::BrowserHistory,
}

impl MobileState {
    pub fn open(&mut self, sheet: Sheet) {
        self.came_from = self.sheet.filter(|current| *current != sheet);
        self.sheet = Some(sheet);
    }

    pub fn back(&mut self) {
        self.sheet = self.came_from.take();
    }

    pub fn close(&mut self) {
        self.sheet = None;
        self.came_from = None;
    }

    pub fn listen_for_back(&self) {
        self.history.listen();
    }

    pub fn escape(&mut self, layer_open: bool) -> bool {
        if layer_open || self.sheet.is_none() {
            return false;
        }
        self.back();
        true
    }

    fn depth(&self) -> usize {
        match (self.sheet, self.came_from) {
            (None, _) => 0,
            (Some(_), None) => 1,
            (Some(_), Some(_)) => 2,
        }
    }
}

pub fn touch_spacing(ctx: &Context, touch: bool) {
    let wanted = if touch {
        TOUCH_TARGET_PX * ctx.native_pixels_per_point().unwrap_or(1.0) / ctx.pixels_per_point()
    } else {
        egui::style::Spacing::default().interact_size.y
    };
    if (ctx.style().spacing.interact_size.y - wanted).abs() > 0.01 {
        ctx.style_mut(|style| style.spacing.interact_size.y = wanted);
    }
}

pub fn touch_square(ui: &Ui) -> f32 {
    let height = ui.spacing().interact_size.y;
    if height > egui::style::Spacing::default().interact_size.y {
        height
    } else {
        0.0
    }
}

pub fn row_hint(ui: &mut Ui, text: impl Into<String>) -> egui::Response {
    let side = touch_square(ui);
    if side > 0.0 {
        let text_height = ui.text_style_height(&egui::TextStyle::Body);
        ui.add_space(-((side - text_height) / 2.0).max(0.0));
    }
    ui.add(egui::Label::new(egui::RichText::new(text.into()).small()).wrap())
}

pub fn compact<'a>(ui: &Ui, button: egui::Button<'a>) -> egui::Button<'a> {
    let side = touch_square(ui);
    if side > 0.0 {
        button.min_size(vec2(side, side))
    } else {
        button.small()
    }
}

pub fn compact_button(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    let button = compact(ui, egui::Button::new(text));
    ui.add(button)
}

fn layer_open(ctx: &Context) -> bool {
    egui::Popup::is_any_open(ctx)
        || ctx.memory(|m| m.top_modal_layer().is_some())
        || ctx.wants_keyboard_input()
        || crate::app::widgets::confirm::is_open(ctx)
}

impl Citrine {
    pub(super) fn render_mobile(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("mobile_bar")
            .show_separator_line(false)
            .show(ctx, |ui| self.mobile_bar(ui));
        self.toast_strip(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| match self.mobile.sheet {
                Some(sheet) => self.mobile_sheet(ui, sheet),
                None => self.mobile_play(ui),
            });
    }

    fn mobile_bar(&mut self, ui: &mut Ui) {
        let side = ui.spacing().interact_size.y;
        let size = egui::vec2(side, side);
        ui.add_space(3.0);

        ui.horizontal(|ui| {
            let open = self.mobile.sheet.is_some();
            let menu_icon = if open { icons::ARROW_LEFT } else { icons::LIST };
            if ui
                .add_sized(size, egui::Button::new(bar_icon(menu_icon)))
                .clicked()
            {
                if open {
                    self.mobile.back();
                } else {
                    self.mobile.open(Sheet::Menu);
                }
                self.release_touch();
            }

            ui.label(header_text(
                self.mobile.sheet,
                self.emulator.gb.cartridge.header.title.trim(),
            ));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let controls = self.ui.settings.touch.show_controls;
                let icon = if controls {
                    icons::GAME_CONTROLLER
                } else {
                    icons::EYE_SLASH
                };
                if ui
                    .add_sized(size, egui::Button::new(bar_icon(icon)))
                    .on_hover_text("Show or hide the on-screen buttons")
                    .clicked()
                {
                    self.ui.settings.touch.show_controls = !controls;
                    self.ui.settings.dirty = true;
                    self.release_touch();
                }

                let running = self.emulator.running;
                let can_play = self.emulator.gb.cartridge.has_rom_loaded;
                let play = if running { icons::PAUSE } else { icons::PLAY };
                if ui
                    .add_enabled(can_play, egui::Button::new(bar_icon(play)).min_size(size))
                    .clicked()
                {
                    self.run_hotkey(HotkeyAction::TogglePause);
                }

                #[cfg(target_arch = "wasm32")]
                if ui
                    .add_sized(size, egui::Button::new(bar_icon(icons::FRAME_CORNERS)))
                    .on_hover_text("Fullscreen")
                    .clicked()
                {
                    toggle_fullscreen();
                }

                if self.emulator.screen_recorder.is_recording()
                    && self.recording_button(ui, Some(size)).clicked()
                {
                    self.run_hotkey(HotkeyAction::ToggleRecording);
                }
            });
        });
        ui.add_space(3.0);
    }

    fn release_touch(&mut self) {
        let stuck = self.mobile.touch.reset();
        for button in ALL_BUTTONS {
            if stuck.contains(button) {
                self.emulator.release(InputSource::Touch, button);
            }
        }
    }

    fn mobile_play(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        let settings = &self.ui.settings;

        let (screen, layout, free) = if !settings.touch.show_controls {
            (area, None, area)
        } else if area.width() >= area.height() {
            let (screen, left, right) = landscape_split(area, settings.touch.landscape);
            (
                screen,
                Some(TouchLayout::columns(left, right, &settings.touch)),
                Rect::from_min_max(egui::pos2(left.right(), area.top()), right.left_bottom()),
            )
        } else {
            let (screen, controls) = portrait_split(area, &settings.touch);
            (
                screen,
                Some(TouchLayout::bar(controls, &settings.touch)),
                screen,
            )
        };
        self.ui.toast_area = Some(free);

        self.paint_screen(ui, screen);

        let Some(layout) = layout else {
            self.release_touch();
            return;
        };

        touch::claim(ui, layout.bounds());

        let held = self.mobile.touch.update(ui, &layout);
        let (pressed, released) = self.mobile.touch.transitions(held);

        if !pressed.is_empty() && self.ui.settings.touch.haptics {
            touch::vibrate(8);
        }

        for button in ALL_BUTTONS {
            if pressed.contains(button) {
                self.emulator.press(InputSource::Touch, button);
            } else if released.contains(button) {
                self.emulator.release(InputSource::Touch, button);
            }
        }

        layout.paint(ui, held, &self.ui.settings.touch);
    }

    fn paint_screen(&mut self, ui: &mut Ui, area: Rect) {
        if !self.emulator.gb.cartridge.has_rom_loaded {
            self.mobile_empty_state(ui, area);
            return;
        }
        self.emulator.ui_at(ui, area);
    }

    fn mobile_empty_state(&mut self, ui: &mut Ui, area: Rect) {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        let button = vec2(
            area.width().min(260.0) * 0.85,
            child.spacing().interact_size.y,
        );

        child.add_space(area.height() * 0.2);
        child.heading("Citrine");
        child.add_space(4.0);
        child.label(if cfg!(target_arch = "wasm32") {
            "A Game Boy emulator that runs in your browser."
        } else {
            "A Game Boy emulator."
        });
        child.add_space(20.0);

        if child
            .add_sized(
                button,
                egui::Button::new(format!("{} Load a ROM", icons::FOLDER)),
            )
            .clicked()
        {
            self.pick_rom();
        }
        child.add_space(10.0);
        if child
            .add_sized(
                button,
                egui::Button::new(format!("{} Homebrew games", icons::JOYSTICK)),
            )
            .clicked()
        {
            self.mobile.open(Sheet::Panel(Tab::Homebrew));
        }
    }

    fn mobile_sheet(&mut self, ui: &mut Ui, sheet: Sheet) {
        match sheet {
            Sheet::Menu => {
                egui::Frame::new()
                    .inner_margin(CONTENT_MARGIN)
                    .show(ui, |ui| self.mobile_menu(ui));
            }
            Sheet::Panel(mut tab) => {
                let mut viewer = TabViewer {
                    audio: &mut self.audio,
                    commonmark: &mut self.commonmark,
                    emulator: &mut self.emulator,
                    pads: self.pads.as_ref(),
                    events: &mut self.events,
                    files: &mut self.files,
                    media: &mut self.media,
                    ui: &mut self.ui,
                };
                if tab.scrolls_itself() {
                    viewer.ui(ui, &mut tab);
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt(("sheet_scroll", tab))
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.style_mut().interaction.selectable_labels = false;
                            viewer.ui(ui, &mut tab);
                        });
                }
            }
        }
    }

    fn mobile_menu(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let entry = |ui: &mut Ui, icon: &str, label: &str| {
            ui.add_sized(
                vec2(width, ui.spacing().interact_size.y),
                egui::Button::new(format!("{icon}  {label}")),
            )
            .clicked()
        };

        if entry(ui, icons::FOLDER, "Load ROM") {
            self.pick_rom();
            self.mobile.close();
        }
        ui.add_space(6.0);
        if entry(ui, icons::JOYSTICK, Tab::Homebrew.label()) {
            self.mobile.open(Sheet::Panel(Tab::Homebrew));
        }
        ui.add_space(6.0);
        if entry(ui, icons::FLOPPY_DISK, Tab::Saves.label()) {
            self.mobile.open(Sheet::Panel(Tab::Saves));
        }
        ui.add_space(6.0);
        if entry(ui, icons::IMAGES, Tab::Media.label()) {
            self.mobile.open(Sheet::Panel(Tab::Media));
        }
        ui.add_space(6.0);
        if entry(ui, icons::GEAR, Tab::Settings.label()) {
            self.mobile.open(Sheet::Panel(Tab::Settings));
        }
        ui.add_space(6.0);
        if entry(ui, icons::CAMERA, "Screenshot") {
            self.run_hotkey(HotkeyAction::Screenshot);
            self.mobile.close();
        }
        ui.add_space(6.0);
        let recording = self.emulator.screen_recorder.is_recording();
        let (icon, label) = if recording {
            (icons::STOP, "Stop recording")
        } else {
            (icons::RECORD, "Record video")
        };
        if entry(ui, icon, label) {
            self.run_hotkey(HotkeyAction::ToggleRecording);
            self.mobile.close();
        }
        ui.add_space(6.0);
        if entry(
            ui,
            icons::ARROW_COUNTER_CLOCKWISE,
            HotkeyAction::SoftReset.label(),
        ) {
            self.mobile.close();
            self.run_hotkey(HotkeyAction::SoftReset);
        }
        ui.add_space(6.0);
        if entry(ui, icons::INFO, Tab::Info.label()) {
            self.mobile.open(Sheet::Panel(Tab::Info));
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);

        let settings = &mut self.ui.settings;
        settings.dirty |= ui
            .checkbox(&mut settings.touch.show_controls, "On-screen buttons")
            .changed();
        row_hint(ui, "Turn these off to play with a paired controller.");
    }

    pub(super) fn mobile_escape(&mut self, ctx: &Context) {
        let touch = self.is_mobile(ctx);
        let escape = touch && ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape && self.mobile.escape(self.mobile.layer_was_open) {
            self.release_touch();
        }
        self.mobile.layer_was_open = layer_open(ctx) || self.ui.capture.is_waiting();
        let depth = if touch { self.mobile.depth() } else { 0 };
        self.mobile.history.sync(depth);
    }

    pub(super) fn browser_backs(&mut self) -> usize {
        self.mobile.history.take_backs()
    }

    pub(super) fn show_tab(&mut self, tab: Tab, touch: bool) {
        if touch {
            self.mobile.open(Sheet::Panel(tab));
            self.release_touch();
        } else {
            self.open_tab(tab);
        }
    }

    pub(super) fn close_mobile_sheet(&mut self) {
        self.mobile.close();
        self.release_touch();
    }

    fn pick_rom(&mut self) {
        self.try_start_audio();
        crate::utils::file_loader::FileLoader::new()
            .title("Load ROM")
            .add_filter("Game Boy ROMs", &["gb", "gbc"])
            .dispatch(self.files.rom_tx.clone());
    }
}

const ALL_BUTTONS: [JoypadState; 8] = [
    JoypadState::UP,
    JoypadState::DOWN,
    JoypadState::LEFT,
    JoypadState::RIGHT,
    JoypadState::A,
    JoypadState::B,
    JoypadState::START,
    JoypadState::SELECT,
];

fn header_text(sheet: Option<Sheet>, game_title: &str) -> &str {
    match sheet {
        Some(Sheet::Panel(tab)) => tab.label(),
        _ if game_title.is_empty() => "Citrine",
        _ => game_title,
    }
}

fn portrait_split(area: Rect, settings: &TouchSettings) -> (Rect, Rect) {
    let needed = TouchLayout::bar_height(area.width(), settings);
    let controls_height = (needed * CONTROL_BREATHING_ROOM)
        .max(needed)
        .clamp(0.0, area.height() * 0.6);

    let screen = Rect::from_min_size(
        area.min,
        vec2(area.width(), area.height() - controls_height),
    );
    let controls = Rect::from_min_max(egui::pos2(area.left(), screen.bottom()), area.max);
    (screen, controls)
}

fn landscape_split(
    area: Rect,
    mode: crate::app::ui_state::touch::LandscapeControls,
) -> (Rect, Rect, Rect) {
    use crate::app::ui_state::touch::LandscapeControls;

    let column = match mode {
        LandscapeControls::Overlay => area.width() * OVERLAY_COLUMN,
        LandscapeControls::Beside => {
            let wanted = area.height() / 144.0 * 160.0;
            ((area.width() - wanted) / 2.0).clamp(area.width() * 0.16, area.width() * 0.32)
        }
    };

    let left = Rect::from_min_size(area.min, vec2(column, area.height()));
    let right = Rect::from_min_size(
        egui::pos2(area.right() - column, area.top()),
        vec2(column, area.height()),
    );
    let screen = match mode {
        LandscapeControls::Overlay => area,
        LandscapeControls::Beside => {
            Rect::from_min_max(egui::pos2(left.right(), area.top()), right.left_bottom())
        }
    };
    (screen, left, right)
}

#[cfg(target_arch = "wasm32")]
fn toggle_fullscreen() {
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
    use egui::pos2;

    #[test]
    fn back_from_a_sheet_opened_in_the_menu_returns_to_the_menu() {
        let mut state = MobileState::default();
        state.open(Sheet::Menu);
        state.open(Sheet::Panel(Tab::Saves));
        state.back();
        assert_eq!(state.sheet, Some(Sheet::Menu));
        state.back();
        assert_eq!(state.sheet, None);
    }

    #[test]
    fn back_from_a_sheet_opened_elsewhere_returns_to_the_game() {
        let mut state = MobileState::default();
        state.open(Sheet::Panel(Tab::Homebrew));
        state.back();
        assert_eq!(state.sheet, None);
    }

    #[test]
    fn escape_steps_back_through_the_sheets() {
        let mut state = MobileState::default();
        state.open(Sheet::Menu);
        state.open(Sheet::Panel(Tab::Settings));
        assert!(state.escape(false));
        assert_eq!(
            state.sheet,
            Some(Sheet::Menu),
            "back to the menu it came from"
        );
        assert!(state.escape(false));
        assert_eq!(state.sheet, None);
        assert!(!state.escape(false), "the game screen stays");
    }

    #[test]
    fn escape_closes_the_top_layer_before_the_sheet() {
        let mut state = MobileState::default();
        state.open(Sheet::Panel(Tab::Saves));
        assert!(!state.escape(true));
        assert_eq!(state.sheet, Some(Sheet::Panel(Tab::Saves)));
    }

    #[test]
    fn a_panel_sheet_shows_its_own_name() {
        assert_eq!(
            header_text(Some(Sheet::Panel(Tab::Saves)), "POKEMON"),
            "Saves & snapshots"
        );
        assert_eq!(
            header_text(Some(Sheet::Panel(Tab::Media)), "POKEMON"),
            "Media"
        );
        assert_eq!(header_text(Some(Sheet::Menu), "POKEMON"), "POKEMON");
        assert_eq!(header_text(None, ""), "Citrine");
    }

    #[test]
    fn portrait_bands_cover_the_area_exactly() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(390.0, 760.0));
        let (screen, controls) = portrait_split(area, &TouchSettings::default());

        assert!((screen.height() + controls.height() - area.height()).abs() < 0.01);
        assert_eq!(screen.top(), area.top());
        assert_eq!(controls.bottom(), area.bottom());
    }

    #[test]
    fn the_control_band_fits_the_controls_without_stranding_them() {
        for height in [600.0_f32, 760.0, 900.0, 1200.0] {
            let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, height));
            let settings = TouchSettings::default();
            let (_, controls) = portrait_split(area, &settings);
            let needed = TouchLayout::bar_height(area.width(), &settings);

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

        let (_, small_band) = portrait_split(area, &small);
        let (_, large_band) = portrait_split(area, &large);
        assert!(large_band.height() > small_band.height());
    }

    #[test]
    fn a_short_portrait_window_still_leaves_room_for_buttons() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        let (screen, controls) = portrait_split(area, &TouchSettings::default());

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
        let (screen, left, right) = landscape_split(area, LandscapeControls::Beside);

        assert!(screen.left() >= left.right());
        assert!(screen.right() <= right.left());
        assert!(screen.width() > 0.0);
    }

    #[test]
    fn the_overlay_mode_hands_the_whole_area_to_the_screen() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(844.0, 390.0));
        let (screen, left, right) = landscape_split(area, LandscapeControls::Overlay);

        assert_eq!(screen, area, "the picture uses the full width");
        assert!(left.width() > 0.0 && right.width() > 0.0);
        assert!(right.right() <= area.right());
    }

    #[test]
    fn a_very_wide_window_still_keeps_the_columns_reachable() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 400.0));
        let (_, left, _) = landscape_split(area, LandscapeControls::Beside);
        assert!(
            left.width() <= area.width() * 0.32,
            "columns never sprawl across half the screen"
        );
    }
}
