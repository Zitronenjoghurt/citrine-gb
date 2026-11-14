use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub trait Preview: Send {
    fn render(&mut self, frames: usize, sample_rate: u32, out: &mut Vec<f32>);
    fn is_done(&self) -> bool;
    fn position(&self) -> f64;
}

#[derive(Clone, Default)]
pub struct PreviewClock(Arc<AtomicU64>);

impl PreviewClock {
    pub fn seconds(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn is(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub struct Playing {
    sound: Box<dyn Preview>,
    clock: PreviewClock,
}

impl Playing {
    pub fn new(sound: Box<dyn Preview>) -> (Self, PreviewClock) {
        let clock = PreviewClock::default();
        let playing = Self {
            sound,
            clock: clock.clone(),
        };
        (playing, clock)
    }

    pub fn clock(&self) -> &PreviewClock {
        &self.clock
    }

    pub fn render(&mut self, frames: usize, sample_rate: u32, out: &mut Vec<f32>) -> bool {
        self.sound.render(frames, sample_rate, out);
        self.clock
            .0
            .store(self.sound.position().to_bits(), Ordering::Relaxed);
        !self.sound.is_done()
    }
}
