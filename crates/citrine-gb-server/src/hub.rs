mod room;

pub use room::{Out, Person, ROOM_PROTOCOL, To};

use citrine_gb_net::{
    EndReason, ErrorCode, Notice, OnlineId, Presence, Sealed, ServerMsg, SessionId,
};
use room::{Member, Room, is_room_msg};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, watch};

pub const QUEUE: usize = 512;

pub struct Queued {
    pub bytes: Vec<u8>,
    pub room: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub struct Outbox {
    tx: mpsc::Sender<Queued>,
    room: Arc<Semaphore>,
    max_bytes: u32,
}

fn encode(msg: &ServerMsg) -> Option<Vec<u8>> {
    match msg.encode() {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            log::error!("dropped a message the protocol can't carry: {e}");
            None
        }
    }
}

impl Outbox {
    pub fn new(max_bytes: u32) -> (Self, mpsc::Receiver<Queued>) {
        let (tx, rx) = mpsc::channel(QUEUE);
        let outbox = Self {
            tx,
            room: Arc::new(Semaphore::new(max_bytes as usize)),
            max_bytes,
        };
        (outbox, rx)
    }

    fn need(&self, bytes: &[u8]) -> u32 {
        (bytes.len() as u32).clamp(1, self.max_bytes)
    }

    pub async fn push(&self, msg: ServerMsg) -> bool {
        let Some(bytes) = encode(&msg) else {
            return true;
        };
        let need = self.need(&bytes);
        let Ok(room) = self.room.clone().acquire_many_owned(need).await else {
            return false;
        };
        self.tx.send(Queued { bytes, room }).await.is_ok()
    }

    pub fn try_push(&self, msg: ServerMsg) -> bool {
        let Some(bytes) = encode(&msg) else {
            return true;
        };
        let need = self.need(&bytes);
        let Ok(room) = self.room.clone().try_acquire_many_owned(need) else {
            return false;
        };
        self.tx.try_send(Queued { bytes, room }).is_ok()
    }
}

pub const MAX_DEVICES_ONLINE: usize = 8;

pub struct Conn {
    pub conn_id: u64,
    pub device: i64,
    pub outbox: Outbox,
    pub close: watch::Sender<Option<Notice>>,
    pub presence: Presence,
    pub ip: IpAddr,
    pub protocol: u16,
    pub person: Option<Person>,
}

impl Conn {
    fn close(&self, notice: Notice) {
        let _ = self.close.send(Some(notice));
    }

    fn push(&self, msg: ServerMsg) {
        if !self.outbox.try_push(msg) {
            self.close(Notice::Disconnected);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Side {
    pub user: i64,
    pub conn: Option<u64>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Left {
    pub out: Out,
    pub dropped: Vec<(SessionId, u32)>,
    pub last: bool,
}

fn merged(conns: &[Conn]) -> Option<Presence> {
    if conns.is_empty() {
        return None;
    }
    let away = conns
        .iter()
        .all(|c| matches!(c.presence, Presence::Online { away: true }));
    Some(Presence::Online { away })
}

#[derive(Default)]
struct State {
    conns: HashMap<i64, Vec<Conn>>,
    rooms: HashMap<SessionId, Room>,
}

pub struct Hub {
    state: Mutex<State>,
    next_conn: AtomicU64,
    gate: broadcast::Sender<Notice>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            next_conn: AtomicU64::new(1),
            gate: broadcast::channel(16).0,
        }
    }
}

fn random_session() -> SessionId {
    let mut bytes = [0; 8];
    getrandom::getrandom(&mut bytes).expect("no system random source");
    u64::from_le_bytes(bytes)
}

impl Hub {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn conn_id(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    pub fn gate(&self) -> broadcast::Receiver<Notice> {
        self.gate.subscribe()
    }

    pub fn close_all(&self, notice: Notice) {
        let _ = self.gate.send(notice);
    }

    pub fn join(&self, user: i64, conn: Conn) -> usize {
        let mut state = self.state();
        let conns = state.conns.entry(user).or_default();
        for old in conns.iter().filter(|old| old.device == conn.device) {
            old.close(Notice::SignedInElsewhere);
        }
        conns.push(conn);
        let open = conns.len();
        for old in conns.iter().take(open.saturating_sub(MAX_DEVICES_ONLINE)) {
            old.close(Notice::SignedInElsewhere);
        }
        state.conns.len()
    }

    pub fn leave(&self, user: i64, conn_id: u64) -> Option<Left> {
        let mut state = self.state();
        let conns = state.conns.get_mut(&user)?;
        let at = conns.iter().position(|c| c.conn_id == conn_id)?;
        conns.remove(at);
        let last = conns.is_empty();
        if last {
            state.conns.remove(&user);
        }
        let mut out = Vec::new();
        let mut dropped = Vec::new();
        state.rooms.retain(|&id, room| {
            if let Some(at) = room.holder(user, conn_id)
                && let Some(drops) = room.drop_member(id, at, &mut out)
            {
                dropped.push((id, drops));
            }
            if last {
                let before = room.invites.len();
                let gone: Vec<_> = room
                    .invites
                    .iter()
                    .filter(|i| i.to == user)
                    .copied()
                    .collect();
                room.invites.retain(|i| i.to != user);
                for invite in gone {
                    if !invite.room && !room.started && room.invites.is_empty() {
                        out.push((
                            To::Conn(invite.from, invite.from_conn),
                            ServerMsg::SessionEnded {
                                session: id,
                                reason: EndReason::Disconnected,
                            },
                        ));
                        room.members.clear();
                    }
                }
                if before != room.invites.len() && !room.started && room.invites.is_empty() {
                    room.members.clear();
                }
            }
            !room.members.is_empty()
        });
        Some(Left { out, dropped, last })
    }

    pub fn send(&self, user: i64, msg: ServerMsg) -> bool {
        let state = self.state();
        let Some(conns) = state.conns.get(&user) else {
            return false;
        };
        for conn in conns {
            conn.push(msg.clone());
        }
        true
    }

    pub fn send_to(&self, side: Side, msg: ServerMsg) -> bool {
        match side.conn {
            Some(conn) => self.send_conn(side.user, conn, msg),
            None => self.send(side.user, msg),
        }
    }

    pub fn send_conn(&self, user: i64, conn_id: u64, msg: ServerMsg) -> bool {
        let state = self.state();
        let Some(conn) = state
            .conns
            .get(&user)
            .and_then(|conns| conns.iter().find(|c| c.conn_id == conn_id))
        else {
            return false;
        };
        conn.push(msg);
        true
    }

    pub fn send_others(&self, user: i64, conn_id: u64, msg: ServerMsg) {
        let state = self.state();
        for conn in state.conns.get(&user).into_iter().flatten() {
            if conn.conn_id != conn_id {
                conn.push(msg.clone());
            }
        }
    }

    pub fn send_each(&self, user: i64, msg: impl Fn(i64) -> ServerMsg) {
        let state = self.state();
        for conn in state.conns.get(&user).into_iter().flatten() {
            conn.push(msg(conn.device));
        }
    }

    pub fn is_online(&self, user: i64) -> bool {
        self.state().conns.contains_key(&user)
    }

    pub fn presence(&self, user: i64) -> Option<Presence> {
        merged(self.state().conns.get(&user)?)
    }

    pub fn set_presence(&self, user: i64, conn_id: u64, presence: Presence) -> Option<Presence> {
        let mut state = self.state();
        let conns = state.conns.get_mut(&user)?;
        let before = merged(conns);
        if let Some(conn) = conns.iter_mut().find(|c| c.conn_id == conn_id) {
            conn.presence = presence;
        }
        let after = merged(conns);
        (after != before).then_some(after).flatten()
    }

    pub fn online_count(&self) -> usize {
        self.state().conns.len()
    }

    pub fn online(&self) -> HashMap<i64, Vec<IpAddr>> {
        self.state()
            .conns
            .iter()
            .map(|(user, conns)| (*user, conns.iter().map(|c| c.ip).collect()))
            .collect()
    }

    pub fn online_devices(&self, user: i64) -> HashSet<i64> {
        self.state()
            .conns
            .get(&user)
            .into_iter()
            .flatten()
            .map(|c| c.device)
            .collect()
    }

    pub fn kick_address(&self, key: IpAddr, notice: Notice) -> usize {
        let state = self.state();
        let mut kicked = 0;
        for conn in state.conns.values().flatten() {
            if crate::connections::address_key(conn.ip) == key {
                conn.close(notice);
                kicked += 1;
            }
        }
        kicked
    }

    pub fn kick(&self, user: i64, notice: Notice) -> bool {
        let state = self.state();
        let Some(conns) = state.conns.get(&user) else {
            return false;
        };
        for conn in conns {
            conn.close(notice);
        }
        true
    }

    pub fn kick_device(&self, user: i64, device: i64, notice: Notice) -> bool {
        let state = self.state();
        let mut kicked = false;
        for conn in state.conns.get(&user).into_iter().flatten() {
            if conn.device == device {
                conn.close(notice);
                kicked = true;
            }
        }
        kicked
    }

    fn joining(&self, state: &State, user: i64, conn: u64, rooms: bool) -> Member {
        let found = state
            .conns
            .get(&user)
            .and_then(|conns| conns.iter().find(|c| c.conn_id == conn));
        let person = found.and_then(|c| c.person.clone());
        let protocol = found.map_or(citrine_gb_net::OLDEST_PROTOCOL_VERSION, |c| c.protocol);
        Member::new(user, conn, person, protocol, rooms)
    }

    fn online_id(state: &State, user: i64) -> Option<OnlineId> {
        state
            .conns
            .get(&user)?
            .iter()
            .find_map(|c| c.person.as_ref().map(|p| p.online_id))
    }

    pub fn deliver(&self, out: Out) {
        let state = self.state();
        for (to, msg) in out {
            let (user, conn) = match to {
                To::Conn(user, conn) => (user, Some(conn)),
                To::User(user) => (user, None),
            };
            for c in state.conns.get(&user).into_iter().flatten() {
                if conn.is_some_and(|id| id != c.conn_id) {
                    continue;
                }
                if c.protocol < ROOM_PROTOCOL && is_room_msg(&msg) {
                    continue;
                }
                c.push(msg.clone());
            }
        }
    }

    pub fn invite(
        &self,
        host: i64,
        host_conn: u64,
        guest: i64,
        max: usize,
    ) -> Option<(SessionId, Option<SessionId>)> {
        let mut state = self.state();
        let replaced = state
            .rooms
            .iter()
            .find(|(_, r)| {
                r.hosts_pending(host) && r.invites.iter().any(|i| i.to == guest && !i.room)
            })
            .map(|(&id, _)| id);
        if let Some(old) = replaced {
            state.rooms.remove(&old);
        }
        let pending = state
            .rooms
            .values()
            .filter(|r| r.hosts_pending(host))
            .count();
        if pending >= max {
            return None;
        }
        let mut id = random_session();
        while state.rooms.contains_key(&id) {
            id = random_session();
        }
        let member = self.joining(&state, host, host_conn, false);
        state.rooms.insert(id, Room::pair(member, guest));
        Some((id, replaced))
    }

    pub fn room_users(&self, id: SessionId) -> Vec<i64> {
        let state = self.state();
        state
            .rooms
            .get(&id)
            .map(|r| r.members.iter().map(|m| m.user).collect())
            .unwrap_or_default()
    }

    #[expect(clippy::too_many_arguments, reason = "one room invite")]
    pub fn room_invite(
        &self,
        id: SessionId,
        user: i64,
        conn: u64,
        to: i64,
        from: OnlineId,
        sealed: Sealed,
        max: usize,
    ) -> Result<Out, ErrorCode> {
        let mut guard = self.state();
        let state = &mut *guard;
        let room = state.rooms.get_mut(&id).ok_or(ErrorCode::UnknownSession)?;
        let at = room
            .holder(user, conn)
            .filter(|&at| room.members[at].hears_rooms())
            .ok_or(ErrorCode::UnknownSession)?;
        if room.members.iter().any(|m| m.user == to) {
            return Err(ErrorCode::CantJoin);
        }
        if room.members.len() >= max {
            return Err(ErrorCode::RoomFull);
        }
        let grows = room.members.len() >= 2;
        let conns: Vec<u64> = state
            .conns
            .get(&to)
            .into_iter()
            .flatten()
            .filter(|c| !grows || c.protocol >= ROOM_PROTOCOL)
            .map(|c| c.conn_id)
            .collect();
        if room.has_old() || conns.is_empty() {
            return Err(ErrorCode::NeedsUpdate);
        }
        room.invites.retain(|i| i.to != to);
        if room.invites.len() >= max {
            return Err(ErrorCode::RateLimited);
        }
        room.members[at].rooms = true;
        room.invites.push(room::Invite {
            from: user,
            from_conn: conn,
            to,
            room: true,
        });
        let invite = ServerMsg::Invite {
            session: id,
            from,
            sealed,
        };
        Ok(conns
            .into_iter()
            .map(|c| (To::Conn(to, c), invite.clone()))
            .collect())
    }

    pub fn answer(
        &self,
        id: SessionId,
        guest: i64,
        guest_conn: u64,
        accept: bool,
        blocked: bool,
        max: usize,
    ) -> Result<Out, ErrorCode> {
        let mut guard = self.state();
        let joining = self.joining(&guard, guest, guest_conn, true);
        let guest_online = Self::online_id(&guard, guest);
        let state = &mut *guard;
        let room = state.rooms.get_mut(&id).ok_or(ErrorCode::UnknownSession)?;
        let mut out = Vec::new();
        if let Some(at) = room.members.iter().position(|m| m.user == guest) {
            if !accept || !room.members[at].rooms || !joining.hears_rooms() {
                return Err(ErrorCode::UnknownSession);
            }
            let member = &mut room.members[at];
            if let (Some(old), Some(who)) = (member.conn, member.who())
                && old != guest_conn
            {
                out.push((
                    To::Conn(guest, old),
                    ServerMsg::MemberGone {
                        room: id,
                        who,
                        reason: EndReason::Left,
                    },
                ));
            }
            member.conn = Some(guest_conn);
            member.protocol = joining.protocol;
            member.person = joining.person.clone().or(member.person.take());
            member.state.reconnecting = false;
            member.drops += 1;
            out.push((
                To::Conn(guest, guest_conn),
                ServerMsg::RoomJoined {
                    room: id,
                    members: room.cards(),
                },
            ));
            room.tell_others(
                at,
                |card| ServerMsg::MemberUpdate {
                    room: id,
                    member: card.clone(),
                },
                &mut out,
            );
            return Ok(out);
        }
        let at = room
            .invites
            .iter()
            .position(|i| i.to == guest)
            .ok_or(ErrorCode::UnknownSession)?;
        if !accept {
            let invite = room.invites.remove(at);
            if invite.room {
                if let Some(who) = guest_online
                    && let Some(to) = room
                        .members
                        .iter()
                        .find(|m| m.user == invite.from)
                        .and_then(Member::to)
                {
                    out.push((
                        to,
                        ServerMsg::MemberGone {
                            room: id,
                            who,
                            reason: EndReason::Declined,
                        },
                    ));
                }
            } else {
                out.push((
                    To::Conn(invite.from, invite.from_conn),
                    ServerMsg::SessionEnded {
                        session: id,
                        reason: EndReason::Declined,
                    },
                ));
            }
            if !room.started && room.invites.is_empty() {
                state.rooms.remove(&id);
            }
            return Ok(out);
        }
        if blocked {
            room.invites.remove(at);
            return Err(ErrorCode::CantJoin);
        }
        if room.members.len() >= max {
            room.invites.remove(at);
            return Err(ErrorCode::RoomFull);
        }
        let invite = room.invites[at];
        let mut member = joining;
        member.rooms = member.rooms && invite.room;
        if room.has_old() && room.members.len() >= 2 || !member.rooms && room.members.len() != 1 {
            return Err(ErrorCode::NeedsUpdate);
        }
        room.invites.remove(at);
        room.started = true;
        let rooms = member.rooms;
        let who = member.who();
        room.members.push(member);
        let at = room.members.len() - 1;
        if rooms {
            out.push((
                To::Conn(guest, guest_conn),
                ServerMsg::RoomJoined {
                    room: id,
                    members: room.cards(),
                },
            ));
            room.tell_others(
                at,
                |card| ServerMsg::MemberUpdate {
                    room: id,
                    member: card.clone(),
                },
                &mut out,
            );
            return Ok(out);
        }
        let host = &room.members[0];
        if let Some(with) = host.who() {
            out.push((
                To::Conn(guest, guest_conn),
                ServerMsg::SessionStarted { session: id, with },
            ));
        }
        if host.rooms {
            room.tell_others(
                at,
                |card| ServerMsg::MemberUpdate {
                    room: id,
                    member: card.clone(),
                },
                &mut out,
            );
        } else if let (Some(to), Some(with)) = (host.to(), who) {
            out.push((to, ServerMsg::SessionStarted { session: id, with }));
            if host.hears_rooms()
                && !room.members[at].hears_rooms()
                && let Some(card) = room.members[at].card()
            {
                out.push((
                    to,
                    ServerMsg::MemberUpdate {
                        room: id,
                        member: card,
                    },
                ));
            }
        }
        Ok(out)
    }

    pub fn end(&self, id: SessionId, user: i64, conn_id: u64) -> Option<Out> {
        let mut guard = self.state();
        let who = Self::online_id(&guard, user);
        let state = &mut *guard;
        let room = state.rooms.get_mut(&id)?;
        let mut out = Vec::new();
        if let Some(at) = room.holder(user, conn_id) {
            room.depart(id, at, EndReason::Left, &mut out);
        } else if room.members.iter().any(|m| m.user == user) {
            return None;
        } else {
            let at = room.invites.iter().position(|i| i.to == user)?;
            let invite = room.invites.remove(at);
            if !invite.room {
                out.push((
                    To::Conn(invite.from, invite.from_conn),
                    ServerMsg::SessionEnded {
                        session: id,
                        reason: EndReason::Left,
                    },
                ));
            } else if let Some(who) = who
                && let Some(to) = room
                    .members
                    .iter()
                    .find(|m| m.user == invite.from)
                    .and_then(Member::to)
            {
                out.push((
                    to,
                    ServerMsg::MemberGone {
                        room: id,
                        who,
                        reason: EndReason::Declined,
                    },
                ));
            }
            if !room.started && room.invites.is_empty() {
                room.members.clear();
            }
        }
        if room.members.is_empty() {
            state.rooms.remove(&id);
        }
        Some(out)
    }

    pub fn expire(&self, id: SessionId, user: i64, drops: u32) -> Out {
        let mut state = self.state();
        let mut out = Vec::new();
        let Some(room) = state.rooms.get_mut(&id) else {
            return out;
        };
        if let Some(at) = room
            .members
            .iter()
            .position(|m| m.user == user && m.conn.is_none() && m.drops == drops)
        {
            room.depart(id, at, EndReason::Disconnected, &mut out);
        }
        if room.members.is_empty() {
            state.rooms.remove(&id);
        }
        out
    }

    pub fn end_between(&self, a: i64, b: i64) -> Vec<SessionId> {
        let mut state = self.state();
        let mut ended = Vec::new();
        state.rooms.retain(|&id, room| {
            let mut users: Vec<i64> = room.members.iter().map(|m| m.user).collect();
            users.extend(room.invites.iter().filter(|i| !i.room).map(|i| i.to));
            users.sort_unstable();
            users.dedup();
            let pair = users.len() == 2 && users.contains(&a) && users.contains(&b);
            if pair {
                ended.push(id);
            }
            !pair
        });
        ended
    }

    pub fn peer(&self, id: SessionId, user: i64, conn_id: u64) -> Option<Side> {
        let state = self.state();
        let room = state.rooms.get(&id).filter(|r| r.started)?;
        if room.members.len() != 2 {
            return None;
        }
        let at = room.holder(user, conn_id)?;
        let other = &room.members[1 - at];
        Some(Side {
            user: other.user,
            conn: Some(other.conn?),
        })
    }

    pub fn route(
        &self,
        id: SessionId,
        user: i64,
        conn: u64,
        to: OnlineId,
    ) -> Result<(OnlineId, Option<To>), ErrorCode> {
        let mut state = self.state();
        let room = state.rooms.get_mut(&id).ok_or(ErrorCode::UnknownSession)?;
        let at = room
            .holder(user, conn)
            .filter(|&at| room.members[at].hears_rooms())
            .ok_or(ErrorCode::UnknownSession)?;
        room.members[at].rooms = true;
        let from = room.members[at].who().ok_or(ErrorCode::UnknownSession)?;
        let target = room
            .members
            .iter()
            .find(|m| m.who() == Some(to) && m.user != user)
            .ok_or(ErrorCode::UnknownSession)?;
        if !target.hears_rooms() {
            return Err(ErrorCode::UnknownSession);
        }
        Ok((from, target.to()))
    }

    pub fn update(
        &self,
        id: SessionId,
        user: i64,
        conn: u64,
        shows_screen: bool,
        playing: bool,
        max_players: usize,
    ) -> Result<Out, ErrorCode> {
        let mut state = self.state();
        let room = state.rooms.get_mut(&id).ok_or(ErrorCode::UnknownSession)?;
        let at = room
            .holder(user, conn)
            .filter(|&at| room.members[at].hears_rooms())
            .ok_or(ErrorCode::UnknownSession)?;
        if playing && !room.members[at].state.playing && room.players() >= max_players {
            return Err(ErrorCode::RoomFull);
        }
        let member = &mut room.members[at];
        member.rooms = true;
        member.state.shows_screen = shows_screen;
        member.state.playing = playing;
        let mut out = Vec::new();
        room.tell_others(
            at,
            |card| ServerMsg::MemberUpdate {
                room: id,
                member: card.clone(),
            },
            &mut out,
        );
        Ok(out)
    }

    pub fn room_chat(
        &self,
        id: SessionId,
        user: i64,
        conn: u64,
        copies: Vec<(OnlineId, Sealed)>,
        at: u64,
    ) -> Result<Out, ErrorCode> {
        let mut state = self.state();
        let room = state.rooms.get_mut(&id).ok_or(ErrorCode::UnknownSession)?;
        let me = room
            .holder(user, conn)
            .filter(|&at| room.members[at].hears_rooms())
            .ok_or(ErrorCode::UnknownSession)?;
        room.members[me].rooms = true;
        let from = room.members[me].who().ok_or(ErrorCode::UnknownSession)?;
        let mut out: Out = Vec::new();
        for (who, sealed) in copies {
            let Some(member) = room
                .members
                .iter()
                .find(|m| m.who() == Some(who) && m.user != user && m.hears_rooms())
            else {
                continue;
            };
            let Some(to) = member.to() else {
                continue;
            };
            if out.iter().any(|(t, _)| *t == to) {
                continue;
            }
            out.push((
                to,
                ServerMsg::RoomChat {
                    room: id,
                    from,
                    at,
                    sealed,
                },
            ));
        }
        Ok(out)
    }

    pub fn rename(&self, user: i64, name: &str) {
        let mut guard = self.state();
        let state = &mut *guard;
        for conn in state.conns.get_mut(&user).into_iter().flatten() {
            if let Some(person) = conn.person.as_mut() {
                person.display_name = name.to_owned();
            }
        }
        for room in state.rooms.values_mut() {
            for member in room.members.iter_mut().filter(|m| m.user == user) {
                if let Some(person) = member.person.as_mut() {
                    person.display_name = name.to_owned();
                }
            }
        }
    }

    pub fn session_count(&self) -> usize {
        let state = self.state();
        state.rooms.values().filter(|r| r.started).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(
        hub: &Hub,
    ) -> (
        Conn,
        mpsc::Receiver<Queued>,
        watch::Receiver<Option<Notice>>,
    ) {
        let (outbox, rx) = Outbox::new(1 << 20);
        let (close, close_rx) = watch::channel(None);
        let conn_id = hub.conn_id();
        (
            Conn {
                conn_id,
                device: conn_id as i64,
                outbox,
                close,
                presence: Presence::Online { away: false },
                ip: IpAddr::from([127, 0, 0, 1]),
                protocol: citrine_gb_net::OLDEST_PROTOCOL_VERSION,
                person: None,
            },
            rx,
            close_rx,
        )
    }

    #[test]
    fn two_devices_stay_online_and_presence_shows_once() {
        let hub = Hub::default();
        let (phone, _rx1, close1) = conn(&hub);
        let (pc, _rx2, _close2) = conn(&hub);
        let (phone_id, pc_id) = (phone.conn_id, pc.conn_id);
        assert_eq!(hub.join(7, phone), 1);
        assert_eq!(hub.join(7, pc), 1);
        assert_eq!(*close1.borrow(), None);
        assert_eq!(hub.online_count(), 1);
        assert_eq!(hub.online()[&7].len(), 2);
        assert_eq!(hub.presence(7), Some(Presence::Online { away: false }));
        assert_eq!(
            hub.set_presence(7, phone_id, Presence::Online { away: true }),
            None
        );
        assert_eq!(
            hub.set_presence(7, pc_id, Presence::Online { away: true }),
            Some(Presence::Online { away: true })
        );
        assert_eq!(
            hub.leave(7, phone_id),
            Some(Left {
                out: vec![],
                dropped: vec![],
                last: false
            })
        );
        assert!(hub.is_online(7));
        assert_eq!(hub.leave(7, phone_id), None);
        assert_eq!(hub.leave(7, pc_id).map(|l| l.last), Some(true));
        assert!(!hub.is_online(7));
    }

    #[test]
    fn the_same_device_twice_closes_the_older_connection() {
        let hub = Hub::default();
        let (first, _rx, close) = conn(&hub);
        let (other, _other_rx, other_close) = conn(&hub);
        let (again, _again_rx, again_close) = conn(&hub);
        let again = Conn {
            device: first.device,
            ..again
        };
        hub.join(7, first);
        hub.join(7, other);
        hub.join(7, again);
        assert_eq!(*close.borrow(), Some(Notice::SignedInElsewhere));
        assert!(other_close.borrow().is_none());
        assert!(again_close.borrow().is_none());
    }

    #[test]
    fn too_many_devices_close_the_oldest() {
        let hub = Hub::default();
        let (first, _rx, close) = conn(&hub);
        hub.join(7, first);
        let mut keep = Vec::new();
        for _ in 0..MAX_DEVICES_ONLINE {
            let (next, rx, close_next) = conn(&hub);
            hub.join(7, next);
            keep.push((rx, close_next));
        }
        assert_eq!(*close.borrow(), Some(Notice::SignedInElsewhere));
        assert!(keep.iter().all(|(_, c)| c.borrow().is_none()));
    }

    #[tokio::test]
    async fn messages_reach_every_device_or_one() {
        let hub = Hub::default();
        let (phone, mut rx1, _c1) = conn(&hub);
        let (pc, mut rx2, _c2) = conn(&hub);
        let pc_id = pc.conn_id;
        hub.join(7, phone);
        hub.join(7, pc);
        assert!(hub.send(7, ServerMsg::Pong(1)));
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
        hub.send_others(7, pc_id, ServerMsg::Pong(2));
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_err());
        assert!(hub.send_conn(7, pc_id, ServerMsg::Pong(3)));
        assert!(rx1.try_recv().is_err());
        assert!(rx2.try_recv().is_ok());
        assert!(!hub.send(8, ServerMsg::Pong(4)));
    }

    #[test]
    fn a_session_belongs_to_the_two_devices_that_started_it() {
        let hub = Hub::default();
        let (host, _rh, _ch) = conn(&hub);
        let (phone, _rp, _cp) = conn(&hub);
        let (pc, _rc, _cc) = conn(&hub);
        let (host_id, phone_id, pc_id) = (host.conn_id, phone.conn_id, pc.conn_id);
        hub.join(1, host);
        hub.join(2, phone);
        hub.join(2, pc);
        let (id, _) = hub.invite(1, host_id, 2, 5).unwrap();
        assert_eq!(hub.peer(id, 1, host_id), None);
        assert_eq!(hub.answer(id, 1, host_id, true, false, 16).ok(), None);
        assert!(hub.answer(id, 2, pc_id, true, false, 16).ok().is_some());
        assert_eq!(hub.answer(id, 2, phone_id, true, false, 16).ok(), None);
        assert_eq!(
            hub.peer(id, 1, host_id),
            Some(Side {
                user: 2,
                conn: Some(pc_id)
            })
        );
        assert_eq!(hub.peer(id, 2, phone_id), None);
        assert_eq!(
            hub.peer(id, 2, pc_id),
            Some(Side {
                user: 1,
                conn: Some(host_id)
            })
        );
        assert_eq!(hub.session_count(), 1);
        assert_eq!(hub.leave(2, phone_id).unwrap().out, vec![]);
        assert_eq!(hub.session_count(), 1);
        assert_eq!(
            hub.leave(2, pc_id).unwrap().out,
            vec![(
                To::Conn(1, host_id),
                ServerMsg::SessionEnded {
                    session: id,
                    reason: EndReason::Disconnected
                }
            )]
        );
        assert_eq!(hub.session_count(), 0);
    }

    #[test]
    fn an_open_invite_ends_when_the_guests_last_device_leaves() {
        let hub = Hub::default();
        let (host, _rh, _ch) = conn(&hub);
        let (phone, _rp, _cp) = conn(&hub);
        let (pc, _rc, _cc) = conn(&hub);
        let (host_id, phone_id, pc_id) = (host.conn_id, phone.conn_id, pc.conn_id);
        hub.join(1, host);
        hub.join(2, phone);
        hub.join(2, pc);
        let (id, _) = hub.invite(1, host_id, 2, 5).unwrap();
        assert!(hub.leave(2, phone_id).unwrap().out.is_empty());
        assert_eq!(hub.leave(2, pc_id).unwrap().out.len(), 1);
        assert_eq!(hub.answer(id, 2, pc_id, true, false, 16).ok(), None);
    }

    #[test]
    fn a_full_queue_in_bytes_closes_the_connection() {
        let hub = Hub::default();
        let (outbox, _rx) = Outbox::new(600 * 1024);
        let (close, close_rx) = watch::channel(None);
        let conn = Conn {
            conn_id: hub.conn_id(),
            device: 1,
            outbox: outbox.clone(),
            close,
            presence: Presence::Online { away: false },
            ip: IpAddr::from([127, 0, 0, 1]),
            protocol: citrine_gb_net::OLDEST_PROTOCOL_VERSION,
            person: None,
        };
        hub.join(9, conn);
        let big = || ServerMsg::FileChunk {
            file: citrine_gb_net::FileId([1; 16]),
            data: vec![7; 256 * 1024],
        };
        assert!(hub.send(9, big()));
        assert!(hub.send(9, big()));
        assert_eq!(*close_rx.borrow(), None);
        assert!(outbox.room.available_permits() < 88 * 1024);
        hub.send(9, big());
        assert_eq!(*close_rx.borrow(), Some(Notice::Disconnected));
    }

    #[tokio::test]
    async fn own_messages_wait_for_room_instead_of_closing() {
        let (outbox, mut rx) = Outbox::new(300 * 1024);
        let msg = || ServerMsg::FileChunk {
            file: citrine_gb_net::FileId([1; 16]),
            data: vec![3; 200 * 1024],
        };
        assert!(outbox.push(msg()).await);
        let waiting = tokio::spawn({
            let outbox = outbox.clone();
            async move { outbox.push(msg()).await }
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        let first = rx.recv().await.unwrap();
        drop(first);
        assert!(waiting.await.unwrap());
        assert!(outbox.room.available_permits() < 100 * 1024);
    }

    #[test]
    fn declined_invites_are_gone() {
        let hub = Hub::default();
        let (id, _) = hub.invite(1, 10, 2, 5).unwrap();
        assert!(hub.answer(id, 2, 20, false, false, 16).ok().is_some());
        assert_eq!(hub.answer(id, 2, 20, true, false, 16).ok(), None);
    }

    #[test]
    fn a_host_has_a_few_open_invites_and_a_new_one_replaces_the_old() {
        let hub = Hub::default();
        let (first, _) = hub.invite(1, 10, 2, 2).unwrap();
        let (second, replaced) = hub.invite(1, 10, 2, 2).unwrap();
        assert_eq!(replaced, Some(first));
        assert!(hub.answer(first, 2, 20, true, false, 16).ok().is_none());
        hub.invite(1, 10, 3, 2).unwrap();
        assert!(hub.invite(1, 10, 4, 2).is_none());
        assert!(hub.invite(5, 50, 4, 2).is_some());
        assert!(hub.answer(second, 2, 20, true, false, 16).ok().is_some());
        assert!(hub.invite(1, 10, 4, 2).is_some());
    }

    #[test]
    fn a_room_member_costs_the_relay_about_a_hundred_bytes() {
        let member = std::mem::size_of::<Member>();
        let room = std::mem::size_of::<Room>();
        let invite = std::mem::size_of::<room::Invite>();
        println!("member {member} room {room} invite {invite}");
        assert!(member <= 112, "member {member}");
        assert!(room <= 56, "room {room}");
        assert!(invite <= 32, "invite {invite}");
    }
}
