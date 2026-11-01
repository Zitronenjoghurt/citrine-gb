use std::fmt::Debug;
use std::time::Duration;

/// Exponential Moving Average
#[derive(Debug, Default, Clone, Copy)]
pub struct EMA<N: EMAValue>(N);

impl<N> EMA<N>
where
    N: EMAValue,
{
    pub fn new(value: N) -> Self {
        Self(value)
    }

    pub fn update(&mut self, new: N, periods: usize) {
        let current = self.0.to_f64();
        if current == 0.0 {
            self.0 = new;
        } else {
            let alpha = 2.0 / (periods as f64 + 1.0);
            self.0 = N::from_f64((new.to_f64() * alpha) + (current * (1.0 - alpha)));
        }
    }

    pub fn value(&self) -> N {
        self.0
    }
}

pub trait EMAValue: Copy {
    fn to_f64(self) -> f64;
    fn from_f64(value: f64) -> Self;
}

impl EMAValue for f64 {
    fn to_f64(self) -> f64 {
        self
    }

    fn from_f64(value: f64) -> Self {
        value
    }
}

impl EMAValue for f32 {
    fn to_f64(self) -> f64 {
        self as f64
    }

    fn from_f64(value: f64) -> Self {
        value as f32
    }
}

impl EMAValue for Duration {
    fn to_f64(self) -> f64 {
        self.as_secs_f64()
    }

    fn from_f64(value: f64) -> Self {
        Duration::from_secs_f64(value)
    }
}

impl EMAValue for u32 {
    fn to_f64(self) -> f64 {
        self as f64
    }

    fn from_f64(value: f64) -> Self {
        value.floor() as u32
    }
}
