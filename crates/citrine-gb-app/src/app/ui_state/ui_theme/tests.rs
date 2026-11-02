use super::{DARK_ERROR, dark};
use egui::Color32;

fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn red_text_reads_on_dark_buttons() {
    let visuals = dark();
    assert_eq!(visuals.error_fg_color, DARK_ERROR);
    for fill in [
        visuals.panel_fill,
        visuals.widgets.inactive.weak_bg_fill,
        visuals.widgets.hovered.weak_bg_fill,
    ] {
        assert!(contrast(DARK_ERROR, fill) >= 4.5, "{fill:?}");
    }
}

#[test]
fn the_dark_red_keeps_the_red_hue() {
    assert_eq!(DARK_ERROR.r(), 255);
    assert_eq!(DARK_ERROR.g(), DARK_ERROR.b());
}
