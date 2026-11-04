use citrine_gb_net::{
    EndReason, Member as Card, MemberState, OnlineId, PublicKey, ServerMsg, SessionId,
};

pub const ROOM_PROTOCOL: u16 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub online_id: OnlineId,
    pub display_name: String,
    pub public_key: PublicKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum To {
    Conn(i64, u64),
    User(i64),
}

pub type Out = Vec<(To, ServerMsg)>;

pub fn is_room_msg(msg: &ServerMsg) -> bool {
    matches!(
        msg,
        ServerMsg::RoomJoined { .. }
            | ServerMsg::MemberUpdate { .. }
            | ServerMsg::MemberGone { .. }
            | ServerMsg::RoomPlay { .. }
            | ServerMsg::RoomGame { .. }
            | ServerMsg::RoomChat { .. }
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user: i64,
    pub person: Option<Person>,
    pub conn: Option<u64>,
    pub protocol: u16,
    pub rooms: bool,
    pub state: MemberState,
    pub drops: u32,
}

impl Member {
    pub fn new(user: i64, conn: u64, person: Option<Person>, protocol: u16, rooms: bool) -> Self {
        Self {
            user,
            person,
            conn: Some(conn),
            protocol,
            rooms: rooms && protocol >= ROOM_PROTOCOL,
            state: MemberState::default(),
            drops: 0,
        }
    }

    pub fn who(&self) -> Option<OnlineId> {
        self.person.as_ref().map(|p| p.online_id)
    }

    pub fn card(&self) -> Option<Card> {
        let person = self.person.as_ref()?;
        Some(Card {
            who: person.online_id,
            display_name: person.display_name.clone(),
            public_key: person.public_key,
            state: MemberState {
                old_client: !self.hears_rooms(),
                ..self.state
            },
        })
    }

    pub fn hears_rooms(&self) -> bool {
        self.protocol >= ROOM_PROTOCOL
    }

    pub fn holds(&self, user: i64, conn: u64) -> bool {
        self.user == user && self.conn == Some(conn)
    }

    pub fn to(&self) -> Option<To> {
        self.conn.map(|conn| To::Conn(self.user, conn))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invite {
    pub from: i64,
    pub from_conn: u64,
    pub to: i64,
    pub room: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Room {
    pub members: Vec<Member>,
    pub invites: Vec<Invite>,
    pub started: bool,
}

impl Room {
    pub fn pair(host: Member, guest: i64) -> Self {
        let invite = Invite {
            from: host.user,
            from_conn: host.conn.unwrap_or_default(),
            to: guest,
            room: false,
        };
        Self {
            members: vec![host],
            invites: vec![invite],
            started: false,
        }
    }

    pub fn holder(&self, user: i64, conn: u64) -> Option<usize> {
        self.members.iter().position(|m| m.holds(user, conn))
    }

    pub fn has_old(&self) -> bool {
        self.members.iter().any(|m| !m.hears_rooms())
    }

    pub fn players(&self) -> usize {
        self.members.iter().filter(|m| m.state.playing).count()
    }

    pub fn cards(&self) -> Vec<Card> {
        self.members.iter().filter_map(Member::card).collect()
    }

    pub fn hosts_pending(&self, host: i64) -> bool {
        !self.started && self.members.first().is_some_and(|m| m.user == host)
    }

    pub fn tell_others(&mut self, at: usize, msg: impl Fn(&Card) -> ServerMsg, out: &mut Out) {
        let Some(card) = self.members[at].card() else {
            return;
        };
        let msg = msg(&card);
        for (i, other) in self.members.iter_mut().enumerate() {
            if i == at || !other.hears_rooms() {
                continue;
            }
            if let Some(to) = other.to() {
                other.rooms = true;
                out.push((to, msg.clone()));
            }
        }
    }

    pub fn part(&mut self, id: SessionId, mut gone: Vec<Member>, reason: EndReason, out: &mut Out) {
        while let Some(i) = self.members.iter().position(|m| !m.rooms) {
            let member = self.members.remove(i);
            if let Some(to) = member.to() {
                out.push((
                    to,
                    ServerMsg::SessionEnded {
                        session: id,
                        reason,
                    },
                ));
            }
            gone.push(member);
        }
        for member in &self.members {
            let Some(to) = member.to() else {
                continue;
            };
            for who in gone.iter().filter_map(Member::who) {
                out.push((
                    to,
                    ServerMsg::MemberGone {
                        room: id,
                        who,
                        reason,
                    },
                ));
            }
        }
        if self.members.is_empty() {
            for invite in self.invites.drain(..) {
                out.push((
                    To::User(invite.to),
                    ServerMsg::SessionEnded {
                        session: id,
                        reason,
                    },
                ));
            }
        }
    }

    pub fn depart(&mut self, id: SessionId, at: usize, reason: EndReason, out: &mut Out) {
        let gone = self.members.remove(at);
        self.part(id, vec![gone], reason, out);
    }

    pub fn drop_member(&mut self, id: SessionId, at: usize, out: &mut Out) -> Option<u32> {
        if !self.members[at].rooms {
            self.depart(id, at, EndReason::Disconnected, out);
            return None;
        }
        let member = &mut self.members[at];
        member.conn = None;
        member.state.reconnecting = true;
        member.drops += 1;
        let drops = member.drops;
        self.tell_others(
            at,
            |card| ServerMsg::MemberUpdate {
                room: id,
                member: card.clone(),
            },
            out,
        );
        self.part(id, Vec::new(), EndReason::Disconnected, out);
        Some(drops)
    }
}
