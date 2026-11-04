use super::{
    Ending, Engine, Incoming, KEPT_PINGS, PARTS_PER_UPDATE, PING_EVERY, PlaySession, Role,
    SILENCE_LIMIT, Shelf, Stage, sha1_of, unpacked,
};
use crate::online::Online;
use citrine_gb::gb::serial::LinkMessage;
use citrine_gb_net::{
    CableByte, ClientMsg, GAME_PART_BYTES, GamePart, MAX_GAME_BYTES, OnlineId, PlayMsg, PublicKey,
    SessionId, plays_with,
};
use web_time::Instant;

impl PlaySession {
    pub(super) fn receive(&mut self, msg: PlayMsg, shelf: &dyn Shelf, now: Instant) {
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
                    let rom = self.known_rom(&game.hash, shelf);
                    let holds = rom.is_some();
                    if let Some(rom) = &rom {
                        self.keep_rom(game.hash, rom);
                    }
                    self.round.their_rom = rom;
                    self.round.i_hold_theirs = Some(holds);
                    self.outbox.push(PlayMsg::Holds {
                        game: game.hash,
                        holds,
                    });
                    if !holds && self.sharing {
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
                if !holds && self.sharing && self.round.mine.is_some() {
                    self.round.sending = Some(0);
                }
            }
            PlayMsg::Setup { round, from } => {
                if round == self.round.number {
                    match unpacked(from) {
                        Some(origin) => self.round.their_origin = Some(origin),
                        None => self.end(Ending::Failed),
                    }
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
            msg @ (PlayMsg::Pads { .. } | PlayMsg::Check { .. } | PlayMsg::Resync { .. }) => {
                if let Some(r) = self.round.rollback_mut() {
                    r.receive(msg);
                }
            }
            msg @ (PlayMsg::LinkRequest(_) | PlayMsg::LinkGroup(_)) => self.links_in.push(msg),
            PlayMsg::MemberLeft { .. }
            | PlayMsg::Paused(_)
            | PlayMsg::Away(_)
            | PlayMsg::WatchFrom { .. }
            | PlayMsg::WatchGroup(_) => {}
            PlayMsg::Rejoin { round, from } => {
                if round == self.round.number {
                    self.round.their_rejoin = Some(from);
                }
            }
        }
    }

    pub(super) fn receive_part(&mut self, part: GamePart) {
        let Some(Some(theirs)) = &self.round.theirs else {
            return;
        };
        if self.round.i_hold_theirs != Some(false)
            || part.hash != theirs.hash
            || part.total != theirs.size
            || part.total > MAX_GAME_BYTES
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
        incoming.data.extend_from_slice(&part.data);
        let got = incoming.data.len() as u32;
        self.stage = Stage::Receiving {
            got,
            total: incoming.total,
        };
        if got == incoming.total {
            let Some(done) = self.round.incoming.take() else {
                return;
            };
            if sha1_of(&done.data) == Some(done.hash) {
                self.keep_rom(done.hash, &done.data);
                self.round.their_rom = Some(done.data);
                self.round.i_hold_theirs = Some(true);
                self.stage = Stage::Preparing;
                self.outbox.push(PlayMsg::Holds {
                    game: done.hash,
                    holds: true,
                });
            } else {
                self.end(Ending::Failed);
            }
        }
    }

    pub(super) fn step(&mut self, shelf: &dyn Shelf, now: Instant) {
        if !self.is_live() || self.stage == Stage::Inviting {
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
            self.fill_borrowed(shelf);
        }
        self.send_parts();
        if self.role == Role::Host && self.round.start.is_none() && !self.grouped {
            self.try_start(now);
        }
    }

    fn fill_borrowed(&mut self, shelf: &dyn Shelf) {
        let Some(card) = self.round.my_card.clone() else {
            return;
        };
        if let Some(rom) = self.known_rom(&card.hash, shelf) {
            self.keep_rom(card.hash, &rom);
        }
    }

    fn send_parts(&mut self) {
        let (Some(offset), Some(mine), Some(card)) =
            (self.round.sending, &self.round.mine, &self.round.my_card)
        else {
            return;
        };
        let total = mine.rom.len();
        let mut offset = offset;
        for _ in 0..PARTS_PER_UPDATE {
            if offset >= total {
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
        self.round.sending = (offset < total).then_some(offset);
    }

    pub(super) fn flush(&mut self, online: &mut Online, aware: bool) {
        let Some(id) = self.id else {
            return;
        };
        if let Some(r) = self.round.rollback_mut() {
            r.flush(&mut self.outbox);
        }
        if self.outbox.is_empty() && self.game_outbox.is_empty() {
            return;
        }
        let Some(key) = self.key(online) else {
            return;
        };
        send_all(
            online,
            id,
            aware.then_some(self.friend),
            &key,
            std::mem::take(&mut self.outbox),
            std::mem::take(&mut self.game_outbox),
        );
    }
}

fn send_all(
    online: &mut Online,
    session: SessionId,
    to: Option<OnlineId>,
    key: &PublicKey,
    msgs: Vec<PlayMsg>,
    parts: Vec<GamePart>,
) {
    let sealed: Vec<ClientMsg> = match online.keys() {
        Some(keys) => msgs
            .iter()
            .map(|msg| {
                let sealed = msg.seal(keys, key);
                match to {
                    Some(to) => ClientMsg::RoomPlay {
                        room: session,
                        to,
                        sealed,
                    },
                    None => ClientMsg::Play { session, sealed },
                }
            })
            .chain(parts.iter().map(|part| {
                let sealed = part.seal(keys, key);
                match to {
                    Some(to) => ClientMsg::RoomGame {
                        room: session,
                        to,
                        sealed,
                    },
                    None => ClientMsg::Game { session, sealed },
                }
            }))
            .collect(),
        None => return,
    };
    for msg in sealed {
        online.send(msg);
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
