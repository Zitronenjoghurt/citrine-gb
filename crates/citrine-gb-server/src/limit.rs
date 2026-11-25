use crate::config::Limits;
use crate::connections::address_key;
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

    pub fn has(&mut self, amount: f64) -> bool {
        self.refill(Instant::now());
        self.tokens >= amount
    }

    pub fn wait(&mut self, amount: f64) -> Duration {
        self.refill(Instant::now());
        Duration::from_secs_f64((amount - self.tokens).max(0.0) / self.per_second)
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

pub trait Tried: Hash + Eq {
    fn grouped(self) -> Self;
}

impl Tried for IpAddr {
    fn grouped(self) -> Self {
        address_key(self)
    }
}

impl Tried for i64 {
    fn grouped(self) -> Self {
        self
    }
}

struct Failures<K> {
    map: HashMap<K, (u32, Instant)>,
    prune_at: usize,
}

pub struct Attempts<K = IpAddr> {
    max: u32,
    window: Duration,
    failures: Mutex<Failures<K>>,
}

impl<K: Tried> Attempts<K> {
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            failures: Mutex::new(Failures {
                map: HashMap::new(),
                prune_at: PRUNE_AT,
            }),
        }
    }

    fn failures(&self) -> std::sync::MutexGuard<'_, Failures<K>> {
        self.failures.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn live(&self, since: Instant, now: Instant) -> bool {
        now.duration_since(since) < self.window
    }

    pub fn blocked(&self, key: K) -> bool {
        let now = Instant::now();
        let failures = self.failures();
        failures
            .map
            .get(&key.grouped())
            .is_some_and(|(count, since)| *count >= self.max && self.live(*since, now))
    }

    pub fn fail(&self, key: K) {
        let now = Instant::now();
        let mut failures = self.failures();
        if failures.map.len() >= failures.prune_at {
            failures
                .map
                .retain(|_, (_, since)| now.duration_since(*since) < self.window);
            failures.prune_at = (failures.map.len() * 2).max(PRUNE_AT);
        }
        let entry = failures.map.entry(key.grouped()).or_insert((0, now));
        if !self.live(entry.1, now) {
            *entry = (0, now);
        }
        entry.0 += 1;
    }

    pub fn forgive(&self, key: K) {
        if let Some(entry) = self.failures().map.get_mut(&key.grouped()) {
            entry.0 = entry.0.saturating_sub(1);
        }
    }

    pub fn clear(&self, key: K) {
        self.failures().map.remove(&key.grouped());
    }

    #[cfg(test)]
    fn kept(&self) -> usize {
        self.failures().map.len()
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
    pub archive_downloads: Buckets<i64>,
    pub sessions: Buckets<(u64, i64)>,
    pub games: Buckets<(u64, i64)>,
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
            archive_downloads: Buckets::new(
                limits.archive_download_bytes_per_day as f64 / 86_400.0,
                limits.archive_download_bytes_per_day as f64,
            ),
            sessions: Buckets::new(
                limits.session_bytes_per_second as f64,
                limits.session_burst_bytes as f64,
            ),
            games: Buckets::new(
                limits.game_bytes_per_second as f64,
                limits.game_burst_bytes as f64,
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
    fn bucket_waits_for_what_it_lacks() {
        let mut bucket = Bucket::new(10.0, 20.0);
        assert_eq!(bucket.wait(20.0), Duration::ZERO);
        assert!(bucket.take(20.0));
        let wait = bucket.wait(5.0);
        assert!(wait > Duration::from_millis(400) && wait <= Duration::from_millis(500));
    }

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
    fn ipv6_tries_count_once_per_network() {
        let attempts = Attempts::new(3, Duration::from_secs(60));
        let ip = |text: &str| text.parse::<IpAddr>().unwrap();
        for n in 1..=3 {
            attempts.fail(ip(&format!("2001:db8:5:6::{n}")));
        }
        assert!(attempts.blocked(ip("2001:db8:5:6:abcd::9")));
        assert!(!attempts.blocked(ip("2001:db8:5:7::1")));
    }

    #[test]
    fn stale_tries_are_dropped_as_new_ones_come() {
        let attempts = Attempts::new(3, Duration::ZERO);
        for n in 0..3 * PRUNE_AT as u32 {
            attempts.fail(IpAddr::from(n.to_be_bytes()));
        }
        assert!(attempts.kept() <= PRUNE_AT);
    }

    #[test]
    fn a_forgiven_attempt_no_longer_counts() {
        let attempts = Attempts::new(2, Duration::from_secs(60));
        attempts.fail(1i64);
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
