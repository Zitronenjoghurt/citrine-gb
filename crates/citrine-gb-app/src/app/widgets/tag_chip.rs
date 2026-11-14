use crate::app::ui_state::ui_theme::{brand, ink_on};
use crate::icons;
use egui::{Align2, Color32, FontId, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

const OWN_TINT: f32 = 0.28;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Own,
    Genre,
    Add,
    Choice,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tap {
    Chip,
    Remove,
}

pub struct TagChip<'a> {
    label: &'a str,
    look: Look,
    height: f32,
    size: f32,
    removable: bool,
    picked: bool,
}

impl<'a> TagChip<'a> {
    pub fn new(label: &'a str, look: Look) -> Self {
        Self {
            label,
            look,
            height: 22.0,
            size: 12.0,
            removable: false,
            picked: false,
        }
    }

    pub fn large(mut self) -> Self {
        self.height = 34.0;
        self.size = 15.0;
        self
    }

    pub fn picked(mut self, picked: bool) -> Self {
        self.picked = picked;
        self
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn removable(mut self) -> Self {
        self.removable = true;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Option<Tap> {
        let visuals = ui.visuals().clone();
        let font = FontId::proportional(self.size);
        let ink = match self.look {
            Look::Own => visuals.strong_text_color(),
            Look::Genre => visuals.text_color(),
            Look::Add => visuals.weak_text_color(),
            Look::Choice if self.picked => ink_on(brand(&visuals)),
            Look::Choice => visuals.text_color(),
        };
        let text = match self.look {
            Look::Add => format!("{} {}", icons::PLUS, self.label),
            Look::Choice if self.picked => format!("{} {}", icons::CHECK, self.label),
            Look::Own | Look::Genre | Look::Choice => self.label.to_owned(),
        };
        let galley = ui.painter().layout_no_wrap(text, font.clone(), ink);
        let pad = self.height * 0.45;
        let mark = if self.removable {
            self.size + pad * 0.4
        } else {
            0.0
        };
        let size = vec2(pad * 2.0 + galley.size().x + mark, self.height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let label = self.label;
        let picked = (self.look == Look::Choice).then_some(self.picked);
        response.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label);
            info.selected = picked;
            info
        });
        let over_mark = self.removable
            && response
                .hover_pos()
                .is_some_and(|at| at.x > rect.right() - pad - mark);
        if ui.is_rect_visible(rect) {
            let radius = self.height * 0.5;
            let amber = brand(&visuals);
            let neutral = visuals.widgets.inactive.weak_bg_fill;
            let (fill, stroke) = match self.look {
                Look::Own if visuals.dark_mode => (neutral, Stroke::new(1.0_f32, amber)),
                Look::Own => (amber.gamma_multiply(OWN_TINT), Stroke::NONE),
                Look::Genre => (neutral, Stroke::NONE),
                Look::Choice if self.picked => (amber, Stroke::NONE),
                Look::Choice => (neutral, Stroke::NONE),
                Look::Add => (
                    Color32::TRANSPARENT,
                    visuals.widgets.noninteractive.bg_stroke,
                ),
            };
            let lifts = self.look == Look::Add || self.look == Look::Choice && !self.picked;
            let fill = if response.hovered() && !over_mark && lifts {
                visuals.widgets.hovered.weak_bg_fill
            } else {
                fill
            };
            let painter = ui.painter();
            painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);
            let text_at = pos2(rect.left() + pad, rect.center().y - galley.size().y * 0.5);
            painter.galley(text_at, galley, ink);
            if self.removable {
                let center = pos2(rect.right() - pad - mark * 0.5 + pad * 0.2, rect.center().y);
                if over_mark {
                    painter.circle_filled(
                        center,
                        self.size * 0.6,
                        visuals.widgets.hovered.weak_bg_fill,
                    );
                }
                painter.text(center, Align2::CENTER_CENTER, icons::X, font, ink);
            }
        }
        if !response.clicked() {
            return None;
        }
        let on_mark = self.removable
            && response
                .interact_pointer_pos()
                .is_some_and(|at| at.x > rect.right() - pad - mark);
        Some(if on_mark { Tap::Remove } else { Tap::Chip })
    }
}
