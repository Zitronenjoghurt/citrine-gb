use crate::app::ui_state::ui_theme::brand_mark;
use crate::emulator::gimmicks::Motion;
use egui::{Painter, Rect, Response, Sense, Stroke, Ui, Visuals, Widget, WidgetInfo, WidgetType};

const BADGE_SIDE: f32 = 26.0;
const BADGE_GAP: f32 = 6.0;
pub const BADGE_ROOM: f32 = BADGE_SIDE + 2.0 * BADGE_GAP;

pub struct TiltIndicator {
    motion: Motion,
    side: f32,
}

impl TiltIndicator {
    pub fn new(motion: Motion) -> Self {
        Self { motion, side: 28.0 }
    }

    pub fn side(mut self, side: f32) -> Self {
        self.side = side;
        self
    }
}

impl Widget for TiltIndicator {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(self.side, self.side), Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, ui.is_enabled(), "Tilt"));
        if ui.is_rect_visible(rect) {
            paint(ui.painter(), ui.visuals(), rect, self.motion);
        }
        response
    }
}

pub fn paint(painter: &Painter, visuals: &Visuals, rect: Rect, motion: Motion) {
    let centre = rect.center();
    let side = rect.width().min(rect.height());
    let radius = side / 2.0 - 1.0;
    let dot = (side / 7.0).max(3.0);
    painter.circle(
        centre,
        radius,
        visuals.extreme_bg_color,
        Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color),
    );
    painter.circle_filled(centre, 1.5_f32, visuals.weak_text_color());
    let motion = motion.clamped();
    let reach = radius - dot - 1.0;
    let at = centre + egui::vec2(motion.x, motion.y) * reach;
    painter.circle_filled(at, dot, brand_mark(visuals));
}

pub fn screen_area(area: Rect, tilt: bool) -> Rect {
    if tilt && area.height() > 2.0 * BADGE_ROOM {
        area.with_max_y(area.bottom() - BADGE_ROOM)
    } else {
        area
    }
}

pub fn badge_rect(screen: Rect, area: Rect) -> Rect {
    let size = egui::vec2(BADGE_SIDE, BADGE_SIDE);
    let below = Rect::from_min_size(
        egui::pos2(screen.right() - BADGE_SIDE, screen.bottom() + BADGE_GAP),
        size,
    );
    if area.contains_rect(below) {
        return below;
    }
    Rect::from_min_size(
        egui::pos2(screen.right() + BADGE_GAP, screen.bottom() - BADGE_SIDE),
        size,
    )
}

pub fn badge(ui: &mut Ui, motion: Motion, screen: Rect, area: Rect) -> bool {
    let rect = badge_rect(screen, area);
    ui.put(rect, TiltIndicator::new(motion).side(BADGE_SIDE))
        .on_hover_text("Tilt. Click to recenter.")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
