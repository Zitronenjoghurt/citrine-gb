use super::{Client, db, screen_size};
use crate::hub::Side;
use crate::screens::PNG_SIGNATURE;
use crate::stats::Refused;
use citrine_gb_net::{ErrorCode, MAX_SCREEN_BYTES, OnlineId, Sealed, ServerMsg, SessionId};

impl Client {
    pub(super) fn session_room(&self, session: SessionId, sealed: &Sealed) -> bool {
        let Some(me) = self.me.as_ref() else {
            return false;
        };
        let bytes = sealed.ciphertext.len() as f64;
        self.app.rates.sessions.take((session, me.id), bytes)
    }

    pub(super) fn peer(&self, session: SessionId) -> Result<Side, ErrorCode> {
        self.app
            .hub
            .peer(session, self.me()?.id, self.conn_id)
            .ok_or(ErrorCode::UnknownSession)
    }

    pub(super) async fn invite(&mut self, to: OnlineId, sealed: Sealed) -> Result<(), ErrorCode> {
        let me = self.me()?;
        let (me_id, me_online) = (me.id, me.online_id);
        let other = self.user(to).await?;
        if !db(self.app.db.are_friends(me_id, other.id).await)? {
            return Err(ErrorCode::NotFriends);
        }
        if !self.app.hub.is_online(other.id) {
            return Err(ErrorCode::Offline);
        }
        let max = self.app.config.limits.max_invites;
        let invited = self.app.hub.invite(me_id, self.conn_id, other.id, max);
        let (session, replaced) = invited.ok_or_else(|| {
            self.app.stats.refused(Refused::Play);
            ErrorCode::RateLimited
        })?;
        if let Some(old) = replaced {
            self.app.hub.send(
                other.id,
                ServerMsg::SessionEnded {
                    session: old,
                    reason: citrine_gb_net::EndReason::Left,
                },
            );
        }
        self.send(ServerMsg::Invited { session, to }).await;
        self.app.hub.send(
            other.id,
            ServerMsg::Invite {
                session,
                from: me_online,
                sealed,
            },
        );
        Ok(())
    }

    async fn blocked_with(&self, user: i64, room: SessionId) -> Result<bool, ErrorCode> {
        for other in self.app.hub.room_users(room) {
            if other == user {
                continue;
            }
            if db(self.app.db.is_blocked(user, other).await)?
                || db(self.app.db.is_blocked(other, user).await)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) async fn answer_invite(
        &mut self,
        session: SessionId,
        accept: bool,
    ) -> Result<(), ErrorCode> {
        let me_id = self.me()?.id;
        let blocked = accept && self.blocked_with(me_id, session).await?;
        let max = self.app.config.limits.room_members;
        let out = self
            .app
            .hub
            .answer(session, me_id, self.conn_id, accept, blocked, max)?;
        self.app.hub.send_others(
            me_id,
            self.conn_id,
            ServerMsg::SessionEnded {
                session,
                reason: citrine_gb_net::EndReason::Left,
            },
        );
        self.app.hub.deliver(out);
        Ok(())
    }

    pub(super) async fn room_invite(
        &mut self,
        room: SessionId,
        to: OnlineId,
        sealed: Sealed,
    ) -> Result<(), ErrorCode> {
        let me = self.me()?;
        let (me_id, me_online) = (me.id, me.online_id);
        let other = self.user(to).await?;
        if !db(self.app.db.are_friends(me_id, other.id).await)? {
            return Err(ErrorCode::NotFriends);
        }
        if !self.app.hub.is_online(other.id) {
            return Err(ErrorCode::Offline);
        }
        if self.blocked_with(other.id, room).await? {
            return Err(ErrorCode::CantJoin);
        }
        let max = self.app.config.limits.room_members;
        let out = self.app.hub.room_invite(
            room,
            me_id,
            self.conn_id,
            other.id,
            me_online,
            sealed,
            max,
        )?;
        self.send(ServerMsg::Invited { session: room, to }).await;
        self.app.hub.deliver(out);
        Ok(())
    }

    pub(super) async fn room_play(
        &mut self,
        room: SessionId,
        to: OnlineId,
        sealed: Sealed,
        game: bool,
    ) -> Result<(), ErrorCode> {
        if game && !self.app.settings.get().game_sharing {
            return Err(ErrorCode::GameSharingOff);
        }
        let me = self.me()?.id;
        let (from, target) = self.app.hub.route(room, me, self.conn_id, to)?;
        if !self.session_room(room, &sealed) {
            self.refused(Refused::Play);
            self.rate_limited().await;
            return Ok(());
        }
        let Some(target) = target else {
            return Ok(());
        };
        let msg = if game {
            ServerMsg::RoomGame { room, from, sealed }
        } else {
            ServerMsg::RoomPlay { room, from, sealed }
        };
        self.app.hub.deliver(vec![(target, msg)]);
        Ok(())
    }

    pub(super) async fn room_chat(
        &mut self,
        room: SessionId,
        copies: Vec<(OnlineId, Sealed)>,
    ) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        if copies.len() > self.app.config.limits.room_members {
            return Err(ErrorCode::BadMessage);
        }
        let max = self.app.settings.get().max_message_bytes;
        if copies
            .iter()
            .any(|(_, sealed)| sealed.ciphertext.len() as u64 > max)
        {
            return Err(ErrorCode::TooLarge);
        }
        let size: u64 = copies
            .iter()
            .map(|(_, sealed)| sealed.ciphertext.len() as u64)
            .sum();
        if !self.app.rates.chat(me, size) {
            return Err(ErrorCode::RateLimited);
        }
        let out = self
            .app
            .hub
            .room_chat(room, me, self.conn_id, copies, crate::now())?;
        self.app.hub.deliver(out);
        Ok(())
    }

    pub(super) async fn screen(&mut self, png: Vec<u8>) {
        let refused = if !self.app.settings.get().pictures {
            Some(ErrorCode::PicturesOff)
        } else if png.len() > MAX_SCREEN_BYTES {
            Some(ErrorCode::TooLarge)
        } else if !png.starts_with(PNG_SIGNATURE) || !screen_size(&png) {
            Some(ErrorCode::BadMessage)
        } else if !self.screen_limit.take(1.0) {
            Some(ErrorCode::RateLimited)
        } else {
            None
        };
        if let Some(error) = refused {
            self.send(ServerMsg::ScreenRefused(error)).await;
            return;
        }
        let token = self
            .screen
            .get_or_insert_with(crate::screens::token)
            .clone();
        self.app.screens.put(&token, png);
        self.send(ServerMsg::ScreenStored {
            path: format!("/f/{token}"),
        })
        .await;
    }
}
