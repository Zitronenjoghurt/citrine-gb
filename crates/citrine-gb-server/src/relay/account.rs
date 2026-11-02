use super::{Client, db, file_path};
use crate::connections::address_key;
use crate::db::{AccountRow, KnownDevice};
use crate::now;
use crate::stats::Refused;
use crate::two_factor::{RECOVERY_CODES, new_recovery_codes, new_secret, step_of, verify_account};
use citrine_gb_net::{DeviceKey, ErrorCode, Notice, OneTimeCode, ServerMsg};

impl Client {
    pub(super) async fn admit(
        &mut self,
        account: &AccountRow,
        device: Option<DeviceKey>,
        code: Option<OneTimeCode>,
    ) -> Result<(), ErrorCode> {
        let now = now();
        if let Some(device) = &device {
            let known = self
                .app
                .db
                .known_device(account.id, &device.hash(), &self.device_name, now)
                .await;
            match db(known)? {
                KnownDevice::Known(id) => {
                    self.device = Some(id);
                    return Ok(());
                }
                KnownDevice::SignedOut => return Err(ErrorCode::SignedOut),
                KnownDevice::Unknown => {}
            }
        }
        if db(self.app.db.account_totp(account.id).await)?.is_some() {
            let code = code.ok_or(ErrorCode::CodeNeeded)?;
            let ip = address_key(self.ip);
            if self.app.wrong_tries.blocked(ip) {
                self.refused(Refused::SignIns);
                return Err(ErrorCode::TooManyTries);
            }
            if !db(verify_account(&self.app.db, account.id, &code.0, now).await)? {
                self.app.wrong_tries.fail(ip);
                return Err(ErrorCode::CodeWrong);
            }
        }
        let key = DeviceKey::random();
        let added = self
            .app
            .db
            .add_device(account.id, &key.hash(), &self.device_name, now)
            .await;
        let id = db(added)?;
        self.device = Some(id);
        self.send(ServerMsg::Device(key)).await;
        Ok(())
    }

    async fn check_code(&self, code: &OneTimeCode) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        let tries = &self.app.account_tries;
        let ip = address_key(self.ip);
        if tries.blocked(me) || self.app.wrong_tries.blocked(ip) {
            self.refused(Refused::SignIns);
            return Err(ErrorCode::TooManyTries);
        }
        if db(verify_account(&self.app.db, me, &code.0, now()).await)? {
            tries.clear(me);
            Ok(())
        } else {
            tries.fail(me);
            self.app.wrong_tries.fail(ip);
            Err(ErrorCode::CodeWrong)
        }
    }

    async fn is_on(&self) -> Result<bool, ErrorCode> {
        Ok(db(self.app.db.account_totp(self.me()?.id).await)?.is_some())
    }

    async fn recovery_codes(&self) -> Result<(Vec<String>, Vec<String>), ErrorCode> {
        let (codes, hashes) = tokio::task::spawn_blocking(new_recovery_codes)
            .await
            .map_err(|_| ErrorCode::Server)?;
        if hashes.len() != RECOVERY_CODES {
            return Err(ErrorCode::Server);
        }
        Ok((codes, hashes))
    }

    pub(super) async fn two_factor_start(&mut self) -> Result<(), ErrorCode> {
        if self.is_on().await? {
            return Err(ErrorCode::BadMessage);
        }
        let secret = new_secret();
        self.pending_totp = Some(secret.clone());
        self.send(ServerMsg::TwoFactorSetup { secret }).await;
        Ok(())
    }

    pub(super) async fn two_factor_confirm(&mut self, code: OneTimeCode) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        if self.is_on().await? {
            return Err(ErrorCode::BadMessage);
        }
        let secret = self.pending_totp.clone().ok_or(ErrorCode::BadMessage)?;
        let tries = &self.app.account_tries;
        if tries.blocked(me) {
            self.refused(Refused::SignIns);
            return Err(ErrorCode::TooManyTries);
        }
        let Some(step) = step_of(&secret, &code.0, now()) else {
            tries.fail(me);
            return Err(ErrorCode::CodeWrong);
        };
        tries.clear(me);
        let (codes, hashes) = self.recovery_codes().await?;
        db(self
            .app
            .db
            .set_account_totp(me, &secret, step, &hashes)
            .await)?;
        self.pending_totp = None;
        log::info!("an account turned two-factor sign-in on");
        self.send(ServerMsg::RecoveryCodes(codes)).await;
        self.send_account().await
    }

    pub(super) async fn two_factor_off(&mut self, code: OneTimeCode) -> Result<(), ErrorCode> {
        if !self.is_on().await? {
            return self.send_account().await;
        }
        self.check_code(&code).await?;
        db(self.app.db.clear_account_totp(self.me()?.id).await)?;
        log::info!("an account turned two-factor sign-in off");
        self.send_account().await
    }

    pub(super) async fn new_recovery_codes(&mut self, code: OneTimeCode) -> Result<(), ErrorCode> {
        if !self.is_on().await? {
            return Err(ErrorCode::BadMessage);
        }
        self.check_code(&code).await?;
        let (codes, hashes) = self.recovery_codes().await?;
        db(self
            .app
            .db
            .replace_account_recovery_codes(self.me()?.id, &hashes)
            .await)?;
        self.send(ServerMsg::RecoveryCodes(codes)).await;
        self.send_account().await
    }

    pub(super) async fn delete_account(
        &mut self,
        code: Option<OneTimeCode>,
    ) -> Result<(), ErrorCode> {
        let me = self.me()?.id;
        if self.is_on().await? {
            self.check_code(&code.ok_or(ErrorCode::CodeNeeded)?).await?;
        }
        let files = db(self.app.db.delete_account(me).await)?.unwrap_or_default();
        for file in files {
            let _ = tokio::fs::remove_file(file_path(&self.app, file)).await;
        }
        log::info!("an account deleted itself");
        self.send(ServerMsg::AccountDeleted).await;
        self.app
            .hub
            .send_others(me, self.conn_id, ServerMsg::AccountDeleted);
        self.app.hub.kick(me, Notice::Disconnected);
        Ok(())
    }
}
