use super::Emulator;
use crate::audio::AudioLink;
use crate::audio::pacer::AudioFeed;
use citrine_gb::console::Console;

impl Emulator {
    pub(super) fn capture_frame(&mut self, heard: usize) -> usize {
        self.machine.take_audio(&mut self.audio);
        let samples = &self.audio;
        if self.screen_recorder.is_recording() {
            let fresh = &samples[heard.min(samples.len())..];
            self.screen_recorder
                .capture(self.machine.frame(), fresh, self.machine.sample_rate());
        }
        samples.len()
    }

    pub fn attach_audio(&mut self, link: AudioLink) {
        self.machine.set_sample_rate(link.sample_rate);
        if let Some(second) = &mut self.link {
            second.machine.set_sample_rate(link.sample_rate);
        }
        self.audio_feed = Some(link.feed);
    }

    pub fn set_channel_enabled(&mut self, index: usize, enabled: bool) {
        self.machine.set_channel_enabled(index, enabled);
    }

    pub fn channel_names(&self) -> &'static [&'static str] {
        self.machine.audio_channels()
    }

    pub fn channel_count(&self) -> usize {
        self.machine.audio_channels().len()
    }

    pub fn sample_rate(&self) -> u32 {
        self.machine.sample_rate()
    }

    pub(super) fn drop_audio(&mut self) {
        self.machine.take_audio(&mut self.audio);
        self.audio.clear();
    }

    pub(super) fn drain_audio(&mut self) {
        if self.fast_forward {
            self.drop_audio();
            if let Some(link) = &mut self.link {
                link.clear_audio();
            }
            if let Some(feed) = &self.audio_feed {
                feed.set_paused(true);
            }
            return;
        }
        self.machine.take_audio(&mut self.audio);
        let mut samples = std::mem::take(&mut self.audio);
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
        self.audio = samples;
    }

    pub fn audio_feed(&self) -> Option<&AudioFeed> {
        self.audio_feed.as_ref()
    }
}
