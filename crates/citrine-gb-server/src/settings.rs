use crate::config::Config;
use crate::db::{self, Db};
use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use subtle::ConstantTimeEq;

pub const NAME: &str = "name";
pub const RELAY: &str = "relay";
pub const KILL: &str = "kill_switch";
pub const PASSWORD: &str = "relay_password";
pub const SIGN_UPS: &str = "sign_ups";
pub const INVITES: &str = "invites";
pub const PICTURES: &str = "discord_pictures";
pub const QUOTA: &str = "default_quota_mb";
pub const MAX_MESSAGE: &str = "max_message_kb";
pub const MAX_FILE: &str = "max_file_mb";

pub const DEFAULT_MAX_MESSAGE_KB: u64 = 64;
pub const DEFAULT_MAX_FILE_MB: u64 = 25;
const NO_PASSWORD: &str = "none";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Default,
    Env,
    Panel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub name: String,
    pub relay: bool,
    pub kill: bool,
    pub password_set: bool,
    pub sign_ups: bool,
    pub invites: bool,
    pub pictures: bool,
    pub quota_bytes: u64,
    pub max_message_bytes: u64,
    pub max_file_bytes: u64,
}

pub fn hash(password: &str) -> [u8; 32] {
    Sha256::digest(password.as_bytes()).into()
}

pub fn same_secret(given: &str, expected: &[u8; 32]) -> bool {
    hash(given).ct_eq(expected).into()
}

pub fn hash_password(password: &str) -> String {
    Argon2::default()
        .hash_password(password.as_bytes())
        .expect("argon2 hashes with default params")
        .to_string()
}

pub fn verify_password(given: &str, stored: &str) -> bool {
    Argon2::default()
        .verify_password(given.as_bytes(), stored)
        .is_ok()
}

enum Stored {
    Argon(String),
    Sha256([u8; 32]),
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let mut out = [0; 32];
    if text.len() != 64 {
        return None;
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn flag(value: &str) -> Option<bool> {
    match value {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

pub struct Settings {
    config: Config,
    env_password: Option<String>,
    panel: RwLock<HashMap<String, String>>,
    db: Arc<Db>,
}

impl Settings {
    pub async fn load(config: &Config, db: Arc<Db>) -> db::Result<Self> {
        let panel = db.settings().await?;
        Ok(Self {
            env_password: config.relay_password.as_deref().map(hash_password),
            config: config.clone(),
            panel: RwLock::new(panel),
            db,
        })
    }

    fn panel(&self, key: &str) -> Option<String> {
        self.panel
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
    }

    fn panel_flag(&self, key: &str) -> Option<bool> {
        self.panel(key).as_deref().and_then(flag)
    }

    fn panel_number(&self, key: &str) -> Option<u64> {
        self.panel(key).and_then(|v| v.parse().ok())
    }

    fn stored_password(&self) -> Option<Stored> {
        let env = || self.env_password.clone().map(Stored::Argon);
        match self.panel(PASSWORD) {
            Some(value) if value == NO_PASSWORD => None,
            Some(value) if value.starts_with("$argon2") => Some(Stored::Argon(value)),
            Some(value) => unhex(&value).map(Stored::Sha256).or_else(env),
            None => env(),
        }
    }

    pub fn default_name(&self) -> String {
        self.config
            .public_url
            .as_deref()
            .map(|url| url.split("://").last().unwrap_or(url))
            .and_then(|rest| rest.split(['/', ':']).next())
            .filter(|host| !host.is_empty())
            .unwrap_or("Citrine")
            .to_string()
    }

    pub fn get(&self) -> Effective {
        Effective {
            name: self.panel(NAME).unwrap_or_else(|| self.default_name()),
            relay: self.panel_flag(RELAY).unwrap_or(self.config.relay),
            kill: self.panel_flag(KILL).unwrap_or(false),
            password_set: self.stored_password().is_some(),
            sign_ups: self.panel_flag(SIGN_UPS).unwrap_or(true),
            invites: self.panel_flag(INVITES).unwrap_or(false),
            pictures: self.panel_flag(PICTURES).unwrap_or(true),
            quota_bytes: self.panel_number(QUOTA).unwrap_or(self.config.storage_mb) << 20,
            max_message_bytes: self
                .panel_number(MAX_MESSAGE)
                .unwrap_or(DEFAULT_MAX_MESSAGE_KB)
                << 10,
            max_file_bytes: self.panel_number(MAX_FILE).unwrap_or(DEFAULT_MAX_FILE_MB) << 20,
        }
    }

    pub fn source(&self, key: &str) -> Source {
        if self.panel(key).is_some() {
            return Source::Panel;
        }
        let var = match key {
            RELAY => "CITRINE_RELAY",
            PASSWORD => "CITRINE_RELAY_PASSWORD",
            QUOTA => "CITRINE_STORAGE_MB",
            NAME => "CITRINE_PUBLIC_URL",
            _ => return Source::Default,
        };
        if self.config.given.contains(var) {
            Source::Env
        } else {
            Source::Default
        }
    }

    pub fn relay_open_allowed(&self) -> bool {
        self.config.relay_open
    }

    pub async fn check_password(&self, given: Option<String>) -> bool {
        let given = given.unwrap_or_default();
        let (ok, upgrade) = match self.stored_password() {
            None => return true,
            Some(Stored::Argon(stored)) => {
                let ok =
                    tokio::task::spawn_blocking(move || verify_password(&given, &stored)).await;
                (ok.unwrap_or(false), None)
            }
            Some(Stored::Sha256(expected)) => {
                if !same_secret(&given, &expected) {
                    return false;
                }
                let hashed = tokio::task::spawn_blocking(move || hash_password(&given)).await;
                (true, hashed.ok())
            }
        };
        if let Some(hashed) = upgrade {
            match self.set(PASSWORD, &hashed).await {
                Ok(()) => log::info!("relay password hash upgraded to Argon2id"),
                Err(e) => log::error!("database: {e}"),
            }
        }
        ok
    }

    pub async fn set(&self, key: &str, value: &str) -> db::Result<()> {
        self.db.set_setting(key, value).await?;
        self.panel
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    pub async fn set_flag(&self, key: &str, on: bool) -> db::Result<()> {
        self.set(key, if on { "on" } else { "off" }).await
    }

    pub async fn set_password(&self, password: Option<&str>) -> db::Result<()> {
        match password {
            Some(p) => self.set(PASSWORD, &hash_password(p)).await,
            None => self.set(PASSWORD, NO_PASSWORD).await,
        }
    }

    pub async fn reset(&self, key: &str) -> db::Result<()> {
        self.db.delete_setting(key).await?;
        self.panel
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn settings(vars: &[(&str, &str)]) -> (tempfile::TempDir, Settings) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(&dir.path().join("t.db")).await.unwrap());
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let config = Config::from_vars(|k| map.get(k).cloned()).unwrap();
        (dir, Settings::load(&config, db).await.unwrap())
    }

    #[tokio::test]
    async fn panel_wins_over_env_until_reset() {
        let (_dir, s) = settings(&[("CITRINE_STORAGE_MB", "50")]).await;
        assert_eq!(s.get().quota_bytes, 50 << 20);
        assert_eq!(s.source(QUOTA), Source::Env);
        s.set(QUOTA, "7").await.unwrap();
        assert_eq!(s.get().quota_bytes, 7 << 20);
        assert_eq!(s.source(QUOTA), Source::Panel);
        s.reset(QUOTA).await.unwrap();
        assert_eq!(s.get().quota_bytes, 50 << 20);
    }

    #[tokio::test]
    async fn password_from_env_then_panel_then_reset() {
        let (_dir, s) = settings(&[("CITRINE_RELAY_PASSWORD", "env-pw")]).await;
        assert!(s.check_password(Some("env-pw".into())).await);
        assert!(!s.check_password(Some("env-pW".into())).await);
        assert!(!s.check_password(None).await);
        s.set_password(Some("panel-pw")).await.unwrap();
        assert!(s.check_password(Some("panel-pw".into())).await);
        assert!(!s.check_password(Some("env-pw".into())).await);
        s.reset(PASSWORD).await.unwrap();
        assert!(s.check_password(Some("env-pw".into())).await);
    }

    #[tokio::test]
    async fn panel_password_is_stored_as_argon2id() {
        let (_dir, s) = settings(&[]).await;
        s.set_password(Some("panel-pw")).await.unwrap();
        let stored = s.db.settings().await.unwrap()[PASSWORD].clone();
        assert!(stored.starts_with("$argon2id$"));
        assert!(!stored.contains("panel-pw"));
    }

    #[tokio::test]
    async fn an_old_sha256_hash_verifies_once_then_upgrades() {
        let (_dir, s) = settings(&[]).await;
        let old: String = hash("old-pw").iter().map(|b| format!("{b:02x}")).collect();
        s.set(PASSWORD, &old).await.unwrap();
        assert!(!s.check_password(Some("old-pW".into())).await);
        assert_eq!(s.db.settings().await.unwrap()[PASSWORD], old);
        assert!(s.check_password(Some("old-pw".into())).await);
        let upgraded = s.db.settings().await.unwrap()[PASSWORD].clone();
        assert!(upgraded.starts_with("$argon2id$"));
        assert!(s.check_password(Some("old-pw".into())).await);
        assert!(!s.check_password(Some("old-pW".into())).await);
        assert!(s.get().password_set);
    }

    #[tokio::test]
    async fn no_password_lets_anyone_in() {
        let (_dir, s) = settings(&[]).await;
        assert!(!s.get().password_set);
        assert!(s.check_password(None).await);
        assert!(s.check_password(Some("anything".into())).await);
    }

    #[tokio::test]
    async fn name_defaults_to_the_public_host() {
        let (_dir, s) = settings(&[("CITRINE_PUBLIC_URL", "https://gb.example.com:8443/x")]).await;
        assert_eq!(s.get().name, "gb.example.com");
        let (_dir, s) = settings(&[]).await;
        assert_eq!(s.get().name, "Citrine");
    }
}
