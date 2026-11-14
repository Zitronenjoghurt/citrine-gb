use super::{Sheet, pick, settled, slide};
use crate::app::Citrine;
use crate::app::big_screen::{self, BACKDROP, ConfirmButton};
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::tabs::Tab;
use crate::app::ui_state::touch::{HiddenButtons, TouchSettings};
use crate::app::widgets::settings::controls;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::icons;
use egui::{Context, Ui};

impl Citrine {
    pub(super) fn controls_page(&mut self, ui: &mut Ui) {
        let s = &mut self.ui.settings;
        let mut open = None;
        let mut dirty = false;
        let mut page = Page::begin(ui);
        page.section(
            Heading::new(icons::DEVICE_MOBILE, "On-screen buttons"),
            |ui| {
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
                        SwitchRow::new("Left-handed", &mut t.mirrored)
                            .info("Puts the d-pad on the right."),
                    );
                })
            },
        );
        let changed =
            !s.bindings.is_default() || !s.hotkeys.is_default() || !s.local_link.keys_are_default();
        page.section(
            Heading::new(icons::KEYBOARD, "Keyboard and controller"),
            |ui| {
                kit::list(ui, |list| {
                    let keys = Row::new("Keys and buttons").value(if changed {
                        "Changed"
                    } else {
                        "Default"
                    });
                    if list.row(keys).clicked() {
                        open = Some(Sheet::Keys);
                    }
                    list.row(
                        Row::new("Menu button")
                            .value(big_screen::MENU_BUTTON)
                            .no_chevron(),
                    );
                    let b = &mut s.big_screen;
                    let confirm = list.row(
                        Row::new("Confirm button")
                            .value(b.confirm.to_string())
                            .info(ConfirmButton::NOTE)
                            .stepper(),
                    );
                    if confirm.clicked() {
                        open = Some(Sheet::ConfirmButton);
                    }
                    if let Some(step) = big_screen::take_step(&confirm) {
                        b.confirm = big_screen::cycle(b.confirm, step);
                        dirty = true;
                    }
                    let glyphs = list.row(
                        Row::new("Button icons")
                            .value(b.icons.to_string())
                            .stepper(),
                    );
                    if glyphs.clicked() {
                        open = Some(Sheet::ButtonIcons);
                    }
                    if let Some(step) = big_screen::take_step(&glyphs) {
                        b.icons = big_screen::cycle(b.icons, step);
                        dirty = true;
                    }
                    let backdrop =
                        list.row(Row::new(BACKDROP).value(b.backdrop.to_string()).stepper());
                    if backdrop.clicked() {
                        open = Some(Sheet::Backdrop);
                    }
                    if let Some(step) = big_screen::take_step(&backdrop) {
                        b.backdrop = big_screen::cycle(b.backdrop, step);
                        dirty = true;
                    }
                })
            },
        );
        page.end();
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
            Sheet::ConfirmButton => {
                let (changed, done) = pick(
                    ctx,
                    "Confirm button",
                    Some(ConfirmButton::NOTE),
                    &mut s.big_screen.confirm,
                );
                s.dirty |= changed;
                done
            }
            Sheet::ButtonIcons => {
                let (changed, done) = pick(ctx, "Button icons", None, &mut s.big_screen.icons);
                s.dirty |= changed;
                done
            }
            Sheet::Backdrop => {
                let (changed, done) = pick(ctx, BACKDROP, None, &mut s.big_screen.backdrop);
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
                        Page::show(ui, |page| controls::bindings(page, s, capture))
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
