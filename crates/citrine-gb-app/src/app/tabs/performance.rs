use crate::app::tabs::TabViewer;
use crate::app::widgets::performance::{PerformanceAction, PerformanceWidget, Timings};
use crate::app::widgets::threads_debug::REFRESH;
use std::time::Duration;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    viewer.ui.performance.mark_seen();
    ui.ctx().request_repaint_after(REFRESH);
    let readout = viewer.ui.frame_log.readout();
    let emulator = &*viewer.emulator;
    let (_, action) = PerformanceWidget {
        budget: emulator.frame_budget(),
        timings: Timings {
            game: Duration::from_secs_f32(emulator.frame_avg_timer.average_secs()),
            app: Duration::from_secs_f32(viewer.ui.update_avg_timer.average_secs()),
            shown: viewer.ui.update_avg_timer.updates_per_sec(),
        },
        readout,
        load: viewer.ui.performance.view(),
        samples: viewer.ui.frame_log.samples(),
        frame_times: viewer.ui.settings.record_frame_times,
        screen_match: emulator.screen_match(),
    }
    .show(ui);
    if let Some(PerformanceAction::FrameTimes(on)) = action {
        viewer.ui.settings.record_frame_times = on;
        viewer.ui.settings.dirty = true;
        if !on {
            viewer.ui.frame_log.stop_recording();
        } else if viewer.emulator.is_recording() {
            viewer
                .ui
                .frame_log
                .start_recording(web_time::Instant::now());
        }
    }
}
