use crate::app::ui_state::keybinds::PadButton;
use crate::controller_debug::PadInfo;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Xbox,
    PlayStation,
    Nintendo,
    Plain,
    Keys,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Face(PadButton),
    Shoulder(PadButton),
    Sideways,
    Arrows,
    Select,
    Start,
}

pub struct Hint {
    pub glyphs: Vec<Glyph>,
    pub label: &'static str,
}

impl Hint {
    pub fn new(glyphs: impl Into<Vec<Glyph>>, label: &'static str) -> Self {
        Self {
            glyphs: glyphs.into(),
            label,
        }
    }
}

pub fn family_of(info: &PadInfo) -> Family {
    let vendor = info
        .details
        .iter()
        .find(|(name, _)| *name == "USB ID")
        .and_then(|(_, id)| u16::from_str_radix(id.split(':').next()?, 16).ok());
    let name = info.name.to_lowercase();
    let named = |words: &[&str]| words.iter().any(|word| name.contains(word));
    match vendor {
        Some(0x057e) => Family::Nintendo,
        Some(0x054c) => Family::PlayStation,
        Some(0x045e) => Family::Xbox,
        _ if named(&["nintendo", "pro controller", "joy-con"]) => Family::Nintendo,
        _ if named(&["dualsense", "dualshock", "playstation", "ps4", "ps5"]) => Family::PlayStation,
        _ if named(&["xbox", "x-box", "xinput"]) => Family::Xbox,
        _ => Family::Plain,
    }
}

fn face_letter(family: Family, button: PadButton) -> Option<&'static str> {
    let letters = match family {
        Family::Xbox => ["A", "B", "X", "Y"],
        Family::Nintendo => ["B", "A", "Y", "X"],
        _ => return None,
    };
    match button {
        PadButton::South => Some(letters[0]),
        PadButton::East => Some(letters[1]),
        PadButton::West => Some(letters[2]),
        PadButton::North => Some(letters[3]),
        _ => None,
    }
}

pub(crate) fn shoulder_text(family: Family, button: PadButton) -> &'static str {
    let names = match family {
        Family::Xbox => ["LB", "RB", "LT", "RT"],
        Family::Nintendo => ["L", "R", "ZL", "ZR"],
        _ => ["L1", "R1", "L2", "R2"],
    };
    match button {
        PadButton::LeftTrigger => names[0],
        PadButton::RightTrigger => names[1],
        PadButton::LeftTrigger2 => names[2],
        _ => names[3],
    }
}

fn select_text(family: Family) -> &'static str {
    match family {
        Family::Xbox => "View",
        Family::PlayStation => "Create",
        Family::Nintendo => "\u{2212}",
        _ => "Select",
    }
}

fn start_text(family: Family) -> &'static str {
    match family {
        Family::Xbox => "Menu",
        Family::PlayStation => "Options",
        Family::Nintendo => "+",
        _ => "Start",
    }
}

fn key_text(glyph: Glyph, confirm: PadButton) -> Option<&'static str> {
    match glyph {
        Glyph::Face(button) if button == confirm => Some("Enter"),
        Glyph::Face(PadButton::South | PadButton::East) => Some("Esc"),
        Glyph::Sideways => Some("\u{2190} \u{2192}"),
        Glyph::Arrows => Some("\u{2190} \u{2191} \u{2193} \u{2192}"),
        _ => None,
    }
}

pub fn shown(family: Family, glyph: Glyph, confirm: PadButton) -> bool {
    family != Family::Keys || key_text(glyph, confirm).is_some()
}

pub fn paint(ui: &mut Ui, family: Family, glyph: Glyph, confirm: PadButton, size: f32) {
    let ink = ui.visuals().strong_text_color();
    let paper = ui.visuals().panel_fill;
    let font = FontId::proportional(size * 0.55);
    let pill = |ui: &mut Ui, text: &str| {
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font.clone(), paper);
        let width = (galley.size().x + size * 0.6).max(size);
        let (rect, _) = ui.allocate_exact_size(vec2(width, size), Sense::hover());
        ui.painter().rect_filled(rect, size / 2.0, ink);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, paper);
    };
    if family == Family::Keys {
        if let Some(text) = key_text(glyph, confirm) {
            pill(ui, text);
        }
        return;
    }
    match glyph {
        Glyph::Face(button) => {
            let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
            face(ui, rect, family, button, ink, paper);
        }
        Glyph::Shoulder(button) => pill(ui, shoulder_text(family, button)),
        Glyph::Select => pill(ui, select_text(family)),
        Glyph::Start => pill(ui, start_text(family)),
        Glyph::Sideways => pill(ui, "\u{25C0} \u{25B6}"),
        Glyph::Arrows => {
            let (rect, _) = ui.allocate_exact_size(vec2(size * 2.6, size), Sense::hover());
            ui.painter().rect_filled(rect, size / 2.0, ink);
            let reach = size * 0.18;
            let ways = [
                vec2(-1.0, 0.0),
                vec2(0.0, -1.0),
                vec2(0.0, 1.0),
                vec2(1.0, 0.0),
            ];
            for (index, way) in ways.into_iter().enumerate() {
                let centre = Pos2::new(
                    rect.left() + size * (0.5 + index as f32 * 0.53),
                    rect.center().y,
                );
                let side = vec2(-way.y, way.x);
                let points = vec![
                    centre + way * reach,
                    centre - way * reach + side * reach,
                    centre - way * reach - side * reach,
                ];
                ui.painter()
                    .add(egui::Shape::convex_polygon(points, paper, Stroke::NONE));
            }
        }
    }
}

fn face(ui: &Ui, rect: Rect, family: Family, button: PadButton, ink: Color32, paper: Color32) {
    let painter = ui.painter();
    let centre = rect.center();
    let radius = rect.width() / 2.0;
    painter.circle_filled(centre, radius, ink);
    if let Some(letter) = face_letter(family, button) {
        painter.text(
            centre,
            Align2::CENTER_CENTER,
            letter,
            FontId::proportional(rect.height() * 0.62),
            paper,
        );
        return;
    }
    let mark = radius * 0.45;
    let stroke = Stroke::new((radius * 0.18).max(1.2), paper);
    if family == Family::PlayStation {
        match button {
            PadButton::South => {
                painter.line_segment(
                    [centre + vec2(-mark, -mark), centre + vec2(mark, mark)],
                    stroke,
                );
                painter.line_segment(
                    [centre + vec2(-mark, mark), centre + vec2(mark, -mark)],
                    stroke,
                );
            }
            PadButton::East => {
                painter.circle_stroke(centre, mark, stroke);
            }
            PadButton::West => {
                painter.rect_stroke(
                    Rect::from_center_size(centre, vec2(mark * 1.7, mark * 1.7)),
                    0.0,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
            _ => {
                let points: Vec<Pos2> = [(0.0, -1.0), (0.95, 0.7), (-0.95, 0.7)]
                    .iter()
                    .map(|(x, y)| centre + vec2(x * mark * 1.05, y * mark))
                    .collect();
                painter.add(egui::Shape::closed_line(points, stroke));
            }
        }
        return;
    }
    let dot = radius * 0.2;
    let reach = radius * 0.5;
    for (spot, offset) in [
        (PadButton::South, vec2(0.0, reach)),
        (PadButton::East, vec2(reach, 0.0)),
        (PadButton::West, vec2(-reach, 0.0)),
        (PadButton::North, vec2(0.0, -reach)),
    ] {
        let at = centre + offset;
        if spot == button {
            painter.circle_filled(at, dot * 1.2, paper);
        } else {
            painter.circle_stroke(at, dot * 0.8, Stroke::new(1.0_f32, paper));
        }
    }
}

pub fn hint_bar(ui: &mut Ui, hints: &[Hint], family: Family, confirm: PadButton) {
    let size = 20.0;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for hint in hints.iter().rev() {
            if !hint
                .glyphs
                .iter()
                .all(|glyph| shown(family, *glyph, confirm))
            {
                continue;
            }
            ui.label(
                egui::RichText::new(hint.label)
                    .size(14.0)
                    .color(ui.visuals().text_color()),
            );
            for glyph in hint.glyphs.iter().rev() {
                paint(ui, family, *glyph, confirm, size);
            }
            ui.add_space(14.0);
        }
    });
}

pub fn hint(ui: &mut Ui, hint: &Hint, family: Family, confirm: PadButton) {
    if !hint
        .glyphs
        .iter()
        .all(|glyph| shown(family, *glyph, confirm))
    {
        return;
    }
    let size = 20.0;
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = size;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for glyph in &hint.glyphs {
                paint(ui, family, *glyph, confirm, size);
            }
            ui.label(
                egui::RichText::new(hint.label)
                    .size(14.0)
                    .color(ui.visuals().text_color()),
            );
        });
    });
}

pub fn hold_ring(ui: &Ui, centre: Pos2, progress: f32) {
    let radius = 22.0;
    let painter = ui.painter();
    painter.circle_filled(centre, radius + 6.0, Color32::from_black_alpha(160));
    painter.circle_stroke(
        centre,
        radius,
        Stroke::new(4.0_f32, Color32::from_white_alpha(50)),
    );
    let brand = ui.visuals().selection.bg_fill;
    let steps = 48;
    let end = (steps as f32 * progress).ceil() as usize;
    let points: Vec<Pos2> = (0..=end)
        .map(|step| {
            let turn = (step as f32 / steps as f32).min(progress) * std::f32::consts::TAU;
            centre + radius * vec2(turn.sin(), -turn.cos())
        })
        .collect();
    if points.len() > 1 {
        painter.add(egui::Shape::line(points, Stroke::new(4.0_f32, brand)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(name: &str, id: &str) -> PadInfo {
        PadInfo {
            id: 0,
            name: name.to_owned(),
            details: vec![("USB ID", id.to_owned())],
            buttons: Vec::new(),
            axes: Vec::new(),
            raw: Vec::new(),
        }
    }

    #[test]
    fn the_family_comes_from_the_vendor_then_the_name() {
        assert_eq!(
            family_of(&pad("Pro Controller", "057e:2009")),
            Family::Nintendo
        );
        assert_eq!(
            family_of(&pad("Wireless Controller", "054c:0ce6")),
            Family::PlayStation
        );
        assert_eq!(family_of(&pad("Controller", "045e:0b13")), Family::Xbox);
        assert_eq!(
            family_of(&pad("Xbox One pad", "unknown:unknown")),
            Family::Xbox
        );
        assert_eq!(family_of(&pad("8BitDo Lite", "2dc8:5112")), Family::Plain);
    }

    #[test]
    fn select_and_start_carry_the_pads_own_names() {
        let names = |family| (select_text(family), start_text(family));
        assert_eq!(names(Family::Xbox), ("View", "Menu"));
        assert_eq!(names(Family::PlayStation), ("Create", "Options"));
        assert_eq!(names(Family::Nintendo), ("\u{2212}", "+"));
        assert_eq!(names(Family::Plain), ("Select", "Start"));
    }
}
