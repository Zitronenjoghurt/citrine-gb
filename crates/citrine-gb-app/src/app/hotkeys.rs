use super::{Citrine, QUICK_LOAD_HOLD};
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::snapshots::{SnapshotNews, snapshot_message};
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

    pub(super) fn raw_keys(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        drop_stray_repeats(ctx, raw);
        self.chat_key(ctx, raw);
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
                self.save_to_slot(self.emulator.store.next_free_slot(&key));
            }
            HotkeyAction::QuickLoad => {
                let slot = self.ui.settings.quick_slot;
                match self.emulator.load_snapshot(slot) {
                    Ok(true) => self.toasts.success(snapshot_message(
                        SnapshotNews::Loaded,
                        &self.emulator.snapshot_name(slot),
                    )),
                    Ok(false) => self.toasts.info(snapshot_message(
                        SnapshotNews::Empty,
                        &self.emulator.snapshot_name(slot),
                    )),
                    Err(err) => self.toasts.error(format!("Quick load failed: {err:?}")),
                };
                self.ui.snapshots.invalidate();
            }
            HotkeyAction::Screenshot => self.take_screenshot(),
            HotkeyAction::ToggleRecording => self.toggle_recording(),
            HotkeyAction::TogglePause => {
                self.emulator.running = !self.emulator.running;
                if !self.emulator.running {
                    self.emulator.release_all();
                }
            }
            HotkeyAction::SoftReset => self.ask_reset(),
            HotkeyAction::ToggleMute => {
                self.ui.settings.muted = !self.ui.settings.muted;
                self.ui.settings.dirty = true;
            }
            HotkeyAction::ToggleFocusMode => self.toggle_focus_mode(),
            HotkeyAction::LeaveFocusMode => {
                if self.ui.settings.focus_mode {
                    self.toggle_focus_mode();
                }
            }
            HotkeyAction::ToggleTouchControls => {
                let show = &mut self.ui.settings.touch.show_controls;
                *show = !*show;
                self.ui.settings.dirty = true;
            }
            HotkeyAction::SwitchGame => self.switch_game(),
            HotkeyAction::FocusChat => {
                self.ui.friends.chat.focus();
                self.events.open_tab(crate::app::tabs::Tab::Friends);
            }
        }
    }

    pub(super) fn save_to_slot(&mut self, slot: usize) {
        match self.emulator.save_snapshot(slot) {
            Ok(()) => self.toasts.success(snapshot_message(
                SnapshotNews::Saved,
                &self.emulator.snapshot_name(slot),
            )),
            Err(err) => self.toasts.error(format!("Quick save failed: {err:?}")),
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
