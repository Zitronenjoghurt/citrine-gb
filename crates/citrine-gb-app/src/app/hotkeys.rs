use super::{Citrine, QUICK_LOAD_HOLD};
use crate::app::ui_state::ds_screens::ScreensAction;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::snapshots::{FailedStep, SnapshotNews, failed_message, snapshot_message};
use citrine_gb::debug::step::Step;
use egui::Context;

impl Citrine {
    pub(super) fn handle_snapshot_hotkeys(&mut self, ctx: &Context) {
        if ctx.wants_keyboard_input() || self.ui.capture.is_waiting() {
            self.quick_load_hold = None;
            return;
        }

        let (pressed, modifiers, held_keys) = ctx.input(|i| {
            let pressed: Vec<egui::Key> = i
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } => Some(*key),
                    _ => None,
                })
                .collect();
            let held: Vec<egui::Key> = egui::Key::ALL
                .iter()
                .copied()
                .filter(|key| i.key_down(*key))
                .collect();
            (pressed, i.modifiers, held)
        });

        for key in pressed {
            if let Some(action) = self.ui.settings.hotkeys.action_for(key, modifiers)
                && !action.is_hold()
            {
                self.run_hotkey(action);
            }
        }

        let hold_active = held_keys.iter().any(|key| {
            self.ui
                .settings
                .hotkeys
                .action_for(*key, modifiers)
                .is_some_and(|action| action.is_hold())
        });

        if !hold_active {
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
        let hotkeys = &self.ui.settings.hotkeys;
        let typing = ctx.wants_keyboard_input() || self.ui.capture.is_waiting();
        let key = !typing
            && ctx.input(|i| {
                hotkeys
                    .keys_for(HotkeyAction::Rewind)
                    .iter()
                    .any(|hotkey| i.key_down(hotkey.key) && hotkey.matches(hotkey.key, i.modifiers))
            });
        let pad = !self.ui.capture.is_waiting()
            && self
                .emulator
                .loose_held()
                .iter()
                .any(|pad| hotkeys.action_for_pad(*pad) == Some(HotkeyAction::Rewind));
        self.emulator.together = self.play.session().is_some() || self.play.room().is_some();
        self.emulator.rewind_held = pressed || key || pad;
        if self.emulator.rewind_held && self.emulator.can_rewind() {
            ctx.request_repaint();
        }
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
        let Some(hotkey) = self
            .ui
            .settings
            .hotkeys
            .keys_for(HotkeyAction::FocusChat)
            .first()
            .copied()
        else {
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
            } if *key == hotkey.key => {
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
                let key = self.emulator.rom_key().unwrap_or_default().to_owned();
                let slot = self.emulator.next_free_slot(&key);
                self.save_to_slot(slot);
            }
            HotkeyAction::QuickLoad => {
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
                        &format!("'{}'", self.emulator.snapshot_name(slot)),
                        &err,
                    )),
                };
                self.ui.snapshots.invalidate();
            }
            HotkeyAction::Screenshot => self.take_screenshot(),
            HotkeyAction::ToggleRecording => self.toggle_recording(),
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
            HotkeyAction::Rewind | HotkeyAction::FastForward => {}
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
                &format!("'{}'", self.emulator.snapshot_name(slot)),
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
            let _ = ctx.run(input, |ctx| app.handle_snapshot_hotkeys(ctx));
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
