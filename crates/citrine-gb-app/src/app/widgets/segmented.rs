use egui::{Color32, FontId, Rect, Sense, Shadow, Stroke, Ui, vec2};

const SEGMENT_PADDING: f32 = 14.0;

pub struct Look {
    pub height: f32,
    pub radius: f32,
    pub font: f32,
    pub track: Color32,
    pub thumb: Color32,
    pub divider: Color32,
    pub full_width: bool,
}

impl Look {
    pub fn desktop(ui: &Ui) -> Self {
        let visuals = ui.visuals();
        let dark = visuals.dark_mode;
        Self {
            height: ui.spacing().interact_size.y + 6.0,
            radius: 6.0,
            font: 13.0,
            track: if dark {
                Color32::from_white_alpha(16)
            } else {
                Color32::from_black_alpha(14)
            },
            thumb: raised(visuals),
            divider: visuals.widgets.noninteractive.bg_stroke.color,
            full_width: false,
        }
    }
}

pub fn raised(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals.panel_fill.lerp_to_gamma(Color32::WHITE, 0.22)
    } else {
        Color32::WHITE
    }
}

pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, options: &[(T, &str)], current: T) -> Option<T> {
    let look = Look::desktop(ui);
    segmented_with(ui, &look, options, current)
}

pub fn segmented_with<T: Copy + PartialEq>(
    ui: &mut Ui,
    look: &Look,
    options: &[(T, &str)],
    current: T,
) -> Option<T> {
    let count = options.len().max(1) as f32;
    let font = FontId::proportional(look.font);
    let widest = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    let width = if look.full_width {
        ui.available_width()
    } else {
        ((widest + 2.0 * SEGMENT_PADDING) * count + 4.0).min(ui.available_width())
    };
    let (whole, _) = ui.allocate_exact_size(vec2(width, look.height), Sense::hover());
    let visuals = ui.visuals().clone();
    let painter = ui.painter_at(whole.expand(2.0));
    painter.rect_filled(whole, look.radius, look.track);
    let inner = whole.shrink(2.0);
    let segment = inner.width() / count;
    let thumb_radius = (look.radius - 2.0).max(0.0);
    let mut picked = None;
    for (index, (value, label)) in options.iter().enumerate() {
        let rect = Rect::from_min_size(
            egui::pos2(inner.left() + segment * index as f32, inner.top()),
            vec2(segment, inner.height()),
        );
        let on = *value == current;
        let response = ui.interact(rect, ui.id().with(("segment", index)), Sense::click());
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *label);
            info.selected = Some(on);
            info
        });
        let after_thumb = index > 0 && options[index - 1].0 == current;
        if index > 0 && !on && !after_thumb {
            let half = rect.height() * 0.25;
            painter.line_segment(
                [
                    egui::pos2(rect.left(), rect.center().y - half),
                    egui::pos2(rect.left(), rect.center().y + half),
                ],
                Stroke::new(1.0_f32, look.divider),
            );
        }
        if on {
            let shadow = Shadow {
                offset: [0, 1],
                blur: 2,
                spread: 0,
                color: Color32::from_black_alpha(40),
            };
            painter.add(shadow.as_shape(rect, thumb_radius));
            painter.rect_filled(rect, thumb_radius, look.thumb);
        } else if response.hovered() {
            painter.rect_filled(rect, thumb_radius, look.track);
        }
        let ink = if on {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            font.clone(),
            ink,
        );
        if response.clicked() && !on {
            picked = Some(*value);
        }
    }
    picked
}
