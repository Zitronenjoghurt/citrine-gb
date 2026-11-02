use egui::{Align, Key, TextBuffer, TextEdit, Ui, vec2};

pub fn row_field<'t>(ui: &Ui, text: &'t mut dyn TextBuffer) -> TextEdit<'t> {
    TextEdit::singleline(text)
        .min_size(vec2(0.0, ui.spacing().interact_size.y))
        .vertical_align(Align::Center)
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
mod tests;
