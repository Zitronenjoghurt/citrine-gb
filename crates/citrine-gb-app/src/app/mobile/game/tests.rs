use super::*;
use crate::app::ui_state::touch::LandscapeControls;
use egui::pos2;

#[test]
fn portrait_bands_cover_the_area_exactly() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(390.0, 760.0));
    let (screen, controls) = portrait_split(area, &TouchSettings::default(), false);

    assert!((screen.height() + controls.height() - area.height()).abs() < 0.01);
    assert_eq!(screen.top(), area.top());
    assert_eq!(controls.bottom(), area.bottom());
}

#[test]
fn one_screen_sits_under_the_strip_with_even_space_around_the_buttons() {
    let area = Rect::from_min_size(pos2(0.0, 60.0), vec2(412.0, 700.0));
    let (screen, controls) = portrait_split(area, &TouchSettings::default(), true);

    assert_eq!(screen.top(), area.top());
    assert!((screen.height() - 412.0 * 144.0 / 160.0).abs() < 0.01);
    let needed = TouchLayout::bar_height(area.width(), &TouchSettings::default());
    let slack = (controls.height() - needed) / 2.0;
    let above = controls.top() - screen.bottom() + slack;
    let below = area.bottom() - controls.bottom();
    assert!(
        above > 0.0,
        "a tall phone leaves room between the game and the buttons"
    );
    assert!(
        (above - below).abs() < 0.01,
        "{above} above the d-pad, {below} below the pills"
    );
}

#[test]
fn the_control_band_fits_the_controls_without_stranding_them() {
    for height in [600.0_f32, 760.0, 900.0, 1200.0] {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, height));
        let settings = TouchSettings::default();
        let (_, controls) = portrait_split(area, &settings, false);
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

    let (_, small_band) = portrait_split(area, &small, false);
    let (_, large_band) = portrait_split(area, &large, false);
    assert!(large_band.height() > small_band.height());
}

#[test]
fn a_short_portrait_window_still_leaves_room_for_buttons() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
    let (screen, controls) = portrait_split(area, &TouchSettings::default(), false);

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
    let (screen, left, right, _) = landscape_split(area, LandscapeControls::Beside, true);

    assert!(screen.left() >= left.right());
    assert!(screen.right() <= right.left());
    assert!(screen.width() > 0.0);
}

#[test]
fn the_overlay_mode_hands_the_whole_area_to_the_screen() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(844.0, 390.0));
    let (screen, left, right, _) = landscape_split(area, LandscapeControls::Overlay, true);

    assert_eq!(screen, area, "the picture uses the full width");
    assert!(left.width() > 0.0 && right.width() > 0.0);
    assert!(right.right() <= area.right());
}

#[test]
fn a_very_wide_window_still_keeps_the_columns_reachable() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 400.0));
    let (_, left, _, _) = landscape_split(area, LandscapeControls::Beside, true);
    assert!(
        left.width() <= area.width() * 0.32,
        "columns never sprawl across half the screen"
    );
}

#[test]
fn beside_puts_the_strip_in_one_row_above_the_picture() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(794.0, 400.0));
    let (screen, _, right, strip) = landscape_split(area, LandscapeControls::Beside, true);

    assert!(
        screen.top() >= strip.bottom(),
        "the picture stays under the strip"
    );
    assert_eq!(
        strip.right(),
        right.right(),
        "the strip ends at the right edge"
    );
    assert!(
        strip.width() >= 5.0 * 52.0,
        "five strip buttons fit one row, got {}",
        strip.width()
    );
}

#[test]
fn with_the_strip_hidden_the_beside_picture_takes_the_full_height() {
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(891.0, 412.0));
    let (screen, left, right, strip) = landscape_split(area, LandscapeControls::Beside, false);
    assert_eq!(screen.top(), area.top());
    assert_eq!(screen.bottom(), area.bottom());
    assert_eq!(screen.left(), left.right());
    assert_eq!(screen.right(), right.left());
    assert_eq!(strip.height(), 0.0);
    let picture = super::hide::picture_in(screen);
    assert!((picture.height() - area.height()).abs() < 0.01);
    assert!(picture.width() <= screen.width() + 0.01);
}
