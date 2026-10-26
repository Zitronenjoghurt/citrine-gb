use super::mp4::{AudioCodec, AudioFormat};
use super::{AUDIO_BITRATE, CHANNELS};
use rusty_aac::{AacEncoder as Segment, AacEncoderConfig};

const BLOCK: usize = 1024;
const SEGMENT_BLOCKS: usize = 94;
const OVERLAP: usize = 2;
const RATES: [u32; 12] = [
    96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
];

pub struct Packet {
    pub data: Vec<u8>,
    pub duration: u32,
}

pub struct AacEncoder {
    sample_rate: u32,
    rate_index: u8,
    pcm: Vec<f32>,
    from: usize,
    next: usize,
    frames: u64,
}

impl AacEncoder {
    pub fn new(sample_rate: u32) -> anyhow::Result<Self> {
        let rate_index = RATES
            .iter()
            .position(|&r| r == sample_rate)
            .ok_or_else(|| anyhow::anyhow!("AAC can't record sound at {sample_rate} Hz"))?
            as u8;
        Ok(Self {
            sample_rate,
            rate_index,
            pcm: Vec::with_capacity((SEGMENT_BLOCKS + 2 * OVERLAP) * BLOCK * CHANNELS),
            from: 0,
            next: 0,
            frames: 0,
        })
    }

    pub fn format(&self) -> AudioFormat {
        let config = vec![
            (2 << 3) | (self.rate_index >> 1),
            ((self.rate_index & 1) << 7) | ((CHANNELS as u8) << 3),
        ];
        AudioFormat {
            codec: AudioCodec::Aac { config },
            sample_rate: self.sample_rate,
            channels: CHANNELS as u16,
            priming: BLOCK as u32,
            frames: self.frames,
        }
    }

    pub fn push(&mut self, samples: &[f32]) -> anyhow::Result<Vec<Packet>> {
        self.frames += (samples.len() / CHANNELS) as u64;
        self.pcm.extend_from_slice(samples);
        let mut packets = Vec::new();
        let block = BLOCK * CHANNELS;
        while self.from + self.pcm.len() / block >= self.next + SEGMENT_BLOCKS {
            let to = self.next + SEGMENT_BLOCKS;
            let encoded = self.encode((to - self.from) * block)?;
            let keep = to - OVERLAP;
            packets.extend(self.take(encoded, keep));
            let from = keep - OVERLAP;
            self.pcm.drain(..(from - self.from) * block);
            self.from = from;
            self.next = keep;
        }
        Ok(packets)
    }

    pub fn finish(&mut self) -> anyhow::Result<Vec<Packet>> {
        if self.frames == 0 {
            return Ok(Vec::new());
        }
        let encoded = self.encode(self.pcm.len())?;
        let end = self.from + encoded.len();
        let packets = self.take(encoded, end);
        self.pcm.clear();
        Ok(packets)
    }

    fn encode(&self, len: usize) -> anyhow::Result<Vec<Vec<u8>>> {
        let mut segment = Segment::new(AacEncoderConfig {
            bitrate_bps: AUDIO_BITRATE,
            ..AacEncoderConfig::default()
        });
        segment
            .push_pcm(&self.pcm[..len], CHANNELS as u16, self.sample_rate)
            .map_err(|e| anyhow::anyhow!("AAC encoder: {e:?}"))?;
        segment.finish();
        let mut packets = Vec::new();
        while let Ok(packet) = segment.next_packet() {
            packets.push(packet.data);
        }
        Ok(packets)
    }

    fn take(&self, encoded: Vec<Vec<u8>>, end: usize) -> Vec<Packet> {
        encoded
            .into_iter()
            .enumerate()
            .map(|(local, data)| (self.from + local, data))
            .filter(|(global, _)| (self.next..end).contains(global))
            .map(|(_, data)| Packet {
                data,
                duration: BLOCK as u32,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let s = (n as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.3;
                [s, -s]
            })
            .collect()
    }

    #[test]
    fn every_block_gets_one_packet_whatever_the_chunk_size() {
        let pcm = tone(48_000 * 5 + 333);
        let mut encoder = AacEncoder::new(48_000).unwrap();
        let mut packets = 0;
        for chunk in pcm.chunks(1607 * 2) {
            packets += encoder.push(chunk).unwrap().len();
        }
        packets += encoder.finish().unwrap().len();
        let blocks = (48_000 * 5 + 333usize).div_ceil(BLOCK);
        assert_eq!(packets, blocks + 1);
        assert_eq!(encoder.format().frames, 48_000 * 5 + 333);
    }

    #[test]
    fn memory_holds_one_segment_at_most() {
        let mut encoder = AacEncoder::new(48_000).unwrap();
        for chunk in tone(48_000 * 20).chunks(1600 * 2) {
            encoder.push(chunk).unwrap();
            assert!(encoder.pcm.len() <= (SEGMENT_BLOCKS + 2 * OVERLAP + 2) * BLOCK * CHANNELS);
        }
    }

    #[test]
    fn an_unusual_rate_is_refused() {
        assert!(AacEncoder::new(47_000).is_err());
    }
}
