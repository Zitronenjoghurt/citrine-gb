use super::{
    BRAND, BRAND_DARK, BRAND_MARK, DARK_ERROR, ON_BRAND, ON_DANGER, UiTheme, brand, contrast,
    danger, dark, focus_ring, text_grounds,
};
use citrine_gb::debug::code::{InstrClass, OperandKind};
use strum::IntoEnumIterator;

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

#[test]
fn every_theme_tells_code_classes_and_operands_apart() {
    for theme in UiTheme::iter() {
        let colours = theme.code_colours();
        let classes: Vec<_> = InstrClass::ALL.map(|c| colours.class(c)).into();
        let operands: Vec<_> = OperandKind::ALL.map(|k| colours.operand(k)).into();
        for list in [classes, operands] {
            for (i, a) in list.iter().enumerate() {
                assert!(!list[i + 1..].contains(a), "{theme} repeats {a:?}");
            }
        }
    }
}

#[test]
fn weak_text_reads_in_every_theme() {
    for theme in UiTheme::iter() {
        let visuals = themed(theme);
        let weak = visuals.weak_text_color();
        for ground in text_grounds(&visuals) {
            let ratio = contrast(weak, ground);
            assert!(ratio >= 4.5, "{theme} weak {weak:?} on {ground:?}: {ratio}");
        }
        assert!(
            contrast(visuals.text_color(), visuals.panel_fill)
                > contrast(weak, visuals.panel_fill) + 0.5,
            "{theme} weak text looks like body text"
        );
    }
}

fn themed(theme: UiTheme) -> egui::Visuals {
    let ctx = egui::Context::default();
    theme.apply(&ctx);
    ctx.style().visuals.clone()
}

#[test]
fn every_theme_selects_slides_and_toggles_in_the_brand_colour() {
    for theme in UiTheme::iter() {
        let visuals = themed(theme);
        let wanted = if visuals.dark_mode { BRAND_DARK } else { BRAND };
        assert_eq!(visuals.selection.bg_fill, wanted, "{theme} selection");
        assert_eq!(brand(&visuals), wanted, "{theme} buttons");
        assert!(visuals.slider_trailing_fill, "{theme} slider fill");
    }
}

#[test]
fn selected_text_and_the_focus_ring_read_in_every_theme() {
    for theme in UiTheme::iter() {
        let visuals = themed(theme);
        let ink = visuals.selection.stroke.color;
        assert_eq!(ink, ON_BRAND, "{theme}");
        let ratio = contrast(ink, visuals.selection.bg_fill);
        assert!(ratio >= 4.5, "{theme} selected text: {ratio}");
        let field = visuals.text_edit_bg_color();
        let wanted = if visuals.dark_mode {
            BRAND_DARK
        } else {
            BRAND_MARK
        };
        assert_eq!(focus_ring(&visuals), wanted, "{theme} focus ring colour");
        let ring = contrast(focus_ring(&visuals), field);
        assert!(ring >= 3.0, "{theme} focus ring: {ring}");
    }
}

#[test]
fn phone_row_values_read_on_cards() {
    for (theme, least) in [(super::UiTheme::Dark, 4.5), (super::UiTheme::Light, 3.0)] {
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        let visuals = ctx.style().visuals.clone();
        let card = crate::app::mobile::kit::card_fill(&visuals);
        let value = crate::app::mobile::kit::value_color(&visuals);
        assert!(
            contrast(value, card) >= least,
            "{theme}: {value:?} on {card:?}"
        );
    }
}

#[test]
fn destructive_buttons_read_in_every_theme() {
    for theme in UiTheme::iter() {
        let visuals = themed(theme);
        let ratio = contrast(ON_DANGER, danger(&visuals));
        assert!(ratio >= 4.5, "{theme} danger label: {ratio}");
    }
}
