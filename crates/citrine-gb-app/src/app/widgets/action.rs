use crate::icons;
use egui::{Button, Color32, Response, RichText, Stroke, Ui, Vec2, WidgetText, vec2};

pub fn action_height(ui: &Ui) -> f32 {
    ui.spacing().interact_size.y.max(32.0)
}

pub fn wide_button(ui: &mut Ui, button: Button) -> Response {
    let size = vec2(ui.available_width(), action_height(ui));
    ui.add_sized(size, button)
}

pub fn icon_text(icon: impl Into<String>) -> RichText {
    RichText::new(icon).size(18.0)
}

pub fn icon_button(icon: impl Into<String>, side: f32) -> Button<'static> {
    Button::new(icon_text(icon)).min_size(Vec2::splat(side))
}

pub fn main_button(ui: &Ui, text: impl Into<WidgetText>) -> Button<'static> {
    let fill = ui.visuals().selection.bg_fill;
    Button::new(
        text.into()
            .color(crate::app::ui_state::ui_theme::ink_on(fill)),
    )
    .fill(fill)
}

pub fn quiet_button(ui: &Ui, text: impl Into<WidgetText>) -> Button<'static> {
    Button::new(text)
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, ui.visuals().widgets.inactive.bg_fill))
}

pub fn dismiss_button() -> Button<'static> {
    Button::new(icons::X).frame(false)
}

pub fn more_button(side: f32) -> Button<'static> {
    Button::new(icons::DOTS_THREE).min_size(Vec2::splat(side))
}
