use citrine_gb::utils::ema::EMA;

#[derive(Clone)]
pub struct AvgTimer {
    start: web_time::Instant,
    last_stop: Option<web_time::Instant>,
    ema_secs: EMA<f32>,
    ema_interval: EMA<f32>,
    stops: u64,
    total: std::time::Duration,
}

impl Default for AvgTimer {
    fn default() -> Self {
        Self {
            start: web_time::Instant::now(),
            last_stop: None,
            ema_secs: Default::default(),
            ema_interval: Default::default(),
            stops: 0,
            total: std::time::Duration::ZERO,
        }
    }
}

impl AvgTimer {
    pub fn start(&mut self) {
        self.start = web_time::Instant::now();
    }

    pub fn stop(&mut self) -> std::time::Duration {
        let now = web_time::Instant::now();
        let spent = now - self.start;
        let elapsed = spent.as_secs_f32();
        self.ema_secs.update(elapsed, 60);

        if let Some(last) = self.last_stop {
            let interval = (now - last).as_secs_f32();
            if interval > 0.0 {
                self.ema_interval.update(interval, 60);
            }
        }
        self.last_stop = Some(now);
        self.stops += 1;
        self.total += spent;
        spent
    }

    pub fn stops(&self) -> u64 {
        self.stops
    }

    pub fn total(&self) -> std::time::Duration {
        self.total
    }

    pub fn average_secs(&self) -> f32 {
        self.ema_secs.value()
    }

    pub fn display_average_secs(&self) -> String {
        format!("{:.01}ms", self.average_secs() * 1000.0)
    }

    pub fn updates_per_sec(&self) -> f32 {
        let interval = self.ema_interval.value();
        if interval > 0.0 { 1.0 / interval } else { 0.0 }
    }

    pub fn display_updates_per_sec(&self) -> String {
        format!("{:.0}/s", self.updates_per_sec())
    }

    pub fn budget(&self) -> f32 {
        self.average_secs() * self.updates_per_sec()
    }

    pub fn display_budget(&self) -> String {
        format!("{:.0}%", self.budget() * 100.0)
    }
}
