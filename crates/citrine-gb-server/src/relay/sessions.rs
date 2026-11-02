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

    pub(super) async fn answer_invite(
        &mut self,
        session: SessionId,
        accept: bool,
    ) -> Result<(), ErrorCode> {
        let me = self.me()?;
        let (me_id, me_online) = (me.id, me.online_id);
        let found = self
            .app
            .hub
            .answer(session, me_id, self.conn_id, accept)
            .ok_or(ErrorCode::UnknownSession)?;
        self.app.hub.send_others(
            me_id,
            self.conn_id,
            ServerMsg::SessionEnded {
                session,
                reason: citrine_gb_net::EndReason::Left,
            },
        );
        if !accept {
            self.app.hub.send_conn(
                found.host,
                found.host_conn,
                ServerMsg::SessionEnded {
                    session,
                    reason: citrine_gb_net::EndReason::Declined,
                },
            );
            return Ok(());
        }
        let host = db(self.app.db.account(found.host).await)?.ok_or(ErrorCode::Server)?;
        self.send(ServerMsg::SessionStarted {
            session,
            with: host.online_id,
        })
        .await;
        self.app.hub.send_conn(
            found.host,
            found.host_conn,
            ServerMsg::SessionStarted {
                session,
                with: me_online,
            },
        );
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
