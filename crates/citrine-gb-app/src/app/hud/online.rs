use crate::app::tabs::playing::RoomFacts;
use crate::online::Online;
use crate::play::{Play, PlaySession};
use citrine_gb::console::Console;
use citrine_gb_net::LinkPath;
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

pub const EVERY: Duration = Duration::from_millis(500);
pub const WAIT_WINDOW: Duration = Duration::from_secs(60);
const NAME_CHARS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Ping {
    #[default]
    Unknown,
    Server(u32),
    Partner(Option<u32>),
    Partners(Vec<(String, Option<u32>)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delay {
    pub frames: u8,
    pub ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Link {
    pub ping: Ping,
    pub delay: Option<Delay>,
    pub path: Option<LinkPath>,
    pub waits: Option<u32>,
}

#[derive(Default)]
struct Waits {
    starts: VecDeque<Instant>,
    last: Option<Instant>,
}

impl Waits {
    fn see(&mut self, stalled: Duration, now: Instant) {
        if !stalled.is_zero()
            && let Some(start) = now.checked_sub(stalled)
            && self
                .last
                .is_none_or(|last| start > last + Duration::from_millis(1))
        {
            self.starts.push_back(start);
            self.last = Some(start);
        }
        while self
            .starts
            .front()
            .is_some_and(|&start| now.saturating_duration_since(start) > WAIT_WINDOW)
        {
            self.starts.pop_front();
        }
    }

    fn count(&self) -> u32 {
        self.starts.len().try_into().unwrap_or(u32::MAX)
    }
}

#[derive(Default)]
pub struct Watch {
    shown: Option<(Instant, Link)>,
    waits: Waits,
}

pub struct Wants {
    pub ping: bool,
    pub any: bool,
}

impl Watch {
    pub fn read(&mut self, play: &Play, online: &mut Online, wants: Wants, now: Instant) -> Link {
        if !wants.any {
            self.shown = None;
            return Link::default();
        }
        let session = play.session().filter(|s| s.is_live());
        match session {
            Some(session) => self.waits.see(session.stalled_for(now), now),
            None => self.waits = Waits::default(),
        }
        if session.is_none() && wants.ping && online.is_connected() {
            online.watch_round_trip(now);
        }
        if let Some((at, link)) = &self.shown
            && now.saturating_duration_since(*at) < EVERY
        {
            return link.clone();
        }
        let link = match session {
            Some(session) => Link {
                ping: partner_ping(play, online, session),
                delay: delay(session),
                path: Some(session.path()),
                waits: Some(self.waits.count()),
            },
            None => Link {
                ping: match online.is_connected().then(|| online.round_trip_ms()) {
                    Some(Some(ms)) => Ping::Server(ms),
                    _ => Ping::Unknown,
                },
                ..Link::default()
            },
        };
        self.shown = Some((now, link.clone()));
        link
    }
}

fn partner_ping(play: &Play, online: &Online, session: &PlaySession) -> Ping {
    if !session.in_group() {
        return Ping::Partner(session.round_trip_ms());
    }
    let Some(facts) = RoomFacts::of(play, online, 0) else {
        return Ping::Partner(session.round_trip_ms());
    };
    Ping::Partners(
        facts
            .partners()
            .into_iter()
            .map(|p| (short(p.short()), p.ms.map(u32::from)))
            .collect(),
    )
}

fn short(name: &str) -> String {
    if name.chars().count() <= NAME_CHARS {
        return name.to_owned();
    }
    let mut cut: String = name.chars().take(NAME_CHARS - 1).collect();
    cut.push('\u{2026}');
    cut
}

fn delay(session: &PlaySession) -> Option<Delay> {
    let frames = session.delay()?;
    let timing = session.console(true).map(Console::timing).or_else(|| {
        session
            .my_game()
            .or(session.friend_game())
            .map(|card| crate::play::kind_of(card.console).timing())
    })?;
    Some(Delay {
        frames,
        ms: (f64::from(frames) * timing.frame_secs() * 1000.0).round() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::MyGame;
    use crate::play::tests::{Pair, homebrew, snake};

    const GB_FRAME_MS: [u32; 11] = [0, 17, 33, 50, 67, 84, 100, 117, 134, 151, 167];

    fn all() -> Wants {
        Wants {
            ping: true,
            any: true,
        }
    }

    fn playing(one_way_ms: u64) -> Pair {
        let mut pair = Pair::new(false, one_way_ms, 0, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair
    }

    fn host_link(pair: &mut Pair, watch: &mut Watch) -> Link {
        let now = pair.now();
        watch.read(&pair.host.play, &mut pair.host.online, all(), now)
    }

    fn fresh_link(pair: &mut Pair) -> Link {
        host_link(pair, &mut Watch::default())
    }

    #[test]
    fn ping_and_delay_are_what_the_session_plays_with() {
        let mut pair = playing(30);
        pair.play_frames(300);
        let link = fresh_link(&mut pair);
        let session = pair.host.session();
        let ms = session.round_trip_ms().unwrap();
        assert!(ms >= 60, "ping {ms} ms over a relay 30 ms away each way");
        assert_eq!(link.ping, Ping::Partner(Some(ms)));
        let frames = session.delay().unwrap();
        assert_eq!(
            link.delay,
            Some(Delay {
                frames,
                ms: GB_FRAME_MS[usize::from(frames)],
            })
        );
        assert_eq!(link.path, Some(LinkPath::Relay));
    }

    #[test]
    fn the_connection_says_direct_when_both_play_direct() {
        let mut pair = Pair::direct(true, true);
        pair.until("both play direct", |p| p.paths() == [LinkPath::Direct; 2]);
        assert_eq!(fresh_link(&mut pair).path, Some(LinkPath::Direct));
    }

    #[test]
    fn the_connection_says_relay_when_one_side_has_direct_off() {
        let mut pair = Pair::direct(true, false);
        pair.play_frames(120);
        assert_eq!(fresh_link(&mut pair).path, Some(LinkPath::Relay));
    }

    #[test]
    fn signed_out_every_online_value_is_empty() {
        let [shelf, _] = homebrew();
        let play = Play::new(shelf);
        let mut online = Online::offline();
        let link = Watch::default().read(&play, &mut online, all(), Instant::now());
        assert_eq!(link, Link::default());
    }

    #[test]
    fn signed_in_without_a_session_the_ping_is_to_the_server() {
        let mut pair = Pair::new(false, 40, 0, homebrew());
        let mut watch = Watch::default();
        let mut link = Link::default();
        for _ in 0..600 {
            pair.tick();
            link = host_link(&mut pair, &mut watch);
            if link.ping != Ping::Unknown {
                break;
            }
        }
        let ms = pair.host.online.round_trip_ms().unwrap();
        assert!(ms >= 40, "ping {ms} ms to a server 20 ms away each way");
        assert_eq!(link.ping, Ping::Server(ms));
        assert_eq!((link.delay, link.path, link.waits), (None, None, None));
    }

    #[test]
    fn shown_values_change_at_most_twice_a_second() {
        let mut pair = playing(0);
        let mut watch = Watch::default();
        for _ in 0..120 {
            pair.tick();
            host_link(&mut pair, &mut watch);
        }
        let before = watch.waits.count();
        let mut shown = host_link(&mut pair, &mut watch);
        let mut changes = Vec::new();
        let mut held = false;
        for (alone, together) in [(60, 120), (60, 120)] {
            for frame in 0..alone + together {
                if frame < alone {
                    pair.run_host_alone(1);
                } else {
                    pair.tick();
                }
                let link = host_link(&mut pair, &mut watch);
                held |= link.waits.is_some_and(|w| w < watch.waits.count());
                if link != shown {
                    changes.push((pair.now(), link.waits));
                    shown = link;
                }
            }
            assert!(pair.host.session().stalled_for(pair.now()).is_zero());
        }
        assert_eq!(watch.waits.count(), before + 2, "{changes:?}");
        assert!(held, "{changes:?}");
        assert!(changes.len() >= 2, "{changes:?}");
        for gap in changes.windows(2) {
            assert!(gap[1].0.duration_since(gap[0].0) >= EVERY, "{changes:?}");
        }
    }

    #[test]
    fn a_pause_for_the_other_player_counts_as_one_wait() {
        let mut pair = playing(0);
        let mut watch = Watch::default();
        for _ in 0..120 {
            pair.tick();
            host_link(&mut pair, &mut watch);
        }
        let before = watch.waits.count();
        pair.run_host_alone(60);
        assert!(!pair.host.session().stalled_for(pair.now()).is_zero());
        host_link(&mut pair, &mut watch);
        for _ in 0..120 {
            pair.tick();
            host_link(&mut pair, &mut watch);
        }
        assert!(pair.host.session().stalled_for(pair.now()).is_zero());
        assert_eq!(watch.waits.count(), before + 1);
    }

    #[test]
    fn waits_count_the_last_minute() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let frame = Duration::from_millis(17);
        let mut waits = Waits::default();
        waits.see(Duration::ZERO, at(0));
        assert_eq!(waits.count(), 0);
        waits.see(frame, at(1_017));
        waits.see(frame * 3, at(1_051));
        assert_eq!(waits.count(), 1);
        waits.see(Duration::ZERO, at(1_068));
        waits.see(frame, at(2_017));
        assert_eq!(waits.count(), 2);
        waits.see(Duration::ZERO, at(61_001));
        assert_eq!(waits.count(), 1);
        waits.see(Duration::ZERO, at(62_001));
        assert_eq!(waits.count(), 0);
    }
}
