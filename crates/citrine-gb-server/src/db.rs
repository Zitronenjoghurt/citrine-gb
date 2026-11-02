use citrine_gb_net::{FileId, OnlineId, PublicKey};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{SqliteConnection, SqlitePool};
use std::path::Path;

mod accounts;
mod devices;
mod files;
mod friends;
mod invites;
mod messages;
mod saves;
mod security;
mod settings;

pub use devices::KnownDevice;
pub use invites::{InviteFilter, InviteRefusal, InviteRow};
pub use saves::{NewSave, SaveWrite};

pub type Result<T> = sqlx::Result<T>;

static MIGRATOR: Migrator = sqlx::migrate!();

const RUSQLITE_MIGRATIONS: i64 = 2;

#[derive(Debug, Clone)]
pub struct AccountRow {
    pub id: i64,
    pub online_id: OnlineId,
    pub public_key: PublicKey,
    pub display_name: String,
    pub last_seen: u64,
    pub quota_bytes: Option<u64>,
    pub banned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub bytes: u64,
    pub files: u64,
    pub saves: u64,
}

#[derive(Debug, Clone)]
pub struct UserRow {
    pub account: AccountRow,
    pub joined: u64,
    pub storage_used: u64,
    pub files: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserOrder {
    LastSeen,
    Newest,
}

struct AccountRecord {
    id: i64,
    online_id: i64,
    public_key: Vec<u8>,
    display_name: String,
    last_seen: i64,
    quota_bytes: Option<i64>,
    banned: bool,
}

impl From<AccountRecord> for AccountRow {
    fn from(r: AccountRecord) -> Self {
        let mut public_key = [0; 32];
        if r.public_key.len() == 32 {
            public_key.copy_from_slice(&r.public_key);
        }
        AccountRow {
            id: r.id,
            online_id: online_id(r.online_id),
            public_key: PublicKey(public_key),
            display_name: r.display_name,
            last_seen: r.last_seen as u64,
            quota_bytes: r.quota_bytes.map(|q| q as u64),
            banned: r.banned,
        }
    }
}

fn online_id(value: i64) -> OnlineId {
    OnlineId::new(value as u64).unwrap_or(OnlineId::new(0).expect("zero fits"))
}

fn file_id(bytes: Vec<u8>) -> FileId {
    let mut id = [0; 16];
    if bytes.len() == 16 {
        id.copy_from_slice(&bytes);
    }
    FileId(id)
}

fn ordered(a: i64, b: i64) -> (i64, i64) {
    if a < b { (a, b) } else { (b, a) }
}

fn like_pattern(search: &str) -> String {
    format!(
        "%{}%",
        search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

async fn insert_account(
    conn: &mut SqliteConnection,
    auth_hash: &[u8; 32],
    public_key: &PublicKey,
    name: &str,
    now: u64,
) -> Result<i64> {
    let auth_hash = &auth_hash[..];
    let public_key = &public_key.0[..];
    let now = now as i64;
    loop {
        let online = OnlineId::random().value() as i64;
        let done = sqlx::query!(
            "INSERT INTO accounts (online_id, auth_hash, public_key, display_name, created_at, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT (online_id) DO NOTHING",
            online,
            auth_hash,
            public_key,
            name,
            now,
        )
        .execute(&mut *conn)
        .await?;
        if done.rows_affected() == 1 {
            return Ok(done.last_insert_rowid());
        }
    }
}

pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .auto_vacuum(SqliteAutoVacuum::Incremental)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .synchronous(SqliteSynchronous::Normal);
        let db = Self {
            pool: SqlitePool::connect_with(options).await?,
        };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        let tracked = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations') AS "tracked!: bool""#
        )
        .fetch_one(&self.pool)
        .await?;
        if !tracked {
            let version: i64 = sqlx::query_scalar("PRAGMA user_version")
                .fetch_one(&self.pool)
                .await?;
            if version > 0 {
                MIGRATOR
                    .skip(&self.pool, Some(version.min(RUSQLITE_MIGRATIONS)))
                    .await?;
            }
        }
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    pub async fn reclaim(&self) -> Result<u64> {
        let free: i64 = sqlx::query_scalar("PRAGMA freelist_count")
            .fetch_one(&self.pool)
            .await?;
        if free > 0 {
            sqlx::raw_sql("PRAGMA incremental_vacuum")
                .execute(&self.pool)
                .await?;
        }
        sqlx::raw_sql("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await?;
        let page: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&self.pool)
            .await?;
        Ok((free.max(0) * page.max(0)) as u64)
    }

    pub async fn back_up(&self, to: &Path) -> Result<()> {
        sqlx::query("VACUUM INTO ?1")
            .bind(to.to_string_lossy().into_owned())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn size(&self) -> Result<u64> {
        let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&self.pool)
            .await?;
        let page: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&self.pool)
            .await?;
        Ok((pages.max(0) * page.max(0)) as u64)
    }
}

#[cfg(test)]
mod tests;
