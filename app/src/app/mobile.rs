use crate::app::Citrine;
use crate::app::tabs::{Tab, TabViewer};
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::touch::TouchSettings;
use crate::app::widgets::touch_controls::{self as touch, TouchInput, TouchLayout};
use crate::icons;
use citrine_gb::gb::joypad::JoypadState;
use egui::{Context, Rect, Ui, vec2};
use egui_dock::TabViewer as _;

const OVERLAY_COLUMN: f32 = 0.28;

const CONTROL_BREATHING_ROOM: f32 = 1.15;

const TOP_BUTTON: f32 = 38.0;

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
    touch: TouchInput,
}

impl Citrine {
    pub(super) fn render_mobile(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("mobile_bar")
            .show_separator_line(false)
            .show(ctx, |ui| self.mobile_bar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| match self.mobile.sheet {
                Some(sheet) => self.mobile_sheet(ui, sheet),
                None => self.mobile_play(ui),
            });
    }

    fn mobile_bar(&mut self, ui: &mut Ui) {
        let size = egui::vec2(TOP_BUTTON, TOP_BUTTON);
        ui.add_space(3.0);

        ui.horizontal(|ui| {
            let open = self.mobile.sheet.is_some();
            let menu_icon = if open { icons::ARROW_LEFT } else { icons::LIST };
            if ui
                .add_sized(size, egui::Button::new(bar_icon(menu_icon)))
                .clicked()
            {
                self.mobile.sheet = if open { None } else { Some(Sheet::Menu) };
                self.release_touch();
            }

            let title = self.emulator.gb.cartridge.header.title.trim();
            ui.label(if title.is_empty() { "Citrine" } else { title });

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
            });
        });
        ui.add_space(3.0);
    }

    fn release_touch(&mut self) {
        let stuck = self.mobile.touch.reset();
        for button in ALL_BUTTONS {
            if stuck.contains(button) {
                self.emulator.release(button);
            }
        }
    }

    fn mobile_play(&mut self, ui: &mut Ui) {
        let area = ui.available_rect_before_wrap();
        let settings = &self.ui.settings;

        let (screen, layout) = if !settings.touch.show_controls {
            (area, None)
        } else if area.width() >= area.height() {
            let (screen, left, right) = landscape_split(area, settings.touch.landscape);
            (
                screen,
                Some(TouchLayout::columns(left, right, &settings.touch)),
            )
        } else {
            let (screen, controls) = portrait_split(area, &settings.touch);
            (screen, Some(TouchLayout::bar(controls, &settings.touch)))
        };

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
                self.emulator.press(button);
            } else if released.contains(button) {
                self.emulator.release(button);
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

        let button = vec2(area.width().min(260.0) * 0.85, 42.0);

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
            self.mobile.sheet = Some(Sheet::Panel(Tab::Homebrew));
        }
    }

    fn mobile_sheet(&mut self, ui: &mut Ui, sheet: Sheet) {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| match sheet {
                Sheet::Menu => self.mobile_menu(ui),
                Sheet::Panel(tab) => {
                    let mut viewer = TabViewer {
                        audio: &mut self.audio,
                        commonmark: &mut self.commonmark,
                        emulator: &mut self.emulator,
                        events: &mut self.events,
                        files: &mut self.files,
                        ui: &mut self.ui,
                    };
                    let mut tab = tab;
                    viewer.ui(ui, &mut tab);
                }
            });
    }

    fn mobile_menu(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let entry = |ui: &mut Ui, icon: &str, label: &str| {
            ui.add_sized(
                vec2(width, 40.0),
                egui::Button::new(format!("{icon}  {label}")),
            )
            .clicked()
        };

        if entry(ui, icons::FOLDER, "Load ROM") {
            self.pick_rom();
            self.mobile.sheet = None;
        }
        ui.add_space(6.0);
        if entry(ui, icons::JOYSTICK, "Homebrew games") {
            self.mobile.sheet = Some(Sheet::Panel(Tab::Homebrew));
        }
        ui.add_space(6.0);
        if entry(ui, icons::FLOPPY_DISK, "Saves & snapshots") {
            self.mobile.sheet = Some(Sheet::Panel(Tab::Saves));
        }
        ui.add_space(6.0);
        if entry(ui, icons::GEAR, "Settings") {
            self.mobile.sheet = Some(Sheet::Panel(Tab::Settings));
        }
        ui.add_space(6.0);
        if entry(ui, icons::CAMERA, "Screenshot") {
            self.run_hotkey(HotkeyAction::Screenshot);
            self.mobile.sheet = None;
        }
        ui.add_space(6.0);
        if entry(ui, icons::INFO, "About & controls") {
            self.mobile.sheet = Some(Sheet::Panel(Tab::Info));
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);

        let settings = &mut self.ui.settings;
        settings.dirty |= ui
            .checkbox(&mut settings.touch.show_controls, "On-screen buttons")
            .changed();
        ui.small("Turn these off to play with a paired controller.");
    }

    pub(super) fn close_mobile_sheet(&mut self) {
        self.mobile.sheet = None;
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
