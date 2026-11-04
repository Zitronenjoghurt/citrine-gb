use super::{Sheet, settled, sheet_id, slide};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount, percent};
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::camera_view;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::tilt;
use crate::emulator::gimmicks::camera::{CameraView, Feed};
use crate::emulator::gimmicks::clock::{hours_label, offset_label};
use crate::emulator::gimmicks::settings::{
    CLOCK_FORWARD_HINT, CLOCK_HINT, DEFAULT_LIGHT, MAX_CLOCK_OFFSET, PRINTER_HINT, PRINTER_LINKED,
    RECENTER,
};
use crate::emulator::gimmicks::{CameraSource, LightSource};
use egui::{Color32, Context, FontId, Rect, RichText, Ui};
use strum::IntoEnumIterator;

const HOUR: i64 = 3600;
const DOT: f32 = 26.0;

impl Citrine {
    pub(super) fn cartridge_rows(&mut self, ui: &mut Ui) -> Option<Sheet> {
        let cart = self.emulator.cart();
        let rom_key = self.emulator.rom_key()?.to_owned();
        if !cart.any() {
            return None;
        }
        let printer_free = self.emulator.printer_free();
        let camera_error = self.senses.camera_error().map(str::to_owned);
        let picture_set = self.emulator.gimmicks.camera_frame().is_some();
        let light_sensor = self.senses.has_light_sensor();
        let live_camera = self.senses.has_live_camera();
        let motion = self.emulator.gimmicks.motion;
        let s = &mut self.ui.settings;
        let mut open = None;
        let mut level = false;
        let mut choose = false;
        kit::section_label(ui, "Cartridge");
        kit::list(ui, |list| {
            if cart.clock
                && list
                    .row(Row::new("Clock").value(offset_label(s.gimmicks.clock_offset(&rom_key))))
                    .clicked()
            {
                open = Some(Sheet::Clock);
            }
            if cart.rumble {
                s.dirty |= list.switch(SwitchRow::new("Rumble", &mut s.gimmicks.rumble));
            }
            if cart.motion() {
                let row = list.row(
                    Row::new(if cart.tilt { "Tilt" } else { "Rotate" })
                        .value(RECENTER)
                        .accent()
                        .no_chevron(),
                );
                level = row.clicked();
                let ui = list.ui();
                let value = ui
                    .painter()
                    .layout_no_wrap(
                        RECENTER.to_owned(),
                        FontId::proportional(15.0),
                        Color32::WHITE,
                    )
                    .size()
                    .x;
                let dot = Rect::from_center_size(
                    egui::pos2(
                        row.rect.right() - kit::VALUE_EDGE - value - 10.0 - DOT / 2.0,
                        row.rect.center().y,
                    ),
                    egui::vec2(DOT, DOT),
                );
                tilt::paint(ui.painter(), ui.visuals(), dot, motion);
            }
            if cart.light {
                let mut sensor = light_sensor && s.gimmicks.light_source == LightSource::Sensor;
                if light_sensor && list.switch(SwitchRow::new("Light sensor", &mut sensor)) {
                    s.gimmicks.light_source = if sensor {
                        LightSource::Sensor
                    } else {
                        LightSource::Slider
                    };
                    s.dirty = true;
                }
                if !sensor
                    && list
                        .row(Row::new("Sunlight").value(percent(s.gimmicks.light.into())))
                        .clicked()
                {
                    open = Some(Sheet::Sunlight);
                }
            }
            if cart.camera {
                let picture = !live_camera || s.gimmicks.camera == CameraSource::Picture;
                if !live_camera {
                    choose = list
                        .row(Row::new("Camera").value(if picture_set {
                            "Picture chosen"
                        } else {
                            "No picture"
                        }))
                        .clicked();
                } else {
                    let detail = if picture {
                        ""
                    } else {
                        camera_error.as_deref().unwrap_or_default()
                    };
                    if list
                        .row(
                            Row::new("Camera")
                                .value(s.gimmicks.camera.to_string())
                                .detail(detail),
                        )
                        .clicked()
                    {
                        open = Some(Sheet::Camera);
                    }
                }
                if live_camera && picture {
                    choose = list.row(Row::new("Choose picture")).clicked();
                }
            }
            if cart.printer {
                let mut on = s.gimmicks.printer(&rom_key);
                let detail = (!printer_free).then_some(PRINTER_LINKED);
                if list.switch(
                    SwitchRow::new("Printer", &mut on)
                        .info(PRINTER_HINT)
                        .detail(detail)
                        .enabled(printer_free),
                ) {
                    s.gimmicks.set_printer(&rom_key, on);
                    s.dirty = true;
                }
            }
        });
        if level {
            self.centre_tilt();
        }
        if choose {
            self.pick_camera_picture();
        }
        open
    }

    pub(super) fn cartridge_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let cart = self.emulator.cart();
        let Some(rom_key) = self.emulator.rom_key().map(str::to_owned) else {
            return true;
        };
        let s = &mut self.ui.settings;
        let (changed, done) = match open {
            Sheet::Clock => {
                let saved = s.gimmicks.clock_offset(&rom_key) / HOUR;
                let most = MAX_CLOCK_OFFSET / HOUR;
                let (least, default, note) = if cart.clock_back {
                    (-most, 0, CLOCK_HINT)
                } else {
                    (saved, saved, CLOCK_FORWARD_HINT)
                };
                let mut value = saved;
                let shown = slide(
                    ctx,
                    "Clock",
                    Some(note),
                    Amount {
                        value: &mut value,
                        range: least..=most,
                        step: 1.0,
                        default,
                        text: hours_label,
                    },
                );
                if value != saved {
                    s.gimmicks.set_clock_offset(&rom_key, value * HOUR);
                }
                (settled(&shown.inner), shown.closed)
            }
            Sheet::Sunlight => {
                let shown = slide(
                    ctx,
                    "Sunlight",
                    None,
                    Amount {
                        value: &mut s.gimmicks.light,
                        range: 0.0..=1.0,
                        step: 0.01,
                        default: DEFAULT_LIGHT,
                        text: percent,
                    },
                );
                (settled(&shown.inner), shown.closed)
            }
            _ => camera_sheet(
                ctx,
                s,
                self.senses.camera_feed(),
                self.senses.camera_error().is_some(),
            ),
        };
        s.dirty |= changed;
        done
    }
}

fn camera_sheet(
    ctx: &Context,
    s: &mut Settings,
    feed: Option<(&Feed, u64)>,
    failed: bool,
) -> (bool, bool) {
    let current = s.gimmicks.camera;
    let mut moved = false;
    let shown = sheet::show(ctx, sheet_id(), "Camera", |ui| {
        let picked = sheet::choices(ui, CameraSource::iter(), current, |v| v.to_string());
        if current == CameraSource::Camera && !failed {
            ui.add_space(16.0);
            sheet::inset(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Part of the camera view")
                            .size(13.0)
                            .color(kit::section_color(ui.visuals())),
                    );
                    info_mark(ui, camera_view::PHONE_HINT);
                });
                ui.add_space(6.0);
                moved |= camera_view::show(ui, feed, &mut s.gimmicks.camera_view);
                if feed.is_some() {
                    ui.add_space(8.0);
                    let off_centre = s.gimmicks.camera_view != CameraView::default();
                    ui.add_enabled_ui(off_centre, |ui| {
                        ui.vertical_centered(|ui| {
                            if kit::pill(ui, "Reset", false).clicked() {
                                s.gimmicks.camera_view = CameraView::default();
                                moved = true;
                            }
                        });
                    });
                }
            });
            ui.add_space(16.0);
        }
        picked
    });
    match shown.inner {
        Some(picked) => {
            s.gimmicks.camera = picked;
            (picked != current, picked == CameraSource::Picture)
        }
        None => (moved, shown.closed),
    }
}
