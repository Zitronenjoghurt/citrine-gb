use citrine_gb_net::{Notice, Presence, ServerMsg, SessionId};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use tokio::sync::{broadcast, mpsc, watch};

pub const QUEUE: usize = 512;

pub struct Conn {
    pub conn_id: u64,
    pub tx: mpsc::Sender<ServerMsg>,
    pub close: watch::Sender<Option<Notice>>,
    pub presence: Presence,
}

impl Conn {
    fn close(&self, notice: Notice) {
        let _ = self.close.send(Some(notice));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    pub host: i64,
    pub guest: i64,
    pub accepted: bool,
}

impl Session {
    pub fn other(&self, user: i64) -> Option<i64> {
        if user == self.host {
            Some(self.guest)
        } else if user == self.guest {
            Some(self.host)
        } else {
            None
        }
    }
}

#[derive(Default)]
struct State {
    conns: HashMap<i64, Conn>,
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
        if let Some(old) = state.conns.insert(user, conn) {
            old.close(Notice::SignedInElsewhere);
        }
        state.conns.len()
    }

    pub fn leave(&self, user: i64, conn_id: u64) -> Option<Vec<(SessionId, i64)>> {
        let mut state = self.state();
        if state.conns.get(&user).is_none_or(|c| c.conn_id != conn_id) {
            return None;
        }
        state.conns.remove(&user);
        let mut ended = Vec::new();
        state
            .sessions
            .retain(|&id, session| match session.other(user) {
                Some(other) => {
                    ended.push((id, other));
                    false
                }
                None => true,
            });
        Some(ended)
    }

    pub fn send(&self, user: i64, msg: ServerMsg) -> bool {
        let state = self.state();
        let Some(conn) = state.conns.get(&user) else {
            return false;
        };
        if conn.tx.try_send(msg).is_err() {
            conn.close(Notice::Disconnected);
        }
        true
    }

    pub fn is_online(&self, user: i64) -> bool {
        self.state().conns.contains_key(&user)
    }

    pub fn presence(&self, user: i64) -> Option<Presence> {
        self.state().conns.get(&user).map(|c| c.presence)
    }

    pub fn set_presence(&self, user: i64, presence: Presence) {
        if let Some(conn) = self.state().conns.get_mut(&user) {
            conn.presence = presence;
        }
    }

    pub fn online_count(&self) -> usize {
        self.state().conns.len()
    }

    pub fn online(&self) -> HashSet<i64> {
        self.state().conns.keys().copied().collect()
    }

    pub fn kick(&self, user: i64, notice: Notice) -> bool {
        match self.state().conns.get(&user) {
            Some(conn) => {
                conn.close(notice);
                true
            }
            None => false,
        }
    }

    pub fn invite(&self, host: i64, guest: i64) -> SessionId {
        let mut state = self.state();
        let mut id = random_session();
        while state.sessions.contains_key(&id) {
            id = random_session();
        }
        state.sessions.insert(
            id,
            Session {
                host,
                guest,
                accepted: false,
            },
        );
        id
    }

    pub fn answer(&self, id: SessionId, guest: i64, accept: bool) -> Option<Session> {
        let mut state = self.state();
        let session = state.sessions.get_mut(&id)?;
        if session.guest != guest || session.accepted {
            return None;
        }
        if accept {
            session.accepted = true;
            Some(*session)
        } else {
            state.sessions.remove(&id)
        }
    }

    pub fn end(&self, id: SessionId, user: i64) -> Option<i64> {
        let mut state = self.state();
        let other = state.sessions.get(&id)?.other(user)?;
        state.sessions.remove(&id);
        Some(other)
    }

    pub fn end_between(&self, a: i64, b: i64) -> Vec<SessionId> {
        let mut state = self.state();
        let mut ended = Vec::new();
        state.sessions.retain(|&id, s| {
            let between = s.other(a) == Some(b);
            if between {
                ended.push(id);
            }
            !between
        });
        ended
    }

    pub fn peer(&self, id: SessionId, user: i64) -> Option<i64> {
        let state = self.state();
        let session = state.sessions.get(&id).filter(|s| s.accepted)?;
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
        mpsc::Receiver<ServerMsg>,
        watch::Receiver<Option<Notice>>,
    ) {
        let (tx, rx) = mpsc::channel(QUEUE);
        let (close, close_rx) = watch::channel(None);
        (
            Conn {
                conn_id: hub.conn_id(),
                tx,
                close,
                presence: Presence::Online { away: false },
            },
            rx,
            close_rx,
        )
    }

    #[test]
    fn a_second_sign_in_closes_the_first() {
        let hub = Hub::default();
        let (first, _rx1, close1) = conn(&hub);
        let first_id = first.conn_id;
        hub.join(7, first);
        let (second, _rx2, _close2) = conn(&hub);
        hub.join(7, second);
        assert_eq!(*close1.borrow(), Some(Notice::SignedInElsewhere));
        assert_eq!(hub.leave(7, first_id), None);
        assert!(hub.is_online(7));
    }

    #[test]
    fn sessions_need_an_accept_and_end_on_leave() {
        let hub = Hub::default();
        let (a, _ra, _ca) = conn(&hub);
        let a_id = a.conn_id;
        let (b, _rb, _cb) = conn(&hub);
        hub.join(1, a);
        hub.join(2, b);
        let id = hub.invite(1, 2);
        assert_eq!(hub.peer(id, 1), None);
        assert_eq!(hub.answer(id, 1, true), None);
        assert!(hub.answer(id, 2, true).is_some());
        assert_eq!(hub.peer(id, 1), Some(2));
        assert_eq!(hub.peer(id, 2), Some(1));
        assert_eq!(hub.peer(id, 3), None);
        assert_eq!(hub.session_count(), 1);
        assert_eq!(hub.leave(1, a_id), Some(vec![(id, 2)]));
        assert_eq!(hub.session_count(), 0);
    }

    #[test]
    fn declined_invites_are_gone() {
        let hub = Hub::default();
        let id = hub.invite(1, 2);
        assert!(hub.answer(id, 2, false).is_some());
        assert_eq!(hub.answer(id, 2, true), None);
    }
}
