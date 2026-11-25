use egui::text_edit::TextEditOutput;
use egui::{Align, Key, Margin, TextBuffer, TextEdit, Ui, vec2};

const ROW_FIELD_MARGIN: Margin = Margin::symmetric(4, 2);

pub fn row_field<'t>(ui: &Ui, text: &'t mut dyn TextBuffer) -> TextEdit<'t> {
    TextEdit::singleline(text)
        .min_size(vec2(0.0, ui.spacing().interact_size.y))
        .vertical_align(Align::Center)
        .margin(ROW_FIELD_MARGIN)
}

pub fn row_field_width(ui: &Ui) -> f32 {
    (super::settings::form::field_width(ui) - ROW_FIELD_MARGIN.sum().x).max(0.0)
}

pub fn framed(ui: &mut Ui, edit: TextEdit<'_>) -> TextEditOutput {
    let back = ui.painter().add(egui::Shape::Noop);
    let output = edit.frame(false).show(ui);
    let response = &output.response;
    let look = ui.style().interact(response);
    let stroke = if response.has_focus() {
        egui::Stroke::new(
            ui.visuals().selection.stroke.width,
            crate::app::ui_state::ui_theme::focus_ring(ui.visuals()),
        )
    } else {
        look.bg_stroke
    };
    ui.painter().set(
        back,
        egui::epaint::RectShape::new(
            response.rect.expand(look.expansion),
            look.corner_radius,
            ui.visuals().text_edit_bg_color(),
            stroke,
            egui::StrokeKind::Inside,
        ),
    );
    output
}

pub fn code_field(ui: &mut Ui, code: &mut String, hint: &str, width: f32, focus: bool) -> bool {
    let response = ui.add(row_field(ui, code).hint_text(hint).desired_width(width));
    let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if focus && !entered && ui.memory(|m| m.focused().is_none()) {
        response.request_focus();
    }
    entered
}

#[cfg(test)]
mod tests {
    use super::code_field;

    fn enter() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn frames(focus: bool, inputs: Vec<Vec<egui::Event>>) -> Vec<bool> {
        let ctx = egui::Context::default();
        let mut code = "123456".to_owned();
        inputs
            .into_iter()
            .map(|events| {
                let mut entered = false;
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        entered = code_field(ui, &mut code, "Code", 200.0, focus);
                    });
                });
                entered
            })
            .collect()
    }

    #[test]
    fn enter_submits_a_code_field_that_keeps_its_focus() {
        let entered = frames(true, vec![vec![], vec![], vec![enter()], vec![]]);
        assert_eq!(entered, [false, false, true, false]);
    }

    #[test]
    fn a_focused_code_field_submits_nothing_without_enter() {
        let entered = frames(true, vec![vec![], vec![], vec![], vec![]]);
        assert_eq!(entered, [false; 4]);
    }
}
