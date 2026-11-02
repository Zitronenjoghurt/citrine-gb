use citrine_gb_net::{Notice, Presence, ServerMsg, SessionId};
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
pub struct Session {
    pub host: i64,
    pub host_conn: u64,
    pub guest: i64,
    pub guest_conn: Option<u64>,
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Side {
    pub user: i64,
    pub conn: Option<u64>,
}

impl Session {
    pub fn other(&self, user: i64) -> Option<Side> {
        if user == self.host {
            Some(Side {
                user: self.guest,
                conn: self.guest_conn,
            })
        } else if user == self.guest {
            Some(Side {
                user: self.host,
                conn: Some(self.host_conn),
            })
        } else {
            None
        }
    }

    fn holds(&self, user: i64, conn: u64) -> bool {
        (self.host == user && self.host_conn == conn)
            || (self.guest == user && self.guest_conn == Some(conn))
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Left {
    pub ended: Vec<(SessionId, Side)>,
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
    sessions: HashMap<SessionId, Session>,
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
        let mut ended = Vec::new();
        state.sessions.retain(|&id, session| {
            let gone = session.holds(user, conn_id)
                || (last && session.guest == user && !session.accepted);
            if gone && let Some(other) = session.other(user) {
                ended.push((id, other));
            }
            !gone
        });
        Some(Left { ended, last })
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

    pub fn invite(
        &self,
        host: i64,
        host_conn: u64,
        guest: i64,
        max: usize,
    ) -> Option<(SessionId, Option<SessionId>)> {
        let mut state = self.state();
        let replaced = state
            .sessions
            .iter()
            .find(|(_, s)| !s.accepted && s.host == host && s.guest == guest)
            .map(|(&id, _)| id);
        if let Some(old) = replaced {
            state.sessions.remove(&old);
        }
        let pending = state
            .sessions
            .values()
            .filter(|s| !s.accepted && s.host == host)
            .count();
        if pending >= max {
            return None;
        }
        let mut id = random_session();
        while state.sessions.contains_key(&id) {
            id = random_session();
        }
        state.sessions.insert(
            id,
            Session {
                host,
                host_conn,
                guest,
                guest_conn: None,
                accepted: false,
            },
        );
        Some((id, replaced))
    }

    pub fn answer(
        &self,
        id: SessionId,
        guest: i64,
        guest_conn: u64,
        accept: bool,
    ) -> Option<Session> {
        let mut state = self.state();
        let session = state.sessions.get_mut(&id)?;
        if session.guest != guest || session.accepted {
            return None;
        }
        if accept {
            session.accepted = true;
            session.guest_conn = Some(guest_conn);
            Some(*session)
        } else {
            state.sessions.remove(&id)
        }
    }

    pub fn end(&self, id: SessionId, user: i64, conn_id: u64) -> Option<Side> {
        let mut state = self.state();
        let session = state.sessions.get(&id)?;
        if session.accepted && !session.holds(user, conn_id) {
            return None;
        }
        let other = session.other(user)?;
        state.sessions.remove(&id);
        Some(other)
    }

    pub fn end_between(&self, a: i64, b: i64) -> Vec<(SessionId, Side)> {
        let mut state = self.state();
        let mut ended = Vec::new();
        state.sessions.retain(|&id, s| match s.other(a) {
            Some(side) if side.user == b => {
                ended.push((id, side));
                false
            }
            _ => true,
        });
        ended
    }

    pub fn peer(&self, id: SessionId, user: i64, conn_id: u64) -> Option<Side> {
        let state = self.state();
        let session = state
            .sessions
            .get(&id)
            .filter(|s| s.accepted && s.holds(user, conn_id))?;
        session.other(user)
    }

    pub fn session_count(&self) -> usize {
        let state = self.state();
        state.sessions.values().filter(|s| s.accepted).count()
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
                ended: vec![],
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
        assert_eq!(hub.answer(id, 1, host_id, true), None);
        assert!(hub.answer(id, 2, pc_id, true).is_some());
        assert_eq!(hub.answer(id, 2, phone_id, true), None);
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
        assert_eq!(hub.leave(2, phone_id).unwrap().ended, vec![]);
        assert_eq!(hub.session_count(), 1);
        assert_eq!(
            hub.leave(2, pc_id).unwrap().ended,
            vec![(
                id,
                Side {
                    user: 1,
                    conn: Some(host_id)
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
        assert!(hub.leave(2, phone_id).unwrap().ended.is_empty());
        assert_eq!(hub.leave(2, pc_id).unwrap().ended.len(), 1);
        assert_eq!(hub.answer(id, 2, pc_id, true), None);
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
        assert!(hub.answer(id, 2, 20, false).is_some());
        assert_eq!(hub.answer(id, 2, 20, true), None);
    }

    #[test]
    fn a_host_has_a_few_open_invites_and_a_new_one_replaces_the_old() {
        let hub = Hub::default();
        let (first, _) = hub.invite(1, 10, 2, 2).unwrap();
        let (second, replaced) = hub.invite(1, 10, 2, 2).unwrap();
        assert_eq!(replaced, Some(first));
        assert!(hub.answer(first, 2, 20, true).is_none());
        hub.invite(1, 10, 3, 2).unwrap();
        assert!(hub.invite(1, 10, 4, 2).is_none());
        assert!(hub.invite(5, 50, 4, 2).is_some());
        assert!(hub.answer(second, 2, 20, true).is_some());
        assert!(hub.invite(1, 10, 4, 2).is_some());
    }
}
