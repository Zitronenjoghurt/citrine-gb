use super::Room;
use crate::play::engine::{GROUP_UNITS, Rollback, Setup};
use crate::play::links::{self, LinkGroup};
use crate::play::{
    Engine, GUEST_POWER_ON, PlaySession, Returned, Role, SETUP_PINGS, SETUP_WAIT, Stage, kind_of,
    random_seed, unix_now,
};
use citrine_gb::netplay::{NEVER, Player};
use citrine_gb_net::{Console, Mode, OnlineId, Origin, PlayMsg, Start};
use std::time::Duration;
use web_time::Instant;

pub(in crate::play) const GROUP_ROUND: u32 = 1 << 31;
const KEPT_PENDING: usize = 64;

pub(in crate::play) struct GroupPlay {
    pub(in crate::play) round: u32,
    pub(in crate::play) members: Vec<OnlineId>,
    pub(in crate::play) start: Option<Start>,
    pub(in crate::play) gone: Vec<OnlineId>,
    pub(in crate::play) left: Vec<(u8, u32)>,
    pending: Vec<(OnlineId, PlayMsg)>,
    pub(in crate::play) watching: Vec<OnlineId>,
}

pub(in crate::play) fn is_group_round(round: u32) -> bool {
    round >= GROUP_ROUND
}

fn group_round_of(msg: &PlayMsg) -> Option<u32> {
    match msg {
        PlayMsg::Pads { round, .. }
        | PlayMsg::Check { round, .. }
        | PlayMsg::Resync { round, .. }
        | PlayMsg::MemberLeft { round, .. } => Some(*round),
        PlayMsg::Start(start) => Some(start.round),
        _ => None,
    }
    .filter(|round| is_group_round(*round))
}

impl GroupPlay {
    fn new(group: &LinkGroup) -> Self {
        Self {
            round: group.round(),
            members: group.members().to_vec(),
            start: None,
            gone: Vec::new(),
            left: Vec::new(),
            pending: Vec::new(),
            watching: Vec::new(),
        }
    }

    pub(in crate::play) fn unit_of(&self, who: OnlineId) -> Option<u8> {
        self.members.iter().position(|w| *w == who).map(|i| i as u8)
    }

    pub(in crate::play) fn playing(&self) -> impl Iterator<Item = OnlineId> + '_ {
        self.members
            .iter()
            .copied()
            .filter(|w| !self.gone.contains(w))
    }
}

impl PlaySession {
    fn group_ready(&self, authority: bool, now: Instant) -> bool {
        let mine = self.round.my_card.is_some();
        if mine && (!self.round.set_up || self.round.they_hold_mine != Some(true)) {
            return false;
        }
        match &self.round.theirs {
            None => return false,
            Some(Some(_)) => {
                if self.round.their_origin.is_none()
                    || self.round.i_hold_theirs != Some(true)
                    || self.round.their_rom.is_none()
                {
                    return false;
                }
            }
            Some(None) => {}
        }
        !authority
            || self.round_trips.len() >= SETUP_PINGS
            || now.duration_since(self.started_at) >= SETUP_WAIT
    }

    fn their_setup(&self) -> Option<Setup> {
        let Some(Some(card)) = &self.round.theirs else {
            return None;
        };
        Some((
            self.round.their_rom.clone()?,
            kind_of(card.console),
            self.round.their_origin.clone()?,
        ))
    }

    fn my_setup(&self) -> Option<Setup> {
        let card = self.round.my_card.as_ref()?;
        Some((
            self.my_rom()?,
            kind_of(card.console),
            self.round.my_origin.clone()?,
        ))
    }

    pub(in crate::play) fn group_engine(&self) -> Option<&Rollback> {
        self.round.rollback().filter(|r| r.is_group())
    }

    fn take_group_engine(&mut self) -> Option<Box<Rollback>> {
        if self.round.rollback().is_some_and(Rollback::is_group) {
            let Some(Engine::Rollback(r)) = self.round.engine.take() else {
                return None;
            };
            return Some(r);
        }
        self.stashed.take()
    }
}

impl Room {
    pub(in crate::play) fn grouped_with(&self, me: OnlineId, who: OnlineId) -> bool {
        if let Some(g) = &self.group
            && g.members.contains(&who)
            && !g.gone.contains(&who)
            && self.links.group_of(me).is_some_and(|l| l.holds(who))
        {
            return true;
        }
        self.links
            .group_of(me)
            .is_some_and(|l| l.members().len() > 2 && l.holds(who))
    }

    pub(in crate::play) fn next_group_round(&mut self) -> u32 {
        self.group_rounds = self.group_rounds.wrapping_add(1) % GROUP_ROUND;
        GROUP_ROUND + self.group_rounds
    }

    fn engine_index(&self) -> Option<usize> {
        self.pairs
            .iter()
            .position(|p| p.group_engine().is_some() || p.stashed.is_some())
    }

    pub(in crate::play) fn group_engine_mut(&mut self) -> Option<&mut Rollback> {
        let index = self.engine_index()?;
        let pair = &mut self.pairs[index];
        if pair.round.rollback().is_some_and(Rollback::is_group) {
            return pair.round.rollback_mut();
        }
        pair.stashed.as_deref_mut()
    }

    pub fn group_engine(&self) -> Option<&Rollback> {
        let pair = &self.pairs[self.engine_index()?];
        pair.group_engine().or(pair.stashed.as_deref())
    }

    pub(super) fn lift_group(&mut self) -> Option<Box<Rollback>> {
        let index = self.engine_index()?;
        self.pairs[index].take_group_engine()
    }

    fn seat_group(&mut self, engine: Option<Box<Rollback>>) {
        let Some(engine) = engine else {
            return;
        };
        let start = self.group.as_ref().and_then(|g| g.start.clone());
        let Some(index) = self.view_index() else {
            self.retire_group(Some(engine));
            return;
        };
        let pair = &mut self.pairs[index];
        if let Some(Engine::Rollback(old)) = pair.round.engine.take()
            && !old.is_group()
        {
            pair.round.engine = Some(Engine::Rollback(old));
            pair.stashed = Some(engine);
            return;
        }
        pair.round.engine = Some(Engine::Rollback(engine));
        pair.round.start = start;
        pair.stage = Stage::Playing;
        let shown = pair.friend_lets_watch && pair.watch_friend;
        if let Some(r) = pair.round.rollback_mut() {
            r.watch(shown);
        }
    }

    pub(super) fn retire_group(&mut self, engine: Option<Box<Rollback>>) {
        self.group = None;
        let Some(engine) = engine else {
            return;
        };
        let friend = self.pairs.first().map(|p| p.friend);
        if let (Some(console), Some(friend)) = ((*engine).into_console(true), friend)
            && self.returned.is_none()
        {
            self.returned = Some(Returned {
                console,
                borrowed: None,
                paused: false,
                friend,
                history: None,
            });
            for pair in &mut self.pairs {
                if let Some(mine) = &mut pair.round.mine {
                    mine.running = true;
                }
            }
        }
    }

    pub(in crate::play) fn hear_group(&mut self, from: OnlineId, msg: PlayMsg) -> Option<PlayMsg> {
        match msg {
            PlayMsg::Paused(on) => {
                self.paused.retain(|w| *w != from);
                if on {
                    self.paused.push(from);
                }
                return None;
            }
            PlayMsg::Away(on) => {
                self.away.retain(|w| *w != from);
                if on {
                    self.away.push(from);
                }
                return None;
            }
            PlayMsg::WatchGroup(on) => {
                self.watch_asks.retain(|w| *w != from);
                if on {
                    self.watch_asks.push(from);
                } else if let Some(me) = self.me
                    && let Some(group) = self.links.group_of(me)
                    && group.watchers().contains(&from)
                {
                    let watchers = group
                        .watchers()
                        .iter()
                        .copied()
                        .filter(|w| *w != from)
                        .collect();
                    self.links.set_watchers(me, watchers);
                    self.sent.clear();
                }
                return None;
            }
            _ => {}
        }
        let Some(round) = group_round_of(&msg) else {
            return Some(msg);
        };
        let Some(g) = self.group.as_mut().filter(|g| g.round == round) else {
            if let Some(g) = &mut self.group {
                g.pending.push((from, msg));
                if g.pending.len() > KEPT_PENDING {
                    g.pending.remove(0);
                }
            } else {
                self.early.push((from, msg));
                if self.early.len() > KEPT_PENDING {
                    self.early.remove(0);
                }
            }
            return None;
        };
        match msg {
            PlayMsg::Start(start) => {
                if g.start.is_none() {
                    g.start = Some(start);
                }
            }
            PlayMsg::MemberLeft { unit, at_frame, .. } => {
                g.left.push((unit, at_frame));
            }
            msg => {
                let unit = g.unit_of(from)?;
                match self.group_engine_mut() {
                    Some(r) => r.receive_from(unit, msg),
                    None => {
                        if let Some(g) = &mut self.group {
                            g.pending.push((from, msg));
                        }
                    }
                }
            }
        }
        None
    }

    pub(in crate::play) fn step_group(&mut self, me: OnlineId, now: Instant) {
        let mut engine = self.lift_group();
        let linked = self.links.group_of(me).cloned();
        let continues = |g: &GroupPlay, l: &LinkGroup| {
            l.round() == g.round
                && l.members().len() >= 2
                && l.members().iter().all(|w| g.members.contains(w))
        };
        let wanted = match (&self.group, &linked) {
            (Some(g), Some(l)) if continues(g, l) => Some(false),
            (_, Some(l)) if l.members().len() > 2 => Some(true),
            _ => None,
        };
        match wanted {
            None => {
                if self.group.is_some() {
                    self.retire_group(engine.take());
                }
                return;
            }
            Some(true) => {
                if let Some(old) = engine.take() {
                    self.retire_group(Some(old));
                }
                let Some(l) = &linked else {
                    return;
                };
                let mut group = GroupPlay::new(l);
                let early = std::mem::take(&mut self.early);
                group.pending = early
                    .into_iter()
                    .filter(|(_, m)| group_round_of(m) == Some(group.round))
                    .collect();
                self.group = Some(group);
                self.restart_group_pairs(me, l, now);
                self.replay_pending();
            }
            Some(false) => {}
        }
        let Some(l) = linked else {
            return;
        };
        self.follow_leaves(&l, engine.as_deref_mut());
        if engine.is_none() {
            engine = self.launch_group(me, &l, now);
        }
        if let Some(r) = engine.as_deref_mut() {
            let authority = self
                .group
                .as_ref()
                .and_then(|g| g.unit_of(l.authority()))
                .unwrap_or(0);
            r.set_authority(authority);
        }
        self.seat_group(engine);
        self.replay_pending();
    }

    fn replay_pending(&mut self) {
        if self.group_engine().is_none() {
            return;
        }
        let Some(g) = &mut self.group else {
            return;
        };
        let round = g.round;
        let pending = std::mem::take(&mut g.pending);
        for (from, msg) in pending {
            if group_round_of(&msg) == Some(round) {
                self.hear_group(from, msg);
            }
        }
    }

    fn follow_leaves(&mut self, linked: &LinkGroup, engine: Option<&mut Rollback>) {
        let Some(g) = &mut self.group else {
            return;
        };
        let gone: Vec<OnlineId> = g
            .members
            .iter()
            .copied()
            .filter(|w| !linked.holds(*w) && !g.gone.contains(w))
            .collect();
        let Some(engine) = engine else {
            g.gone.extend(gone);
            return;
        };
        for who in gone {
            let Some(unit) = g.unit_of(who) else {
                continue;
            };
            let told = g.left.iter().find(|(u, _)| *u == unit).map(|(_, at)| *at);
            let live = self.pairs.iter().any(|p| p.friend == who && p.is_live());
            let at = match told {
                Some(at) => at,
                None if live => continue,
                None => 0,
            };
            engine.leave_unit(unit, at);
            g.gone.push(who);
        }
    }

    fn restart_group_pairs(&mut self, me: OnlineId, linked: &LinkGroup, now: Instant) {
        let mine = self
            .pairs
            .iter()
            .find_map(|p| p.round.mine.clone())
            .or_else(|| self.shelved.as_ref().and_then(|s| s.mine.clone()));
        let card = self.my_card();
        let host = linked.authority() == me;
        let sync = links::sync_for(Some(Console::Gba), linked.members().len());
        for pair in self.pairs.iter_mut() {
            if !linked.holds(pair.friend) || !pair.is_live() || !pair.began {
                continue;
            }
            pair.grouped = true;
            pair.apart = false;
            pair.role = if host { Role::Host } else { Role::Guest };
            pair.sync = sync;
            let mine = mine.clone().map(|mut m| {
                m.running = true;
                m
            });
            pair.restart_with(now, mine, card.clone());
        }
    }

    fn group_pairs(&self) -> impl Iterator<Item = &PlaySession> + '_ {
        let group = self.group.as_ref();
        self.pairs.iter().filter(move |p| {
            group.is_some_and(|g| g.playing().any(|w| w == p.friend)) && p.is_live()
        })
    }

    pub(in crate::play) fn share_group_state(&mut self) {
        let Some(g) = &self.group else {
            return;
        };
        let members: Vec<OnlineId> = g.playing().collect();
        let origin = self
            .pairs
            .iter()
            .filter(|p| members.contains(&p.friend))
            .find_map(|p| match &p.round.my_origin {
                Some(Origin::Resume(state)) if p.round.set_up => Some(state.clone()),
                _ => None,
            });
        let Some(state) = origin else {
            return;
        };
        for pair in &mut self.pairs {
            if members.contains(&pair.friend) && pair.wants_state {
                pair.give_state(state.clone(), None);
            }
        }
    }

    fn launch_group(
        &mut self,
        me: OnlineId,
        linked: &LinkGroup,
        now: Instant,
    ) -> Option<Box<Rollback>> {
        let g = self.group.as_ref()?;
        let authority = linked.authority() == me;
        let members = g.members.clone();
        let round = g.round;
        let pairs: Vec<&PlaySession> = self.group_pairs().collect();
        if pairs.len() + 1 < members.len() || !pairs.iter().all(|p| p.group_ready(authority, now)) {
            return None;
        }
        let view = self.view_index().map(|i| &self.pairs[i])?;
        let mine = view.my_setup();
        if view.round.my_card.is_some() && mine.is_none() {
            return None;
        }
        let mut games: [Option<Setup>; GROUP_UNITS] = Default::default();
        for (unit, who) in members.iter().enumerate().take(GROUP_UNITS) {
            games[unit] = if *who == me {
                mine.clone()
            } else {
                pairs.iter().find(|p| p.friend == *who)?.their_setup()
            };
        }
        let trips: Vec<Option<u32>> = pairs.iter().map(|p| p.round_trip_ms()).collect();
        let input_delay = view.input_delay;
        let sample_rate = view.sample_rate;
        let local = members.iter().position(|w| *w == me)?;
        let start = match self.group.as_ref()?.start.clone() {
            Some(start) => start,
            None if authority => {
                let sync = links::sync_for(Some(Console::Gba), members.len());
                let delay = links::group_delay(input_delay, &trips, sync.budget());
                let power_on = games
                    .iter()
                    .take(members.len())
                    .enumerate()
                    .map(|(unit, game)| match game {
                        None => NEVER,
                        Some((_, _, Origin::Resume(_))) => 0,
                        Some((_, _, Origin::Boot(_))) if unit == 0 => 0,
                        Some((_, _, Origin::Boot(_))) => GUEST_POWER_ON,
                    })
                    .collect();
                let start = Start {
                    round,
                    mode: Mode::Rollback,
                    seed: random_seed(),
                    start_time: unix_now(),
                    delay: delay as u8,
                    power_on,
                    plugged: true,
                    start_frame: 0,
                };
                for pair in &mut self.pairs {
                    if members.contains(&pair.friend) && pair.is_live() {
                        pair.outbox.push(PlayMsg::Start(start.clone()));
                    }
                }
                start
            }
            None => return None,
        };
        let sync = links::sync_for(Some(Console::Gba), members.len());
        let authority_unit = members
            .iter()
            .position(|w| *w == linked.authority())
            .unwrap_or(0) as u8;
        let engine = Rollback::group(
            Player::ALL[local],
            start.clone(),
            games,
            sample_rate,
            sync,
            authority_unit,
        )
        .ok()?;
        if let Some(g) = &mut self.group {
            g.start = Some(start);
        }
        Some(Box::new(engine))
    }

    pub(in crate::play) fn flush_group(&mut self) {
        let Some(r) = self.group_engine_mut() else {
            return;
        };
        r.flush(&mut Vec::new());
        let out = r.take_addressed();
        if out.is_empty() {
            return;
        }
        let Some(g) = &self.group else {
            return;
        };
        let members = g.members.clone();
        let playing: Vec<OnlineId> = g.playing().collect();
        let watching = g.watching.clone();
        for (to, msg) in out {
            match to {
                Some(unit) => {
                    if let Some(who) = members.get(usize::from(unit))
                        && let Some(pair) = self.pairs.iter_mut().find(|p| p.friend == *who)
                    {
                        pair.outbox.push(msg);
                    }
                }
                None => {
                    for pair in &mut self.pairs {
                        let to_watcher =
                            watching.contains(&pair.friend) && matches!(msg, PlayMsg::Pads { .. });
                        if (playing.contains(&pair.friend) || to_watcher) && pair.is_live() {
                            pair.outbox.push(msg.clone());
                        }
                    }
                }
            }
        }
    }

    pub(in crate::play) fn leave_group(&mut self, me: OnlineId) {
        let Some(g) = &self.group else {
            return;
        };
        let (Some(unit), round) = (g.unit_of(me), g.round) else {
            return;
        };
        let playing: Vec<OnlineId> = g
            .playing()
            .chain(g.watching.iter().copied())
            .filter(|w| *w != me)
            .collect();
        let Some(at_frame) = self.group_engine().map(Rollback::stop_frame) else {
            return;
        };
        let left = PlayMsg::MemberLeft {
            round,
            unit,
            at_frame,
        };
        for pair in &mut self.pairs {
            if playing.contains(&pair.friend) && pair.is_live() {
                pair.outbox.push(left.clone());
            }
        }
    }

    pub fn group_member(&self, unit: u8) -> Option<OnlineId> {
        self.group.as_ref()?.members.get(usize::from(unit)).copied()
    }

    pub fn group_waiting_for(&self, now: Instant) -> Option<(OnlineId, Duration)> {
        let r = self.group_engine()?;
        let stalled = r.stalled_for(now);
        if stalled < crate::play::STALL_SHOWN {
            return None;
        }
        let who = self.group_member(r.waiting_unit()?)?;
        Some((who, stalled))
    }

    pub fn paused_members(&self) -> &[OnlineId] {
        &self.paused
    }

    pub fn away_members(&self) -> &[OnlineId] {
        &self.away
    }

    pub(in crate::play) fn set_status(&mut self, paused: bool, away: bool) {
        if self.my_status != (paused, away) {
            self.my_status = (paused, away);
            self.status_told.clear();
        }
        let aware: Vec<OnlineId> = self.members.iter().map(|m| m.who).collect();
        for pair in &mut self.pairs {
            let friend = pair.friend;
            if pair.is_live()
                && pair.began
                && aware.contains(&friend)
                && !self.status_told.contains(&friend)
            {
                pair.outbox.push(PlayMsg::Paused(paused));
                pair.outbox.push(PlayMsg::Away(away));
                self.status_told.push(friend);
            }
        }
    }

    pub(in crate::play) fn unlink_member(&mut self, who: OnlineId) -> bool {
        let Some(me) = self.me.filter(|m| *m != who) else {
            return false;
        };
        let Some(g) = &self.group else {
            return false;
        };
        let (Some(unit), round) = (g.unit_of(who), g.round) else {
            return false;
        };
        let Some(at_frame) = self.group_engine().map(Rollback::confirmed_frame) else {
            return false;
        };
        let Some(linked) = self.links.group_of(me).filter(|l| l.holds(who)) else {
            return false;
        };
        let members: Vec<OnlineId> = linked
            .members()
            .iter()
            .copied()
            .filter(|w| *w != who)
            .collect();
        let left = PlayMsg::MemberLeft {
            round,
            unit,
            at_frame,
        };
        let playing: Vec<OnlineId> = g
            .playing()
            .chain(g.watching.iter().copied())
            .filter(|w| *w != me)
            .collect();
        for pair in &mut self.pairs {
            if playing.contains(&pair.friend) && pair.is_live() {
                pair.outbox.push(left.clone());
            }
        }
        if let Some(g) = &mut self.group {
            g.left.push((unit, at_frame));
        }
        let console = self.console();
        self.links.set(members.clone(), console);
        self.sent.clear();
        self.tell_links(&members);
        true
    }
}
