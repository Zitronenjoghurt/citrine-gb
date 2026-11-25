use crate::app::tabs::TabViewer;
use crate::app::widgets::settings::this_game::{ThisGame, ThisGameSheet};

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let game = ThisGame::of(
        viewer.emulator,
        viewer.play.session().is_some(),
        viewer.senses,
    );
    let sheet = ThisGameSheet::new(
        game.as_ref(),
        &mut viewer.ui.settings,
        Some(&mut *viewer.events),
    )
    .feed(viewer.senses.camera_feed())
    .empty(ThisGame::empty_text(viewer.emulator));
    ui.add(sheet);
}
