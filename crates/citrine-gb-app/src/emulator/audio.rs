use super::Emulator;
use crate::audio::AudioLink;
use crate::audio::pacer::AudioFeed;

impl Emulator {
    pub(super) fn capture_frame(&mut self, heard: usize) -> usize {
        let samples = &self.gb.apu.audio_buffer;
        if self.screen_recorder.is_recording() {
            let fresh = &samples[heard.min(samples.len())..];
            self.screen_recorder.capture(
                self.gb.frame().as_slice(),
                fresh,
                self.gb.apu.output_sample_rate,
            );
        }
        samples.len()
    }

    pub fn attach_audio(&mut self, link: AudioLink) {
        self.gb.apu.set_sample_rate(link.sample_rate);
        if let Some(second) = &mut self.link {
            second.gb.apu.set_sample_rate(link.sample_rate);
        }
        self.audio_feed = Some(link.feed);
    }

    pub(super) fn drain_audio(&mut self) {
        if self.fast_forward {
            self.gb.apu.audio_buffer.clear();
            if let Some(link) = &mut self.link {
                link.clear_audio();
            }
            if let Some(feed) = &self.audio_feed {
                feed.set_paused(true);
            }
            return;
        }
        let mut samples = std::mem::take(&mut self.gb.apu.audio_buffer);
        if let Some(link) = &mut self.link {
            link.mix(&mut samples);
        }
        self.mixer
            .mix(self.sound, &mut samples, &mut self.friend_audio);
        if let Some(feed) = &mut self.audio_feed {
            feed.set_paused(false);
            feed.push(&samples);
        }
        samples.clear();
        self.gb.apu.audio_buffer = samples;
    }

    pub fn audio_feed(&self) -> Option<&AudioFeed> {
        self.audio_feed.as_ref()
    }
}
