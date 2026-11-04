use crate::app::ui_state::settings::Settings;
use egui::{Align, Color32, Layout, Response, Ui, Widget, vec2};
use egui_plot::{GridMark, Line, Plot, PlotBounds, PlotPoints};
use std::collections::VecDeque;

const WAVE_LEN: usize = 512;
const NAME_WIDTH: f32 = 72.0;
const MIN_ROW_HEIGHT: f32 = 40.0;
const MAX_ROW_HEIGHT: f32 = 160.0;
const WAVE_COLORS: [(Color32, Color32); 6] = [
    (
        Color32::from_rgb(255, 140, 140),
        Color32::from_rgb(200, 40, 40),
    ),
    (
        Color32::from_rgb(255, 205, 60),
        Color32::from_rgb(190, 110, 0),
    ),
    (
        Color32::from_rgb(144, 238, 144),
        Color32::from_rgb(30, 130, 30),
    ),
    (
        Color32::from_rgb(150, 170, 255),
        Color32::from_rgb(40, 80, 200),
    ),
    (
        Color32::from_rgb(210, 160, 255),
        Color32::from_rgb(130, 50, 190),
    ),
    (
        Color32::from_rgb(110, 220, 220),
        Color32::from_rgb(0, 130, 140),
    ),
];

pub struct ApuWidget<'a> {
    names: &'static [&'static str],
    waves: &'a [VecDeque<f32>],
    settings: &'a mut Settings,
}

impl<'a> ApuWidget<'a> {
    pub fn new(
        names: &'static [&'static str],
        waves: &'a [VecDeque<f32>],
        settings: &'a mut Settings,
    ) -> Self {
        Self {
            names,
            waves,
            settings,
        }
    }
}

impl Widget for ApuWidget<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let rows = self.names.len().max(1) as f32;
        let gap = ui.spacing().item_spacing.y;
        let row_height = ((ui.available_height() - gap * (rows - 1.0)) / rows)
            .clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT);
        let dark = ui.visuals().dark_mode;
        let font = egui::TextStyle::Body.resolve(ui.style());
        let widest = self
            .names
            .iter()
            .map(|name| {
                ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap((*name).to_owned(), font.clone(), Color32::WHITE)
                        .size()
                        .x
                })
            })
            .fold(0.0, f32::max);
        let name_width = (widest + ui.spacing().icon_width + ui.spacing().icon_spacing)
            .max(NAME_WIDTH)
            .ceil();
        let empty = VecDeque::new();
        let mut dirty = false;

        let response = ui
            .vertical(|ui| {
                for (index, (name, enabled)) in self
                    .names
                    .iter()
                    .zip(self.settings.channels_for(self.names.len()).iter_mut())
                    .enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            vec2(name_width, row_height),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_width(name_width);
                                dirty |= crate::app::widgets::check::checkbox(ui, enabled, *name)
                                    .changed();
                            },
                        );
                        let wave = self.waves.get(index).unwrap_or(&empty);
                        let (on_dark, on_light) = WAVE_COLORS[index % WAVE_COLORS.len()];
                        let color = if dark { on_dark } else { on_light };
                        wave_plot(ui, index, name, wave, color, row_height);
                    });
                }
            })
            .response;
        self.settings.dirty |= dirty;
        response
    }
}

fn wave_plot(
    ui: &mut Ui,
    index: usize,
    name: &str,
    wave: &VecDeque<f32>,
    color: Color32,
    height: f32,
) {
    Plot::new(("apu_wave", index))
        .height(height)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show_axes([false, false])
        .x_grid_spacer(quarter_marks)
        .show(ui, |plot_ui| {
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([0.0, 0.0], [WAVE_LEN as f64, 1.0]));
            plot_ui.line(Line::new(name, wave_points(wave, WAVE_LEN)).color(color));
        });
}

fn quarter_marks(_: egui_plot::GridInput) -> Vec<GridMark> {
    let step = WAVE_LEN as f64 / 4.0;
    (1..4)
        .map(|quarter| GridMark {
            value: quarter as f64 * step,
            step_size: step,
        })
        .collect()
}

fn wave_points(wave: &VecDeque<f32>, len: usize) -> PlotPoints<'static> {
    let search_limit = wave.len().saturating_sub(len);
    if search_limit == 0 {
        return PlotPoints::default();
    }

    let (min, max) = wave
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)));
    let amplitude = max - min;
    let midpoint = (min + max) / 2.0;

    let mut start = 0;
    if amplitude > 0.01 {
        let band = amplitude * 0.1;
        let lower = midpoint - band;
        let upper = midpoint + band;

        let mut armed = false;
        for (i, &sample) in wave.iter().take(search_limit).enumerate() {
            if sample < lower {
                armed = true;
            }
            if armed && sample >= upper {
                start = i;
                break;
            }
        }
    }

    wave.iter()
        .skip(start)
        .take(len)
        .enumerate()
        .map(|(i, &sample)| [i as f64, sample as f64])
        .collect()
}
