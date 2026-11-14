use citrine_gb::rom::nds::{ICON_BYTES, ICON_SIZE, NdsBanner, NdsHeader, NdsLanguage, icon_rgba};
use egui::{Context, TextureHandle, TextureOptions};
use std::collections::HashMap;

pub fn language() -> NdsLanguage {
    sys_locale::get_locales()
        .find_map(|tag| NdsLanguage::from_tag(&tag))
        .unwrap_or(NdsLanguage::English)
}

pub fn banner(rom: &[u8]) -> Option<NdsBanner> {
    NdsHeader::parse(rom)?.banner(rom)
}

pub fn title(rom: &[u8]) -> Option<String> {
    banner(rom)?.title(language())
}

pub fn game_name(rom: &[u8], sha1: &[u8; 20], file_name: Option<&str>) -> Option<String> {
    let header = NdsHeader::parse(rom)?;
    Some(crate::game_name::ds_game_name(
        sha1,
        header.game_code,
        title(rom).as_deref(),
        file_name,
        &header.title,
    ))
}

pub fn icon_text(icon: &[u8; ICON_BYTES]) -> String {
    icon.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn icon_of_text(text: &str) -> Option<[u8; ICON_BYTES]> {
    if text.len() != ICON_BYTES * 2 || !text.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..ICON_BYTES)
        .map(|at| u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).ok())
        .collect();
    bytes?.try_into().ok()
}

pub fn icon_rect(ctx: &Context, center: egui::Pos2, wanted: f32) -> egui::Rect {
    let ppp = ctx.pixels_per_point();
    let scale = (wanted * ppp / ICON_SIZE as f32).floor().max(1.0);
    let pixels = scale * ICON_SIZE as f32;
    let min = ((center.to_vec2() * ppp) - egui::Vec2::splat(pixels / 2.0)).round() / ppp;
    egui::Rect::from_min_size(min.to_pos2(), egui::Vec2::splat(pixels / ppp))
}

#[derive(Clone, Default)]
struct Icons(HashMap<[u8; ICON_BYTES], TextureHandle>);

pub fn texture(ctx: &Context, icon: &[u8; ICON_BYTES]) -> Option<TextureHandle> {
    let id = egui::Id::new("ds_banner_icons");
    if let Some(texture) = ctx.data(|data| {
        data.get_temp::<Icons>(id)
            .and_then(|icons| icons.0.get(icon).cloned())
    }) {
        return Some(texture);
    }
    let rgba = icon_rgba(icon)?;
    let image = egui::ColorImage::from_rgba_unmultiplied([ICON_SIZE, ICON_SIZE], &rgba);
    let texture = ctx.load_texture("ds_banner_icon", image, TextureOptions::NEAREST);
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Icons>(id)
            .0
            .insert(*icon, texture.clone());
    });
    Some(texture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_keeps_its_bytes_as_text() {
        let mut icon = [0; ICON_BYTES];
        icon[0] = 0xAB;
        icon[ICON_BYTES - 1] = 0x01;
        let text = icon_text(&icon);
        assert_eq!(icon_of_text(&text), Some(icon));
        assert_eq!(icon_of_text("zz"), None);
    }

    #[test]
    fn an_icon_covers_whole_device_pixels() {
        let ctx = Context::default();
        ctx.set_pixels_per_point(2.03);
        let mut rect = egui::Rect::NOTHING;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            rect = icon_rect(ctx, egui::pos2(100.3, 50.7), 32.0);
        });
        let pixels = rect.width() * 2.03;
        assert!((pixels - 64.0).abs() < 0.01, "{pixels}");
        assert!((rect.min.x * 2.03 - (rect.min.x * 2.03).round()).abs() < 0.01);
    }
}
