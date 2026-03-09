use crate::emulator::Emulator;
use egui::{Color32, Response, Ui, Widget};
use egui_plot::{Line, Plot, PlotBounds, PlotPoints};
use std::collections::VecDeque;

pub struct ApuWaves<'a> {
    emulator: &'a mut Emulator,
}

impl<'a> ApuWaves<'a> {
    pub fn new(emulator: &'a mut Emulator) -> Self {
        Self { emulator }
    }
}

impl Widget for ApuWaves<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let debugger = &self.emulator.gb.debugger;

        let create_points = |queue: &VecDeque<f32>, zoom_len: usize| -> PlotPoints {
            let search_limit = queue.len().saturating_sub(zoom_len);
            if search_limit == 0 {
                return PlotPoints::default();
            }

            let (min, max) = queue
                .iter()
                .take(search_limit + zoom_len)
                .fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)));
            let amplitude = max - min;
            let midpoint = (min + max) / 2.0;

            let mut start_idx = 0;

            if amplitude > 0.01 {
                let band = amplitude * 0.1;
                let lower = midpoint - band;
                let upper = midpoint + band;

                let mut armed = false;
                for i in 0..search_limit {
                    if queue[i] < lower {
                        armed = true;
                    }
                    if armed && queue[i] >= upper {
                        start_idx = i;
                        break;
                    }
                }
            }

            queue
                .iter()
                .skip(start_idx)
                .take(zoom_len)
                .enumerate()
                .map(|(i, &sample)| [i as f64, sample as f64])
                .collect()
        };

        let response = ui
            .vertical(|ui| {
                let plot_height = ui.available_height() * 0.25;

                ui.vertical(|ui| {
                    ui.set_height(plot_height);
                    Plot::new("apu_ch1_plot")
                        .allow_drag(false)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .show_axes([false, false])
                        .show(ui, |plot_ui| {
                            plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                                [0.0, 0.0],
                                [512.0, 1.0],
                            ));
                            plot_ui.line(
                                Line::new(
                                    "CH1 (Square+Sweep)",
                                    create_points(&debugger.ch1_samples, 512),
                                )
                                .color(Color32::LIGHT_RED),
                            );
                        });
                });

                ui.vertical(|ui| {
                    ui.set_height(plot_height);
                    Plot::new("apu_ch2_plot")
                        .allow_drag(false)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .show_axes([false, false])
                        .show(ui, |plot_ui| {
                            plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                                [0.0, 0.0],
                                [512.0, 1.0],
                            ));
                            plot_ui.line(
                                Line::new(
                                    "CH2 (Square)",
                                    create_points(&debugger.ch2_samples, 512),
                                )
                                .color(Color32::GOLD),
                            );
                        });
                });

                ui.vertical(|ui| {
                    ui.set_height(plot_height);
                    Plot::new("apu_ch3_plot")
                        .height(plot_height)
                        .allow_drag(false)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .show_axes([false, false])
                        .show(ui, |plot_ui| {
                            plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                                [0.0, 0.0],
                                [512.0, 1.0],
                            ));
                            plot_ui.line(
                                Line::new("CH3 (Wave)", create_points(&debugger.ch3_samples, 512))
                                    .color(Color32::LIGHT_GREEN),
                            );
                        });
                });

                ui.vertical(|ui| {
                    ui.set_height(plot_height);
                    Plot::new("apu_ch4_plot")
                        .height(plot_height)
                        .allow_drag(false)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .show_axes([false, false])
                        .show(ui, |plot_ui| {
                            plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                                [0.0, 0.0],
                                [512.0, 1.0],
                            ));
                            plot_ui.line(
                                Line::new("CH4 (Noise)", create_points(&debugger.ch4_samples, 512))
                                    .color(Color32::LIGHT_BLUE),
                            );
                        });
                });
            })
            .response;

        response
    }
}
