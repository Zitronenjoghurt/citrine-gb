use crate::icons;
use egui::emath::Numeric;
use egui::{
    Context, Id, Rect, Response, Sense, TextStyle, TextWrapMode, Ui, Vec2, Widget, WidgetText, vec2,
};
use std::fmt::Display;

pub struct ResetSlider<'a, V: Numeric> {
    value: &'a mut V,
    range: std::ops::RangeInclusive<V>,
    default: V,
    step: Option<f64>,
    suffix: Option<&'static str>,
    text: Option<fn(f64) -> String>,
}

impl<'a, V: Numeric> ResetSlider<'a, V> {
    pub fn new(value: &'a mut V, range: std::ops::RangeInclusive<V>) -> Self {
        let default = *value;
        Self {
            value,
            range,
            default,
            step: None,
            suffix: None,
            text: None,
        }
    }

    pub fn default_value(mut self, default: V) -> Self {
        self.default = default;
        self
    }

    pub fn step_by(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    pub fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = Some(suffix);
        self
    }

    pub fn text(mut self, text: fn(f64) -> String) -> Self {
        self.text = Some(text);
        self
    }
}

pub fn percent_slider(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    default: f32,
) -> Response {
    let percent = |fraction: f32| (fraction * 100.0).round();
    let mut shown = percent(*value);
    let response = ResetSlider::new(&mut shown, percent(*range.start())..=percent(*range.end()))
        .step_by(f64::from(percent(step)))
        .suffix("%")
        .default_value(percent(default))
        .ui(ui);
    if response.changed() {
        *value = shown / 100.0;
    }
    response
}

impl<V: Numeric> Widget for ResetSlider<'_, V> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = RESET_GAP;

            let decimals = self
                .step
                .map_or(0, |step| (-step.log10()).ceil().max(0.0) as usize);
            let suffix = self.suffix.unwrap_or_default();
            let (start, end) = (self.range.start().to_f64(), self.range.end().to_f64());
            let widest = match self.text {
                Some(text) => [start, end, start + 1.0, end - 1.0, self.default.to_f64()]
                    .map(text)
                    .into_iter()
                    .max_by_key(String::len)
                    .unwrap_or_default(),
                None => format!("{:.*}{suffix}", decimals, end),
            };
            let value_width = WidgetText::from(widest)
                .into_galley(
                    ui,
                    Some(TextWrapMode::Extend),
                    f32::INFINITY,
                    ui.style().drag_value_text_style.clone(),
                )
                .size()
                .x
                + 2.0 * ui.spacing().button_padding.x;
            let value_width = super::settings::form::value_box(ui, value_width);

            let reset = reset_room(ui);
            super::settings::form::need(ui, SLIDER_MIN + value_width + RESET_GAP + reset);
            ui.spacing_mut().slider_width =
                (ui.available_width() - value_width - RESET_GAP - reset)
                    .max(ui.spacing().interact_size.y);
            ui.spacing_mut().interact_size.x = value_width;

            let mut slider = egui::Slider::new(self.value, self.range)
                .fixed_decimals(decimals)
                .suffix(suffix);
            if let Some(step) = self.step {
                slider = slider.step_by(step);
            }
            if let Some(text) = self.text {
                slider = slider
                    .custom_formatter(move |value, _| text(value))
                    .custom_parser(|input| input.trim().trim_end_matches('h').trim().parse().ok());
            }
            let mut response = slider.ui(ui);

            let is_default = self.value.to_f64() == self.default.to_f64();
            if reset_button(ui, !is_default, self.default.to_f64()).clicked() {
                *self.value = self.default;
                response.mark_changed();
            }

            response
        })
        .inner
    }
}

pub const RESET_GAP: f32 = 4.0;

const SLIDER_MIN: f32 = 40.0;

fn reset_icon(ui: &Ui) -> Vec2 {
    WidgetText::from(icons::ARROW_COUNTER_CLOCKWISE)
        .into_galley(
            ui,
            Some(TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Button,
        )
        .size()
}

pub fn reset_width(ui: &Ui) -> f32 {
    (reset_icon(ui).x + 2.0 * ui.spacing().button_padding.x)
        .max(crate::app::mobile::touch_square(ui))
}

pub fn reset_height(ui: &Ui) -> f32 {
    reset_icon(ui).y.max(crate::app::mobile::touch_square(ui))
}

fn hidden_key() -> Id {
    Id::new("reset_hidden")
}

pub fn without_reset<R>(ctx: &Context, hide: bool, add: impl FnOnce() -> R) -> R {
    let outer = ctx.data(|d| d.get_temp::<bool>(hidden_key()));
    ctx.data_mut(|d| d.insert_temp(hidden_key(), hide));
    let inner = add();
    ctx.data_mut(|d| match outer {
        Some(outer) => d.insert_temp(hidden_key(), outer),
        None => d.remove::<bool>(hidden_key()),
    });
    inner
}

pub fn reset_shown(ui: &Ui) -> bool {
    !ui.data(|d| d.get_temp::<bool>(hidden_key()))
        .unwrap_or(false)
}

pub fn reset_room(ui: &Ui) -> f32 {
    if reset_shown(ui) {
        reset_width(ui) + RESET_GAP
    } else {
        0.0
    }
}

pub fn reset_button(ui: &mut Ui, enabled: bool, default: impl Display) -> Response {
    if !reset_shown(ui) {
        return ui.interact(Rect::NOTHING, ui.id().with("no_reset"), Sense::hover());
    }
    let width = reset_width(ui);
    let button = crate::app::mobile::compact(ui, egui::Button::new(icons::ARROW_COUNTER_CLOCKWISE))
        .min_size(vec2(width, 0.0));
    ui.add_enabled(enabled, button)
        .on_hover_text(format!("Reset to {default}"))
}

pub fn with_reset(
    ui: &mut Ui,
    changed: bool,
    default: impl Display,
    add: impl FnOnce(&mut Ui),
) -> bool {
    let spacing = ui.spacing().item_spacing;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = RESET_GAP;
        let width = (ui.available_width() - reset_room(ui)).max(0.0);
        let size = vec2(width, ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            size,
            egui::Layout::top_down_justified(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing = spacing;
                super::settings::form::reset_taken(ui, add);
            },
        );
        if !reset_shown(ui) {
            return false;
        }
        let row = vec2(
            reset_width(ui),
            ui.spacing().interact_size.y.max(reset_height(ui)),
        );
        ui.allocate_ui_with_layout(
            row,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| reset_button(ui, changed, default).clicked(),
        )
        .inner
    })
    .inner
}
