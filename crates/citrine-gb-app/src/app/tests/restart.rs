use super::*;
use std::collections::HashMap;

const ANDROID_MAIN_HEADROOM: usize = 256 * 1024;

#[derive(Default)]
struct Saved(HashMap<String, String>);

impl Storage for Saved {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }

    fn flush(&mut self) {}
}

fn first_run() -> Saved {
    let mut app = crystal_app("restart", LayoutMode::Mobile, None);
    let key = app.emulator.rom_key().map(str::to_string);
    let _ = app
        .ui
        .recent
        .record(std::path::Path::new("crystal.gbc"), "Crystal", key);
    let ctx = Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(412.0, 915.0),
        )),
        ..Default::default()
    };
    for _ in 0..3 {
        let _ = ctx.run(input.clone(), |ctx| app.render(ctx));
    }
    let mut saved = Saved::default();
    eframe::set_value(&mut saved, eframe::APP_KEY, &app);
    saved
}

fn on_small_stack<T: Send + 'static>(start: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(ANDROID_MAIN_HEADROOM)
        .spawn(start)
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn second_start_restores_what_the_first_saved_within_an_android_stack() {
    let saved = first_run();
    let recent: Vec<String> = Citrine::restore(Some(&saved))
        .ui
        .recent
        .entries()
        .iter()
        .map(crate::app::ui_state::recent::RecentRom::display_name)
        .collect();
    assert!(!recent.is_empty());
    let restored = on_small_stack(move || {
        Citrine::restore(Some(&saved))
            .ui
            .recent
            .entries()
            .iter()
            .map(crate::app::ui_state::recent::RecentRom::display_name)
            .collect::<Vec<_>>()
    });
    assert_eq!(restored, recent);
}

#[test]
fn first_start_builds_within_an_android_stack() {
    let fresh = on_small_stack(|| Citrine::restore(None).ui.recent.is_empty());
    assert!(fresh);
}

#[test]
fn app_state_stays_small_on_the_stack() {
    assert!(std::mem::size_of::<Citrine>() < 16 * 1024);
}
