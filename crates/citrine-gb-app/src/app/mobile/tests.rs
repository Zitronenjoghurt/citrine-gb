use super::*;
use crate::app::ui_state::touch::LandscapeControls;
use egui::pos2;

#[test]
fn back_from_a_sheet_opened_in_the_menu_returns_to_the_menu() {
    let mut state = MobileState::default();
    state.open(Sheet::Menu);
    state.open(Sheet::Panel(Tab::Saves));
    state.back();
    assert_eq!(state.sheet, Some(Sheet::Menu));
    state.back();
    assert_eq!(state.sheet, None);
}

#[test]
fn back_from_a_sheet_opened_elsewhere_returns_to_the_game() {
    let mut state = MobileState::default();
    state.open(Sheet::Panel(Tab::Homebrew));
    state.back();
    assert_eq!(state.sheet, None);
}

#[test]
fn escape_steps_back_through_the_sheets() {
    let mut state = MobileState::default();
    state.open(Sheet::Menu);
    state.open(Sheet::Panel(Tab::Settings));
    assert!(state.escape(false));
    assert_eq!(
        state.sheet,
        Some(Sheet::Menu),
        "back to the menu it came from"
    );
    assert!(state.escape(false));
    assert_eq!(state.sheet, None);
    assert!(!state.escape(false), "the game screen stays");
}

#[test]
fn escape_closes_the_top_layer_before_the_sheet() {
    let mut state = MobileState::default();
    state.open(Sheet::Panel(Tab::Saves));
    assert!(!state.escape(true));
    assert_eq!(state.sheet, Some(Sheet::Panel(Tab::Saves)));
}

#[test]
fn a_panel_sheet_shows_its_own_name() {
    assert_eq!(
        header_text(Some(Sheet::Panel(Tab::Saves)), "POKEMON"),
        "Saves & snapshots"
    );
    assert_eq!(
        header_text(Some(Sheet::Panel(Tab::Media)), "POKEMON"),
        "Media"
    );
    assert_eq!(header_text(Some(Sheet::Menu), "POKEMON"), "POKEMON");
    assert_eq!(header_text(None, ""), "Citrine");
}

#[test]
fn portrait_bands_cover_the_area_exactly() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(390.0, 760.0));
    let (screen, controls) = portrait_split(area, &TouchSettings::default());

    assert!((screen.height() + controls.height() - area.height()).abs() < 0.01);
    assert_eq!(screen.top(), area.top());
    assert_eq!(controls.bottom(), area.bottom());
}

#[test]
fn the_control_band_fits_the_controls_without_stranding_them() {
    for height in [600.0_f32, 760.0, 900.0, 1200.0] {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, height));
        let settings = TouchSettings::default();
        let (_, controls) = portrait_split(area, &settings);
        let needed = TouchLayout::bar_height(area.width(), &settings);

        assert!(
            controls.height() >= needed - 0.01,
            "controls were squeezed at height {height}: {} < {needed}",
            controls.height()
        );
        assert!(
            controls.height() <= needed * CONTROL_BREATHING_ROOM + 0.01,
            "a taller window must not strand the buttons in empty space at \
             height {height}: {} for {needed} needed",
            controls.height()
        );
    }
}

#[test]
fn bigger_buttons_get_a_taller_band() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 900.0));
    let small = TouchSettings {
        scale: TouchSettings::MIN_SCALE,
        ..Default::default()
    };
    let large = TouchSettings {
        scale: TouchSettings::MAX_SCALE,
        ..Default::default()
    };

    let (_, small_band) = portrait_split(area, &small);
    let (_, large_band) = portrait_split(area, &large);
    assert!(large_band.height() > small_band.height());
}

#[test]
fn a_short_portrait_window_still_leaves_room_for_buttons() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
    let (screen, controls) = portrait_split(area, &TouchSettings::default());

    assert!(
        screen.height() > 0.0,
        "the screen never collapses to nothing"
    );
    assert!(
        controls.height() <= area.height() * 0.6 + 0.01,
        "the screen keeps at least 40% of a short window"
    );
}

#[test]
fn landscape_columns_do_not_overlap_the_screen() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(844.0, 390.0));
    let (screen, left, right) = landscape_split(area, LandscapeControls::Beside);

    assert!(screen.left() >= left.right());
    assert!(screen.right() <= right.left());
    assert!(screen.width() > 0.0);
}

#[test]
fn the_overlay_mode_hands_the_whole_area_to_the_screen() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(844.0, 390.0));
    let (screen, left, right) = landscape_split(area, LandscapeControls::Overlay);

    assert_eq!(screen, area, "the picture uses the full width");
    assert!(left.width() > 0.0 && right.width() > 0.0);
    assert!(right.right() <= area.right());
}

#[test]
fn a_very_wide_window_still_keeps_the_columns_reachable() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 400.0));
    let (_, left, _) = landscape_split(area, LandscapeControls::Beside);
    assert!(
        left.width() <= area.width() * 0.32,
        "columns never sprawl across half the screen"
    );
}
