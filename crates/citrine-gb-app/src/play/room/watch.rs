use super::Room;
use super::group::is_group_round;
use crate::play::Picture;
use crate::play::engine::{GROUP_UNITS, Rollback, Setup, Ticks};
use crate::play::pace::Pace;
use crate::play::seen::Still;
use crate::play::shelf::{Finder, Look};
use citrine_gb::console::{ConsoleKind, Keys};
use citrine_gb::netplay::Player;
use citrine_gb_net::{GameHash, OnlineId, Origin, PlayMsg, Start};
use citrine_gb_threads::{Lent, Threads};
use web_time::Instant;

const KEPT_EARLY: usize = 1024;
const SAMPLE_RATE: u32 = 48_000;

struct WatchFrom {
    frame: u32,
    members: Vec<OnlineId>,
    games: Vec<(u8, GameHash)>,
    cable: Vec<u8>,
}

pub(in crate::play) struct GroupWatch {
    of: OnlineId,
    authority: OnlineId,
    round: u32,
    start: Option<Start>,
    from: Option<WatchFrom>,
    states: Vec<(u8, Vec<u8>)>,
    early: Vec<(OnlineId, PlayMsg)>,
    engine: Option<Lent<Rollback>>,
    seen: Still,
    missing: bool,
    pace: Pace,
    last: Option<Instant>,
    audio: Vec<f32>,
}

impl GroupWatch {
    fn unit_of(&self, who: OnlineId) -> Option<u8> {
        let from = self.from.as_ref()?;
        from.members.iter().position(|w| *w == who).map(|i| i as u8)
    }

    fn hears_from(&self, who: OnlineId, linked: bool) -> bool {
        linked
            || who == self.authority
            || self.from.as_ref().is_some_and(|f| f.members.contains(&who))
    }

    fn deliver(&mut self, from: OnlineId, msg: PlayMsg) {
        let Some(unit) = self.unit_of(from) else {
            return;
        };
        let Some(r) = self.engine.as_mut().map(Lent::get_mut) else {
            return;
        };
        match msg {
            PlayMsg::MemberLeft { unit, at_frame, .. } => {
                if from == self.authority {
                    r.follow_leave(unit, at_frame);
                }
            }
            msg => r.receive_from(unit, msg),
        }
    }

    fn launch(&mut self, finder: &mut Finder, lanes: usize) {
        if self.engine.is_some() || self.missing {
            return;
        }
        let (Some(start), Some(from)) = (&self.start, &self.from) else {
            return;
        };
        let mut games: [Option<Setup>; GROUP_UNITS] = Default::default();
        for (unit, hash) in &from.games {
            let Some(slot) = games.get_mut(usize::from(*unit)) else {
                return;
            };
            let Some((_, state)) = self.states.iter().find(|(u, _)| u == unit) else {
                return;
            };
            let rom = match finder.look(hash) {
                Look::Found(rom) => rom,
                Look::Missing => {
                    self.missing = true;
                    return;
                }
                Look::Looking => return,
            };
            *slot = Some((rom, ConsoleKind::Gba, Origin::Resume(state.clone())));
        }
        let local = games
            .iter()
            .position(Option::is_none)
            .or_else(|| self.unit_of(self.of).map(usize::from))
            .unwrap_or(0);
        let start = Start {
            start_frame: from.frame,
            ..start.clone()
        };
        let sync = crate::play::links::sync_for(
            Some(citrine_gb_net::Console::Gba),
            from.members.len(),
            lanes,
        );
        let Ok(mut r) = Rollback::group(
            Player::ALL[local],
            start,
            games,
            Default::default(),
            SAMPLE_RATE,
            sync,
            u8::MAX,
        ) else {
            return;
        };
        if !r.restore_cable(&from.cable) {
            return;
        }
        r.watch(true);
        self.engine = Some(Lent::new(Box::new(r)));
        self.states.clear();
        for (from, msg) in std::mem::take(&mut self.early) {
            self.deliver(from, msg);
        }
    }

    fn tick(&mut self, now: Instant) {
        let dt = self
            .last
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64());
        self.last = Some(now);
        let unit = self.unit_of(self.of);
        let Some(lent) = self.engine.as_mut() else {
            return;
        };
        let away = lent.has_worker();
        let r = lent.get_mut();
        let mut friend = Vec::new();
        r.drain_audio(&mut self.audio, &mut friend);
        self.audio.clear();
        r.take_ticked();
        let frames = self.pace.frames(dt, r.timing()) as usize;
        if frames == 0 {
            return;
        }
        if away {
            self.seen.keep(unit.and_then(|u| r.unit_console(u)), true);
        }
        lent.lend(Ticks {
            pads: vec![Keys::empty(); frames],
            touch: None,
            cable: None,
            saves: false,
            round_trip_ms: 0,
            now,
        });
    }

    fn place(&mut self, threads: Option<&Threads>) {
        let Some(lent) = self.engine.as_mut() else {
            return;
        };
        match threads {
            Some(threads) if !lent.has_worker() => {
                lent.run_on(threads, format!("watch: {}", self.of))
            }
            Some(_) => {}
            None if lent.has_worker() => lent.park(),
            None => {}
        }
    }
}

impl Room {
    pub(in crate::play) fn watch_group(&mut self, who: OnlineId, on: bool) -> bool {
        let Some(me) = self.me.filter(|m| *m != who) else {
            return false;
        };
        if self.grouped_with(me, who) {
            let unit = self.group.as_ref().and_then(|g| g.unit_of(who));
            let Some(r) = self.group_engine_mut() else {
                return false;
            };
            r.watch_unit(if on { unit } else { None });
            return true;
        }
        if !on {
            let Some(watch) = self.watch.take() else {
                return false;
            };
            self.tell_watch(watch.authority, false);
            return true;
        }
        if !self.in_other_group(who) {
            return false;
        }
        let Some(group) = self.links.group_of(who) else {
            return false;
        };
        let (authority, round) = (group.authority(), group.round());
        if let Some(watch) = &mut self.watch
            && watch.authority == authority
            && watch.round == round
        {
            watch.of = who;
            return true;
        }
        if let Some(old) = self.watch.take() {
            self.tell_watch(old.authority, false);
        }
        self.watch = Some(GroupWatch {
            of: who,
            authority,
            round,
            start: None,
            from: None,
            states: Vec::new(),
            early: Vec::new(),
            engine: None,
            seen: Still::default(),
            missing: false,
            pace: Pace::default(),
            last: None,
            audio: Vec::new(),
        });
        self.tell_watch(authority, true);
        true
    }

    fn tell_watch(&mut self, authority: OnlineId, on: bool) {
        if let Some(pair) = self.pairs.iter_mut().find(|p| p.friend == authority) {
            pair.outbox.push(PlayMsg::WatchGroup(on));
        }
    }

    pub(in crate::play) fn hear_watch(&mut self, from: OnlineId, msg: PlayMsg) -> Option<PlayMsg> {
        let mine = self
            .group
            .as_ref()
            .is_some_and(|g| g.members.contains(&from));
        let linked = self.watch.as_ref().is_some_and(|w| {
            self.links
                .group_of(w.authority)
                .is_some_and(|g| g.round() == w.round && g.holds(from))
        });
        let Some(watch) = self
            .watch
            .as_mut()
            .filter(|w| !mine && w.hears_from(from, linked))
        else {
            return Some(msg);
        };
        let round = match &msg {
            PlayMsg::Start(start) => start.round,
            PlayMsg::Pads { round, .. }
            | PlayMsg::Resync { round, .. }
            | PlayMsg::MemberLeft { round, .. }
            | PlayMsg::WatchFrom { round, .. } => *round,
            _ => return Some(msg),
        };
        if !is_group_round(round) || round != watch.round {
            return Some(msg);
        }
        match msg {
            PlayMsg::Start(start) if from == watch.authority => watch.start = Some(start),
            PlayMsg::WatchFrom {
                frame,
                members,
                games,
                cable,
                ..
            } if from == watch.authority => {
                watch.from = Some(WatchFrom {
                    frame,
                    members,
                    games,
                    cable,
                });
            }
            PlayMsg::Resync { unit, data, .. }
                if from == watch.authority && watch.engine.is_none() =>
            {
                watch.states.retain(|(u, _)| *u != unit);
                watch.states.push((unit, data));
            }
            msg if watch.engine.is_some() => watch.deliver(from, msg),
            msg => {
                watch.early.push((from, msg));
                if watch.early.len() > KEPT_EARLY {
                    watch.early.remove(0);
                }
            }
        }
        None
    }

    pub(in crate::play) fn step_watch(&mut self, finder: &mut Finder) {
        let Some(watch) = &self.watch else {
            return;
        };
        let alive = self
            .links
            .group_of(watch.authority)
            .is_some_and(|g| g.round() == watch.round && g.holds(watch.of));
        if !alive {
            self.watch = None;
            return;
        }
        if let Some(watch) = &mut self.watch {
            watch.launch(finder, self.lanes);
        }
    }

    pub(in crate::play) fn tick_watch(&mut self, now: Instant) {
        if let Some(watch) = &mut self.watch {
            watch.tick(now);
        }
    }

    pub(in crate::play) fn place_engines(&mut self, threads: Option<&Threads>) {
        if let Some(threads) = threads {
            self.lanes = threads.pool_size();
        }
        if let Some(watch) = &mut self.watch {
            watch.place(threads);
        }
        for pair in &mut self.pairs {
            pair.place_engine(threads);
        }
    }

    pub fn in_other_group(&self, who: OnlineId) -> bool {
        let Some(me) = self.me else {
            return false;
        };
        self.links
            .group_of(who)
            .is_some_and(|g| g.members().len() > 2 && !g.holds(me))
    }

    pub fn watching(&self) -> Option<OnlineId> {
        self.watch.as_ref().map(|w| w.of)
    }

    pub fn watch_missing(&self) -> bool {
        self.watch.as_ref().is_some_and(|w| w.missing)
    }

    #[cfg(all(test, feature = "gba"))]
    pub(in crate::play) fn watch_engine(&self) -> Option<&Rollback> {
        self.watch.as_ref()?.engine.as_ref().map(Lent::get)
    }

    pub fn watched_picture(&self) -> Option<Picture<'_>> {
        let watch = self.watch.as_ref()?;
        let unit = watch.unit_of(watch.of)?;
        match watch.engine.as_ref()?.home() {
            Some(r) => r.unit_console(unit).map(Picture::of),
            None => watch.seen.picture(),
        }
    }

    #[cfg(all(test, feature = "gba"))]
    pub(in crate::play) fn watch_home(&self) -> bool {
        self.watch
            .as_ref()
            .and_then(|w| w.engine.as_ref())
            .is_none_or(Lent::is_home)
    }

    pub(in crate::play) fn serve_watchers(&mut self, me: OnlineId) {
        let Some(linked) = self.links.group_of(me).cloned() else {
            return;
        };
        let new: Vec<OnlineId> = match &self.group {
            Some(g) => linked
                .watchers()
                .iter()
                .copied()
                .filter(|w| !g.watching.contains(w))
                .collect(),
            None => return,
        };
        if let Some(g) = &mut self.group {
            g.watching = linked.watchers().to_vec();
        }
        if !new.is_empty()
            && let Some(pads) = self.group_engine().and_then(Rollback::recent_pads)
        {
            for pair in &mut self.pairs {
                if new.contains(&pair.friend) && pair.is_live() {
                    pair.outbox.push(pads.clone());
                }
            }
        }
        if linked.authority() != me || self.watch_asks.is_empty() {
            return;
        }
        let Some(g) = &self.group else {
            return;
        };
        let (members, round) = (g.members.clone(), g.round);
        let Some(start) = g.start.clone() else {
            return;
        };
        let Some(point) = self.group_engine().and_then(Rollback::watch_point) else {
            return;
        };
        let games: Option<Vec<(u8, GameHash)>> = point
            .states
            .iter()
            .map(|(unit, _)| {
                let who = *members.get(usize::from(*unit))?;
                let hash = if who == me {
                    self.my_card()?.hash
                } else {
                    self.pairs
                        .iter()
                        .find(|p| p.friend == who)?
                        .friend_game()?
                        .hash
                };
                Some((*unit, hash))
            })
            .collect();
        let Some(games) = games else {
            return;
        };
        let asks: Vec<OnlineId> = std::mem::take(&mut self.watch_asks)
            .into_iter()
            .filter(|w| !linked.holds(*w))
            .collect();
        let mut watchers = linked.watchers().to_vec();
        for who in asks {
            let Some(pair) = self
                .pairs
                .iter_mut()
                .find(|p| p.friend == who && p.is_live())
            else {
                continue;
            };
            pair.outbox.push(PlayMsg::Start(Start {
                start_frame: point.frame,
                ..start.clone()
            }));
            for (unit, data) in &point.states {
                pair.outbox.push(PlayMsg::Resync {
                    round,
                    frame: point.frame,
                    unit: *unit,
                    data: data.clone(),
                    cable: None,
                });
            }
            pair.outbox.push(PlayMsg::WatchFrom {
                round,
                frame: point.frame,
                members: members.clone(),
                games: games.clone(),
                cable: point.cable.clone(),
            });
            if !watchers.contains(&who) {
                watchers.push(who);
            }
        }
        if watchers.len() != linked.watchers().len() {
            self.links.set_watchers(me, watchers);
            self.sent.clear();
        }
    }
}
