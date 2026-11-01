use crate::media::{Frame, decode_png, thumbnail_of};
use std::collections::HashMap;

const PNG_MAGIC: &[u8] = b"\x89PNG";
const WEBP_MAGIC: &[u8] = b"RIFF";
const MAX_PIXELS: u64 = 4096 * 4096;

pub struct Packed {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub preview: Vec<u8>,
}

pub fn pack_screenshot(png: &[u8]) -> Option<Packed> {
    let frame = decode_png(png)?;
    let (width, height) = (frame.width, frame.height);
    let native = shrink(&frame);
    let data = encode_lossless(&native)?;
    let preview = match thumbnail_of(&native) {
        Some(thumb) if thumb.width == native.width => data.clone(),
        Some(thumb) => encode_lossless(&thumb)?,
        None => return None,
    };
    Some(Packed {
        data,
        width,
        height,
        preview,
    })
}

pub fn preview_of(frame: &Frame) -> Vec<u8> {
    thumbnail_of(frame)
        .and_then(|thumb| encode_lossless(&shrink(&thumb)))
        .unwrap_or_default()
}

pub fn unpack_screenshot(data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let native = decode_image(data)?;
    let full = grow(&native, width, height);
    crate::utils::screenshot::encode_png(full.width as usize, full.height as usize, &full.rgba)
}

pub fn encode_lossless(frame: &Frame) -> Option<Vec<u8>> {
    let png = encode_png_small(frame);
    let webp = encode_webp(frame);
    match (png, webp) {
        (Some(png), Some(webp)) if webp.len() < png.len() => Some(webp),
        (Some(png), _) => Some(png),
        (None, webp) => webp,
    }
}

pub fn decode_image(bytes: &[u8]) -> Option<Frame> {
    if bytes.starts_with(PNG_MAGIC) {
        return decode_png(bytes);
    }
    if !bytes.starts_with(WEBP_MAGIC) {
        return None;
    }
    let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    let mut buf = vec![0; decoder.output_buffer_size()?];
    decoder.read_image(&mut buf).ok()?;
    let rgba = if decoder.has_alpha() {
        buf
    } else {
        buf.as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect()
    };
    let frame = Frame {
        width,
        height,
        rgba,
    };
    (frame.rgba.len() == width as usize * height as usize * 4).then_some(frame)
}

fn encode_webp(frame: &Frame) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let opaque = frame.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let rgb: Vec<u8>;
    let (data, color) = if opaque {
        rgb = frame
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [r, g, b])
            .collect();
        (rgb.as_slice(), image_webp::ColorType::Rgb8)
    } else {
        (frame.rgba.as_slice(), image_webp::ColorType::Rgba8)
    };
    image_webp::WebPEncoder::new(&mut out)
        .encode(data, frame.width, frame.height, color)
        .ok()?;
    Some(out)
}

fn palette_of(frame: &Frame) -> Option<(Vec<[u8; 4]>, Vec<u8>)> {
    let mut colors: Vec<[u8; 4]> = Vec::new();
    let mut index: HashMap<[u8; 4], u8> = HashMap::new();
    let mut pixels = Vec::with_capacity(frame.rgba.len() / 4);
    for &color in frame.rgba.as_chunks::<4>().0 {
        let i = match index.get(&color) {
            Some(&i) => i,
            None => {
                if colors.len() == 256 {
                    return None;
                }
                let i = colors.len() as u8;
                colors.push(color);
                index.insert(color, i);
                i
            }
        };
        pixels.push(i);
    }
    Some((colors, pixels))
}

fn pack_bits(pixels: &[u8], width: usize, bits: u8) -> Vec<u8> {
    let per_byte = 8 / bits as usize;
    let row_bytes = width.div_ceil(per_byte);
    let mut out = Vec::with_capacity(row_bytes * pixels.len() / width.max(1));
    for row in pixels.chunks(width) {
        let mut packed = vec![0u8; row_bytes];
        for (x, &p) in row.iter().enumerate() {
            let shift = 8 - bits as usize * (x % per_byte + 1);
            packed[x / per_byte] |= p << shift;
        }
        out.extend_from_slice(&packed);
    }
    out
}

fn encode_png_small(frame: &Frame) -> Option<Vec<u8>> {
    let mut best: Option<Vec<u8>> = None;
    for filter in [png::Filter::NoFilter, png::Filter::Adaptive] {
        let attempt = match palette_of(frame) {
            Some((colors, pixels)) => encode_indexed(frame, &colors, &pixels, filter),
            None => encode_rgba(frame, filter),
        };
        if let Some(png) = attempt
            && best.as_ref().is_none_or(|b| png.len() < b.len())
        {
            best = Some(png);
        }
    }
    best
}

fn encode_indexed(
    frame: &Frame,
    colors: &[[u8; 4]],
    pixels: &[u8],
    filter: png::Filter,
) -> Option<Vec<u8>> {
    let bits: u8 = match colors.len() {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    };
    let depth = match bits {
        1 => png::BitDepth::One,
        2 => png::BitDepth::Two,
        4 => png::BitDepth::Four,
        _ => png::BitDepth::Eight,
    };
    let palette: Vec<u8> = colors.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
    let alpha: Vec<u8> = colors.iter().map(|c| c[3]).collect();
    let data = pack_bits(pixels, frame.width as usize, bits);
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(depth);
        encoder.set_palette(palette);
        if alpha.iter().any(|&a| a != 255) {
            let last = alpha.iter().rposition(|&a| a != 255).unwrap_or(0);
            encoder.set_trns(alpha[..=last].to_vec());
        }
        encoder.set_deflate_compression(png::DeflateCompression::Level(9));
        encoder.set_filter(filter);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&data).ok()?;
    }
    Some(out)
}

fn encode_rgba(frame: &Frame, filter: png::Filter) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_deflate_compression(png::DeflateCompression::Level(9));
        encoder.set_filter(filter);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&frame.rgba).ok()?;
    }
    Some(out)
}

fn pixel(frame: &Frame, x: u32, y: u32) -> &[u8] {
    let at = ((y * frame.width + x) * 4) as usize;
    &frame.rgba[at..at + 4]
}

fn scale_of(frame: &Frame) -> u32 {
    let mut scale = gcd(frame.width, frame.height);
    while scale > 1 {
        let uniform = (0..frame.height).all(|y| {
            (0..frame.width)
                .all(|x| pixel(frame, x, y) == pixel(frame, x - x % scale, y - y % scale))
        });
        if uniform {
            return scale;
        }
        scale = (2..scale)
            .rev()
            .find(|s| scale.is_multiple_of(*s))
            .unwrap_or(1);
    }
    1
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

pub fn shrink(frame: &Frame) -> Frame {
    let scale = scale_of(frame);
    if scale == 1 {
        return frame.clone();
    }
    let (width, height) = (frame.width / scale, frame.height / scale);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(pixel(frame, x * scale, y * scale));
        }
    }
    Frame {
        width,
        height,
        rgba,
    }
}

pub fn grow(frame: &Frame, width: u32, height: u32) -> Frame {
    if frame.width == 0
        || !width.is_multiple_of(frame.width)
        || !height.is_multiple_of(frame.height.max(1))
        || width / frame.width != height / frame.height
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return frame.clone();
    }
    let scale = width / frame.width;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(pixel(frame, x / scale, y / scale));
        }
    }
    Frame {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_frame(colors: &[[u8; 4]]) -> Frame {
        let (width, height) = (160u32, 144u32);
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let i = ((x / 8 + y / 8 + (x * y) % 3) as usize) % colors.len();
                rgba.extend_from_slice(&colors[i]);
            }
        }
        Frame {
            width,
            height,
            rgba,
        }
    }

    const GREENS: [[u8; 4]; 4] = [
        [0x0f, 0x38, 0x0f, 255],
        [0x30, 0x62, 0x30, 255],
        [0x8b, 0xac, 0x0f, 255],
        [0x9b, 0xbc, 0x0f, 255],
    ];

    #[test]
    fn a_four_colour_frame_round_trips_and_shrinks() {
        let frame = game_frame(&GREENS);
        let plain = crate::utils::screenshot::encode_png(160, 144, &frame.rgba).expect("plain png");
        let small = encode_lossless(&frame).unwrap();
        assert!(
            small.len() * 2 < plain.len(),
            "{} vs {}",
            small.len(),
            plain.len()
        );
        assert_eq!(decode_image(&small).unwrap(), frame);
    }

    #[test]
    fn both_formats_decode_to_the_same_pixels() {
        let mut colors = Vec::new();
        for i in 0..300u32 {
            colors.push([(i * 7) as u8, (i * 13) as u8, (i * 3) as u8, 255]);
        }
        let frame = game_frame(&colors);
        let webp = encode_webp(&frame).unwrap();
        let png = encode_png_small(&frame).unwrap();
        assert_eq!(decode_image(&webp).unwrap(), frame);
        assert_eq!(decode_image(&png).unwrap(), frame);
        let chosen = encode_lossless(&frame).unwrap();
        assert_eq!(chosen.len(), webp.len().min(png.len()));
    }

    #[test]
    fn transparency_survives() {
        let mut frame = game_frame(&GREENS);
        frame.rgba[3] = 0;
        let small = encode_lossless(&frame).unwrap();
        assert_eq!(decode_image(&small).unwrap(), frame);
    }

    #[test]
    fn an_upscaled_screenshot_is_stored_at_game_size_and_restored() {
        let native = game_frame(&GREENS);
        let big = grow(&native, 640, 576);
        assert_eq!((big.width, big.height), (640, 576));
        let png = crate::utils::screenshot::encode_png(640, 576, &big.rgba).unwrap();
        let packed = pack_screenshot(&png).unwrap();
        assert_eq!((packed.width, packed.height), (640, 576));
        assert_eq!(decode_image(&packed.data).unwrap(), native);
        assert_eq!(packed.preview, packed.data);
        let back = unpack_screenshot(&packed.data, 640, 576).unwrap();
        assert_eq!(decode_png(&back).unwrap(), big);
        assert!(
            packed.data.len() * 10 < png.len(),
            "{} vs {}",
            packed.data.len(),
            png.len()
        );
    }

    #[test]
    fn a_frame_that_is_not_an_upscale_keeps_its_size() {
        let frame = game_frame(&GREENS);
        assert_eq!(shrink(&frame), frame);
        let odd = grow(&frame, 300, 200);
        assert_eq!(odd, frame);
    }

    #[test]
    fn garbage_is_not_an_image() {
        assert!(decode_image(b"hello").is_none());
        assert!(decode_image(b"RIFF1234WEBP").is_none());
    }
}
