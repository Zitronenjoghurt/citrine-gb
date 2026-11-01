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
}
