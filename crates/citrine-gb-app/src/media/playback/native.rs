use super::OpenError;
use super::demux::{self, Codec, Track};
use crate::media::Frame;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use openh264::decoder::{Decoder, DecoderConfig};
use openh264::formats::YUVSource;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapProd, HeapRb};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use symphonia_codec_aac::AacDecoder;
use symphonia_core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia_core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia_core::packet::Packet;
use symphonia_core::units::{Duration, Timestamp};
use web_time::Instant;

const AHEAD_SECS: f64 = 0.25;

pub struct Player {
    bytes: Vec<u8>,
    length: f64,
    video: Option<Video>,
    sound: Option<Sound>,
    output: Option<Output>,
    from: f64,
    started: Option<Instant>,
    frame: Option<Frame>,
    fresh: bool,
    muted: bool,
    volume: f32,
    silent: bool,
}

impl Player {
    pub fn open(bytes: Vec<u8>) -> Result<Self, OpenError> {
        let movie = demux::read(&bytes).ok_or(OpenError::Unreadable)?;
        let length = movie.length();
        let video = match movie.video {
            Some(track) => Some(Video::new(track)?),
            None => None,
        };
        let had_sound = movie.audio.is_some();
        let sound = movie.audio.and_then(Sound::new);
        let mut player = Self {
            bytes,
            length,
            video,
            silent: had_sound && sound.is_none(),
            sound,
            output: None,
            from: 0.0,
            started: None,
            frame: None,
            fresh: false,
            muted: false,
            volume: 1.0,
        };
        player.seek(0.0);
        Ok(player)
    }

    pub fn update(&mut self) {
        if self.started.is_none() {
            return;
        }
        let now = self.position();
        if now >= self.length {
            self.from = self.length;
            self.stop_clock();
        }
        if let Some(video) = &mut self.video
            && let Some(frame) = video.decode_until(&self.bytes, now)
        {
            self.frame = Some(frame);
            self.fresh = true;
        }
        self.feed(now);
    }

    pub fn take_frame(&mut self) -> Option<&Frame> {
        if !std::mem::take(&mut self.fresh) {
            return None;
        }
        self.frame.as_ref()
    }

    pub fn is_playing(&self) -> bool {
        self.started.is_some()
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub fn failed(&self) -> bool {
        false
    }

    pub fn play(&mut self) {
        if self.is_playing() {
            return;
        }
        if self.position() >= self.length - 0.05 {
            self.seek(0.0);
        }
        if self.output.is_none() && self.sound.is_some() {
            self.output = Output::open();
            self.apply_gain();
        }
        if let Some(sound) = &mut self.sound {
            sound.seek(self.from);
        }
        if let Some(output) = &mut self.output {
            output.flush();
            output.resume();
        }
        self.started = Some(Instant::now());
    }

    pub fn pause(&mut self) {
        self.from = self.position();
        self.stop_clock();
    }

    fn stop_clock(&mut self) {
        self.started = None;
        if let Some(output) = &mut self.output {
            output.flush();
            output.halt();
        }
    }

    pub fn toggle(&mut self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn position(&self) -> f64 {
        let moved = self.started.map_or(0.0, |at| at.elapsed().as_secs_f64());
        (self.from + moved).min(self.length)
    }

    pub fn length(&self) -> f64 {
        self.length
    }

    pub fn seek(&mut self, seconds: f64) {
        self.from = seconds.clamp(0.0, self.length);
        if self.started.is_some() {
            self.started = Some(Instant::now());
        }
        if let Some(video) = &mut self.video
            && let Some(frame) = video.seek(&self.bytes, self.from)
        {
            self.frame = Some(frame);
            self.fresh = true;
        }
        if let Some(sound) = &mut self.sound {
            sound.seek(self.from);
        }
        if let Some(output) = &mut self.output {
            output.flush();
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        self.apply_gain();
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.apply_gain();
    }

    pub fn is_silent_here(&self) -> bool {
        self.silent
    }

    fn apply_gain(&self) {
        if let Some(output) = &self.output {
            let gain = if self.muted {
                0.0
            } else {
                self.volume * self.volume
            };
            output.gain.store(gain.to_bits(), Ordering::Relaxed);
        }
    }

    fn feed(&mut self, now: f64) {
        let (Some(sound), Some(output)) = (&mut self.sound, &mut self.output) else {
            return;
        };
        while sound.fed < now + AHEAD_SECS && output.producer.vacant_len() >= 8192 {
            if !sound.decode_next(&self.bytes) {
                break;
            }
            output.push(&mut sound.pending, &mut sound.phase, sound.rate);
        }
    }
}

struct Video {
    track: Track,
    decoder: Decoder,
    sets: Vec<u8>,
    next: usize,
    unit: Vec<u8>,
    rgba: Vec<u8>,
}

impl Video {
    fn new(track: Track) -> Result<Self, OpenError> {
        let Codec::Avc { config } = &track.codec else {
            return Err(OpenError::Unreadable);
        };
        let sets = demux::parameter_sets(config);
        let api = crate::media::recording::video_api().map_err(|_| OpenError::NeedsCodec)?;
        let decoder = Decoder::with_api_config(api, DecoderConfig::new())
            .map_err(|_| OpenError::Unreadable)?;
        Ok(Self {
            track,
            decoder,
            sets,
            next: 0,
            unit: Vec::new(),
            rgba: Vec::new(),
        })
    }

    fn seek(&mut self, bytes: &[u8], seconds: f64) -> Option<Frame> {
        if self.track.samples.is_empty() {
            return None;
        }
        let index = self.track.index_at(seconds);
        self.next = self.track.sync_before(index);
        self.decode_until(bytes, seconds)
    }

    fn decode_until(&mut self, bytes: &[u8], seconds: f64) -> Option<Frame> {
        let due = |track: &Track, index: usize| {
            track
                .samples
                .get(index)
                .is_some_and(|s| track.seconds(s.time) <= seconds + 1e-6)
        };
        let mut shown = None;
        while due(&self.track, self.next) {
            let index = self.next;
            let last = !due(&self.track, index + 1);
            self.unit.clear();
            if self.track.samples[index].sync {
                self.unit.extend_from_slice(&self.sets);
            }
            if let Some(data) = self.track.data(bytes, index) {
                demux::annex_b(data, &mut self.unit);
            }
            self.next += 1;
            let Ok(Some(yuv)) = self.decoder.decode(&self.unit) else {
                continue;
            };
            if last {
                let (width, height) = yuv.dimensions();
                self.rgba.resize(width * height * 4, 0);
                yuv.write_rgba8(&mut self.rgba);
                shown = Some(super::shrink(&self.rgba, width, height));
            }
        }
        shown
    }
}

struct Sound {
    track: Track,
    decoder: AacDecoder,
    next: usize,
    drop: usize,
    rate: u32,
    fed: f64,
    decoded: Vec<f32>,
    pending: Vec<f32>,
    phase: f64,
}

impl Sound {
    fn new(track: Track) -> Option<Self> {
        let Codec::Aac { config } = &track.codec else {
            return None;
        };
        let mut params = AudioCodecParameters::new();
        params
            .for_codec(CODEC_ID_AAC)
            .with_sample_rate(track.sample_rate)
            .with_extra_data(config.clone().into_boxed_slice());
        let options = AudioDecoderOptions::default().gapless(false);
        let decoder = AacDecoder::try_new(&params, &options).ok()?;
        Some(Self {
            rate: track.sample_rate.max(1),
            track,
            decoder,
            next: 0,
            drop: 0,
            fed: 0.0,
            decoded: Vec::new(),
            pending: Vec::new(),
            phase: 0.0,
        })
    }

    fn seek(&mut self, seconds: f64) {
        self.decoder.reset();
        self.pending.clear();
        self.phase = 0.0;
        self.fed = seconds;
        if self.track.samples.is_empty() {
            return;
        }
        let index = self.track.index_at(seconds).saturating_sub(1);
        let wanted = self.track.ticks(seconds) + self.track.skip;
        self.next = index;
        self.drop = wanted.saturating_sub(self.track.samples[index].time) as usize;
    }

    fn decode_next(&mut self, bytes: &[u8]) -> bool {
        let Some(data) = self.track.data(bytes, self.next) else {
            return false;
        };
        let sample = self.track.samples[self.next];
        self.next += 1;
        let packet = Packet::new(
            0,
            Timestamp::new(sample.time as i64),
            Duration::new(1024),
            data.to_vec(),
        );
        let channels = match self.decoder.decode(&packet) {
            Ok(buffer) => {
                let channels = buffer.spec().channels().count().max(1);
                self.decoded.clear();
                buffer.copy_to_vec_interleaved(&mut self.decoded);
                channels
            }
            Err(_) => return true,
        };
        let frames = self.decoded.len() / channels;
        let skip = self.drop.min(frames);
        self.drop -= skip;
        for frame in self.decoded.chunks_exact(channels).skip(skip) {
            let left = frame[0];
            let right = *frame.get(1).unwrap_or(&left);
            self.pending.extend_from_slice(&[left, right]);
        }
        self.fed += (frames - skip) as f64 / self.rate as f64;
        true
    }
}

struct Output {
    stream: cpal::Stream,
    producer: HeapProd<f32>,
    rate: u32,
    channels: usize,
    gain: Arc<AtomicU32>,
    flush: Arc<AtomicBool>,
}

impl Output {
    fn open() -> Option<Self> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let channels = supported.channels() as usize;
        let config = supported.config();
        let rate = config.sample_rate;
        let (producer, mut consumer) = HeapRb::<f32>::new(rate as usize * channels).split();
        let gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let flush = Arc::new(AtomicBool::new(false));
        let (stream_gain, stream_flush) = (gain.clone(), flush.clone());
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    if stream_flush.swap(false, Ordering::Relaxed) {
                        consumer.clear();
                    }
                    let gain = f32::from_bits(stream_gain.load(Ordering::Relaxed));
                    let read = consumer.pop_slice(data);
                    for sample in &mut data[..read] {
                        *sample *= gain;
                    }
                    data[read..].fill(0.0);
                },
                |err| log::error!("Playback stream error: {err}"),
                None,
            )
            .ok()?;
        Some(Self {
            stream,
            producer,
            rate,
            channels,
            gain,
            flush,
        })
    }

    fn flush(&mut self) {
        self.flush.store(true, Ordering::Relaxed);
    }

    fn resume(&self) {
        let _ = self.stream.play();
    }

    fn halt(&self) {
        let _ = self.stream.pause();
    }

    fn push(&mut self, pending: &mut Vec<f32>, phase: &mut f64, rate: u32) {
        let step = rate as f64 / self.rate.max(1) as f64;
        let frames = pending.len() / 2;
        let mut out = Vec::with_capacity(((frames as f64 / step) as usize + 1) * self.channels);
        while *phase + 1.0 < frames as f64 {
            let at = *phase as usize;
            let t = (*phase - at as f64) as f32;
            let mix = |c: usize| pending[at * 2 + c] * (1.0 - t) + pending[(at + 1) * 2 + c] * t;
            let (left, right) = (mix(0), mix(1));
            match self.channels {
                1 => out.push((left + right) / 2.0),
                n => {
                    out.push(left);
                    out.push(right);
                    out.extend(std::iter::repeat_n(0.0, n - 2));
                }
            }
            *phase += step;
        }
        let used = (*phase as usize).min(frames);
        pending.drain(..used * 2);
        *phase -= used as f64;
        self.producer.push_slice(&out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::recording::headless;

    #[test]
    fn a_recording_plays_back_its_frames_and_sound() {
        let recorded = headless::record(
            include_bytes!("../../../homebrew/tobu.gb"),
            "playback.mp4",
            180,
        );
        let bytes = std::fs::read(&recorded.done.output.path).unwrap();
        let mut player = Player::open(bytes).unwrap();
        assert!((player.length() - 3.0).abs() < 0.1, "{}", player.length());
        assert!(player.sound.is_some());
        assert!(!player.is_silent_here());

        let first = player.take_frame().unwrap().clone();
        assert_eq!((first.width, first.height), (160, 144));
        assert!(close(&first.rgba, &recorded.frames[0]));

        player.seek(2.0);
        let at = player.take_frame().unwrap().clone();
        let index = (2.0 * crate::media::recording::fps()) as usize;
        let near = (index - 1..=index + 1).any(|i| close(&at.rgba, &recorded.frames[i]));
        assert!(near, "the frame at two seconds matches the game");
        assert!(player.take_frame().is_none(), "a frame is handed out once");

        let sound = player.sound.as_mut().unwrap();
        sound.seek(1.0);
        while sound.fed < 1.5 && sound.decode_next(&player.bytes) {}
        let loud = sound.pending.iter().any(|s| s.abs() > 0.01);
        assert!(loud, "the sound decodes to more than silence");
        assert!(sound.pending.len() / 2 >= (0.45 * sound.rate as f64) as usize);
    }

    #[test]
    fn bytes_that_are_no_recording_are_refused() {
        assert_eq!(
            Player::open(b"hello".to_vec()).err(),
            Some(OpenError::Unreadable)
        );
    }

    fn close(a: &[u8], b: &[u8]) -> bool {
        let off = a
            .iter()
            .zip(b)
            .filter(|(x, y)| x.abs_diff(**y) > 24)
            .count();
        off * 100 < a.len()
    }
}
