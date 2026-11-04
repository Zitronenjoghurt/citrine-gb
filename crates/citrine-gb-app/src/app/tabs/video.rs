use crate::app::tabs::TabViewer;
use crate::app::widgets::video::VideoView;
use egui::Ui;

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let state = &mut viewer.ui.video;
    state.refresh(viewer.emulator.debug_console());
    VideoView::new(state).show(ui);
}
