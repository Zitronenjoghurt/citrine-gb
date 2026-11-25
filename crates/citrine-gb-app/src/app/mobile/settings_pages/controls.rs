use super::{KeysPart, Sheet, label, opens, pick, slide, stepped};
use crate::app::Citrine;
use crate::app::big_screen::{self, ConfirmButton};
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::tabs::Tab;
use crate::app::ui_state::keybinds::KeyBindings;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::ui_state::touch::{HiddenButtons, TouchSettings};
use egui::{Context, Ui};

const DEAD_ZONE_NOTE: &str = "How far the stick moves before it presses a direction.";

fn changed_or_default(changed: bool) -> &'static str {
    if changed { "Changed" } else { "Default" }
}

fn whole_percent(value: f64) -> String {
    format!("{value:.0}%")
}

impl Citrine {
    pub(super) fn controls_page(&mut self, ui: &mut Ui) {
        self.tree_page(
            ui,
            SettingsTab::Controls,
            |group| {
                (group == GroupId::Touch)
                    .then_some("The touch layout shows these on phones and small windows.")
            },
            Self::controls_row,
            |_| {},
        );
    }

    fn controls_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let s = &mut self.ui.settings;
        let mut dirty = false;
        let open = match row {
            RowId::GameButtons => opens(
                list,
                Row::new(label(row)).value(changed_or_default(!s.bindings.buttons_are_default())),
                Sheet::Keys(KeysPart::Buttons),
            ),
            RowId::Shortcuts => opens(
                list,
                Row::new(label(row)).value(changed_or_default(!s.hotkeys.is_default())),
                Sheet::Keys(KeysPart::Shortcuts),
            ),
            RowId::StickSteers => {
                dirty |= list.switch(
                    SwitchRow::new(label(row), &mut s.bindings.stick_as_dpad)
                        .info("Also reads the analog stick as a d-pad."),
                );
                None
            }
            RowId::StickDeadZone => opens(
                list,
                Row::new(label(row))
                    .value(whole_percent(s.bindings.stick_deadzone.into()))
                    .enabled(s.bindings.stick_as_dpad),
                Sheet::DeadZone,
            ),
            RowId::Rumble => {
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.gimmicks.rumble));
                None
            }
            RowId::ControllerTest => {
                if list.row(Row::new(label(row))).clicked() {
                    self.mobile.open(Tab::ControllerDebug);
                    self.release_touch();
                }
                return None;
            }
            RowId::SplitKeys => opens(
                list,
                Row::new(label(row)).value(changed_or_default(!s.local_link.keys_are_default())),
                Sheet::Keys(KeysPart::Split),
            ),
            RowId::TiltKeys => opens(
                list,
                Row::new(label(row)).value(changed_or_default(!s.bindings.tilt_keys_are_default())),
                Sheet::Keys(KeysPart::Tilt),
            ),
            RowId::TouchShow => {
                dirty |= list.switch(SwitchRow::new(label(row), &mut s.touch.show_controls));
                None
            }
            RowId::TouchSize => opens(
                list,
                Row::new(label(row)).value(sheet::percent(s.touch.scale.into())),
                Sheet::ButtonSize,
            ),
            RowId::TouchOpacity => opens(
                list,
                Row::new(label(row)).value(sheet::percent(s.touch.opacity.into())),
                Sheet::Opacity,
            ),
            RowId::TouchLandscape => opens(
                list,
                Row::new(label(row)).value(s.touch.landscape.to_string()),
                Sheet::Landscape,
            ),
            RowId::TouchHidden => opens(
                list,
                Row::new(label(row)).value(s.touch.hidden_buttons.to_string()),
                Sheet::HiddenButtons,
            ),
            RowId::LeftHanded => {
                dirty |= list.switch(
                    SwitchRow::new(label(row), &mut s.touch.mirrored)
                        .info("Puts the d-pad on the right."),
                );
                None
            }
            RowId::Haptics => {
                let vibrate = SwitchRow::new(label(row), &mut s.touch.haptics);
                #[cfg(target_arch = "wasm32")]
                let vibrate = vibrate.info(crate::app::widgets::settings::controls::RINGER_HINT);
                dirty |= list.switch(vibrate);
                None
            }
            RowId::BigScreenMode => {
                if !crate::platform::is_tv() {
                    let mut on = s.big_screen.on;
                    if list.switch(SwitchRow::new(label(row), &mut on)) {
                        self.set_big_screen(on);
                    }
                }
                return None;
            }
            RowId::MenuButton => {
                list.row(
                    Row::new(label(row))
                        .value(big_screen::MENU_BUTTON)
                        .no_chevron(),
                );
                None
            }
            RowId::ConfirmButton => stepped(
                list,
                Row::new(label(row))
                    .value(s.big_screen.confirm.to_string())
                    .info(ConfirmButton::NOTE),
                &mut s.big_screen.confirm,
                &mut dirty,
                Sheet::ConfirmButton,
            ),
            RowId::ButtonIcons => stepped(
                list,
                Row::new(label(row)).value(s.big_screen.icons.to_string()),
                &mut s.big_screen.icons,
                &mut dirty,
                Sheet::ButtonIcons,
            ),
            RowId::AroundTheGame => stepped(
                list,
                Row::new(label(row)).value(s.big_screen.backdrop.to_string()),
                &mut s.big_screen.backdrop,
                &mut dirty,
                Sheet::Backdrop,
            ),
            _ => None,
        };
        s.dirty |= dirty;
        open
    }

    pub(super) fn controls_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let s = &mut self.ui.settings;
        let t = &mut s.touch;
        match open {
            Sheet::ButtonSize => {
                let shown = slide(
                    ctx,
                    "Button size",
                    None,
                    Amount {
                        value: &mut t.scale,
                        range: TouchSettings::MIN_SCALE..=TouchSettings::MAX_SCALE,
                        step: 0.05,
                        default: 1.0,
                        text: sheet::percent,
                    },
                );
                s.dirty |= shown.inner.changed();
                shown.closed
            }
            Sheet::Opacity => {
                let shown = slide(
                    ctx,
                    label(RowId::TouchOpacity),
                    None,
                    Amount {
                        value: &mut t.opacity,
                        range: TouchSettings::MIN_OPACITY..=1.0,
                        step: 0.05,
                        default: TouchSettings::DEFAULT_OPACITY,
                        text: sheet::percent,
                    },
                );
                s.dirty |= shown.inner.changed();
                shown.closed
            }
            Sheet::DeadZone => {
                let shown = slide(
                    ctx,
                    label(RowId::StickDeadZone),
                    Some(DEAD_ZONE_NOTE),
                    Amount {
                        value: &mut s.bindings.stick_deadzone,
                        range: KeyBindings::DEADZONES,
                        step: 1.0,
                        default: KeyBindings::DEFAULT_DEADZONE,
                        text: whole_percent,
                    },
                );
                if super::settled(&shown.inner) {
                    s.dirty = true;
                }
                shown.closed
            }
            Sheet::TouchScaling => {
                let (changed, done) = pick(ctx, label(RowId::Scaling), None, &mut t.scaling);
                s.dirty |= changed;
                done
            }
            Sheet::Landscape => {
                let (changed, done) =
                    pick(ctx, label(RowId::TouchLandscape), None, &mut t.landscape);
                s.dirty |= changed;
                done
            }
            Sheet::HiddenButtons => {
                let (changed, done) = pick(
                    ctx,
                    label(RowId::TouchHidden),
                    Some(HiddenButtons::NOTE),
                    &mut t.hidden_buttons,
                );
                s.dirty |= changed;
                done
            }
            Sheet::ConfirmButton => {
                let (changed, done) = pick(
                    ctx,
                    label(RowId::ConfirmButton),
                    Some(ConfirmButton::NOTE),
                    &mut s.big_screen.confirm,
                );
                s.dirty |= changed;
                done
            }
            Sheet::ButtonIcons => {
                let (changed, done) = pick(
                    ctx,
                    label(RowId::ButtonIcons),
                    None,
                    &mut s.big_screen.icons,
                );
                s.dirty |= changed;
                done
            }
            Sheet::Backdrop => {
                let (changed, done) = pick(
                    ctx,
                    label(RowId::AroundTheGame),
                    None,
                    &mut s.big_screen.backdrop,
                );
                s.dirty |= changed;
                done
            }
            Sheet::Keys(part) => {
                let capture = &mut self.ui.capture;
                let row = match part {
                    KeysPart::Buttons => RowId::GameButtons,
                    KeysPart::Shortcuts => RowId::Shortcuts,
                    KeysPart::Split => RowId::SplitKeys,
                    KeysPart::Tilt => RowId::TiltKeys,
                };
                let shown =
                    crate::app::mobile::sheet::show(ctx, super::sheet_id(), label(row), |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        super::keys::sheet_body(ui, s, capture, part);
                    });
                shown.closed && !self.ui.capture.is_waiting()
            }
            _ => true,
        }
    }
}
