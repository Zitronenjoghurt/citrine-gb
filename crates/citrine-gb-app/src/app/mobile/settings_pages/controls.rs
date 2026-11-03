use super::{Sheet, pick, settled, slide};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::tabs::Tab;
use crate::app::ui_state::touch::{HiddenButtons, TouchSettings};
use crate::app::widgets::settings::controls;
use egui::{Context, Ui};

impl Citrine {
    pub(super) fn controls_page(&mut self, ui: &mut Ui) {
        let s = &mut self.ui.settings;
        let mut open = None;
        let mut dirty = false;
        kit::section_label(ui, "On-screen buttons");
        kit::list(ui, |list| {
            let t = &mut s.touch;
            dirty |= list.switch(SwitchRow::new("Show", &mut t.show_controls));
            let vibrate = SwitchRow::new("Vibrate on press", &mut t.haptics);
            #[cfg(target_arch = "wasm32")]
            let vibrate = vibrate.info(controls::RINGER_HINT);
            dirty |= list.switch(vibrate);
            let rows = [
                (Sheet::ButtonSize, "Size", sheet::percent(t.scale.into())),
                (Sheet::Opacity, "Opacity", sheet::percent(t.opacity.into())),
                (Sheet::Landscape, "In landscape", t.landscape.to_string()),
                (
                    Sheet::HiddenButtons,
                    "With the UI hidden",
                    t.hidden_buttons.to_string(),
                ),
            ];
            for (sheet, label, value) in rows {
                if list.row(Row::new(label).value(value)).clicked() {
                    open = Some(sheet);
                }
            }
            dirty |= list.switch(
                SwitchRow::new("Left-handed", &mut t.mirrored).info("Puts the d-pad on the right."),
            );
        });
        kit::section_label(ui, "Keyboard and controller");
        let changed =
            !s.bindings.is_default() || !s.hotkeys.is_default() || !s.local_link.keys_are_default();
        let keys = Row::new("Keys and buttons").value(if changed { "Changed" } else { "Default" });
        if kit::rows(ui, vec![keys]).is_some() {
            open = Some(Sheet::Keys);
        }
        s.dirty |= dirty;
        self.open_sheet(open);
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
                    "Opacity",
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
            Sheet::UiSize => {
                let shown = slide(
                    ctx,
                    "UI size",
                    None,
                    Amount {
                        value: &mut t.ui_scale,
                        range: TouchSettings::MIN_UI_SCALE..=TouchSettings::MAX_UI_SCALE,
                        step: 0.05,
                        default: 1.0,
                        text: sheet::percent,
                    },
                );
                if settled(&shown.inner) {
                    s.dirty = true;
                }
                s.note_scale_drag(&shown.inner);
                shown.closed
            }
            Sheet::TouchScaling => {
                let (changed, done) = pick(ctx, "Scaling", None, &mut t.scaling);
                s.dirty |= changed;
                done
            }
            Sheet::Landscape => {
                let (changed, done) = pick(ctx, "In landscape", None, &mut t.landscape);
                s.dirty |= changed;
                done
            }
            Sheet::HiddenButtons => {
                let (changed, done) = pick(
                    ctx,
                    "With the UI hidden",
                    Some(HiddenButtons::NOTE),
                    &mut t.hidden_buttons,
                );
                s.dirty |= changed;
                done
            }
            _ => {
                let capture = &mut self.ui.capture;
                let shown = crate::app::mobile::sheet::show(
                    ctx,
                    super::sheet_id(),
                    "Keys and buttons",
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        controls::bindings(ui, s, capture)
                    },
                );
                if shown.inner {
                    self.mobile.open(Tab::ControllerDebug);
                    return true;
                }
                shown.closed && !self.ui.capture.is_waiting()
            }
        }
    }
}
