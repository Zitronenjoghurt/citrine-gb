use super::{Client, db, quota_of};
use crate::db::AccountRow;
use crate::now;
use crate::stats::Refused;
use citrine_gb_net::{
    Account, ChatMessage, ErrorCode, Friend, FriendState, OnlineId, Presence, Sealed, ServerMsg,
    Tag, display_name,
};

impl Client {
    pub(super) async fn account(&self, row: &AccountRow) -> Result<Account, ErrorCode> {
        Ok(Account {
            online_id: row.online_id,
            display_name: row.display_name.clone(),
            storage_used: db(self.app.db.storage_used(row.id).await)?,
            quota_bytes: quota_of(&self.app, row),
            two_factor: db(self.app.db.account_totp(row.id).await)?.is_some(),
            codes_left: db(self.app.db.account_codes_left(row.id).await)?.min(u8::MAX as u64) as u8,
        })
    }

    pub(super) async fn my_row(&self) -> Result<AccountRow, ErrorCode> {
        db(self.app.db.account(self.me()?.id).await)?.ok_or(ErrorCode::Server)
    }

    pub(super) async fn send_account(&self) -> Result<(), ErrorCode> {
        let account = self.account(&self.my_row().await?).await?;
        self.tell_me(ServerMsg::Account(account)).await;
        Ok(())
    }

    pub(super) fn friend(&self, row: &AccountRow, state: FriendState) -> Friend {
        let presence = (state == FriendState::Friend).then(|| {
            self.app.hub.presence(row.id).unwrap_or(Presence::Offline {
                last_seen: row.last_seen,
            })
        });
        Friend {
            online_id: row.online_id,
            display_name: row.display_name.clone(),
            public_key: row.public_key,
            state,
            presence,
        }
    }

    pub(super) async fn user(&self, online: OnlineId) -> Result<AccountRow, ErrorCode> {
        let me = self.me()?;
        match db(self.app.db.account_by_online(online).await)? {
            Some(row) if row.id != me.id => Ok(row),
            _ => Err(ErrorCode::UnknownUser),
        }
    }

    pub(super) async fn tell_friends(&self, presence: Presence) -> Result<(), ErrorCode> {
        let me = self.me()?;
        for friend in db(self.app.db.friend_ids(me.id).await)? {
            self.app.hub.send(
                friend,
                ServerMsg::Presence {
                    who: me.online_id,
                    presence,
                },
            );
        }
        Ok(())
    }

    pub(super) async fn set_name(&mut self, name: &str) -> Result<(), ErrorCode> {
        let name = display_name(name).map_err(|_| ErrorCode::BadName)?;
        let me = self.me()?.id;
        db(self.app.db.set_display_name(me, &name).await)?;
        let row = self.my_row().await?;
        self.tell_me(ServerMsg::Account(self.account(&row).await?))
            .await;
        for (other, state) in db(self.app.db.relations(me).await)? {
            let seen_as = match state {
                FriendState::Friend => FriendState::Friend,
                FriendState::Incoming => FriendState::Outgoing,
                FriendState::Outgoing => FriendState::Incoming,
                FriendState::Blocked => continue,
            };
            if seen_as == FriendState::Outgoing && db(self.app.db.is_blocked(other.id, me).await)? {
                continue;
            }
            self.app
                .hub
                .send(other.id, ServerMsg::Friend(self.friend(&row, seen_as)));
        }
        Ok(())
    }

    pub(super) async fn add_friend(&mut self, to: OnlineId) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let other = self.user(to).await?;
        let app = self.app.clone();
        if db(app.db.are_friends(me, other.id).await)? {
            self.tell_me(ServerMsg::Friend(self.friend(&other, FriendState::Friend)))
                .await;
            return Ok(());
        }
        if db(app.db.has_request(other.id, me).await)? {
            return self.answer_friend(to, true).await;
        }
        let limits = &app.config.limits;
        let fresh = !db(app.db.has_request(me, other.id).await)?;
        if fresh
            && (db(app.db.requests_of(me).await)?.0 >= limits.friend_requests_out
                || !app.rates.friend_requests.take(me, 1.0))
        {
            self.refused(Refused::FriendRequests);
            return Err(ErrorCode::RateLimited);
        }
        db(app.db.unblock(me, other.id).await)?;
        let room = !fresh || db(app.db.requests_of(other.id).await)?.1 < limits.friend_requests_in;
        let added = room && db(app.db.add_request(me, other.id, now()).await)?;
        self.tell_me(ServerMsg::Friend(
            self.friend(&other, FriendState::Outgoing),
        ))
        .await;
        if added && !db(app.db.is_blocked(other.id, me).await)? {
            let row = self.my_row().await?;
            app.hub.send(
                other.id,
                ServerMsg::Friend(self.friend(&row, FriendState::Incoming)),
            );
        }
        Ok(())
    }

    pub(super) async fn answer_friend(
        &mut self,
        from: OnlineId,
        accept: bool,
    ) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let my_online = self.me()?.online_id;
        let other = self.user(from).await?;
        let app = self.app.clone();
        if !db(app.db.has_request(other.id, me).await)? {
            return Err(ErrorCode::UnknownUser);
        }
        if accept {
            db(app.db.befriend(me, other.id, now()).await)?;
            self.tell_me(ServerMsg::Friend(self.friend(&other, FriendState::Friend)))
                .await;
            let row = self.my_row().await?;
            app.hub.send(
                other.id,
                ServerMsg::Friend(self.friend(&row, FriendState::Friend)),
            );
        } else {
            db(app.db.unfriend(me, other.id).await)?;
            self.tell_me(ServerMsg::FriendGone(from)).await;
            app.hub.send(other.id, ServerMsg::FriendGone(my_online));
        }
        Ok(())
    }

    pub(super) async fn remove_friend(
        &mut self,
        who: OnlineId,
        block: bool,
    ) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let my_online = self.me()?.online_id;
        let other = self.user(who).await?;
        let app = self.app.clone();
        if block {
            db(app.db.block(me, other.id).await)?;
            self.tell_me(ServerMsg::Friend(self.friend(&other, FriendState::Blocked)))
                .await;
        } else {
            db(app.db.unfriend(me, other.id).await)?;
            self.tell_me(ServerMsg::FriendGone(who)).await;
        }
        app.hub.send(other.id, ServerMsg::FriendGone(my_online));
        for (session, _) in app.hub.end_between(me, other.id) {
            let ended = ServerMsg::SessionEnded {
                session,
                reason: citrine_gb_net::EndReason::Left,
            };
            app.hub.send(other.id, ended.clone());
            app.hub.send(me, ended);
        }
        Ok(())
    }

    pub(super) async fn chat(
        &mut self,
        tag: Tag,
        to: OnlineId,
        sealed: Sealed,
    ) -> Result<(), ErrorCode> {
        let me = self.me()?;
        let (me_id, me_online) = (me.id, me.online_id);
        let other = self.user(to).await?;
        if !db(self.app.db.are_friends(me_id, other.id).await)? {
            return Err(ErrorCode::NotFriends);
        }
        let size = sealed.ciphertext.len() as u64;
        if size > self.app.settings.get().max_message_bytes {
            return Err(ErrorCode::TooLarge);
        }
        if !self.app.rates.chat(me_id, size) {
            self.refused(Refused::Chats);
            return Err(ErrorCode::RateLimited);
        }
        let limits = &self.app.config.limits;
        let (waiting, waiting_bytes) = db(self.app.db.waiting_between(me_id, other.id).await)?;
        if waiting >= limits.inbox_messages || waiting_bytes + size > limits.inbox_bytes {
            self.refused(Refused::Inbox);
            return Err(ErrorCode::InboxFull);
        }
        self.room(size).await?;
        let at = now();
        let id = db(self
            .app
            .db
            .insert_message(me_id, other.id, self.device, at, &sealed)
            .await)?;
        self.app.stats.message();
        self.send(ServerMsg::ChatSent { tag, id, at }).await;
        let message = ChatMessage {
            id,
            from: me_online,
            at,
            sealed,
        };
        self.app.hub.send_others(
            me_id,
            self.conn_id,
            ServerMsg::ChatCopy {
                to,
                message: message.clone(),
            },
        );
        self.app.hub.send(other.id, ServerMsg::Chat(message));
        Ok(())
    }
}
