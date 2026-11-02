use crate::config::Limits;
use std::collections::HashMap;
use std::hash::Hash;
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

    pub fn full(&mut self) -> bool {
        self.refill(Instant::now());
        self.tokens >= self.capacity
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.per_second).min(self.capacity);
    }

    fn take_at(&mut self, amount: f64, now: Instant) -> bool {
        self.refill(now);
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

pub struct Attempts<K = IpAddr> {
    max: u32,
    window: Duration,
    failures: Mutex<HashMap<K, (u32, Instant)>>,
}

impl<K: Hash + Eq> Attempts<K> {
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            failures: Mutex::new(HashMap::new()),
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<K, (u32, Instant)>> {
        self.failures.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn blocked(&self, key: K) -> bool {
        let now = Instant::now();
        let mut map = self.map();
        map.retain(|_, (_, since)| now.duration_since(*since) < self.window);
        map.get(&key).is_some_and(|(count, _)| *count >= self.max)
    }

    pub fn fail(&self, key: K) {
        let now = Instant::now();
        let mut map = self.map();
        let entry = map.entry(key).or_insert((0, now));
        if now.duration_since(entry.1) >= self.window {
            *entry = (0, now);
        }
        entry.0 += 1;
    }

    pub fn forgive(&self, key: K) {
        if let Some(entry) = self.map().get_mut(&key) {
            entry.0 = entry.0.saturating_sub(1);
        }
    }

    pub fn clear(&self, key: K) {
        self.map().remove(&key);
    }
}

const PRUNE_AT: usize = 4096;

pub struct Buckets<K> {
    per_second: f64,
    capacity: f64,
    map: Mutex<HashMap<K, Bucket>>,
}

impl<K: Hash + Eq> Buckets<K> {
    pub fn new(per_second: f64, capacity: f64) -> Self {
        Self {
            per_second,
            capacity,
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn capacity(&self) -> f64 {
        self.capacity
    }

    fn with<T>(&self, key: K, f: impl FnOnce(&mut Bucket) -> T) -> T {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= PRUNE_AT {
            map.retain(|_, b| !b.full());
        }
        let (per_second, capacity) = (self.per_second, self.capacity);
        f(map
            .entry(key)
            .or_insert_with(|| Bucket::new(per_second, capacity)))
    }

    pub fn take(&self, key: K, amount: f64) -> bool {
        self.with(key, |b| b.take(amount))
    }

    pub fn has(&self, key: K, amount: f64) -> bool {
        self.with(key, |b| {
            b.refill(Instant::now());
            b.tokens >= amount
        })
    }
}

pub struct Rates {
    pub messages: Buckets<i64>,
    chats: Buckets<i64>,
    chat_bytes: Buckets<i64>,
    pub friend_requests: Buckets<i64>,
    pub downloads: Buckets<i64>,
    pub sessions: Buckets<(u64, i64)>,
    pub sign_ups: Buckets<IpAddr>,
    pub all_sign_ups: Buckets<()>,
}

impl Rates {
    pub fn new(limits: &Limits) -> Self {
        let chat_bytes = limits.chat_bytes_per_second as f64;
        let per_hour = limits.friend_requests_per_hour as f64 / 3600.0;
        let per_day = limits.download_bytes_per_day as f64;
        let per_ip = limits.sign_ups_per_ip as f64;
        let all = limits.sign_ups_per_hour as f64;
        Self {
            messages: Buckets::new(
                limits.messages_per_second as f64,
                limits.message_burst as f64,
            ),
            chats: Buckets::new(limits.chats_per_second as f64, limits.chat_burst as f64),
            chat_bytes: Buckets::new(chat_bytes, chat_bytes * 4.0),
            friend_requests: Buckets::new(per_hour, limits.friend_request_burst as f64),
            downloads: Buckets::new(per_day / 86_400.0, per_day),
            sessions: Buckets::new(
                limits.session_bytes_per_second as f64,
                limits.session_burst_bytes as f64,
            ),
            sign_ups: Buckets::new(per_ip / 3600.0, per_ip),
            all_sign_ups: Buckets::new(all / 3600.0, all),
        }
    }

    pub fn chat(&self, account: i64, bytes: u64) -> bool {
        let bytes = (bytes as f64).min(self.chat_bytes.capacity());
        if !self.chats.has(account, 1.0) || !self.chat_bytes.has(account, bytes) {
            return false;
        }
        self.chats.take(account, 1.0) && self.chat_bytes.take(account, bytes)
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

    #[test]
    fn a_forgiven_attempt_no_longer_counts() {
        let attempts = Attempts::new(2, Duration::from_secs(60));
        attempts.fail(1);
        attempts.fail(1);
        assert!(attempts.blocked(1));
        attempts.forgive(1);
        assert!(!attempts.blocked(1));
    }

    #[test]
    fn buckets_are_kept_per_key() {
        let buckets = Buckets::new(0.001, 2.0);
        assert!(buckets.take(1, 1.0));
        assert!(buckets.take(1, 1.0));
        assert!(!buckets.take(1, 1.0));
        assert!(!buckets.has(1, 1.0));
        assert!(buckets.has(2, 2.0));
        assert!(buckets.take(2, 2.0));
    }
}
