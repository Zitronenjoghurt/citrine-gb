use super::{Client, MESSAGE_PAGE, Me, db};
use crate::connections::address_key;
use crate::db::{AccountRow, InviteRefusal};
use crate::hub::{Conn, Person};
use crate::now;
use crate::settings::Check;
use crate::stats::Refused;
use citrine_gb_net::{
    AuthKey, ChatMessage, DeviceId, DeviceInfo, DeviceKey, ErrorCode, InviteCode, Notice, OldChat,
    OneTimeCode, PROTOCOL_VERSION, Presence, PublicKey, RelayPassword, ServerMsg, display_name,
    speaks_protocol,
};

impl Client {
    pub(super) async fn sign_in(
        &mut self,
        password: Option<RelayPassword>,
        auth_key: AuthKey,
        new: Option<(PublicKey, String, Option<InviteCode>)>,
        known: Option<(Option<DeviceKey>, Option<OneTimeCode>)>,
    ) -> Result<(), ErrorCode> {
        if self.me.is_some() {
            return Err(ErrorCode::AlreadySignedIn);
        }
        if !self.hello.is_some_and(speaks_protocol) {
            return Err(ErrorCode::Protocol {
                server: PROTOCOL_VERSION,
            });
        }
        let hash = auth_key.hash();
        let existing = db(self.app.db.account_by_auth(&hash).await)?;
        let existing = match existing {
            Some(account) if new.is_none() && !account.banned => {
                let (device, code) = known.unwrap_or_default();
                return self.sign_in_known(account, password, device, code).await;
            }
            other => other,
        };
        let key = address_key(self.ip);
        let wrong = &self.app.wrong_tries;
        let unknown = &self.app.unknown_tries;
        if wrong.blocked(key) || unknown.blocked(key) {
            self.refused(Refused::SignIns);
            return Err(ErrorCode::TooManyTries);
        }
        wrong.fail(key);
        match self
            .app
            .settings
            .check_password(password.map(|p| p.0))
            .await
        {
            Check::Right => wrong.forgive(key),
            Check::Wrong => return Err(ErrorCode::WrongPassword),
            Check::Busy => {
                wrong.forgive(key);
                self.refused(Refused::SignIns);
                return Err(ErrorCode::TooManyTries);
            }
        }
        let account = match (new, existing) {
            (Some(_), Some(_)) => return Err(ErrorCode::AccountExists),
            (Some((public_key, name, invite)), None) => {
                let settings = self.app.settings.get();
                if !settings.sign_ups {
                    return Err(ErrorCode::SignUpsClosed);
                }
                let name = display_name(&name).map_err(|_| ErrorCode::BadName)?;
                let rates = &self.app.rates;
                if !rates.sign_ups.has(key, 1.0) {
                    self.refused(Refused::SignUps);
                    return Err(ErrorCode::TooManyTries);
                }
                if !settings.invites && !rates.all_sign_ups.has((), 1.0) {
                    self.refused(Refused::SignUps);
                    return Err(ErrorCode::SignUpsClosed);
                }
                let account = if !settings.invites {
                    let account = db(self
                        .app
                        .db
                        .create_account(&hash, &public_key, &name, now())
                        .await)?;
                    rates.all_sign_ups.take((), 1.0);
                    account
                } else {
                    let invite = invite.ok_or(ErrorCode::InviteNeeded)?;
                    let made = self
                        .app
                        .db
                        .create_account_with_invite(&hash, &public_key, &name, now(), invite)
                        .await;
                    match db(made)? {
                        Ok(account) => account,
                        Err(refusal) => {
                            wrong.fail(key);
                            return Err(match refusal {
                                InviteRefusal::Wrong => ErrorCode::InviteWrong,
                                InviteRefusal::Used => ErrorCode::InviteUsed,
                            });
                        }
                    }
                };
                rates.sign_ups.take(key, 1.0);
                account
            }
            (None, Some(account)) => account,
            (None, None) => {
                unknown.fail(key);
                return Err(ErrorCode::UnknownAccount);
            }
        };
        if account.banned {
            return Err(ErrorCode::Banned);
        }
        self.admit(&account, None, None).await?;
        self.enter(account).await
    }

    async fn sign_in_known(
        &mut self,
        account: AccountRow,
        password: Option<RelayPassword>,
        device: Option<DeviceKey>,
        code: Option<OneTimeCode>,
    ) -> Result<(), ErrorCode> {
        let tries = &self.app.account_tries;
        if tries.blocked(account.id) {
            self.refused(Refused::SignIns);
            return Err(ErrorCode::TooManyTries);
        }
        tries.fail(account.id);
        match self
            .app
            .settings
            .check_password(password.map(|p| p.0))
            .await
        {
            Check::Right => {
                let admitted = self.admit(&account, device, code).await;
                let tries = &self.app.account_tries;
                match admitted {
                    Err(ErrorCode::CodeNeeded) => tries.forgive(account.id),
                    Err(ErrorCode::CodeWrong | ErrorCode::TooManyTries) => {}
                    _ => tries.clear(account.id),
                }
                admitted?;
                self.enter(account).await
            }
            Check::Wrong => Err(ErrorCode::WrongPassword),
            Check::Busy => {
                tries.forgive(account.id);
                self.refused(Refused::SignIns);
                Err(ErrorCode::TooManyTries)
            }
        }
    }

    async fn enter(&mut self, account: AccountRow) -> Result<(), ErrorCode> {
        let presence = Presence::Online { away: false };
        let device = self.device.ok_or(ErrorCode::Server)?;
        self.slot.signed_in();
        self.me = Some(Me {
            id: account.id,
            online_id: account.online_id,
        });
        let before = self.app.hub.presence(account.id);
        let online = self.app.hub.join(
            account.id,
            Conn {
                conn_id: self.conn_id,
                device,
                outbox: self.outbox.clone(),
                close: self.close.clone(),
                presence,
                ip: self.ip,
                protocol: self
                    .hello
                    .unwrap_or(citrine_gb_net::OLDEST_PROTOCOL_VERSION),
                person: Some(Person {
                    online_id: account.online_id,
                    display_name: account.display_name.clone(),
                    public_key: account.public_key,
                }),
            },
        );
        self.app.stats.connected(online);
        db(self.app.db.set_last_seen(account.id, now()).await)?;
        let welcome = self.account(&account).await?;
        self.send(ServerMsg::Welcome(welcome)).await;
        let friends = db(self.app.db.relations(account.id).await)?
            .into_iter()
            .map(|(row, state)| self.friend(&row, state))
            .collect();
        self.send(ServerMsg::Friends(friends)).await;
        let mut after = 0;
        loop {
            let page = db(self
                .app
                .db
                .pending_messages(device, after, MESSAGE_PAGE)
                .await)?;
            let full = page.len() as u32 == MESSAGE_PAGE;
            for stored in page {
                after = stored.id;
                let message = ChatMessage {
                    id: stored.id,
                    from: stored.from,
                    at: stored.at,
                    sealed: stored.sealed,
                };
                if let Some(key) = stored.old_key {
                    self.send(ServerMsg::OldChat {
                        id: stored.id,
                        chat: OldChat {
                            friend: stored.to,
                            key,
                            at: stored.at,
                            sealed: message.sealed,
                        },
                    })
                    .await;
                    continue;
                }
                self.send(if stored.from_id == account.id {
                    ServerMsg::ChatCopy {
                        to: stored.to,
                        message,
                    }
                } else {
                    ServerMsg::Chat(message)
                })
                .await;
            }
            if !full {
                break;
            }
        }
        self.send_devices().await?;
        if db(self.app.db.old_chats_wanted(device).await)? {
            self.send(ServerMsg::OldChatsWanted).await;
        }
        if before != Some(presence) {
            self.tell_friends(presence).await?;
        }
        Ok(())
    }

    pub(super) async fn send_devices(&self) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let rows = db(self.app.db.devices_of(me).await)?;
        let online = self.app.hub.online_devices(me);
        let list: Vec<DeviceInfo> = rows
            .into_iter()
            .map(|row| DeviceInfo {
                id: row.id as DeviceId,
                online: online.contains(&row.id),
                name: row.name,
                added: row.added,
                last_seen: row.last_seen,
            })
            .collect();
        self.app.hub.send_each(me, |device| ServerMsg::Devices {
            list: list.clone(),
            this: device as DeviceId,
        });
        Ok(())
    }

    pub(super) async fn sign_out_device(&mut self, device: DeviceId) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let device = i64::try_from(device).map_err(|_| ErrorCode::UnknownDevice)?;
        if Some(device) == self.device {
            db(self.app.db.leave_device(me, device, now()).await)?;
            self.close.send_replace(Some(Notice::Disconnected));
            return Ok(());
        }
        if !db(self.app.db.sign_out_device(me, device, now()).await)? {
            return Err(ErrorCode::UnknownDevice);
        }
        log::info!("an account signed out one of its devices");
        self.app.hub.kick_device(me, device, Notice::SignedOut);
        self.send_devices().await
    }
}
