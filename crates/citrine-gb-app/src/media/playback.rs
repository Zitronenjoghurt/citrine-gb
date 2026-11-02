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
    use crate::media::recording::{SOURCE_HEIGHT, SOURCE_WIDTH};
    let scale = width / SOURCE_WIDTH;
    if scale > 0 && width == SOURCE_WIDTH * scale && height == SOURCE_HEIGHT * scale {
        (SOURCE_WIDTH, SOURCE_HEIGHT)
    } else {
        (width, height)
    }
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
    }
}
