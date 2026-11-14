use crate::app::report::{Hand, Outgoing};
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
        recording: viewer.ui.frame_log.recording(),
        screen_match: emulator.screen_match(),
    }
    .show(ui);
    match action {
        Some(PerformanceAction::StartRecording) => {
            viewer
                .ui
                .frame_log
                .start_recording(web_time::Instant::now());
        }
        Some(PerformanceAction::StopRecording) => save_recording(viewer, ui),
        None => {}
    }
}

fn save_recording(viewer: &mut TabViewer, ui: &egui::Ui) {
    let Some(recorded) = viewer.ui.frame_log.stop_recording() else {
        return;
    };
    if recorded.frames == 0 {
        return;
    }
    let file_name = format!(
        "{} frames.zip",
        crate::media::file_stem(viewer.emulator.game_name(), crate::storage::now_unix())
    );
    let pack =
        viewer
            .ui
            .record_watch
            .finish(recorded, ui.ctx(), viewer.emulator, &viewer.ui.settings);
    let hand = Hand::of(viewer.files, "recording");
    viewer.files.spawner.run("frame times", move || {
        let bytes = match pack.zip() {
            Ok(bytes) => bytes,
            Err(err) => {
                log::error!("recording: can't pack the zip: {err}");
                return;
            }
        };
        hand.over(Outgoing {
            file_name,
            bytes,
            filter: "Frame recording",
            extension: "zip",
            mime: "application/zip",
        });
    });
}
