#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(not(feature = "audio-codec"), path = "playback/no_aac.rs")]
mod aac;
#[cfg(not(target_arch = "wasm32"))]
mod demux;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use native::Player;
#[cfg(target_arch = "wasm32")]
pub use web::Player;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    NeedsCodec,
    Unreadable,
}

pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn native_size(width: usize, height: usize) -> (usize, usize) {
    use citrine_gb::console::ConsoleKind;
    ConsoleKind::ALL
        .into_iter()
        .map(ConsoleKind::video)
        .find(|video| {
            let scale = width / video.width;
            scale > 0 && width == video.width * scale && height == video.height * scale
        })
        .map_or((width, height), |video| (video.width, video.height))
}

#[cfg(not(target_arch = "wasm32"))]
const BLOCK_STEP: usize = 3;
#[cfg(not(target_arch = "wasm32"))]
const BLOCK_SPREAD: u8 = 48;

#[cfg(not(target_arch = "wasm32"))]
pub fn shown(rgba: &[u8], width: usize, height: usize, sharp: &mut bool) -> crate::media::Frame {
    if !*sharp && blown_up(rgba, width, height) {
        return shrink(rgba, width, height);
    }
    *sharp = true;
    crate::media::Frame {
        width: width as u32,
        height: height as u32,
        rgba: rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, _]| [r, g, b, 255])
            .collect(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn blown_up(rgba: &[u8], width: usize, height: usize) -> bool {
    let (w, h) = native_size(width, height);
    let scale = width / w;
    if scale < 2 {
        return true;
    }
    let pixel = |x: usize, y: usize| {
        let at = (y * width + x) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2]]
    };
    let mut blocks = 0;
    let mut mixed = 0;
    for by in (0..h).step_by(BLOCK_STEP) {
        for bx in (0..w).step_by(BLOCK_STEP) {
            let (left, top) = (bx * scale, by * scale);
            let centre = pixel(left + scale / 2, top + scale / 2);
            let spread = (top..top + scale)
                .flat_map(|y| (left..left + scale).map(move |x| (x, y)))
                .flat_map(|(x, y)| pixel(x, y).into_iter().zip(centre))
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap_or(0);
            blocks += 1;
            mixed += usize::from(spread > BLOCK_SPREAD);
        }
    }
    mixed * 100 <= blocks
}

#[cfg(not(target_arch = "wasm32"))]
pub fn shrink(rgba: &[u8], width: usize, height: usize) -> crate::media::Frame {
    let (w, h) = native_size(width, height);
    let scale = (width / w).max(1);
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = (y * scale + scale / 2).min(height - 1) * width;
        for x in 0..w {
            let at = (row + (x * scale + scale / 2).min(width - 1)) * 4;
            out.extend_from_slice(&rgba[at..at + 3]);
            out.push(255);
        }
    }
    crate::media::Frame {
        width: w as u32,
        height: h as u32,
        rgba: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(65.9), "1:05");
        assert_eq!(clock(-3.0), "0:00");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_scaled_frame_shrinks_back_to_the_game_boy_size() {
        let (w, h) = (640, 576);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let at = (y * w + x) * 4;
                rgba[at] = (x / 4) as u8;
                rgba[at + 1] = (y / 4) as u8;
            }
        }
        let frame = shrink(&rgba, w, h);
        assert_eq!((frame.width, frame.height), (160, 144));
        let at = (10 * 160 + 37) * 4;
        assert_eq!(&frame.rgba[at..at + 4], &[37, 10, 0, 255]);
        assert_eq!(native_size(160, 144), (160, 144));
        assert_eq!(native_size(500, 400), (500, 400));
        assert_eq!(native_size(720, 480), (240, 160));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn fixture(bytes: &[u8]) -> crate::media::Frame {
        crate::media::decode_png(bytes).expect("a png fixture")
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_recorded_4x_game_boy_frame_shrinks_to_the_game_boy_size() {
        let gb = fixture(include_bytes!("../../fixtures/media/gb-4x.png"));
        let mut sharp = false;
        let frame = shown(&gb.rgba, gb.width as usize, gb.height as usize, &mut sharp);
        assert_eq!((frame.width, frame.height), (160, 144));
        assert!(!sharp);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_recorded_4x_ds_frame_with_upscaled_3d_plays_at_full_size() {
        let kart = fixture(include_bytes!("../../fixtures/media/ds-4x-kart.png"));
        let (width, height) = (1024, 1536);
        let mut native = vec![0u8; 256 * 384 * 4];
        for (index, pixel) in native.chunks_mut(4).enumerate() {
            let (x, y) = (index % 256, index / 256);
            pixel.copy_from_slice(&[(x * 7) as u8, (y * 5) as u8, ((x ^ y) * 3) as u8, 255]);
        }
        let mut rgba = vec![0u8; width * height * 4];
        crate::media::recording::upscale_rgba(&native, 256, 384, 4, &mut rgba);
        let mut sharp = false;
        let flat = shown(&rgba, width, height, &mut sharp);
        assert_eq!((flat.width, flat.height), (256, 384));
        let side = kart.width as usize;
        for y in 0..height / 2 {
            for x in 0..width {
                let from = ((y % side) * side + x % side) * 4;
                let to = (y * width + x) * 4;
                rgba[to..to + 4].copy_from_slice(&kart.rgba[from..from + 4]);
            }
        }
        let frame = shown(&rgba, width, height, &mut sharp);
        assert_eq!((frame.width, frame.height), (1024, 1536));
        assert!(sharp);
        let at = (100 * width + 100) * 4;
        assert_eq!(&frame.rgba[at..at + 3], &rgba[at..at + 3]);
        let after = shown(&rgba[..], width, height, &mut sharp);
        assert_eq!(
            after.width, 1024,
            "a recording stays at full size once it showed 3D"
        );
    }
}
