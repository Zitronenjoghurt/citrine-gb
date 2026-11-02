use super::mp4::AudioFormat;

#[allow(dead_code)]
pub struct Packet {
    pub data: Vec<u8>,
    pub duration: u32,
}

pub struct AacEncoder;

impl AacEncoder {
    pub fn new(_sample_rate: u32) -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub fn format(&self) -> Option<AudioFormat> {
        None
    }

    pub fn push(&mut self, _samples: &[f32]) -> anyhow::Result<Vec<Packet>> {
        Ok(Vec::new())
    }

    pub fn finish(&mut self) -> anyhow::Result<Vec<Packet>> {
        Ok(Vec::new())
    }
}
