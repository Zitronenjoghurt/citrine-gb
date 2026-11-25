use std::sync::Arc;

pub use egui_phosphor::regular::*;

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

static FONT: &[u8] = include_bytes!("../assets/fonts/CitrineIcons.ttf");
const FONT_NAME: &str = "citrine-icons";

pub(crate) fn add_font(fonts: &mut egui::FontDefinitions) {
    fonts.font_data.insert(
        FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_static(FONT)),
    );
    let family = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    let at = family
        .iter()
        .position(|name| name == "phosphor")
        .map_or(family.len(), |index| index + 1);
    family.insert(at, FONT_NAME.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_own_icon_draws_from_the_app_fonts() {
        let ctx = egui::Context::default();
        crate::app::Citrine::setup_fonts(&ctx);
        let _ = ctx.run(Default::default(), |_| {});
        let font = egui::FontId::proportional(12.5);
        for (name, glyph) in OWN {
            assert!(
                ctx.fonts_mut(|fonts| fonts.has_glyphs(&font, glyph)),
                "{name} has no glyph in the app fonts"
            );
        }
    }

    #[test]
    fn the_icon_font_is_built_from_the_current_svgs() {
        let wanted = format!("sources {SOURCES}");
        assert!(
            FONT.windows(wanted.len())
                .any(|window| window == wanted.as_bytes()),
            "CitrineIcons.ttf is older than assets/icons, run scripts/icons-font.py"
        );
    }

    #[test]
    fn own_icons_sit_after_phosphor() {
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        add_font(&mut fonts);
        let family = &fonts.families[&egui::FontFamily::Proportional];
        let phosphor = family.iter().position(|name| name == "phosphor");
        let own = family.iter().position(|name| name == FONT_NAME);
        assert_eq!(own, phosphor.map(|index| index + 1));
    }
}
