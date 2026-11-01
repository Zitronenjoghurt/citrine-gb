use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Bucket {
    capacity: f64,
    per_second: f64,
    tokens: f64,
    last: Instant,
}

impl Bucket {
    pub fn new(per_second: f64, capacity: f64) -> Self {
        Self {
            capacity,
            per_second,
            tokens: capacity,
            last: Instant::now(),
        }
    }

    pub fn take(&mut self, amount: f64) -> bool {
        self.take_at(amount, Instant::now())
    }

    fn take_at(&mut self, amount: f64, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.per_second).min(self.capacity);
        if self.tokens >= amount {
            self.tokens -= amount;
            true
        } else {
            false
        }
    }
}

pub struct ConnLimit {
    messages: Bucket,
    bytes: Bucket,
}

impl Default for ConnLimit {
    fn default() -> Self {
        Self {
            messages: Bucket::new(400.0, 800.0),
            bytes: Bucket::new(4.0 * 1024.0 * 1024.0, 8.0 * 1024.0 * 1024.0),
        }
    }
}

impl ConnLimit {
    pub fn allow(&mut self, bytes: usize) -> bool {
        self.messages.take(1.0) && self.bytes.take(bytes as f64)
    }
}

pub struct Attempts {
    max: u32,
    window: Duration,
    failures: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl Attempts {
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            failures: Mutex::new(HashMap::new()),
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<IpAddr, (u32, Instant)>> {
        self.failures.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn blocked(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut map = self.map();
        map.retain(|_, (_, since)| now.duration_since(*since) < self.window);
        map.get(&ip).is_some_and(|(count, _)| *count >= self.max)
    }

    pub fn fail(&self, ip: IpAddr) {
        let now = Instant::now();
        let mut map = self.map();
        let entry = map.entry(ip).or_insert((0, now));
        if now.duration_since(entry.1) >= self.window {
            *entry = (0, now);
        }
        entry.0 += 1;
    }

    pub fn clear(&self, ip: IpAddr) {
        self.map().remove(&ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_refills_over_time() {
        let start = Instant::now();
        let mut bucket = Bucket::new(10.0, 2.0);
        bucket.last = start;
        assert!(bucket.take_at(1.0, start));
        assert!(bucket.take_at(1.0, start));
        assert!(!bucket.take_at(1.0, start));
        assert!(bucket.take_at(1.0, start + Duration::from_millis(100)));
        assert!(!bucket.take_at(1.0, start + Duration::from_millis(100)));
        assert!(bucket.take_at(2.0, start + Duration::from_secs(10)));
    }

    #[test]
    fn attempts_block_after_max_failures() {
        let attempts = Attempts::new(3, Duration::from_secs(60));
        let ip = IpAddr::from([10, 0, 0, 1]);
        let other = IpAddr::from([10, 0, 0, 2]);
        for _ in 0..3 {
            assert!(!attempts.blocked(ip));
            attempts.fail(ip);
        }
        assert!(attempts.blocked(ip));
        assert!(!attempts.blocked(other));
        attempts.clear(ip);
        assert!(!attempts.blocked(ip));
    }
}
