mod group;
mod watch;

use super::links::Links;
use super::{Ending, GameFile, MyGame, PlaySession, Returned, Role, Shelf, Stage};
use crate::online::Online;
use citrine_gb_net::{
    Body, ClientMsg, Console, EndReason, Envelope, ErrorCode, GameCard, Member, MemberState,
    OnlineId, PROTOCOL_VERSION, PlayMsg, PublicKey, Sealed, SessionId,
};
use std::time::Duration;
use web_time::Instant;

#[cfg(not(test))]
const REJOIN_FOR: Duration = Duration::from_secs(60);
#[cfg(test)]
const REJOIN_FOR: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomMember {
    pub who: OnlineId,
    pub name: String,
    pub key: PublicKey,
    pub state: MemberState,
}

impl From<Member> for RoomMember {
    fn from(member: Member) -> Self {
        Self {
            who: member.who,
            name: member.display_name,
            key: member.public_key,
            state: member.state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomLine {
    pub from: OnlineId,
    pub at: u64,
    pub envelope: Envelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lost {
    Waiting(Instant),
    Asked(Instant),
}

struct Theirs {
    with: OnlineId,
    card: GameCard,
    game: MyGame,
}

struct Shelved {
    mine: Option<GameFile>,
    card: Option<GameCard>,
    borrowing: bool,
    borrowed_save: Option<Vec<u8>>,
}

pub struct Room {
    id: Option<SessionId>,
    pairs: Vec<PlaySession>,
    members: Vec<RoomMember>,
    chat: Vec<RoomLine>,
    told: Option<(bool, bool)>,
    lost: Option<Lost>,
    inviting: Vec<OnlineId>,
    old_pair: Option<OnlineId>,
    invite_error: Option<ErrorCode>,
    invite_failed: Option<OnlineId>,
    returned: Option<Returned>,
    links: Links,
    me: Option<OnlineId>,
    seeded: bool,
    sent: Vec<OnlineId>,
    theirs: Option<Theirs>,
    shelved: Option<Shelved>,
    group: Option<group::GroupPlay>,
    group_rounds: u32,
    early: Vec<(OnlineId, PlayMsg)>,
    paused: Vec<OnlineId>,
    away: Vec<OnlineId>,
    my_status: (bool, bool),
    status_told: Vec<OnlineId>,
    watch_asks: Vec<OnlineId>,
    watch: Option<watch::GroupWatch>,
}

impl Room {
    pub(super) fn new(pair: PlaySession) -> Self {
        Self {
            id: pair.id,
            pairs: vec![pair],
            members: Vec::new(),
            chat: Vec::new(),
            told: None,
            lost: None,
            inviting: Vec::new(),
            old_pair: None,
            invite_error: None,
            invite_failed: None,
            returned: None,
            links: Links::default(),
            me: None,
            seeded: false,
            sent: Vec::new(),
            theirs: None,
            shelved: None,
            group: None,
            group_rounds: 0,
            early: Vec::new(),
            paused: Vec::new(),
            away: Vec::new(),
            my_status: (false, false),
            status_told: Vec::new(),
            watch_asks: Vec::new(),
            watch: None,
        }
    }

    pub fn id(&self) -> Option<SessionId> {
        self.id
    }

    pub fn members(&self) -> &[RoomMember] {
        &self.members
    }

    pub fn member(&self, who: OnlineId) -> Option<&RoomMember> {
        self.members.iter().find(|m| m.who == who)
    }

    pub fn old_client(&self) -> Option<OnlineId> {
        self.members
            .iter()
            .find(|m| m.state.old_client)
            .map(|m| m.who)
            .or(self.old_pair)
    }

    pub fn others(&self) -> impl Iterator<Item = OnlineId> + '_ {
        self.pairs.iter().filter(|p| p.is_live()).map(|p| p.friend)
    }

    pub fn chat(&self) -> &[RoomLine] {
        &self.chat
    }

    pub fn links(&self) -> &Links {
        &self.links
    }

    pub fn rejoining(&self) -> bool {
        self.lost.is_some()
    }

    pub fn invite_error(&self) -> Option<ErrorCode> {
        self.invite_error
    }

    pub fn invite_failed(&self) -> Option<OnlineId> {
        self.invite_failed
    }

    pub fn inviting(&self) -> &[OnlineId] {
        &self.inviting
    }

    pub fn pair(&self, who: OnlineId) -> Option<&PlaySession> {
        self.pairs.iter().find(|p| p.friend == who)
    }

    pub fn shelved(&self) -> Option<&GameCard> {
        self.shelved.as_ref().and_then(|s| s.card.as_ref())
    }

    pub fn is_live(&self) -> bool {
        self.pairs.iter().any(PlaySession::is_live)
    }

    pub fn view(&self) -> Option<&PlaySession> {
        self.view_index().map(|i| &self.pairs[i])
    }

    pub fn view_mut(&mut self) -> Option<&mut PlaySession> {
        let index = self.view_index()?;
        let pair = &mut self.pairs[index];
        if pair.returned.is_none() {
            pair.returned = self.returned.take();
        }
        Some(pair)
    }

    fn view_index(&self) -> Option<usize> {
        self.pairs
            .iter()
            .position(|p| p.is_live() && !p.apart)
            .or_else(|| self.pairs.iter().position(PlaySession::is_live))
            .or((!self.pairs.is_empty()).then_some(0))
    }

    pub(super) fn into_view(mut self) -> Option<PlaySession> {
        let index = self.view_index()?;
        let mut pair = self.pairs.swap_remove(index);
        if pair.returned.is_none() {
            pair.returned = self.returned.take();
        }
        Some(pair)
    }

    fn pair_mut(&mut self, who: OnlineId) -> Option<&mut PlaySession> {
        self.pairs.iter_mut().find(|p| p.friend == who)
    }

    pub(super) fn set_id(&mut self, id: SessionId) {
        self.id = Some(id);
        for pair in &mut self.pairs {
            pair.id = Some(id);
        }
    }

    pub(super) fn first_mut(&mut self) -> Option<&mut PlaySession> {
        self.pairs.first_mut()
    }

    pub(super) fn end(&mut self, ending: Ending) {
        self.lost = None;
        for pair in &mut self.pairs {
            if pair.is_live() {
                pair.end(ending);
            }
        }
    }

    pub(super) fn invited(&mut self, to: OnlineId) {
        self.inviting.retain(|w| *w != to);
        self.inviting.push(to);
        self.invite_error = None;
        self.invite_failed = None;
    }

    pub(super) fn error(&mut self, code: ErrorCode) -> bool {
        if matches!(self.lost, Some(Lost::Asked(_)))
            && matches!(
                code,
                ErrorCode::UnknownSession | ErrorCode::RoomFull | ErrorCode::CantJoin
            )
        {
            self.end(Ending::Dropped);
            return true;
        }
        if self.inviting.is_empty() {
            return false;
        }
        if matches!(
            code,
            ErrorCode::RoomFull | ErrorCode::CantJoin | ErrorCode::NeedsUpdate
        ) {
            self.invite_failed = self.inviting.pop();
            self.invite_error = Some(code);
            return true;
        }
        false
    }

    pub(super) fn lose(&mut self, now: Instant) {
        if self.lost.is_none() {
            self.lost = Some(Lost::Waiting(now));
        }
        if let Some(Lost::Waiting(at) | Lost::Asked(at)) = self.lost
            && now.duration_since(at) > REJOIN_FOR
        {
            self.end(Ending::Dropped);
        }
    }

    pub(super) fn rejoin(&mut self, online: &mut Online) {
        let (Some(Lost::Waiting(at)), Some(id)) = (self.lost, self.id) else {
            return;
        };
        if online.send(ClientMsg::AnswerInvite {
            session: id,
            accept: true,
        }) {
            self.lost = Some(Lost::Asked(at));
            self.told = None;
        }
    }

    pub(super) fn can_rejoin(&self) -> bool {
        self.told.is_some() && self.id.is_some()
    }

    pub(super) fn joined(&mut self, me: OnlineId, list: Vec<Member>, sharing: bool, now: Instant) {
        let back = self.lost.take().is_some();
        let mine = list.iter().position(|m| m.who == me).unwrap_or(list.len());
        let others: Vec<(usize, RoomMember)> = list
            .into_iter()
            .enumerate()
            .filter(|(_, m)| m.who != me)
            .map(|(i, m)| (i, m.into()))
            .collect();
        for pair in &mut self.pairs {
            if pair.is_live() && !others.iter().any(|(_, m)| m.who == pair.friend) {
                pair.end(Ending::FriendLeft);
            }
        }
        let room = others.len() > 1;
        let shows = self.shows();
        let card = self.my_card();
        for (index, member) in &others {
            let role = if *index < mine {
                Role::Guest
            } else {
                Role::Host
            };
            match self.pairs.iter_mut().find(|p| p.friend == member.who) {
                Some(pair) if back => {
                    pair.heard_at = now;
                    pair.key = Some(member.key);
                    if pair.is_live() && pair.stage != Stage::Inviting {
                        pair.restart(now);
                    }
                }
                Some(pair) => {
                    pair.key = Some(member.key);
                    pair.apart |= room;
                    pair.begin(now);
                }
                None => {
                    let mut pair = PlaySession::new(member.who, role, Stage::Preparing, sharing);
                    pair.id = self.id;
                    pair.key = Some(member.key);
                    pair.apart = true;
                    pair.round.my_card = card.clone();
                    pair.let_watch = shows;
                    pair.begin(now);
                    self.pairs.push(pair);
                }
            }
        }
        self.members = others.into_iter().map(|(_, m)| m).collect();
        self.tidy();
    }

    pub(super) fn update_member(&mut self, member: Member, sharing: bool, now: Instant) {
        let member: RoomMember = member.into();
        self.inviting.retain(|w| *w != member.who);
        if member.state.old_client && !self.aware(member.who) {
            self.old_pair = Some(member.who);
            return;
        }
        match self.members.iter_mut().find(|m| m.who == member.who) {
            Some(known) => {
                let back = known.state.reconnecting && !member.state.reconnecting;
                *known = member.clone();
                if back && let Some(pair) = self.pair_mut(member.who) {
                    pair.heard_at = now;
                    if pair.is_live() {
                        pair.restart(now);
                    }
                }
            }
            None => self.members.push(member.clone()),
        }
        if let Some(pair) = self.pair_mut(member.who) {
            pair.key = Some(member.key);
            return;
        }
        let mut pair = PlaySession::new(member.who, Role::Host, Stage::Preparing, sharing);
        pair.id = self.id;
        pair.key = Some(member.key);
        pair.apart = true;
        pair.round.my_card = self.my_card();
        pair.let_watch = self.shows();
        pair.begin(now);
        self.pairs.push(pair);
    }

    pub(super) fn member_gone(&mut self, who: OnlineId, reason: EndReason, online: &mut Online) {
        self.inviting.retain(|w| *w != who);
        if self.old_pair == Some(who) {
            self.old_pair = None;
        }
        if self.invite_failed != Some(who) {
            self.invite_error = None;
            self.invite_failed = None;
        }
        self.links.remove(who);
        self.sent.retain(|w| *w != who);
        self.members.retain(|m| m.who != who);
        let Some(index) = self.pairs.iter().position(|p| p.friend == who) else {
            return;
        };
        if self.pairs[index].is_live() {
            self.pairs[index].end(match reason {
                EndReason::Declined => Ending::Declined,
                EndReason::Left => Ending::FriendLeft,
                EndReason::Disconnected => Ending::Dropped,
            });
        }
        if !self.is_live() {
            if let Some(id) = self.id {
                online.send(ClientMsg::Leave(id));
            }
            return;
        }
        let mut pair = self.pairs.remove(index);
        let (mine, card, borrowed) = match self.shelved.take_if(|_| pair.lent) {
            Some(shelf) => (
                shelf.mine,
                shelf.card,
                Some((shelf.borrowing, shelf.borrowed_save)),
            ),
            None => (pair.round.mine.take(), pair.round.my_card.clone(), None),
        };
        if self.theirs.as_ref().is_some_and(|t| t.with == who) {
            self.theirs = None;
        }
        let back = pair.finish();
        if let Some(mut mine) = mine
            && let Some(heir) = self
                .pairs
                .iter_mut()
                .find(|p| p.is_live() && p.round.mine.is_none())
        {
            mine.running |= back.is_some();
            heir.round.mine = Some(mine);
            if heir.round.my_card.is_none() {
                heir.round.my_card = card;
            }
            if let Some((borrowing, save)) = borrowed {
                heir.borrowing = borrowing;
                heir.borrowed_save = save;
            }
        }
        if let Some(back) = back
            && self.returned.is_none()
        {
            self.returned = Some(back);
        }
    }

    fn tidy(&mut self) {
        if !self.is_live() {
            return;
        }
        let mut kept = Vec::with_capacity(self.pairs.len());
        for pair in std::mem::take(&mut self.pairs) {
            if pair.is_live() {
                kept.push(pair);
            } else if let Some(back) = pair.finish()
                && self.returned.is_none()
            {
                self.returned = Some(back);
            }
        }
        self.pairs = kept;
    }

    pub(super) fn route(
        &self,
        from: Option<OnlineId>,
        online: &Online,
    ) -> Option<(usize, PublicKey)> {
        let index = match from {
            Some(who) => self.pairs.iter().position(|p| p.friend == who)?,
            None if self.pairs.len() == 1 => 0,
            None => return None,
        };
        Some((index, self.pairs[index].key(online)?))
    }

    pub(super) fn pair_at(&mut self, index: usize) -> &mut PlaySession {
        &mut self.pairs[index]
    }

    pub(super) fn hear_chat(&mut self, from: OnlineId, at: u64, sealed: &Sealed, online: &Online) {
        let Some(key) = self
            .pair(from)
            .and_then(|p| p.key(online))
            .or_else(|| self.member(from).map(|m| m.key))
        else {
            return;
        };
        let Some(keys) = online.keys() else {
            return;
        };
        let Some(envelope) = keys
            .open(&key, sealed)
            .ok()
            .and_then(|plain| Envelope::unpacked(&plain).ok())
        else {
            return;
        };
        self.chat.push(RoomLine { from, at, envelope });
    }

    pub(super) fn say(&mut self, online: &mut Online, text: String) -> bool {
        let Some(id) = self.id.filter(|_| self.is_live() && self.lost.is_none()) else {
            return false;
        };
        let Some(keys) = online.keys() else {
            return false;
        };
        let envelope = Envelope::new(Body::Text(text));
        let packed = envelope.packed();
        let copies: Vec<(OnlineId, Sealed)> = self
            .pairs
            .iter()
            .filter(|p| p.is_live() && p.stage != Stage::Inviting)
            .filter_map(|p| {
                p.key(online)
                    .map(|key| (p.friend, keys.seal(&key, &packed)))
            })
            .collect();
        if copies.is_empty() || !online.send(ClientMsg::RoomChat { room: id, copies }) {
            return false;
        }
        let me = online.account().map(|a| a.online_id);
        if let Some(me) = me {
            self.chat.push(RoomLine {
                from: me,
                at: super::unix_now(),
                envelope,
            });
        }
        true
    }

    fn shows(&self) -> bool {
        self.view().is_none_or(|v| v.let_watch)
    }

    pub(super) fn set_let_watch(&mut self, on: bool) {
        for pair in self.pairs.iter_mut().filter(|p| p.is_live()) {
            pair.set_let_watch(on);
        }
    }

    pub(super) fn set_input_delay(&mut self, frames: Option<u32>) {
        for pair in &mut self.pairs {
            pair.input_delay = frames;
        }
    }

    pub(super) fn step(
        &mut self,
        shelf: &dyn Shelf,
        sharing: bool,
        me: Option<OnlineId>,
        now: Instant,
    ) {
        if self.lost.is_some() {
            return;
        }
        self.me = me;
        if let Some(me) = me {
            self.relink(me, now);
            self.step_group(me, now);
            self.share_group_state();
            self.serve_watchers(me);
            self.step_watch(shelf);
        }
        for pair in &mut self.pairs {
            pair.sharing = sharing;
            let away = self
                .members
                .iter()
                .any(|m| m.who == pair.friend && m.state.reconnecting);
            if away {
                pair.heard_at = now;
                continue;
            }
            pair.step(shelf, now);
        }
    }

    pub(super) fn tell(&mut self, online: &mut Online) {
        if online.protocol() < PROTOCOL_VERSION || self.lost.is_some() {
            return;
        }
        let Some(id) = self.id else {
            return;
        };
        let Some(view) = self
            .view()
            .filter(|v| v.is_live() && v.stage != Stage::Inviting)
        else {
            return;
        };
        if !view.began {
            return;
        }
        let now = (view.lets_friend_watch(), view.my_game().is_some());
        if self.told == Some(now) {
            return;
        }
        if online.send(ClientMsg::MemberUpdate {
            room: id,
            shows_screen: now.0,
            playing: now.1,
        }) {
            self.told = Some(now);
        }
    }

    pub(super) fn flush(&mut self, online: &mut Online) {
        if self.lost.is_some() {
            return;
        }
        self.flush_group();
        for pair in &mut self.pairs {
            let aware = self.members.iter().any(|m| m.who == pair.friend);
            pair.flush(online, aware);
        }
    }

    pub(super) fn leave(&mut self, online: &mut Online) {
        if let Some(id) = self.id
            && self.is_live()
        {
            online.send(ClientMsg::Leave(id));
        }
        if self.is_live() {
            self.end(Ending::Left);
        }
    }

    fn my_card(&self) -> Option<GameCard> {
        match &self.shelved {
            Some(shelf) => shelf.card.clone(),
            None => self
                .pairs
                .iter()
                .filter(|p| p.round.mine.is_some())
                .chain(&self.pairs)
                .find_map(|p| p.round.my_card.clone()),
        }
    }

    fn console(&self) -> Option<Console> {
        self.pairs
            .iter()
            .filter(|p| p.is_live() && !p.apart)
            .chain(&self.pairs)
            .find_map(|p| p.round.my_card.as_ref().map(|c| c.console))
    }

    fn aware(&self, who: OnlineId) -> bool {
        self.members.iter().any(|m| m.who == who)
    }

    pub(super) fn link(&mut self, who: OnlineId) -> bool {
        if !self.aware(who) || self.lost.is_some() {
            return false;
        }
        let Some(pair) = self
            .pairs
            .iter_mut()
            .find(|p| p.friend == who && p.is_live())
        else {
            return false;
        };
        pair.outbox.push(PlayMsg::LinkRequest(true));
        true
    }

    pub(super) fn play_theirs(&mut self, who: OnlineId, game: MyGame) -> bool {
        let Some(pair) = self.pair(who) else {
            return false;
        };
        let Some(card) = pair.friend_game().cloned() else {
            return false;
        };
        let fits = match &game {
            MyGame::Own(file) => file.card().is_some_and(|c| c.hash == card.hash),
            MyGame::Borrow { .. } => pair.sharing,
            MyGame::Nothing => false,
        };
        if !fits || self.shelved.is_some() || !self.link(who) {
            return false;
        }
        self.theirs = Some(Theirs {
            with: who,
            card,
            game,
        });
        true
    }

    pub(super) fn bring(&mut self, game: MyGame) -> bool {
        let Some(view) = self.view_index() else {
            return false;
        };
        let card = match &game {
            MyGame::Own(file) => file.card(),
            _ => None,
        };
        if !self.pairs[view].bring(game) {
            return false;
        }
        let file = self.pairs[view].round.mine.clone();
        for (i, pair) in self.pairs.iter_mut().enumerate() {
            if i == view || !pair.began {
                continue;
            }
            match (&file, pair.round.mine.is_some()) {
                (Some(file), true) => pair.bring(MyGame::Own(file.clone())),
                _ => pair.show_card(card.clone()),
            };
        }
        true
    }

    pub(super) fn unlink(&mut self) -> bool {
        let console = self.console();
        let Some(me) = self.me.filter(|_| self.lost.is_none()) else {
            return false;
        };
        let Some(group) = self.links.group_of(me) else {
            return false;
        };
        let members: Vec<OnlineId> = group
            .members()
            .iter()
            .copied()
            .filter(|w| *w != me)
            .collect();
        self.leave_group(me);
        self.links.set(members.clone(), console);
        self.sent.clear();
        self.tell_links(&members);
        if self.group.is_some() {
            let engine = self.lift_group();
            self.retire_group(engine);
        }
        true
    }

    pub(super) fn take_link(&mut self, who: OnlineId) -> bool {
        if !self.links.prefer(who) {
            return false;
        }
        let Some(me) = self.me else {
            return true;
        };
        let waits = self
            .links
            .group_of(me)
            .is_some_and(|g| g.is_full() || g.authority() != me);
        if waits {
            self.unlink();
        }
        true
    }

    pub(super) fn drop_asks(&mut self) {
        self.links.clear_asking();
    }

    fn tell_links(&mut self, members: &[OnlineId]) {
        let aware: Vec<OnlineId> = self.members.iter().map(|m| m.who).collect();
        let msg = match members
            .first()
            .and_then(|first| self.links.group_of(*first))
        {
            Some(group) => group_msg(group, self.group_frame()),
            None => PlayMsg::LinkGroup(citrine_gb_net::LinkGroup {
                round: 0,
                at_frame: self.group_frame(),
                members: members.to_vec(),
                plugged: true,
                console: self.console(),
                watchers: Vec::new(),
            }),
        };
        for pair in &mut self.pairs {
            if pair.is_live() && aware.contains(&pair.friend) {
                pair.outbox.push(msg.clone());
            }
        }
    }

    fn group_frame(&self) -> u32 {
        self.group_engine().map_or(0, |r| r.stop_frame())
    }

    fn relink(&mut self, me: OnlineId, now: Instant) {
        let console = self.console();
        for i in 0..self.pairs.len() {
            let friend = self.pairs[i].friend;
            for msg in std::mem::take(&mut self.pairs[i].links_in) {
                match msg {
                    PlayMsg::LinkRequest(true) => self.links.ask(friend),
                    PlayMsg::LinkRequest(false) => self.links.cancel(friend),
                    PlayMsg::LinkGroup(group) => {
                        self.links.set_group(
                            group.members,
                            group.console.or(console),
                            group.round,
                            group.watchers,
                        );
                        self.sent.clear();
                    }
                    _ => {}
                }
            }
        }
        let consoles: Vec<(OnlineId, Option<Console>)> = self
            .pairs
            .iter()
            .map(|p| (p.friend, p.friend_game().map(|g| g.console)))
            .collect();
        let asker = |who: OnlineId| {
            consoles
                .iter()
                .find(|(w, _)| *w == who)
                .and_then(|(_, c)| *c)
        };
        let group_console = self
            .links
            .group_of(me)
            .and_then(|g| g.console())
            .or(console);
        if let Some(members) = self.links.next_for(me, group_console, asker) {
            self.links.set(members, group_console);
            let round = self.next_group_round();
            self.links.bump(me, round);
            self.sent.clear();
        }
        if !self.seeded {
            let Some(first) = self.pairs.first().filter(|p| p.began) else {
                return;
            };
            if !first.apart && self.pairs.len() == 1 {
                let members = match first.role {
                    Role::Host => vec![me, first.friend],
                    Role::Guest => vec![first.friend, me],
                };
                self.links.set(members, console);
            }
            self.seeded = true;
        }
        self.sync_pairs(me, now);
        let Some(group) = self.links.group_of(me).filter(|g| g.authority() == me) else {
            return;
        };
        let msg = group_msg(group, self.group_frame());
        let aware: Vec<OnlineId> = self.members.iter().map(|m| m.who).collect();
        for pair in &mut self.pairs {
            if pair.is_live() && aware.contains(&pair.friend) && !self.sent.contains(&pair.friend) {
                pair.outbox.push(msg.clone());
                self.sent.push(pair.friend);
            }
        }
    }

    fn sync_pairs(&mut self, me: OnlineId, now: Instant) {
        let mine = self.links.group_of(me).cloned();
        let flips: Vec<(usize, bool)> = self
            .pairs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_live() && p.began && p.stage != Stage::Inviting)
            .filter(|(_, p)| !self.grouped_with(me, p.friend))
            .map(|(i, p)| (i, mine.as_ref().is_some_and(|g| g.holds(p.friend))))
            .filter(|&(i, linked)| self.pairs[i].apart == linked)
            .collect();
        for &(i, _) in flips.iter().filter(|(_, linked)| !linked) {
            let pair = &mut self.pairs[i];
            pair.apart = true;
            pair.grouped = false;
            match self.shelved.take_if(|_| pair.lent) {
                Some(shelf) => {
                    pair.restart_with(now, shelf.mine, shelf.card);
                    pair.lent = false;
                    pair.borrowing = shelf.borrowing;
                    pair.borrowed_save = shelf.borrowed_save;
                }
                None => pair.restart(now),
            }
        }
        let Some(group) = mine else {
            return;
        };
        for &(i, _) in flips.iter().filter(|(_, linked)| *linked) {
            let donor = self
                .pairs
                .iter()
                .position(|q| q.round.mine.is_some())
                .filter(|&d| d != i && self.pairs[i].round.mine.is_none());
            let (mine, card, returned) = match donor {
                Some(d) => {
                    let q = &mut self.pairs[d];
                    let returned = q.returned.take();
                    let mine = q.round.mine.take().map(|mut m| {
                        m.running |= returned.is_some();
                        m
                    });
                    (mine, q.round.my_card.clone(), returned)
                }
                None => {
                    let p = &mut self.pairs[i];
                    (p.round.mine.take(), p.round.my_card.clone(), None)
                }
            };
            let theirs = self
                .theirs
                .take_if(|t| group.holds(t.with) && self.shelved.is_none());
            let pair = &mut self.pairs[i];
            let (mine, card) = match theirs {
                Some(theirs) => {
                    self.shelved = Some(Shelved {
                        mine,
                        card,
                        borrowing: pair.borrowing,
                        borrowed_save: pair.borrowed_save.take(),
                    });
                    pair.lent = true;
                    pair.borrowing = matches!(theirs.game, MyGame::Borrow { .. });
                    match theirs.game {
                        MyGame::Own(file) => (Some(file), Some(theirs.card)),
                        MyGame::Borrow { save } => {
                            pair.borrowed_save = save;
                            (None, Some(theirs.card))
                        }
                        MyGame::Nothing => (None, None),
                    }
                }
                None => (mine, card),
            };
            pair.apart = false;
            pair.role = if group.authority() == me {
                Role::Host
            } else {
                Role::Guest
            };
            pair.sync = group.sync();
            if returned.is_some() {
                pair.returned = returned;
            }
            pair.restart_with(now, mine, card);
        }
    }
}

fn group_msg(group: &super::LinkGroup, at_frame: u32) -> PlayMsg {
    PlayMsg::LinkGroup(citrine_gb_net::LinkGroup {
        round: group.round(),
        at_frame,
        members: group.members().to_vec(),
        plugged: true,
        console: group.console(),
        watchers: group.watchers().to_vec(),
    })
}
