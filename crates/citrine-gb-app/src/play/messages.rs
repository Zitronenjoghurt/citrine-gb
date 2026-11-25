use super::shelf::{Finder, Look};
use super::{
    Ending, Engine, Incoming, KEPT_PINGS, PARTS_PER_UPDATE, PING_EVERY, PlaySession, RESEND_AFTER,
    Role, SETUP_PINGS, SILENCE_LIMIT, Stage, unpacked,
};
use crate::online::Online;
use citrine_gb::gb::serial::LinkMessage;
use citrine_gb_direct::Direct;
use citrine_gb_net::{
    CableByte, ClientMsg, GAME_ACK_BYTES, GAME_PART_BYTES, GAME_WINDOW_BYTES, GamePart, Keys,
    LinkPath, OnlineId, PeerLink, PlayMsg, PublicKey, UNPACED_GAME_BYTES, game_cap, plays_with,
};
use citrine_gb_threads::Threads;
use std::sync::Arc;
use web_time::Instant;

pub(in crate::play) const HELD_BYTES: usize = 64 << 20;
pub(in crate::play) const HELD_MESSAGES: usize = 16384;

pub(in crate::play) fn fits<'a>(
    held: impl ExactSizeIterator<Item = &'a PlayMsg>,
    msg: &PlayMsg,
) -> bool {
    held.len() < HELD_MESSAGES
        && held.map(PlayMsg::held_bytes).sum::<usize>() + msg.held_bytes() <= HELD_BYTES
}

pub(in crate::play) fn keep(
    queue: &mut Vec<(OnlineId, PlayMsg)>,
    from: OnlineId,
    msg: PlayMsg,
    most: usize,
) {
    queue.push((from, msg));
    let mut total: usize = queue.iter().map(|(_, m)| m.held_bytes()).sum();
    let mut cut = 0;
    while queue.len() - cut > most || total > HELD_BYTES {
        total -= queue[cut].1.held_bytes();
        cut += 1;
    }
    queue.drain(..cut);
}

impl PlaySession {
    pub(super) fn receive(&mut self, msg: PlayMsg, finder: &mut Finder, now: Instant) {
        if !self.parked.is_empty() || self.waits_for_shelf(&msg, finder) {
            if !fits(self.parked.iter(), &msg) {
                self.end(Ending::Failed);
                return;
            }
            self.parked.push_back(msg);
            return;
        }
        self.take(msg, finder, now);
    }

    fn waits_for_shelf(&self, msg: &PlayMsg, finder: &mut Finder) -> bool {
        match msg {
            PlayMsg::Join {
                game: Some(game), ..
            } if !self.apart => matches!(self.known_rom(&game.hash, finder), Look::Looking),
            _ => false,
        }
    }

    fn take_parked(&mut self, finder: &mut Finder, now: Instant) {
        while let Some(msg) = self.parked.front() {
            if self.waits_for_shelf(msg, finder) {
                return;
            }
            if let Some(msg) = self.parked.pop_front() {
                self.take(msg, finder, now);
            }
        }
    }

    fn take(&mut self, msg: PlayMsg, finder: &mut Finder, now: Instant) {
        if !self.is_live() {
            return;
        }
        match msg {
            PlayMsg::Join {
                version,
                round,
                game,
            } => {
                if !plays_with(version) {
                    self.end(Ending::Failed);
                    return;
                }
                if round < self.round.number {
                    return;
                }
                if round > self.round.number {
                    let mine = self.round.mine.take();
                    let card = self.round.my_card.clone();
                    self.next_round(round, mine, card);
                }
                if self.round.theirs.is_some() {
                    return;
                }
                if let Some(game) = game.as_ref().filter(|_| !self.apart) {
                    let rom = match self.known_rom(&game.hash, finder) {
                        Look::Found(rom) => Some(rom),
                        Look::Missing | Look::Looking => None,
                    };
                    let holds = rom.is_some();
                    if let Some(rom) = &rom {
                        self.keep_rom(game.hash, rom.clone());
                    }
                    self.round.their_rom = rom;
                    self.round.i_hold_theirs = Some(holds);
                    self.outbox.push(PlayMsg::Holds {
                        game: game.hash,
                        holds,
                    });
                    if !holds && self.sharing.is_some() {
                        self.outbox.push(PlayMsg::Got {
                            game: game.hash,
                            upto: 0,
                        });
                        self.stage = Stage::Receiving {
                            got: 0,
                            total: game.size,
                        };
                    }
                }
                self.round.theirs = Some(game);
            }
            PlayMsg::Holds { game, holds } => {
                if self.round.my_card.as_ref().map(|c| c.hash) != Some(game) {
                    return;
                }
                self.round.they_hold_mine = Some(holds);
                if !holds && self.sharing.is_some() && self.round.mine.is_some() {
                    self.round.sending = Some(0);
                    self.round.acked = None;
                }
            }
            PlayMsg::Setup {
                round,
                from,
                budget,
            } => {
                if round == self.round.number {
                    self.round.their_budget = Some(u32::from(budget));
                    let console = self
                        .round
                        .theirs
                        .as_ref()
                        .and_then(|card| card.as_ref())
                        .map(|card| card.console);
                    match unpacked(from, console) {
                        Some(origin) => self.round.their_origin = Some(origin),
                        None => self.end(Ending::Failed),
                    }
                }
            }
            PlayMsg::DsUser { round, user } => {
                if round == self.round.number {
                    self.round.their_user = Some(user);
                }
            }
            PlayMsg::Ping(n) => self.outbox.push(PlayMsg::Pong(n)),
            PlayMsg::Pong(n) => {
                if let Some(index) = self.pings.iter().position(|(sent, _)| *sent == n) {
                    let (_, at) = self.pings.remove(index).unwrap_or((n, now));
                    let ms = now.duration_since(at).as_millis().min(u32::MAX as u128) as u32;
                    self.round_trips.push_back(ms);
                    while self.round_trips.len() > KEPT_PINGS {
                        self.round_trips.pop_front();
                        self.stale_trips = self.stale_trips.saturating_sub(1);
                    }
                    if self.stale_trips > 0
                        && self.round_trips.len() - self.stale_trips >= SETUP_PINGS
                    {
                        self.round_trips.drain(..self.stale_trips);
                        self.stale_trips = 0;
                    }
                }
            }
            PlayMsg::Start(start) => {
                if self.role == Role::Guest
                    && start.round == self.round.number
                    && self.round.start.is_none()
                    && start.power_on.len() == 2
                    && start.start_frame == 0
                {
                    self.launch(start, now);
                }
            }
            PlayMsg::Watch(on) => {
                self.friend_lets_watch = on;
                let shown = on && self.watch_friend;
                if let Some(r) = self.round.rollback_mut() {
                    r.watch(shown);
                }
            }
            PlayMsg::Cable(plugged) => {
                self.friend_cable = plugged;
                self.join_cables();
            }
            PlayMsg::Bytes(bytes) => {
                if let Some(Engine::Cable(c)) = &mut self.round.engine
                    && c.plugged
                {
                    c.incoming.extend(bytes.into_iter().map(from_byte));
                }
            }
            msg @ (PlayMsg::Pads { .. }
            | PlayMsg::Check { .. }
            | PlayMsg::Resync { .. }
            | PlayMsg::Delay { .. }) => {
                if let Some(r) = self.round.rollback_mut() {
                    r.receive(msg);
                } else if self.round.building.is_some() {
                    if !fits(self.round.early.iter(), &msg) {
                        self.end(Ending::Failed);
                        return;
                    }
                    self.round.early.push(msg);
                }
            }
            msg @ (PlayMsg::LinkRequest(_) | PlayMsg::LinkGroup(_)) => self.links_in.push(msg),
            PlayMsg::Got { game, upto } => {
                let before = self.round.acked.map(|(acked, _)| acked);
                if self.round.my_card.as_ref().map(|c| c.hash) == Some(game)
                    && before.is_none_or(|acked| upto as usize > acked)
                {
                    self.round.acked = Some((upto as usize, now));
                }
            }
            PlayMsg::MemberLeft { .. }
            | PlayMsg::Paused(_)
            | PlayMsg::Away(_)
            | PlayMsg::WatchFrom { .. }
            | PlayMsg::WatchGroup(_)
            | PlayMsg::Direct(_) => {}
            PlayMsg::Rejoin { round, from } => {
                if round == self.round.number {
                    self.round.their_rejoin = Some(from);
                }
            }
        }
    }

    pub(super) fn receive_part(&mut self, part: GamePart, finder: &mut Finder) {
        let Some(Some(theirs)) = &self.round.theirs else {
            return;
        };
        if self.round.i_hold_theirs != Some(false)
            || self.round.checking.is_some()
            || part.hash != theirs.hash
            || part.total != theirs.size
            || self
                .sharing
                .is_none_or(|cap| part.total > game_cap(cap, theirs.console))
        {
            return;
        }
        let incoming = self.round.incoming.get_or_insert_with(|| Incoming {
            hash: part.hash,
            total: part.total,
            data: Vec::with_capacity(part.total as usize),
        });
        if part.offset as usize != incoming.data.len()
            || incoming.data.len() + part.data.len() > incoming.total as usize
        {
            return;
        }
        let before = incoming.data.len() as u32;
        incoming.data.extend_from_slice(&part.data);
        let got = incoming.data.len() as u32;
        if got / GAME_ACK_BYTES != before / GAME_ACK_BYTES || got == incoming.total {
            self.outbox.push(PlayMsg::Got {
                game: part.hash,
                upto: got,
            });
        }
        self.stage = Stage::Receiving {
            got,
            total: incoming.total,
        };
        if got == incoming.total {
            let Some(done) = self.round.incoming.take() else {
                return;
            };
            self.round.checking = Some(done.hash);
            finder.check(done.hash, done.data);
        }
    }

    fn take_checked(&mut self, finder: &mut Finder) {
        let Some(hash) = self.round.checking else {
            return;
        };
        let Some(checked) = finder.checked(&hash) else {
            return;
        };
        self.round.checking = None;
        let Some(data) = checked else {
            self.end(Ending::Failed);
            return;
        };
        self.keep_rom(hash, data.clone());
        self.round.their_rom = Some(data);
        self.round.i_hold_theirs = Some(true);
        self.stage = Stage::Preparing;
        self.outbox.push(PlayMsg::Holds {
            game: hash,
            holds: true,
        });
    }

    pub(super) fn step(&mut self, finder: &mut Finder, threads: &Threads, now: Instant) {
        self.take_parked(finder, now);
        if !self.is_live() || self.stage == Stage::Inviting {
            return;
        }
        self.take_checked(finder);
        self.build_engine(threads, now);
        if !self.is_live() {
            return;
        }
        if now.duration_since(self.heard_at) > SILENCE_LIMIT {
            self.end(Ending::Dropped);
            return;
        }
        if self
            .pinged_at
            .is_none_or(|at| now.duration_since(at) >= PING_EVERY)
        {
            self.ping(now);
        }
        if self.borrowing && self.round.mine.is_none() {
            self.fill_borrowed(finder);
        }
        self.send_parts(now);
        self.seal_parts(threads);
        if self.role == Role::Host && self.round.start.is_none() && !self.grouped {
            self.try_start(now);
        }
    }

    fn fill_borrowed(&mut self, finder: &mut Finder) {
        let Some(card) = self.round.my_card.clone() else {
            return;
        };
        if let Look::Found(rom) = self.known_rom(&card.hash, finder) {
            self.keep_rom(card.hash, rom);
        }
    }

    pub(super) fn send_parts(&mut self, now: Instant) {
        let (Some(offset), Some(mine), Some(card)) =
            (self.round.sending, &self.round.mine, &self.round.my_card)
        else {
            return;
        };
        let total = mine.rom.len();
        if self
            .sharing
            .is_none_or(|cap| total > game_cap(cap, card.console) as usize)
        {
            return;
        }
        let mut offset = offset;
        let limit = match self.round.acked {
            Some((acked, at)) => {
                if offset > acked && now.duration_since(at) >= RESEND_AFTER {
                    offset = acked;
                    self.round.acked = Some((acked, now));
                }
                offset = offset.max(acked);
                total.min(acked + GAME_WINDOW_BYTES as usize)
            }
            None if total > UNPACED_GAME_BYTES as usize => return,
            None => total,
        };
        for _ in 0..PARTS_PER_UPDATE {
            if offset >= limit {
                break;
            }
            let end = (offset + GAME_PART_BYTES).min(total);
            self.game_outbox.push(GamePart {
                hash: card.hash,
                offset: offset as u32,
                total: total as u32,
                data: mine.rom[offset..end].to_vec(),
            });
            offset = end;
        }
        let done = self.round.acked.map_or(offset, |(acked, _)| acked);
        self.round.sending = (done < total).then_some(offset);
    }

    fn seal_parts(&mut self, threads: &Threads) {
        if self.game_outbox.is_empty() {
            return;
        }
        let Some(sealer) = self.sealer.clone() else {
            return;
        };
        let parts = std::mem::take(&mut self.game_outbox);
        self.sealing
            .push_back(threads.background("game parts", move || {
                parts.iter().map(|part| part.seal_for(&sealer)).collect()
            }));
    }

    fn take_sealed(&mut self) {
        while let Some(pending) = self.sealing.front_mut() {
            match pending.try_take() {
                Some(Ok(parts)) => self.sealed.extend(parts),
                Some(Err(_)) => {}
                None => return,
            }
            self.sealing.pop_front();
        }
    }

    pub(super) fn link(&mut self, keys: &Keys, key: &PublicKey) -> &mut PeerLink {
        let sealer = self
            .sealer
            .get_or_insert_with(|| Arc::new(keys.sealer(key)))
            .clone();
        self.link.get_or_insert_with(|| PeerLink::new(sealer))
    }

    pub(super) fn flush(
        &mut self,
        online: &mut Online,
        direct: &mut Direct,
        want: bool,
        aware: bool,
        now: Instant,
    ) {
        let Some(id) = self.id else {
            return;
        };
        self.settle_frames();
        if let Some(r) = self.round.rollback_mut() {
            r.flush(&mut self.outbox);
        }
        let mut offerer = false;
        if let (Some(keys), Some(key)) = (online.keys(), self.key(online)) {
            offerer = keys.public_key().0 < key.0;
            self.link(keys, &key);
        }
        self.take_sealed();
        let want = want && self.is_live();
        let Some(link) = &mut self.link else {
            return;
        };
        self.direct
            .step(want, offerer, link, direct, &mut self.outbox, now);
        for msg in std::mem::take(&mut self.outbox) {
            link.send(&msg, now);
        }
        let to = aware.then_some(self.friend);
        for (path, sealed) in link.poll(now) {
            if path == LinkPath::Direct {
                if let Ok(frame) = postcard::to_allocvec(&sealed) {
                    direct.send(self.direct.tag(), frame);
                }
                continue;
            }
            online.send(match to {
                Some(to) => ClientMsg::RoomPlay {
                    room: id,
                    to,
                    sealed,
                },
                None => ClientMsg::Play {
                    session: id,
                    sealed,
                },
            });
        }
        for sealed in std::mem::take(&mut self.sealed) {
            online.send(match to {
                Some(to) => ClientMsg::RoomGame {
                    room: id,
                    to,
                    sealed,
                },
                None => ClientMsg::Game {
                    session: id,
                    sealed,
                },
            });
        }
    }
}

pub(super) fn to_byte(msg: LinkMessage) -> CableByte {
    match msg {
        LinkMessage::Transfer {
            id,
            byte,
            seq,
            since,
        } => CableByte::Transfer {
            id,
            byte,
            seq,
            since,
        },
        LinkMessage::Reply { id, byte } => CableByte::Reply { id, byte },
    }
}

fn from_byte(byte: CableByte) -> LinkMessage {
    match byte {
        CableByte::Transfer {
            id,
            byte,
            seq,
            since,
        } => LinkMessage::Transfer {
            id,
            byte,
            seq,
            since,
        },
        CableByte::Reply { id, byte } => LinkMessage::Reply { id, byte },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resync(mib: usize) -> PlayMsg {
        PlayMsg::Resync {
            round: 1,
            frame: 0,
            unit: 0,
            data: vec![0; mib << 20],
            cable: None,
        }
    }

    #[test]
    fn held_play_messages_stop_at_a_byte_budget() {
        let friend = OnlineId::new(7).unwrap();
        let mut queue = Vec::new();
        for _ in 0..3 {
            keep(&mut queue, friend, resync(30), 1024);
        }
        assert_eq!(queue.len(), 2);
        let held: Vec<PlayMsg> = queue.into_iter().map(|(_, m)| m).collect();
        assert!(fits(held.iter().take(1), &resync(30)));
        assert!(!fits(held.iter(), &resync(30)));
    }

    #[test]
    fn held_play_messages_stop_at_a_count() {
        let held = vec![PlayMsg::Ping(0); HELD_MESSAGES];
        assert!(fits(held.iter().skip(1), &PlayMsg::Ping(0)));
        assert!(!fits(held.iter(), &PlayMsg::Ping(0)));
    }
}
