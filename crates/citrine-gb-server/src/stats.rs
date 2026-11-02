use crate::now;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const HOUR_MINUTES: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Blocked,
    Connections,
    SignIns,
    SignUps,
    Messages,
    Chats,
    Inbox,
    FriendRequests,
    Downloads,
    Storage,
    RelayStorage,
    Play,
}

impl Refused {
    pub const ALL: [Refused; 12] = [
        Refused::Blocked,
        Refused::Connections,
        Refused::SignIns,
        Refused::SignUps,
        Refused::Messages,
        Refused::Chats,
        Refused::Inbox,
        Refused::FriendRequests,
        Refused::Downloads,
        Refused::Storage,
        Refused::RelayStorage,
        Refused::Play,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Refused::Blocked => "blocked",
            Refused::Connections => "connections",
            Refused::SignIns => "sign_ins",
            Refused::SignUps => "sign_ups",
            Refused::Messages => "messages",
            Refused::Chats => "chats",
            Refused::Inbox => "inbox",
            Refused::FriendRequests => "friend_requests",
            Refused::Downloads => "downloads",
            Refused::Storage => "storage",
            Refused::RelayStorage => "relay_storage",
            Refused::Play => "play",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Refused::Blocked => "Blocked addresses",
            Refused::Connections => "Connections",
            Refused::SignIns => "Sign-in tries",
            Refused::SignUps => "Sign-ups",
            Refused::Messages => "Messages",
            Refused::Chats => "Chat messages",
            Refused::Inbox => "Full inboxes",
            Refused::FriendRequests => "Friend requests",
            Refused::Downloads => "Downloads",
            Refused::Storage => "Account storage",
            Refused::RelayStorage => "Relay storage",
            Refused::Play => "Play data",
        }
    }

    pub fn index(self) -> usize {
        Refused::ALL
            .iter()
            .position(|k| *k == self)
            .unwrap_or_default()
    }
}

pub type RefusedCounts = [u64; Refused::ALL.len()];

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
    refusals: Mutex<VecDeque<(u64, RefusedCounts)>>,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            daily: Mutex::new(Daily::default()),
            refusals: Mutex::new(VecDeque::new()),
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

    fn minute(&self) -> u64 {
        self.started.elapsed().as_secs() / 60
    }

    pub fn refused(&self, kind: Refused) {
        let minute = self.minute();
        let mut minutes = self.refusals.lock().unwrap_or_else(|e| e.into_inner());
        if minutes.back().is_none_or(|(m, _)| *m != minute) {
            minutes.push_back((minute, RefusedCounts::default()));
        }
        while minutes
            .front()
            .is_some_and(|(m, _)| minute - m >= HOUR_MINUTES)
        {
            minutes.pop_front();
        }
        if let Some((_, counts)) = minutes.back_mut() {
            counts[kind.index()] += 1;
        }
    }

    pub fn refused_last_hour(&self) -> RefusedCounts {
        let minute = self.minute();
        let minutes = self.refusals.lock().unwrap_or_else(|e| e.into_inner());
        let mut total = RefusedCounts::default();
        for (m, counts) in minutes.iter() {
            if minute - m < HOUR_MINUTES {
                for (sum, n) in total.iter_mut().zip(counts) {
                    *sum += n;
                }
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_count_by_kind() {
        let stats = Stats::default();
        stats.refused(Refused::Chats);
        stats.refused(Refused::Chats);
        stats.refused(Refused::Play);
        let counts = stats.refused_last_hour();
        assert_eq!(counts[Refused::Chats.index()], 2);
        assert_eq!(counts[Refused::Play.index()], 1);
        assert_eq!(counts.iter().sum::<u64>(), 3);
    }
}
