use crate::proto::Frame;

pub const MAX_SCREEN_PIXELS: usize = 1024 * 1024;
pub const MAX_SCREENS: usize = 4;

#[derive(Debug, Default)]
pub struct Encoder {
    last: Vec<u8>,
    width: u16,
    height: u16,
}

impl Encoder {
    pub fn restart(&mut self) {
        self.last.clear();
    }

    pub fn encode(&mut self, screen: u8, seq: u32, rgba: &[u8], width: u16, height: u16) -> Frame {
        let rgb = packed(rgba);
        let key = self.last.len() != rgb.len() || self.width != width || self.height != height;
        let delta = if key {
            rgb.clone()
        } else {
            rgb.iter().zip(&self.last).map(|(a, b)| a ^ b).collect()
        };
        self.last = rgb;
        self.width = width;
        self.height = height;
        Frame {
            screen,
            seq,
            width,
            height,
            key,
            data: lz4_flex::compress_prepend_size(&delta),
        }
    }
}

#[derive(Debug, Default)]
pub struct Decoder {
    rgb: Vec<u8>,
    rgba: Vec<u8>,
}

impl Decoder {
    pub fn decode(&mut self, frame: &Frame) -> Option<&[u8]> {
        let pixels = usize::from(frame.width) * usize::from(frame.height);
        if pixels > MAX_SCREEN_PIXELS {
            return None;
        }
        let size = pixels * 3;
        let (claimed, packed) = frame.data.split_first_chunk::<4>()?;
        if u32::from_le_bytes(*claimed) as usize != size {
            return None;
        }
        let mut delta = vec![0; size];
        if lz4_flex::decompress_into(packed, &mut delta).ok()? != size {
            return None;
        }
        if frame.key {
            self.rgb = delta;
        } else if self.rgb.len() == size {
            self.rgb.iter_mut().zip(&delta).for_each(|(a, b)| *a ^= b);
        } else {
            return None;
        }
        self.rgba.clear();
        self.rgba.reserve(size / 3 * 4);
        for pixel in self.rgb.as_chunks::<3>().0 {
            self.rgba.extend_from_slice(pixel);
            self.rgba.push(255);
        }
        Some(&self.rgba)
    }
}

fn packed(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(seed: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(256 * 192 * 4);
        let mut state = seed.wrapping_mul(2_654_435_761).max(1);
        for _ in 0..256 * 192 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let [r, g, b, _] = state.to_le_bytes();
            out.extend_from_slice(&[r, g, b, 255]);
        }
        out
    }

    #[test]
    fn frames_round_trip_through_deltas() {
        let mut encoder = Encoder::default();
        let mut decoder = Decoder::default();
        for seed in 1..5 {
            let rgba = picture(seed);
            let frame = encoder.encode(1, seed, &rgba, 256, 192);
            assert_eq!(frame.key, seed == 1);
            assert_eq!(decoder.decode(&frame), Some(rgba.as_slice()));
        }
    }

    #[test]
    fn a_still_screen_costs_almost_nothing() {
        let mut encoder = Encoder::default();
        let rgba = picture(3);
        encoder.encode(0, 0, &rgba, 256, 192);
        let still = encoder.encode(0, 1, &rgba, 256, 192);
        assert!(!still.key);
        assert!(still.data.len() < 1024, "{} bytes", still.data.len());
    }

    #[test]
    fn a_restart_sends_a_key_frame() {
        let mut encoder = Encoder::default();
        let rgba = picture(3);
        encoder.encode(0, 0, &rgba, 256, 192);
        encoder.restart();
        let frame = encoder.encode(0, 1, &rgba, 256, 192);
        assert!(frame.key);
        let mut fresh = Decoder::default();
        assert_eq!(fresh.decode(&frame), Some(rgba.as_slice()));
    }

    #[test]
    fn a_frame_that_claims_more_than_its_screen_is_refused() {
        let mut lying = Encoder::default().encode(0, 0, &picture(3), 256, 192);
        lying.data[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Decoder::default().decode(&lying), None);
        let blank = vec![0; 1025 * 1024 * 4];
        let huge = Encoder::default().encode(0, 0, &blank, 1025, 1024);
        assert_eq!(Decoder::default().decode(&huge), None);
    }

    #[test]
    fn a_delta_without_its_key_frame_is_refused() {
        let mut encoder = Encoder::default();
        let rgba = picture(3);
        encoder.encode(0, 0, &rgba, 256, 192);
        let delta = encoder.encode(0, 1, &rgba, 256, 192);
        assert_eq!(Decoder::default().decode(&delta), None);
    }

    #[test]
    fn a_frame_claiming_a_bigger_picture_is_refused() {
        let mut encoder = Encoder::default();
        let mut frame = encoder.encode(0, 0, &picture(4), 256, 192);
        frame.data[..4].copy_from_slice(&0xffff_ff40_u32.to_le_bytes());
        assert_eq!(Decoder::default().decode(&frame), None);
    }

    #[test]
    fn an_oversized_screen_is_refused() {
        let frame = Frame {
            screen: 0,
            seq: 0,
            width: u16::MAX,
            height: u16::MAX,
            key: true,
            data: lz4_flex::compress_prepend_size(&[]),
        };
        assert_eq!(Decoder::default().decode(&frame), None);
    }

    #[test]
    fn a_size_change_is_a_key_frame() {
        let mut encoder = Encoder::default();
        let mut decoder = Decoder::default();
        let small = vec![9; 160 * 144 * 4];
        let frame = encoder.encode(0, 0, &picture(2), 256, 192);
        decoder.decode(&frame);
        let frame = encoder.encode(0, 1, &small, 160, 144);
        assert!(frame.key);
        assert_eq!(decoder.decode(&frame).map(<[u8]>::len), Some(160 * 144 * 4));
    }
}
