use super::{Citrine, QUICK_LOAD_HOLD};
use crate::app::ui_state::ds_screens::ScreensAction;
use crate::app::ui_state::hotkeys::{Hotkey, HotkeyAction};
use crate::app::widgets::snapshots::{FailedStep, SnapshotNews, failed_message, snapshot_message};
use citrine_gb::debug::step::Step;
use egui::Context;

impl Citrine {
    pub(super) fn follow_quick_load(&mut self, ctx: &Context) {
        let typing = ctx.wants_keyboard_input() || self.ui.capture.is_waiting();
        if typing
            || !self.ui.settings.holds(HotkeyAction::QuickLoad)
            || !self.emulator.holds_shortcut(HotkeyAction::QuickLoad)
        {
            self.quick_load_hold = None;
            return;
        }
        let started = *self
            .quick_load_hold
            .get_or_insert_with(web_time::Instant::now);
        if started.elapsed() >= QUICK_LOAD_HOLD {
            self.quick_load_hold = None;
            self.run_hotkey(HotkeyAction::QuickLoad);
        }
    }

    pub(super) fn follow_rewind(&mut self, ctx: &Context) {
        let pressed = std::mem::take(&mut self.rewind_pressed);
        let held =
            !self.ui.capture.is_waiting() && self.emulator.holds_shortcut(HotkeyAction::Rewind);
        self.emulator.together = self.play.session().is_some() || self.play.room().is_some();
        self.emulator.rewind_held = pressed || held;
        if self.emulator.rewind_held && self.emulator.can_rewind() {
            ctx.request_repaint();
        }
    }

    #[cfg(test)]
    pub(super) fn take_input(&mut self, ctx: &Context) {
        self.emulator.set_hotkeys(&self.ui.settings.hotkeys);
        let Self {
            emulator, pads, ui, ..
        } = self;
        emulator.handle_input(
            ctx,
            pads.as_mut(),
            &ui.settings.bindings,
            &mut ui.capture,
            None,
        );
        self.run_shortcuts();
        self.follow_quick_load(ctx);
    }

    pub(super) fn raw_keys(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        drop_stray_repeats(ctx, raw);
        self.chat_key(ctx, raw);
        self.fast_forward_key(ctx, raw);
    }

    fn chat_key(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        let Some(field) = self.ui.friends.chat.field() else {
            return;
        };
        let focused = ctx.memory(|m| m.focused());
        let in_field = focused == Some(field);
        if self.ui.capture.is_waiting()
            || in_field && self.leave_field_on_escape(ctx, raw)
            || focused.is_some() && !in_field
        {
            return;
        }
        let Some((hotkey, chat_key)) = self.stolen_key(HotkeyAction::FocusChat) else {
            return;
        };
        if in_field && hotkey.types_text() {
            return;
        }
        let mut hit = false;
        raw.events.retain(|event| match event {
            egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } if *key == chat_key => {
                hit |= *pressed && !*repeat && hotkey.matches(*key, *modifiers);
                false
            }
            _ => true,
        });
        if !hit {
            return;
        }
        if in_field {
            self.ui.friends.chat.leave_field();
        } else {
            self.run_hotkey(HotkeyAction::FocusChat);
        }
    }

    pub(super) fn stolen_key(&self, action: HotkeyAction) -> Option<(Hotkey, egui::Key)> {
        let settings = &self.ui.settings;
        let hotkey = settings.hotkeys.keys_for(action).into_iter().next()?;
        let key = hotkey.single_key()?;
        let in_combo = settings.hotkeys.key_in_combo(key) || settings.bindings.key_in_combo(key);
        (!in_combo).then_some((hotkey, key))
    }

    fn leave_field_on_escape(&mut self, ctx: &Context, raw: &mut egui::RawInput) -> bool {
        if !self.ui.friends.chat.leaves_on_escape() || egui::Popup::is_any_open(ctx) {
            return false;
        }
        let mut hit = false;
        raw.events.retain(|event| match event {
            egui::Event::Key {
                key: egui::Key::Escape,
                pressed,
                modifiers,
                ..
            } if modifiers.is_none() => {
                hit |= *pressed;
                false
            }
            _ => true,
        });
        if hit {
            self.ui.friends.chat.leave_field();
        }
        hit
    }

    pub fn run_hotkey(&mut self, action: HotkeyAction) {
        let needs_rom = matches!(
            action,
            HotkeyAction::QuickSave
                | HotkeyAction::QuickSaveNewSlot
                | HotkeyAction::QuickLoad
                | HotkeyAction::Screenshot
                | HotkeyAction::SoftReset
        );
        if needs_rom && self.emulator.rom_key().is_none() {
            self.toasts.error("No ROM loaded");
            return;
        }

        match action {
            HotkeyAction::QuickSave => self.save_to_slot(self.ui.settings.quick_slot),
            HotkeyAction::QuickSaveNewSlot => {
                let key = self.emulator.saves_key().unwrap_or_default();
                let slot = self.emulator.next_free_slot(&key);
                self.save_to_slot(slot);
            }
            HotkeyAction::QuickLoad => {
                if self.emulator.hardcore() {
                    self.hardcore_refused("Loading states is off in hardcore");
                    return;
                }
                let slot = self.ui.settings.quick_slot;
                if super::tabs::playing::group_partners(&self.play, &self.online).is_some() {
                    self.ui.playing.unlink_load = Some(slot);
                    return;
                }
                match self.emulator.load_snapshot(slot) {
                    Ok(true) => self.toasts.success(snapshot_message(
                        SnapshotNews::Loaded,
                        &self.emulator.snapshot_name(slot),
                    )),
                    Ok(false) => self.toasts.info(snapshot_message(
                        SnapshotNews::Empty,
                        &self.emulator.snapshot_name(slot),
                    )),
                    Err(err) => self.toasts.error(failed_message(
                        FailedStep::Load,
                        &self.emulator.snapshot_name(slot),
                        &err,
                    )),
                };
                self.ui.snapshots.invalidate();
            }
            HotkeyAction::Screenshot => self.take_screenshot(),
            HotkeyAction::ToggleRecording => self.toggle_recording(),
            HotkeyAction::SaveClip => self.save_a_clip(),
            HotkeyAction::TogglePause if self.ui.clip.is_some() => {
                self.clip_action(crate::app::widgets::clip_panel::ClipAction::Toggle)
            }
            HotkeyAction::TogglePause if self.emulator.is_playing_tape() => {
                self.emulator.toggle_tape()
            }
            HotkeyAction::TogglePause => self.emulator.toggle_running(),
            HotkeyAction::SoftReset => self.ask_reset(),
            HotkeyAction::ToggleMute => {
                self.ui.settings.muted = !self.ui.settings.muted;
                self.ui.settings.dirty = true;
            }
            HotkeyAction::ToggleFocusMode => self.toggle_focus_mode(),
            HotkeyAction::LeaveFocusMode => {
                if self.ui.focus_mode {
                    self.toggle_focus_mode();
                } else if self.ui.clip.is_some() {
                    self.leave_clip();
                } else if self.replay_show.is_open() {
                    self.close_replay_show();
                }
            }
            HotkeyAction::ToggleHuds => {
                let hud = &mut self.ui.settings.hud;
                hud.shown = !hud.shown;
                self.ui.settings.dirty = true;
            }
            HotkeyAction::ToggleTouchControls => {
                let show = &mut self.ui.settings.touch.show_controls;
                *show = !*show;
                self.ui.settings.dirty = true;
            }
            HotkeyAction::SwapScreens => self.screens_action(ScreensAction::Swap),
            HotkeyAction::OneScreen => self.screens_action(ScreensAction::OneScreen),
            HotkeyAction::NextScreenLayout => self.screens_action(ScreensAction::NextLayout),
            HotkeyAction::ToggleLid => self.emulator.toggle_lid(),
            HotkeyAction::Rewind | HotkeyAction::FastForward | HotkeyAction::PushToTalk => {}
            HotkeyAction::SwitchGame => self.switch_game(),
            HotkeyAction::FocusChat => {
                self.ui.friends.chat.focus();
                self.events.open_tab(crate::app::tabs::Tab::Friends);
            }
            HotkeyAction::StepInstruction => self.debug_step(Step::Instruction),
            HotkeyAction::StepOver => self.debug_step(Step::Over),
            HotkeyAction::StepOut => self.debug_step(Step::Out),
            HotkeyAction::StepLine => self.debug_step(Step::Line),
            HotkeyAction::StepFrame => self.debug_step(Step::Frame),
            HotkeyAction::Fullscreen => {
                self.ui.settings.fullscreen = !self.ui.settings.fullscreen;
                self.ui.settings.dirty = true;
            }
            HotkeyAction::OpenSettings => {
                self.ui.settings.focus_search = true;
                self.events.open_settings(self.ui.settings.current_tab);
            }
        }
    }

    fn debug_step(&mut self, step: Step) {
        if self.ui.settings.dev_mode && self.emulator.has_game() {
            self.emulator.step(step);
        }
    }

    pub(super) fn save_to_slot(&mut self, slot: usize) {
        match self.emulator.save_snapshot(slot) {
            Ok(()) => self.toasts.success(snapshot_message(
                SnapshotNews::Saved,
                &self.emulator.snapshot_name(slot),
            )),
            Err(err) => self.toasts.error(failed_message(
                FailedStep::Save,
                &self.emulator.snapshot_name(slot),
                &err,
            )),
        };
        self.ui.snapshots.invalidate();
    }
}

fn drop_stray_repeats(ctx: &Context, raw: &mut egui::RawInput) {
    let down = ctx.input(|i| i.keys_down.clone());
    raw.events.retain(|event| {
        !matches!(
            event,
            egui::Event::Key {
                key,
                pressed: true,
                repeat: true,
                ..
            } if !down.contains(key)
        )
    });
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;
    use crate::app::tabs::Tab;
    use crate::app::tests::phone_app;
    use crate::app::ui_state::settings::LayoutMode;
    use egui::{Key, Modifiers};

    fn desktop(name: &str) -> Citrine {
        let mut app = phone_app(name);
        app.ui.settings.layout = LayoutMode::Desktop;
        app
    }

    fn frame(app: &mut Citrine, ctx: &Context, press: Option<(Key, Modifiers)>) -> Vec<bool> {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        if let Some((key, modifiers)) = press {
            input.modifiers = modifiers;
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            });
        } else {
            let down = ctx.input(|i| i.keys_down.clone());
            input
                .events
                .extend(down.into_iter().map(|key| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }));
        }
        let out = ctx.run(input, |ctx| {
            app.render(ctx);
            app.take_input(ctx);
            app.handle_event_queue(false);
            app.ui
                .settings
                .apply(ctx, &mut app.audio, &mut app.emulator, false);
        });
        out.viewport_output
            .values()
            .flat_map(|viewport| &viewport.commands)
            .filter_map(|command| match command {
                egui::ViewportCommand::Fullscreen(on) => Some(*on),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn ctrl_comma_opens_settings_and_asks_the_page_to_focus_search() {
        let mut app = desktop("open-settings-key");
        let ctx = Context::default();
        frame(&mut app, &ctx, None);
        assert!(app.dock.find_tab(&Tab::Settings).is_none());
        frame(&mut app, &ctx, Some((Key::Comma, Modifiers::COMMAND)));
        assert!(app.dock.find_tab(&Tab::Settings).is_some());
        assert!(app.ui.settings.focus_search);
        frame(&mut app, &ctx, None);
        assert!(
            !app.ui.settings.focus_search,
            "the Settings page takes the request when it draws"
        );
    }

    #[test]
    fn ctrl_comma_on_the_phone_opens_the_settings_list_with_its_search() {
        use crate::app::mobile::nav::Place;
        use crate::app::ui_state::settings_tree::SettingsTab;
        let mut app = desktop("open-settings-phone");
        app.open_settings(SettingsTab::Sound, true);
        assert_eq!(app.mobile.nav.settings_page(), Some(SettingsTab::Sound));
        app.ui.settings.focus_search = true;
        app.open_settings(SettingsTab::Sound, true);
        assert_eq!(app.mobile.nav.place(), Place::Settings);
        assert_eq!(
            app.mobile.nav.settings_page(),
            None,
            "the list holds the search"
        );
    }

    #[test]
    fn shift_f11_turns_real_fullscreen_on_and_off() {
        let mut app = desktop("fullscreen-key");
        let ctx = Context::default();
        assert!(frame(&mut app, &ctx, None).is_empty(), "starts in a window");
        let sent = frame(&mut app, &ctx, Some((Key::F11, Modifiers::SHIFT)));
        assert!(app.ui.settings.fullscreen);
        assert!(!app.ui.focus_mode, "plain F11 stays focus mode");
        assert_eq!(sent, [true]);
        assert!(frame(&mut app, &ctx, None).is_empty(), "sent once");
        let sent = frame(&mut app, &ctx, Some((Key::F11, Modifiers::SHIFT)));
        assert!(!app.ui.settings.fullscreen);
        assert_eq!(sent, [false]);
    }

    #[test]
    fn quick_load_runs_on_a_press_or_waits_for_a_hold() {
        use crate::app::ui_state::hotkeys::QuickLoadKey;
        let mut app = desktop("quick-load-press");
        let ctx = Context::default();
        assert_eq!(app.ui.settings.quick_load, QuickLoadKey::Hold);
        frame(&mut app, &ctx, Some((Key::F9, Modifiers::NONE)));
        assert!(app.quick_load_hold.is_some(), "a hold starts");
        assert!(app.toasts.texts().is_empty());

        let mut app = desktop("quick-load-press");
        let ctx = Context::default();
        app.ui.settings.quick_load = QuickLoadKey::Press;
        frame(&mut app, &ctx, Some((Key::F9, Modifiers::NONE)));
        assert!(app.quick_load_hold.is_none());
        assert_eq!(app.toasts.texts(), ["No ROM loaded"], "it ran at once");
    }

    #[test]
    fn a_saved_fullscreen_setting_goes_fullscreen_on_the_first_frame() {
        let mut app = desktop("fullscreen-saved");
        let mut json = serde_json::to_value(&app.ui.settings).unwrap();
        json["fullscreen"] = serde_json::Value::Bool(true);
        app.ui.settings = serde_json::from_value(json).unwrap();
        app.ui.settings.layout = LayoutMode::Desktop;
        let ctx = Context::default();
        assert_eq!(frame(&mut app, &ctx, None), [true]);
        assert!(frame(&mut app, &ctx, None).is_empty());
    }
}

#[cfg(all(test, feature = "nds"))]
mod tests {
    use super::*;
    use crate::app::tests::{load_game, phone_app};
    use crate::app::ui_state::hotkeys::Hotkey;
    use citrine_gb::nds::Cpu;

    const RCNT_EXTKEYIN: u32 = 0x0400_0134;
    const LID: u32 = 1 << 7;

    fn ds_app(name: &str) -> Citrine {
        let mut app = phone_app(name);
        app.emulator
            .load_nds(include_bytes!(
                "../../fixtures/nds/blocksds-console-both-screens.nds"
            ))
            .unwrap();
        app
    }

    fn core_lid_closed(app: &mut Citrine) -> bool {
        let nds = app.emulator.as_nds_mut().expect("ds");
        (nds.buses().peek_io(Cpu::Arm7, RCNT_EXTKEYIN) >> 16) & LID != 0
    }

    fn press(app: &mut Citrine, ctx: &Context, key: egui::Key) {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        for pressed in [true, false] {
            let input = egui::RawInput {
                events: vec![event(pressed)],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.take_input(ctx));
        }
    }

    #[test]
    fn a_bound_key_closes_and_opens_the_lid_and_the_core_sees_it() {
        let mut app = ds_app("lid-key");
        let ctx = Context::default();
        app.ui
            .settings
            .hotkeys
            .bind(Hotkey::plain(egui::Key::L), HotkeyAction::ToggleLid);
        assert_eq!(app.emulator.lid(), Some(false));
        assert!(!core_lid_closed(&mut app));
        press(&mut app, &ctx, egui::Key::L);
        assert_eq!(app.emulator.lid(), Some(true));
        assert!(core_lid_closed(&mut app));
        assert_eq!(app.emulator.lend_one_frame(), 1);
        assert!(core_lid_closed(&mut app));
        press(&mut app, &ctx, egui::Key::L);
        assert_eq!(app.emulator.lid(), Some(false));
        assert!(!core_lid_closed(&mut app));
    }

    #[test]
    fn the_lid_shows_only_for_ds_games() {
        let mut none = phone_app("lid-none");
        assert_eq!(none.emulator.lid(), None);
        none.run_hotkey(HotkeyAction::ToggleLid);
        assert!(!none.emulator.lid_closed());
        let mut crystal = phone_app("lid-crystal");
        load_game(
            &mut crystal,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        assert_eq!(crystal.emulator.lid(), None);
        crystal.run_hotkey(HotkeyAction::ToggleLid);
        assert!(!crystal.emulator.lid_closed());
        assert_eq!(ds_app("lid-ds").emulator.lid(), Some(false));
    }
}
