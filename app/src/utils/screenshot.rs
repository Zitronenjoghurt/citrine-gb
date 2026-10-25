pub fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || rgba.len() != width * height * 4 {
        return None;
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

pub fn file_name(title: &str) -> String {
    let title = title.trim();
    let stem = if title.is_empty() { "citrine" } else { title };
    let stem: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let stamp = web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}-{stamp}.png", stem.trim_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_is_produced_for_matching_dimensions() {
        let png = encode_png(2, 2, &[0u8; 16]).expect("encodes");
        assert_eq!(&png[1..4], b"PNG", "has a PNG signature");
    }

    #[test]
    fn a_mismatched_buffer_is_refused() {
        assert!(encode_png(4, 4, &[0u8; 16]).is_none());
        assert!(encode_png(0, 0, &[]).is_none());
    }

    #[test]
    fn file_names_are_safe_and_stay_recognisable() {
        let name = file_name("POKEMON RED/BLUE");
        assert!(name.starts_with("POKEMON-RED-BLUE-"), "got {name}");
        assert!(name.ends_with(".png"));
        assert!(!name.contains('/'));
    }

    #[test]
    fn an_untitled_rom_still_gets_a_name() {
        let name = file_name("   ");
        assert!(name.starts_with("citrine-"), "got {name}");
    }
}
