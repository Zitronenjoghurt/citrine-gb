use super::cells::{Facts, cell};
use super::paint::Look;
use super::{DEFAULT_OPACITY, Direction, Group, Hud, SIZES, Value, addable};
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::menu_row::{menu_row, menu_row_width};
use crate::app::widgets::reset_slider::ResetSlider;
use crate::icons;
use egui::text::LayoutJob;
use egui::{Align, Color32, Id, Layout, Response, RichText, Sense, Stroke, TextFormat, Ui, vec2};

pub const WIDTH: f32 = 300.0;
const ADD_COLUMN: f32 = 150.0;
pub const CARD_HINT: &str = "The card behind every HUD. At 0%, only the text shows.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shown {
    pub changed: bool,
    pub remove: bool,
}

pub fn popup(
    ui: &Ui,
    anchor: &Response,
    hud: &mut Hud,
    opacity: &mut f32,
    facts: &Facts,
    open: &mut bool,
) -> Shown {
    let look = Look::of(ui.visuals(), *opacity);
    egui::Popup::menu(anchor)
        .id(Id::new(("hud_editor", hud.id)))
        .open_bool(open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(WIDTH)
        .show(|ui| body(ui, hud, opacity, facts, look))
        .map(|shown| shown.inner)
        .unwrap_or_default()
}

pub fn body(ui: &mut Ui, hud: &mut Hud, opacity: &mut f32, facts: &Facts, look: Look) -> Shown {
    let mut shown = Shown::default();
    ui.horizontal(|ui| {
        for (direction, icon, name) in [
            (Direction::Row, icons::SQUARE_SPLIT_HORIZONTAL, "Row"),
            (Direction::Column, icons::SQUARE_SPLIT_VERTICAL, "Column"),
        ] {
            shown.changed |= ui
                .selectable_value(&mut hud.direction, direction, format!("{icon} {name}"))
                .changed();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(egui::Button::new(icons::TRASH).frame(false))
                .on_hover_text("Remove HUD")
                .clicked()
            {
                shown.remove = true;
            }
        });
    });
    ui.add_space(2.0);
    egui::Grid::new(("hud_sliders", hud.id))
        .num_columns(2)
        .spacing(vec2(10.0, 4.0))
        .show(ui, |ui| {
            ui.label("Size");
            let mut size = (hud.scale() * 100.0).round();
            let range = SIZES.start() * 100.0..=SIZES.end() * 100.0;
            if ui
                .add(
                    ResetSlider::new(&mut size, range)
                        .step_by(5.0)
                        .suffix("%")
                        .default_value(100.0),
                )
                .changed()
            {
                hud.size = size / 100.0;
                shown.changed = true;
            }
            ui.end_row();
            ui.horizontal(|ui| {
                ui.label("Card");
                info_mark(ui, CARD_HINT);
            });
            let mut card = (opacity.clamp(0.0, 1.0) * 100.0).round();
            if ui
                .add(
                    ResetSlider::new(&mut card, 0.0..=100.0)
                        .step_by(5.0)
                        .suffix("%")
                        .default_value((DEFAULT_OPACITY * 100.0).round()),
                )
                .changed()
            {
                *opacity = card / 100.0;
                shown.changed = true;
            }
            ui.end_row();
        });
    ui.separator();
    shown.changed |= values(ui, hud, facts, look);
    shown.changed |= add_value(ui, hud, facts.ds, look);
    shown
}

fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

fn values(ui: &mut Ui, hud: &mut Hud, facts: &Facts, look: Look) -> bool {
    let mut changed = false;
    let mut moved = None;
    let mut removed = None;
    let rows: Vec<(usize, Value)> = hud
        .values
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, value)| facts.ds || !value.ds_only())
        .collect();
    for (index, value) in rows {
        let row = ui.horizontal(|ui| {
            ui.dnd_drag_source(Id::new(("hud_value", hud.id, value)), index, |ui| {
                ui.label(RichText::new(icons::DOTS_SIX_VERTICAL).weak());
            })
            .response
            .on_hover_cursor(egui::CursorIcon::Grab);
            dot(ui, look.group(value.group()));
            ui.label(value.name()).on_hover_text(value.hint());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(icons::X).frame(false))
                    .on_hover_text("Remove")
                    .clicked()
                {
                    removed = Some(index);
                }
                if value != Value::Graph {
                    let cell = cell(value, facts);
                    let text = if cell.unit.is_empty() {
                        cell.value
                    } else if cell.unit == "%" {
                        format!("{}%", cell.value)
                    } else {
                        format!("{} {}", cell.value, cell.unit)
                    };
                    ui.label(RichText::new(text).weak());
                }
            });
        });
        let response = row.response;
        if let Some(from) = response.dnd_hover_payload::<usize>() {
            let below = ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|pos| pos.y > response.rect.center().y);
            let y = if below {
                response.rect.bottom()
            } else {
                response.rect.top()
            };
            if *from != index {
                ui.painter().hline(
                    response.rect.x_range(),
                    y,
                    Stroke::new(2.0_f32, ui.visuals().selection.stroke.color),
                );
            }
            if let Some(from) = response.dnd_release_payload::<usize>() {
                moved = Some((*from, drop_index(*from, index, below)));
            }
        }
    }
    if let Some((from, to)) = moved {
        hud.move_value(from, to);
        changed = true;
    }
    if let Some(index) = removed {
        hud.values.remove(index);
        changed = true;
    }
    changed
}

pub fn drop_index(from: usize, target: usize, below: bool) -> usize {
    match (from < target, below) {
        (true, false) => target - 1,
        (true, true) | (false, false) => target,
        (false, true) => target + 1,
    }
}

pub fn value_job(ui: &Ui, value: Value, look: Look) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = egui::TextStyle::Button.resolve(ui.style());
    job.append(
        "\u{25CF}  ",
        0.0,
        TextFormat::simple(font.clone(), look.group(value.group())),
    );
    job.append(
        value.name(),
        0.0,
        TextFormat::simple(font, ui.visuals().text_color()),
    );
    job
}

fn add_value(ui: &mut Ui, hud: &mut Hud, ds: bool, look: Look) -> bool {
    let groups: Vec<Group> = Group::ALL
        .into_iter()
        .filter(|&group| addable(&hud.values, ds, group).next().is_some())
        .collect();
    if groups.is_empty() {
        return false;
    }
    let mut picked = None;
    ui.menu_button(format!("{}  Add a value", icons::PLUS), |ui| {
        let half = groups.len().div_ceil(2);
        ui.horizontal_top(|ui| {
            for part in groups.chunks(half) {
                let width = part
                    .iter()
                    .flat_map(|&group| addable(&hud.values, ds, group))
                    .map(|value| menu_row_width(ui, value_job(ui, value, look)))
                    .fold(ADD_COLUMN, f32::max);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    for &group in part {
                        ui.label(RichText::new(group.name()).weak().small());
                        for value in addable(&hud.values, ds, group) {
                            if menu_row(ui, value_job(ui, value, look), None)
                                .on_hover_text(value.hint())
                                .clicked()
                            {
                                picked = Some(value);
                            }
                        }
                    }
                });
            }
        });
    });
    match picked {
        Some(value) => {
            hud.values.push(value);
            true
        }
        None => false,
    }
}
