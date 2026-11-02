use symphonia_codec_aac::AacDecoder as Decoder;
use symphonia_core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia_core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia_core::packet::Packet;
use symphonia_core::units::{Duration, Timestamp};

pub struct AacDecoder(Decoder);

impl AacDecoder {
    pub fn new(config: &[u8], sample_rate: u32) -> Option<Self> {
        let mut params = AudioCodecParameters::new();
        params
            .for_codec(CODEC_ID_AAC)
            .with_sample_rate(sample_rate)
            .with_extra_data(config.to_vec().into_boxed_slice());
        let options = AudioDecoderOptions::default().gapless(false);
        Decoder::try_new(&params, &options).ok().map(Self)
    }

    pub fn reset(&mut self) {
        self.0.reset();
    }

    pub fn decode(&mut self, time: u64, data: &[u8], out: &mut Vec<f32>) -> Option<usize> {
        let packet = Packet::new(
            0,
            Timestamp::new(time as i64),
            Duration::new(1024),
            data.to_vec(),
        );
        let buffer = self.0.decode(&packet).ok()?;
        let channels = buffer.spec().channels().count().max(1);
        out.clear();
        buffer.copy_to_vec_interleaved(out);
        Some(channels)
    }
}
