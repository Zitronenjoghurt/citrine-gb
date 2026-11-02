pub enum AacDecoder {}

impl AacDecoder {
    pub fn new(_config: &[u8], _sample_rate: u32) -> Option<Self> {
        None
    }

    pub fn reset(&mut self) {
        match *self {}
    }

    pub fn decode(&mut self, _time: u64, _data: &[u8], _out: &mut Vec<f32>) -> Option<usize> {
        match *self {}
    }
}
