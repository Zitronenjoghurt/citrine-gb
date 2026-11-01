use crate::now;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Default)]
struct Daily {
    day: u64,
    peak: u64,
    messages: u64,
}

pub struct Stats {
    started: Instant,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    daily: Mutex<Daily>,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            daily: Mutex::new(Daily::default()),
        }
    }
}

impl Stats {
    fn today(&self) -> std::sync::MutexGuard<'_, Daily> {
        let mut daily = self.daily.lock().unwrap_or_else(|e| e.into_inner());
        let day = now() / 86_400;
        if daily.day != day {
            *daily = Daily {
                day,
                ..Daily::default()
            };
        }
        daily
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub fn received(&self, bytes: usize) {
        self.bytes_in.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn sent(&self, bytes: usize) {
        self.bytes_out.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn bytes_in(&self) -> u64 {
        self.bytes_in.load(Ordering::Relaxed)
    }

    pub fn bytes_out(&self) -> u64 {
        self.bytes_out.load(Ordering::Relaxed)
    }

    pub fn connected(&self, now_online: usize) {
        let mut daily = self.today();
        daily.peak = daily.peak.max(now_online as u64);
    }

    pub fn message(&self) {
        self.today().messages += 1;
    }

    pub fn peak_today(&self) -> u64 {
        self.today().peak
    }

    pub fn messages_today(&self) -> u64 {
        self.today().messages
    }
}
