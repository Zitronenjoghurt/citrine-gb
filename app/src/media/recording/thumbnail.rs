use super::{FRAME_TICKS, VIDEO_TIMESCALE};

const KEEP: usize = 64;

pub struct Thumbnails {
    kept: Vec<(u64, Vec<u8>)>,
    step: u64,
    last: Vec<u8>,
}

impl Default for Thumbnails {
    fn default() -> Self {
        Self {
            kept: Vec::new(),
            step: 1,
            last: Vec::new(),
        }
    }
}

fn second_of(frame: u64) -> u64 {
    frame * FRAME_TICKS as u64 / VIDEO_TIMESCALE as u64
}

fn starts_second(frame: u64) -> bool {
    frame == 0 || second_of(frame) != second_of(frame - 1)
}

impl Thumbnails {
    pub fn push(&mut self, frame: u64, rgba: &[u8]) {
        self.last.clear();
        self.last.extend_from_slice(rgba);
        if !starts_second(frame) || !second_of(frame).is_multiple_of(self.step) {
            return;
        }
        self.kept.push((frame, rgba.to_vec()));
        if self.kept.len() > KEEP {
            self.step *= 2;
            let step = self.step;
            self.kept
                .retain(|(f, _)| second_of(*f).is_multiple_of(step));
        }
    }

    pub fn pick(self, total: u64) -> Vec<u8> {
        if second_of(total) < 2 {
            return self.last;
        }
        let middle = total / 2;
        self.kept
            .into_iter()
            .min_by_key(|(f, _)| f.abs_diff(middle))
            .map(|(_, rgba)| rgba)
            .unwrap_or(self.last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(frames: u64) -> Thumbnails {
        let mut thumbs = Thumbnails::default();
        for frame in 0..frames {
            thumbs.push(frame, &frame.to_le_bytes());
        }
        thumbs
    }

    fn frame_of(rgba: Vec<u8>) -> u64 {
        u64::from_le_bytes(rgba.try_into().unwrap())
    }

    #[test]
    fn a_short_recording_shows_its_last_frame() {
        assert_eq!(frame_of(record(90).pick(90)), 89);
    }

    #[test]
    fn a_longer_recording_shows_its_middle() {
        let frames = 60 * 60;
        let picked = frame_of(record(frames).pick(frames));
        assert!(picked.abs_diff(frames / 2) <= 60, "picked {picked}");
    }

    #[test]
    fn ten_minutes_keep_few_frames_and_still_find_the_middle() {
        let frames = 60 * 600;
        let thumbs = record(frames);
        assert!(thumbs.kept.len() <= KEEP);
        let picked = frame_of(thumbs.pick(frames));
        assert!(picked.abs_diff(frames / 2) <= 60 * 16, "picked {picked}");
    }
}
