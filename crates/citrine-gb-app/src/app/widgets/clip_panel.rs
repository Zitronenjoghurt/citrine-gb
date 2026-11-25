use crate::app::widgets::action::{ICON_SIZE, icon_button, main_button};
use crate::icons;
use crate::media::playback::clock;
use egui::{Align, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, TextureId, Ui, vec2};

pub const TITLE: &str = "Save a clip";
pub const SAVE: &str = "Save clip";
pub const CANCEL: &str = "Cancel";
pub const SAVING: &str = "Saving the clip";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipFacts {
    pub frames: u64,
    pub from: u64,
    pub to: u64,
    pub at: u64,
    pub running: bool,
    pub frame_secs: f64,
    pub saving: bool,
    pub phone: bool,
}

impl ClipFacts {
    fn secs(&self, frames: u64) -> String {
        clock(frames as f64 * self.frame_secs)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipAction {
    Toggle,
    Seek(u64),
    Range(u64, u64),
    Cancel,
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripPicture {
    pub frame: u64,
    pub texture: TextureId,
    pub uv: Rect,
    pub aspect: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grab {
    From,
    To,
    Head,
}

pub fn strip_height(phone: bool) -> f32 {
    if phone { 56.0 } else { 44.0 }
}

fn time_row_height(ui: &Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body)
        .max(ui.text_style_height(&egui::TextStyle::Monospace))
}

fn side_of(ui: &Ui, phone: bool) -> f32 {
    let icon = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(ICON_SIZE)))
        + ui.spacing().button_padding.y * 2.0;
    let side = if phone {
        crate::app::mobile::touch_square(ui).max(ui.spacing().interact_size.y)
    } else {
        ui.spacing().interact_size.y
    };
    side.max(icon)
}

pub fn panel_height(ui: &Ui, phone: bool) -> f32 {
    let side = side_of(ui, phone);
    let text = time_row_height(ui);
    side * 2.0 + strip_height(phone) + text + ui.spacing().item_spacing.y * 3.0
}

const HANDLE: f32 = 6.0;
const FRAME: f32 = 2.0;

pub fn clip_panel(ui: &mut Ui, facts: &ClipFacts, pictures: &[StripPicture]) -> Option<ClipAction> {
    let side = side_of(ui, facts.phone);
    let mut action = None;
    ui.vertical(|ui| {
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), side),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.label(RichText::new(TITLE).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(facts.secs(facts.at)).monospace());
                    let (icon, hint) = if facts.running {
                        (icons::PAUSE, "Pause")
                    } else {
                        (icons::PLAY, "Play")
                    };
                    if ui
                        .add_enabled(!facts.saving, icon_button(icon, side))
                        .on_hover_text(hint)
                        .clicked()
                    {
                        action = Some(ClipAction::Toggle);
                    }
                });
            },
        );
        if let Some(moved) = strip(ui, facts, pictures) {
            action = Some(moved);
        }
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), time_row_height(ui)),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.label(RichText::new(facts.secs(0)).monospace().weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(facts.secs(facts.frames)).monospace().weak());
                });
            },
        );
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), side),
            Layout::left_to_right(Align::Center),
            |ui| {
                let length = facts.to.saturating_sub(facts.from);
                ui.label(format!("Length {}", facts.secs(length)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = if facts.saving {
                        SAVING.to_owned()
                    } else {
                        format!("{} {SAVE}", icons::FLOPPY_DISK)
                    };
                    let save = main_button(ui, label).min_size(vec2(0.0, side));
                    if ui.add_enabled(!facts.saving, save).clicked() {
                        action = Some(ClipAction::Save);
                    }
                    let cancel = egui::Button::new(CANCEL).min_size(vec2(0.0, side));
                    if ui.add_enabled(!facts.saving, cancel).clicked() {
                        action = Some(ClipAction::Cancel);
                    }
                });
            },
        );
    });
    action
}

fn x_of(rect: Rect, frames: u64, frame: u64) -> f32 {
    rect.left() + rect.width() * frame as f32 / frames.max(1) as f32
}

fn frame_at(rect: Rect, frames: u64, x: f32) -> u64 {
    let share = ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
    (share * frames as f32).round() as u64
}

fn grab_at(rect: Rect, facts: &ClipFacts, x: f32) -> Grab {
    let reach = if facts.phone { 24.0 } else { 12.0 };
    let from = (x - x_of(rect, facts.frames, facts.from)).abs();
    let to = (x - x_of(rect, facts.frames, facts.to)).abs();
    if from.min(to) > reach {
        Grab::Head
    } else if from < to || (from == to && x < x_of(rect, facts.frames, facts.from)) {
        Grab::From
    } else {
        Grab::To
    }
}

fn moved(facts: &ClipFacts, grab: Grab, frame: u64) -> ClipAction {
    let last = facts.frames.max(1);
    match grab {
        Grab::From => ClipAction::Range(frame.min(facts.to.saturating_sub(1)), facts.to),
        Grab::To => ClipAction::Range(facts.from, frame.clamp(facts.from + 1, last)),
        Grab::Head => ClipAction::Seek(frame.clamp(facts.from.max(1), facts.to.max(1))),
    }
}

fn strip(ui: &mut Ui, facts: &ClipFacts, pictures: &[StripPicture]) -> Option<ClipAction> {
    let height = strip_height(facts.phone);
    let (outer, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), height),
        if facts.saving {
            Sense::hover()
        } else {
            Sense::click_and_drag()
        },
    );
    let rect = outer.shrink2(vec2(HANDLE, 0.0));
    let grab_id = response.id.with("grab");
    let mut action = None;
    if let Some(pos) = response.interact_pointer_pos() {
        let grab = match ui.data(|d| d.get_temp::<Grab>(grab_id)) {
            Some(grab) => grab,
            None => {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
                let grab = grab_at(rect, facts, origin.x);
                ui.data_mut(|d| d.insert_temp(grab_id, grab));
                grab
            }
        };
        let next = moved(facts, grab, frame_at(rect, facts.frames, pos.x));
        let same = match next {
            ClipAction::Range(from, to) => (from, to) == (facts.from, facts.to),
            ClipAction::Seek(at) => at == facts.at,
            _ => false,
        };
        if !same {
            action = Some(next);
        }
    }
    if !response.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove::<Grab>(grab_id));
    }
    if response.hovered() && !facts.saving {
        let hover = response.hover_pos().map(|pos| grab_at(rect, facts, pos.x));
        if matches!(hover, Some(Grab::From | Grab::To)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    paint(ui, rect, facts, pictures);
    action
}

fn paint(ui: &Ui, rect: Rect, facts: &ClipFacts, pictures: &[StripPicture]) {
    let painter = ui.painter_at(rect.expand2(vec2(HANDLE, 0.0)));
    let visuals = ui.visuals();
    let radius = 4.0;
    painter.rect_filled(rect, radius, visuals.extreme_bg_color);
    tiles(&painter, rect, facts.frames, pictures);
    let from = x_of(rect, facts.frames, facts.from);
    let to = x_of(rect, facts.frames, facts.to);
    let shade = Color32::from_black_alpha(150);
    painter.rect_filled(
        Rect::from_x_y_ranges(rect.left()..=from, rect.y_range()),
        0.0,
        shade,
    );
    painter.rect_filled(
        Rect::from_x_y_ranges(to..=rect.right(), rect.y_range()),
        0.0,
        shade,
    );
    let accent = crate::app::ui_state::ui_theme::brand(visuals);
    let ink = crate::app::ui_state::ui_theme::ink_on(accent);
    let kept = Rect::from_x_y_ranges(from..=to, rect.y_range());
    painter.rect_stroke(
        kept,
        0.0,
        Stroke::new(FRAME, accent),
        egui::StrokeKind::Inside,
    );
    for (x, outward) in [(from, -1.0), (to, 1.0)] {
        let bar = Rect::from_x_y_ranges(
            if outward < 0.0 {
                x - HANDLE..=x
            } else {
                x..=x + HANDLE
            },
            rect.y_range(),
        );
        painter.rect_filled(bar, 2.0, accent);
        let mid = bar.center();
        painter.line_segment(
            [Pos2::new(mid.x, mid.y - 8.0), Pos2::new(mid.x, mid.y + 8.0)],
            Stroke::new(1.5_f32, ink),
        );
    }
    let head = x_of(rect, facts.frames, facts.at);
    painter.line_segment(
        [Pos2::new(head, rect.top()), Pos2::new(head, rect.bottom())],
        Stroke::new(2.0_f32, Color32::WHITE),
    );
}

fn tiles(painter: &egui::Painter, rect: Rect, frames: u64, pictures: &[StripPicture]) {
    let Some(first) = pictures.first() else {
        return;
    };
    let width = (rect.height() * first.aspect).max(8.0);
    let count = (rect.width() / width).ceil().max(1.0) as usize;
    let width = rect.width() / count as f32;
    for index in 0..count {
        let left = rect.left() + width * index as f32;
        let tile = Rect::from_min_size(Pos2::new(left, rect.top()), vec2(width, rect.height()));
        let frame = frame_at(rect, frames, tile.center().x);
        let picture = pictures
            .iter()
            .min_by_key(|picture| picture.frame.abs_diff(frame))
            .unwrap_or(first);
        let uv = fit_uv(picture.uv, picture.aspect, tile.width() / tile.height());
        painter.image(picture.texture, tile, uv, Color32::WHITE);
    }
}

fn fit_uv(uv: Rect, aspect: f32, wanted: f32) -> Rect {
    if wanted >= aspect {
        let keep = aspect / wanted;
        let cut = uv.height() * (1.0 - keep) / 2.0;
        Rect::from_x_y_ranges(uv.x_range(), uv.top() + cut..=uv.bottom() - cut)
    } else {
        let keep = wanted / aspect;
        let cut = uv.width() * (1.0 - keep) / 2.0;
        Rect::from_x_y_ranges(uv.left() + cut..=uv.right() - cut, uv.y_range())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> ClipFacts {
        ClipFacts {
            frames: 1800,
            from: 0,
            to: 1800,
            at: 1800,
            running: false,
            frame_secs: 1.0 / 60.0,
            saving: false,
            phone: false,
        }
    }

    fn strip_rect() -> Rect {
        Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(1000.0, 44.0))
    }

    #[test]
    fn a_press_near_an_end_grabs_that_handle_and_elsewhere_the_playhead() {
        let rect = strip_rect();
        let facts = ClipFacts {
            from: 300,
            to: 1500,
            ..facts()
        };
        assert_eq!(
            grab_at(rect, &facts, x_of(rect, 1800, 300) + 5.0),
            Grab::From
        );
        assert_eq!(
            grab_at(rect, &facts, x_of(rect, 1800, 1500) - 5.0),
            Grab::To
        );
        assert_eq!(grab_at(rect, &facts, 550.0), Grab::Head);
    }

    #[test]
    fn handles_never_cross_and_the_playhead_stays_inside_them() {
        let facts = ClipFacts {
            from: 300,
            to: 1500,
            at: 900,
            ..facts()
        };
        assert_eq!(
            moved(&facts, Grab::From, 1700),
            ClipAction::Range(1499, 1500)
        );
        assert_eq!(moved(&facts, Grab::To, 10), ClipAction::Range(300, 301));
        assert_eq!(moved(&facts, Grab::To, 5000), ClipAction::Range(300, 1800));
        assert_eq!(moved(&facts, Grab::Head, 10), ClipAction::Seek(300));
        assert_eq!(moved(&facts, Grab::Head, 1700), ClipAction::Seek(1500));
    }

    #[test]
    fn a_frame_maps_to_x_and_back() {
        let rect = strip_rect();
        for frame in [0, 1, 450, 1799, 1800] {
            assert_eq!(frame_at(rect, 1800, x_of(rect, 1800, frame)), frame);
        }
    }

    #[test]
    fn a_wide_tile_crops_its_picture_top_and_bottom() {
        let uv = fit_uv(
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            1.0,
            2.0,
        );
        assert_eq!(
            uv,
            Rect::from_min_max(egui::pos2(0.0, 0.25), egui::pos2(1.0, 0.75))
        );
    }

    fn run(ctx: &egui::Context, events: Vec<egui::Event>, facts: &ClipFacts) -> Option<ClipAction> {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 400.0))),
            events,
            ..Default::default()
        };
        let mut action = None;
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                action = clip_panel(ui, facts, &[]);
            });
        });
        action
    }

    fn strip_of(ctx: &egui::Context, facts: &ClipFacts) -> Rect {
        let mut found = Rect::NOTHING;
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 400.0))),
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                clip_panel(ui, facts, &[]);
                found = ui.min_rect();
            });
        });
        found
    }

    #[test]
    fn the_panel_fits_the_height_it_asks_for() {
        for phone in [false, true] {
            let ctx = egui::Context::default();
            if phone {
                ctx.style_mut(|style| style.spacing.interact_size.y = 44.0);
            }
            let facts = ClipFacts { phone, ..facts() };
            let mut drawn = Rect::NOTHING;
            let mut asked = 0.0;
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 400.0))),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    asked = panel_height(ui, phone);
                    drawn = ui.scope(|ui| clip_panel(ui, &facts, &[])).response.rect;
                });
            });
            assert!(
                drawn.height() <= asked + 0.5,
                "phone {phone}: drawn {} asked {asked}",
                drawn.height()
            );
        }
    }

    #[test]
    fn dragging_the_start_handle_moves_the_start() {
        let ctx = egui::Context::default();
        let facts = facts();
        let panel = strip_of(&ctx, &facts);
        let side = ctx.style().spacing.interact_size.y;
        let y = panel.top() + side + ctx.style().spacing.item_spacing.y + 20.0;
        let left = panel.left() + HANDLE;
        let width = panel.width() - 2.0 * HANDLE;
        let press = |x: f32, pressed: bool| egui::Event::PointerButton {
            pos: egui::pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let start = left + 2.0;
        let _ = run(
            &ctx,
            vec![
                egui::Event::PointerMoved(egui::pos2(start, y)),
                press(start, true),
            ],
            &facts,
        );
        let quarter = left + width / 4.0;
        let mut got = None;
        for step in 1..=4 {
            let x = start + (quarter - start) * step as f32 / 4.0;
            got = run(
                &ctx,
                vec![egui::Event::PointerMoved(egui::pos2(x, y))],
                &facts,
            )
            .or(got);
        }
        let _ = run(&ctx, vec![press(quarter, false)], &facts);
        let Some(ClipAction::Range(from, to)) = got else {
            panic!("{got:?}");
        };
        assert!(from.abs_diff(450) <= 2, "{from}");
        assert_eq!(to, 1800);
    }
}
