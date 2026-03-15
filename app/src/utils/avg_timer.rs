use citrine_gb::utils::ema::EMA;
use std::fmt::Display;

pub struct AvgTimer {
    start: web_time::Instant,
    last_stop: Option<web_time::Instant>,
    ema_secs: EMA<f32>,
    ema_interval: EMA<f32>,
}

impl Default for AvgTimer {
    fn default() -> Self {
        Self {
            start: web_time::Instant::now(),
            last_stop: None,
            ema_secs: Default::default(),
            ema_interval: Default::default(),
        }
    }
}

impl AvgTimer {
    pub fn start(&mut self) {
        self.start = web_time::Instant::now();
    }

    pub fn stop(&mut self) {
        let now = web_time::Instant::now();
        let elapsed = (now - self.start).as_secs_f32();
        self.ema_secs.update(elapsed, 1200);

        if let Some(last) = self.last_stop {
            let interval = (now - last).as_secs_f32();
            if interval > 0.0 {
                self.ema_interval.update(interval, 1200);
            }
        }
        self.last_stop = Some(now);
    }

    pub fn average_secs(&self) -> f32 {
        self.ema_secs.value()
    }

    pub fn display_average_secs(&self) -> String {
        format!("{:.02}ms", self.average_secs() * 1000.0)
    }

    pub fn updates_per_sec(&self) -> f32 {
        let interval = self.ema_interval.value();
        if interval > 0.0 { 1.0 / interval } else { 0.0 }
    }

    pub fn display_updates_per_sec(&self) -> String {
        format!("{:.01}/s", self.updates_per_sec())
    }

    pub fn budget(&self) -> f32 {
        self.average_secs() * self.updates_per_sec()
    }

    pub fn display_budget(&self) -> String {
        format!("{:.01}%", self.budget() * 100.0)
    }
}
