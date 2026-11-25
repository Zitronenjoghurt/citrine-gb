use super::Emulator;
use crate::audio::AudioLink;
use crate::audio::pacer::AudioFeed;
use crate::audio::preview::{Playing, PreviewClock};
use citrine_gb::console::Console;

impl Emulator {
    pub fn attach_audio(&mut self, link: AudioLink) {
        let core = self.seat.core_mut();
        core.machine.set_sample_rate(link.sample_rate);
        if let Some(second) = &mut core.link {
            second.machine.set_sample_rate(link.sample_rate);
        }
        core.audio_feed = Some(link.feed);
    }

    #[cfg(feature = "nds")]
    pub fn attach_mic(&mut self, line: &crate::audio::mic::MicLine) {
        if self.seat.facts().mic_line {
            return;
        }
        let core = self.seat.core_mut();
        if core.mic.is_none() {
            core.mic = Some(line.clone());
        }
    }

    #[cfg(feature = "nds")]
    pub fn mic_in_use(&self) -> bool {
        self.seat.facts().mic
    }

    pub fn set_channel_enabled(&mut self, index: usize, enabled: bool) {
        self.seat
            .core_mut()
            .machine
            .set_channel_enabled(index, enabled);
    }

    pub fn sample_rate(&self) -> u32 {
        self.seat.facts().sample_rate
    }

    pub(super) fn drain_audio(&mut self) {
        let (sound, fast_forward, rewinding) = (self.sound, self.fast_forward, self.rewinding);
        let pace = self.clock.pace(self.seat.facts().timing);
        self.seat
            .core_mut()
            .drain_audio(sound, pace, fast_forward, rewinding);
    }

    pub fn audio_feed(&self) -> Option<&AudioFeed> {
        self.seat.core().audio_feed.as_ref()
    }

    pub fn play_preview(&mut self, preview: Playing) {
        self.seat.core_mut().preview = Some(preview);
    }

    pub fn stop_preview(&mut self) {
        self.seat.core_mut().preview = None;
    }

    pub fn is_previewing(&self, clock: &PreviewClock) -> bool {
        self.seat
            .core()
            .preview
            .as_ref()
            .is_some_and(|current| current.clock().is(clock))
    }
}
