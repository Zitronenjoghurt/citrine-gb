use citrine_gb_net::{FileId, FileInfo, FriendState, MessageId, OnlineId, PublicKey, Sealed};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{SqliteConnection, SqlitePool};
use std::collections::HashMap;
use std::path::Path;

mod invites;
mod saves;

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

#[derive(Debug, Clone)]
pub struct UserRow {
    pub account: AccountRow,
    pub storage_used: u64,
    pub files: u64,
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

    pub async fn create_account(
        &self,
        auth_hash: &[u8; 32],
        public_key: &PublicKey,
        name: &str,
        now: u64,
    ) -> Result<AccountRow> {
        let mut conn = self.pool.acquire().await?;
        let id = insert_account(&mut conn, auth_hash, public_key, name, now).await?;
        drop(conn);
        self.account(id).await?.ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn account_by_auth(&self, auth_hash: &[u8; 32]) -> Result<Option<AccountRow>> {
        let auth_hash = &auth_hash[..];
        let row = sqlx::query_as!(
            AccountRecord,
            r#"SELECT id AS "id!", online_id, public_key, display_name, last_seen, quota_bytes, banned AS "banned: bool"
               FROM accounts WHERE auth_hash = ?1"#,
            auth_hash,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(AccountRow::from))
    }

    pub async fn account_by_online(&self, online: OnlineId) -> Result<Option<AccountRow>> {
        let online = online.value() as i64;
        let row = sqlx::query_as!(
            AccountRecord,
            r#"SELECT id AS "id!", online_id, public_key, display_name, last_seen, quota_bytes, banned AS "banned: bool"
               FROM accounts WHERE online_id = ?1"#,
            online,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(AccountRow::from))
    }

    pub async fn account(&self, id: i64) -> Result<Option<AccountRow>> {
        let row = sqlx::query_as!(
            AccountRecord,
            r#"SELECT id AS "id!", online_id, public_key, display_name, last_seen, quota_bytes, banned AS "banned: bool"
               FROM accounts WHERE id = ?1"#,
            id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(AccountRow::from))
    }

    pub async fn set_display_name(&self, id: i64, name: &str) -> Result<()> {
        sqlx::query!(
            "UPDATE accounts SET display_name = ?2 WHERE id = ?1",
            id,
            name
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_last_seen(&self, id: i64, now: u64) -> Result<()> {
        let now = now as i64;
        sqlx::query!("UPDATE accounts SET last_seen = ?2 WHERE id = ?1", id, now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_quota(&self, id: i64, quota: Option<u64>) -> Result<()> {
        let quota = quota.map(|q| q as i64);
        sqlx::query!(
            "UPDATE accounts SET quota_bytes = ?2 WHERE id = ?1",
            id,
            quota
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_banned(&self, id: i64, banned: bool) -> Result<()> {
        sqlx::query!("UPDATE accounts SET banned = ?2 WHERE id = ?1", id, banned)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn count_accounts(&self) -> Result<u64> {
        let n = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "n!: i64" FROM accounts"#)
            .fetch_one(&self.pool)
            .await?;
        Ok(n as u64)
    }

    pub async fn users(
        &self,
        search: &str,
        offset: u64,
        limit: u64,
    ) -> Result<(Vec<UserRow>, u64)> {
        let pattern = like_pattern(search);
        let online = search.parse::<OnlineId>().ok().map(|o| o.value() as i64);
        let (limit, offset) = (limit as i64, offset as i64);
        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM accounts
               WHERE (?1 = '%%' OR display_name LIKE ?1 ESCAPE '\' OR online_id = ?2)"#,
            pattern,
            online,
        )
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", online_id, public_key, display_name, last_seen, quota_bytes, banned AS "banned: bool",
                   (SELECT COALESCE(SUM(size), 0) FROM files WHERE owner = accounts.id) AS "storage_used!: i64",
                   (SELECT COUNT(*) FROM files WHERE owner = accounts.id) AS "files!: i64"
               FROM accounts
               WHERE (?1 = '%%' OR display_name LIKE ?1 ESCAPE '\' OR online_id = ?2)
               ORDER BY last_seen DESC, id DESC LIMIT ?3 OFFSET ?4"#,
            pattern,
            online,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| UserRow {
            account: AccountRecord {
                id: r.id,
                online_id: r.online_id,
                public_key: r.public_key,
                display_name: r.display_name,
                last_seen: r.last_seen,
                quota_bytes: r.quota_bytes,
                banned: r.banned,
            }
            .into(),
            storage_used: r.storage_used as u64,
            files: r.files as u64,
        })
        .collect();
        Ok((rows, total as u64))
    }

    pub async fn are_friends(&self, a: i64, b: i64) -> Result<bool> {
        let (a, b) = ordered(a, b);
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM friendships WHERE a = ?1 AND b = ?2) AS "yes!: bool""#,
            a,
            b,
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn has_request(&self, from: i64, to: i64) -> Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM friend_requests WHERE from_id = ?1 AND to_id = ?2) AS "yes!: bool""#,
            from,
            to,
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn is_blocked(&self, blocker: i64, blocked: i64) -> Result<bool> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM blocks WHERE blocker = ?1 AND blocked = ?2) AS "yes!: bool""#,
            blocker,
            blocked,
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn add_request(&self, from: i64, to: i64, now: u64) -> Result<()> {
        let now = now as i64;
        sqlx::query!(
            "INSERT OR IGNORE INTO friend_requests (from_id, to_id, created_at) VALUES (?1, ?2, ?3)",
            from,
            to,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn befriend(&self, a: i64, b: i64, now: u64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "DELETE FROM friend_requests WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)",
            a,
            b,
        )
        .execute(&mut *tx)
        .await?;
        let (x, y) = ordered(a, b);
        let now = now as i64;
        sqlx::query!(
            "INSERT OR IGNORE INTO friendships (a, b, since) VALUES (?1, ?2, ?3)",
            x,
            y,
            now,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    pub async fn unfriend(&self, a: i64, b: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "DELETE FROM friend_requests WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)",
            a,
            b,
        )
        .execute(&mut *tx)
        .await?;
        let (x, y) = ordered(a, b);
        sqlx::query!("DELETE FROM friendships WHERE a = ?1 AND b = ?2", x, y)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    pub async fn block(&self, blocker: i64, blocked: i64) -> Result<()> {
        self.unfriend(blocker, blocked).await?;
        sqlx::query!(
            "INSERT OR IGNORE INTO blocks (blocker, blocked) VALUES (?1, ?2)",
            blocker,
            blocked,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn unblock(&self, blocker: i64, blocked: i64) -> Result<()> {
        sqlx::query!(
            "DELETE FROM blocks WHERE blocker = ?1 AND blocked = ?2",
            blocker,
            blocked,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn friend_ids(&self, id: i64) -> Result<Vec<i64>> {
        sqlx::query_scalar!(
            r#"SELECT b AS "id!: i64" FROM friendships WHERE a = ?1 UNION SELECT a FROM friendships WHERE b = ?1"#,
            id,
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn relations(&self, id: i64) -> Result<Vec<(AccountRow, FriendState)>> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!: i64", online_id AS "online_id!: i64", public_key AS "public_key!: Vec<u8>",
                   display_name AS "display_name!: String", last_seen AS "last_seen!: i64",
                   quota_bytes AS "quota_bytes?: i64", banned AS "banned!: bool", 0 AS "state!: i64"
               FROM accounts WHERE id IN
                   (SELECT b FROM friendships WHERE a = ?1 UNION SELECT a FROM friendships WHERE b = ?1)
               UNION ALL
               SELECT id, online_id, public_key, display_name, last_seen, quota_bytes, banned, 1
               FROM accounts WHERE id IN
                   (SELECT from_id FROM friend_requests WHERE to_id = ?1
                    AND from_id NOT IN (SELECT blocked FROM blocks WHERE blocker = ?1))
               UNION ALL
               SELECT id, online_id, public_key, display_name, last_seen, quota_bytes, banned, 2
               FROM accounts WHERE id IN
                   (SELECT to_id FROM friend_requests WHERE from_id = ?1)
               UNION ALL
               SELECT id, online_id, public_key, display_name, last_seen, quota_bytes, banned, 3
               FROM accounts WHERE id IN
                   (SELECT blocked FROM blocks WHERE blocker = ?1)"#,
            id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let state = match r.state {
                    0 => FriendState::Friend,
                    1 => FriendState::Incoming,
                    2 => FriendState::Outgoing,
                    _ => FriendState::Blocked,
                };
                let account = AccountRecord {
                    id: r.id,
                    online_id: r.online_id,
                    public_key: r.public_key,
                    display_name: r.display_name,
                    last_seen: r.last_seen,
                    quota_bytes: r.quota_bytes,
                    banned: r.banned,
                };
                (account.into(), state)
            })
            .collect())
    }

    pub async fn insert_message(
        &self,
        from: i64,
        to: i64,
        at: u64,
        sealed: &Sealed,
    ) -> Result<MessageId> {
        let at = at as i64;
        let nonce = &sealed.nonce[..];
        let done = sqlx::query!(
            "INSERT INTO messages (from_id, to_id, sent_at, nonce, body) VALUES (?1, ?2, ?3, ?4, ?5)",
            from,
            to,
            at,
            nonce,
            sealed.ciphertext,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.last_insert_rowid() as MessageId)
    }

    pub async fn pending_messages(
        &self,
        to: i64,
    ) -> Result<Vec<(MessageId, OnlineId, u64, Sealed)>> {
        let rows = sqlx::query!(
            r#"SELECT m.id AS "id!: i64", a.online_id AS "online_id!: i64", m.sent_at AS "sent_at!: i64",
                   m.nonce AS "nonce!: Vec<u8>", m.body AS "body!: Vec<u8>"
               FROM messages m JOIN accounts a ON a.id = m.from_id
               WHERE m.to_id = ?1 ORDER BY m.id"#,
            to,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let mut nonce = [0; citrine_gb_net::NONCE_BYTES];
                if r.nonce.len() == nonce.len() {
                    nonce.copy_from_slice(&r.nonce);
                }
                (
                    r.id as MessageId,
                    online_id(r.online_id),
                    r.sent_at as u64,
                    Sealed {
                        nonce,
                        ciphertext: r.body,
                    },
                )
            })
            .collect())
    }

    pub async fn ack_messages(&self, to: i64, ids: &[MessageId]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for &id in ids {
            let id = id as i64;
            sqlx::query!("DELETE FROM messages WHERE id = ?1 AND to_id = ?2", id, to)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }

    pub async fn purge_messages(&self, before: u64) -> Result<usize> {
        let before = before as i64;
        let done = sqlx::query!("DELETE FROM messages WHERE sent_at < ?1", before)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() as usize)
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

    pub async fn storage_used(&self, owner: i64) -> Result<u64> {
        let n = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(size), 0) AS "n!: i64" FROM files WHERE owner = ?1"#,
            owner,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(n as u64)
    }

    pub async fn total_storage(&self) -> Result<u64> {
        let n = sqlx::query_scalar!(r#"SELECT COALESCE(SUM(size), 0) AS "n!: i64" FROM files"#)
            .fetch_one(&self.pool)
            .await?;
        Ok(n as u64)
    }

    pub async fn insert_file(&self, id: FileId, owner: i64, size: u64, now: u64) -> Result<()> {
        let id = &id.0[..];
        let (size, now) = (size as i64, now as i64);
        sqlx::query!(
            "INSERT INTO files (id, owner, size, created_at) VALUES (?1, ?2, ?3, ?4)",
            id,
            owner,
            size,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn file(&self, id: FileId) -> Result<Option<(i64, FileInfo)>> {
        let key = &id.0[..];
        let row = sqlx::query!(
            "SELECT owner, size, created_at FROM files WHERE id = ?1",
            key
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            (
                r.owner,
                FileInfo {
                    id,
                    size: r.size as u64,
                    created: r.created_at as u64,
                },
            )
        }))
    }

    pub async fn files_of(&self, owner: i64) -> Result<Vec<FileInfo>> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", size, created_at FROM files
                WHERE owner = ?1 AND id NOT IN (SELECT file FROM saves)
                ORDER BY created_at, id"#,
            owner,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| FileInfo {
                id: file_id(r.id),
                size: r.size as u64,
                created: r.created_at as u64,
            })
            .collect())
    }

    pub async fn delete_file(&self, id: FileId, owner: i64) -> Result<bool> {
        let id = &id.0[..];
        let done = sqlx::query!("DELETE FROM files WHERE id = ?1 AND owner = ?2", id, owner)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn delete_files_of(&self, owner: i64) -> Result<Vec<FileId>> {
        let ids: Vec<FileId> = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM files WHERE owner = ?1 ORDER BY created_at, id"#,
            owner,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(file_id)
        .collect();
        sqlx::query!("DELETE FROM files WHERE owner = ?1", owner)
            .execute(&self.pool)
            .await?;
        Ok(ids)
    }

    pub async fn settings(&self) -> Result<HashMap<String, String>> {
        let rows = sqlx::query!(r#"SELECT key AS "key!", value FROM settings"#)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| (r.key, r.value)).collect())
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query!(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            key,
            value,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_setting(&self, key: &str) -> Result<()> {
        sqlx::query!("DELETE FROM settings WHERE key = ?1", key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn admin_totp(&self) -> Result<Option<Vec<u8>>> {
        sqlx::query_scalar!("SELECT secret FROM admin_totp WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn set_admin_totp(&self, secret: &[u8], step: u64, codes: &[String]) -> Result<()> {
        let step = step as i64;
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "INSERT INTO admin_totp (id, secret, last_step) VALUES (1, ?1, ?2)
             ON CONFLICT (id) DO UPDATE SET secret = excluded.secret, last_step = excluded.last_step",
            secret,
            step,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!("DELETE FROM admin_recovery_codes")
            .execute(&mut *tx)
            .await?;
        for hash in codes {
            sqlx::query!("INSERT INTO admin_recovery_codes (hash) VALUES (?1)", hash)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }

    pub async fn use_admin_step(&self, step: u64) -> Result<bool> {
        let step = step as i64;
        let done = sqlx::query!(
            "UPDATE admin_totp SET last_step = ?1 WHERE id = 1 AND last_step < ?1",
            step
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn admin_recovery_codes(&self) -> Result<Vec<(i64, String)>> {
        let rows =
            sqlx::query!(r#"SELECT id AS "id!", hash FROM admin_recovery_codes ORDER BY id"#)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|r| (r.id, r.hash)).collect())
    }

    pub async fn use_admin_recovery_code(&self, id: i64) -> Result<bool> {
        let done = sqlx::query!("DELETE FROM admin_recovery_codes WHERE id = ?1", id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn replace_admin_recovery_codes(&self, codes: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("DELETE FROM admin_recovery_codes")
            .execute(&mut *tx)
            .await?;
        for hash in codes {
            sqlx::query!("INSERT INTO admin_recovery_codes (hash) VALUES (?1)", hash)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }

    pub async fn clear_admin_totp(&self) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let removed = sqlx::query!("DELETE FROM admin_totp")
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query!("DELETE FROM admin_recovery_codes")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(removed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::migrate::{Migration, MigrationType};
    use sqlx::{AssertSqlSafe, SqlSafeStr};

    const RUSQLITE_SQL: &[&str] = &[
        r"
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    online_id INTEGER NOT NULL UNIQUE,
    auth_hash BLOB NOT NULL UNIQUE,
    public_key BLOB NOT NULL,
    display_name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    quota_bytes INTEGER,
    banned INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE friend_requests (
    from_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    to_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (from_id, to_id)
);
CREATE TABLE friendships (
    a INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    b INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    since INTEGER NOT NULL,
    PRIMARY KEY (a, b),
    CHECK (a < b)
);
CREATE TABLE blocks (
    blocker INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    blocked INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    PRIMARY KEY (blocker, blocked)
);
CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    from_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    to_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    sent_at INTEGER NOT NULL,
    nonce BLOB NOT NULL,
    body BLOB NOT NULL
);
CREATE INDEX messages_to ON messages(to_id, id);
CREATE TABLE files (
    id BLOB PRIMARY KEY,
    owner INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    size INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX files_owner ON files(owner);
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
    ",
        r"
CREATE TABLE admin_totp (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    secret BLOB NOT NULL,
    last_step INTEGER NOT NULL
);
CREATE TABLE admin_recovery_codes (
    id INTEGER PRIMARY KEY,
    hash TEXT NOT NULL
);
    ",
    ];

    async fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).await.unwrap();
        (dir, db)
    }

    async fn account(db: &Db, seed: u8, name: &str) -> AccountRow {
        db.create_account(&[seed; 32], &PublicKey([seed; 32]), name, 10)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn migrations_run_once_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let db = Db::open(&path).await.unwrap();
            account(&db, 1, "a").await;
        }
        let db = Db::open(&path).await.unwrap();
        assert_eq!(db.count_accounts().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn accounts_by_auth_and_online_id() {
        let (_dir, db) = db().await;
        let a = account(&db, 1, "Clara").await;
        assert_eq!(
            db.account_by_auth(&[1; 32]).await.unwrap().unwrap().id,
            a.id
        );
        assert!(db.account_by_auth(&[2; 32]).await.unwrap().is_none());
        assert_eq!(
            db.account_by_online(a.online_id)
                .await
                .unwrap()
                .unwrap()
                .display_name,
            "Clara"
        );
        assert!(
            db.create_account(&[1; 32], &PublicKey([9; 32]), "x", 1)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn relations_cover_every_state() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await;
        let friend = account(&db, 2, "friend").await;
        let incoming = account(&db, 3, "incoming").await;
        let outgoing = account(&db, 4, "outgoing").await;
        let blocked = account(&db, 5, "blocked").await;
        db.befriend(friend.id, me.id, 1).await.unwrap();
        db.add_request(incoming.id, me.id, 1).await.unwrap();
        db.add_request(me.id, outgoing.id, 1).await.unwrap();
        db.add_request(blocked.id, me.id, 1).await.unwrap();
        db.block(me.id, blocked.id).await.unwrap();
        let mut got: Vec<(String, FriendState)> = db
            .relations(me.id)
            .await
            .unwrap()
            .into_iter()
            .map(|(a, s)| (a.display_name, s))
            .collect();
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            got,
            vec![
                ("blocked".into(), FriendState::Blocked),
                ("friend".into(), FriendState::Friend),
                ("incoming".into(), FriendState::Incoming),
                ("outgoing".into(), FriendState::Outgoing),
            ]
        );
        assert!(db.are_friends(me.id, friend.id).await.unwrap());
        assert_eq!(db.friend_ids(me.id).await.unwrap(), vec![friend.id]);
        db.unfriend(me.id, friend.id).await.unwrap();
        assert!(!db.are_friends(friend.id, me.id).await.unwrap());
    }

    #[tokio::test]
    async fn search_treats_wildcards_as_text() {
        let (_dir, db) = db().await;
        account(&db, 1, "100%_lemon").await;
        account(&db, 2, "plain").await;
        assert_eq!(db.users("%", 0, 10).await.unwrap().1, 1);
        assert_eq!(db.users("_", 0, 10).await.unwrap().1, 1);
        assert_eq!(db.users("", 0, 10).await.unwrap().1, 2);
        assert_eq!(db.users("' OR 1=1 --", 0, 10).await.unwrap().1, 0);
    }

    #[tokio::test]
    async fn messages_store_ack_and_purge() {
        let (_dir, db) = db().await;
        let a = account(&db, 1, "a").await;
        let b = account(&db, 2, "b").await;
        let sealed = Sealed {
            nonce: [3; 24],
            ciphertext: vec![1, 2, 3],
        };
        let first = db.insert_message(a.id, b.id, 100, &sealed).await.unwrap();
        let second = db.insert_message(a.id, b.id, 200, &sealed).await.unwrap();
        let pending = db.pending_messages(b.id).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].1, a.online_id);
        assert_eq!(pending[0].3, sealed);
        db.ack_messages(a.id, &[first]).await.unwrap();
        assert_eq!(db.pending_messages(b.id).await.unwrap().len(), 2);
        db.ack_messages(b.id, &[first]).await.unwrap();
        assert_eq!(db.pending_messages(b.id).await.unwrap()[0].0, second);
        assert_eq!(db.purge_messages(201).await.unwrap(), 1);
        assert!(db.pending_messages(b.id).await.unwrap().is_empty());
    }

    async fn auto_vacuum(db: &Db) -> i64 {
        sqlx::query_scalar("PRAGMA auto_vacuum")
            .fetch_one(&db.pool)
            .await
            .unwrap()
    }

    async fn pages(db: &Db) -> i64 {
        sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&db.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn deleted_messages_give_their_space_back() {
        let (dir, db) = db().await;
        let a = account(&db, 1, "a").await;
        let b = account(&db, 2, "b").await;
        let empty = pages(&db).await;
        let sealed = Sealed {
            nonce: [3; 24],
            ciphertext: vec![0x5A; 16 << 10],
        };
        let mut ids = Vec::new();
        for i in 0..64 {
            ids.push(db.insert_message(a.id, b.id, i, &sealed).await.unwrap());
        }
        db.reclaim().await.unwrap();
        let full = pages(&db).await;
        let full_bytes = std::fs::metadata(dir.path().join("t.db")).unwrap().len();
        assert!(full > empty + 200, "{empty} then {full} pages");
        db.ack_messages(b.id, &ids).await.unwrap();
        let freed = db.reclaim().await.unwrap();
        assert!(freed >= 1 << 20, "{freed} bytes freed");
        assert!(pages(&db).await < empty + 10);
        let bytes = std::fs::metadata(dir.path().join("t.db")).unwrap().len();
        assert!(bytes < full_bytes / 4, "{full_bytes} then {bytes} bytes");
        assert_eq!(db.reclaim().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn files_and_storage() {
        let (_dir, db) = db().await;
        let a = account(&db, 1, "a").await;
        db.insert_file(FileId([1; 16]), a.id, 100, 5).await.unwrap();
        db.insert_file(FileId([2; 16]), a.id, 50, 6).await.unwrap();
        assert_eq!(db.storage_used(a.id).await.unwrap(), 150);
        assert_eq!(db.total_storage().await.unwrap(), 150);
        assert!(!db.delete_file(FileId([1; 16]), a.id + 1).await.unwrap());
        assert!(db.delete_file(FileId([1; 16]), a.id).await.unwrap());
        assert_eq!(
            db.delete_files_of(a.id).await.unwrap(),
            vec![FileId([2; 16])]
        );
        assert_eq!(db.storage_used(a.id).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn settings_upsert_and_delete() {
        let (_dir, db) = db().await;
        db.set_setting("relay", "on").await.unwrap();
        db.set_setting("relay", "off").await.unwrap();
        assert_eq!(db.settings().await.unwrap()["relay"], "off");
        db.delete_setting("relay").await.unwrap();
        assert!(db.settings().await.unwrap().is_empty());
    }

    const RUSQLITE_ROWS: &str = r"
INSERT INTO accounts (id, online_id, auth_hash, public_key, display_name, created_at, last_seen, quota_bytes, banned) VALUES
    (1, 123456789, X'0101010101010101010101010101010101010101010101010101010101010101', X'1111111111111111111111111111111111111111111111111111111111111111', 'Clara', 100, 200, NULL, 0),
    (2, 987654321, X'0202020202020202020202020202020202020202020202020202020202020202', X'2222222222222222222222222222222222222222222222222222222222222222', 'Lime', 101, 201, 5242880, 0),
    (3, 555555555, X'0303030303030303030303030303030303030303030303030303030303030303', X'3333333333333333333333333333333333333333333333333333333333333333', 'Troll', 102, 202, NULL, 1),
    (4, 444444444, X'0404040404040404040404040404040404040404040404040404040404040404', X'4444444444444444444444444444444444444444444444444444444444444444', 'Pending', 103, 203, NULL, 0);
INSERT INTO friendships (a, b, since) VALUES (1, 2, 300);
INSERT INTO friend_requests (from_id, to_id, created_at) VALUES (4, 1, 301);
INSERT INTO blocks (blocker, blocked) VALUES (1, 3);
INSERT INTO messages (id, from_id, to_id, sent_at, nonce, body) VALUES (7, 2, 1, 400, X'ABABABABABABABABABABABABABABABABABABABABABABABAB', X'C0FFEE');
INSERT INTO files (id, owner, size, created_at) VALUES (X'5A5A5A5A5A5A5A5A5A5A5A5A5A5A5A5A', 2, 1234, 500);
INSERT INTO settings (key, value) VALUES ('name', 'Lemon relay'), ('relay_password', '$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA');
INSERT INTO admin_totp (id, secret, last_step) VALUES (1, X'7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E', 58726080);
INSERT INTO admin_recovery_codes (id, hash) VALUES (1, '$argon2id$one'), (2, '$argon2id$two');
";

    async fn raw(path: &Path) -> SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        SqlitePool::connect_with(options).await.unwrap()
    }

    async fn rusqlite_db(path: &Path, version: usize) {
        let pool = raw(path).await;
        for (i, sql) in RUSQLITE_SQL.iter().enumerate().take(version) {
            let mut tx = pool.begin().await.unwrap();
            sqlx::raw_sql(*sql).execute(&mut *tx).await.unwrap();
            sqlx::raw_sql(AssertSqlSafe(format!("PRAGMA user_version = {}", i + 1)))
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
        if version == RUSQLITE_SQL.len() {
            sqlx::raw_sql(RUSQLITE_ROWS).execute(&pool).await.unwrap();
        }
        pool.close().await;
    }

    async fn applied(db: &Db) -> Vec<(i64, bool)> {
        sqlx::query!(
            r#"SELECT version AS "version!", execution_time = -1 AS "skipped!: bool"
               FROM _sqlx_migrations ORDER BY version"#
        )
        .fetch_all(&db.pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.version, r.skipped))
        .collect()
    }

    fn skipped_up_to(version: i64) -> Vec<(i64, bool)> {
        MIGRATOR
            .iter()
            .map(|m| (m.version, m.version <= version))
            .collect()
    }

    async fn schema(db: &Db) -> Vec<(String, String, String, Option<String>)> {
        sqlx::query!(
            r#"SELECT type AS "kind!: String", name AS "name!: String", tbl_name AS "table!: String", sql
               FROM sqlite_master ORDER BY type, name"#
        )
        .fetch_all(&db.pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.kind, r.name, r.table, r.sql))
        .collect()
    }

    #[tokio::test]
    async fn a_rusqlite_database_keeps_every_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        rusqlite_db(&path, 2).await;
        let db = Db::open(&path).await.unwrap();
        assert_eq!(applied(&db).await, skipped_up_to(2));

        assert_eq!(db.count_accounts().await.unwrap(), 4);
        let clara = db.account_by_auth(&[1; 32]).await.unwrap().unwrap();
        assert_eq!(clara.id, 1);
        assert_eq!(clara.online_id.value(), 123_456_789);
        assert_eq!(clara.public_key, PublicKey([0x11; 32]));
        assert_eq!(clara.display_name, "Clara");
        assert_eq!(clara.last_seen, 200);
        assert_eq!(clara.quota_bytes, None);
        assert!(!clara.banned);
        let lime = db.account(2).await.unwrap().unwrap();
        assert_eq!(lime.quota_bytes, Some(5_242_880));
        assert_eq!(lime.online_id.value(), 987_654_321);
        assert!(db.account(3).await.unwrap().unwrap().banned);

        let mut relations: Vec<(String, FriendState)> = db
            .relations(1)
            .await
            .unwrap()
            .into_iter()
            .map(|(a, s)| (a.display_name, s))
            .collect();
        relations.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            relations,
            vec![
                ("Lime".into(), FriendState::Friend),
                ("Pending".into(), FriendState::Incoming),
                ("Troll".into(), FriendState::Blocked),
            ]
        );

        let pending = db.pending_messages(1).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, 7);
        assert_eq!(pending[0].1, lime.online_id);
        assert_eq!(pending[0].2, 400);
        assert_eq!(
            pending[0].3,
            Sealed {
                nonce: [0xAB; 24],
                ciphertext: vec![0xC0, 0xFF, 0xEE],
            }
        );

        assert_eq!(
            db.files_of(2).await.unwrap(),
            vec![FileInfo {
                id: FileId([0x5A; 16]),
                size: 1234,
                created: 500,
            }]
        );
        assert_eq!(db.storage_used(2).await.unwrap(), 1234);

        let settings = db.settings().await.unwrap();
        assert_eq!(settings.len(), 2);
        assert_eq!(settings["name"], "Lemon relay");
        assert_eq!(
            settings["relay_password"],
            "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
        );

        assert_eq!(db.admin_totp().await.unwrap(), Some(vec![0x7E; 20]));
        assert_eq!(
            db.admin_recovery_codes().await.unwrap(),
            vec![(1, "$argon2id$one".into()), (2, "$argon2id$two".into())]
        );
        assert!(!db.use_admin_step(58_726_080).await.unwrap());
        assert!(db.use_admin_step(58_726_081).await.unwrap());

        db.pool.close().await;
        let db = Db::open(&path).await.unwrap();
        assert_eq!(applied(&db).await, skipped_up_to(2));
        assert_eq!(db.count_accounts().await.unwrap(), 4);

        let later = MIGRATOR.iter().map(|m| m.version).max().unwrap() + 1;
        let next = Migration::new(
            later,
            "later".into(),
            MigrationType::Simple,
            "CREATE TABLE later (id INTEGER PRIMARY KEY); INSERT INTO later (id) SELECT id FROM accounts;"
                .into_sql_str(),
            false,
        );
        Migrator::with_migrations(MIGRATOR.iter().cloned().chain([next]).collect())
            .run(&db.pool)
            .await
            .unwrap();
        let mut expected = skipped_up_to(2);
        expected.push((later, false));
        assert_eq!(applied(&db).await, expected);
        assert!(
            schema(&db)
                .await
                .iter()
                .any(|(kind, name, _, _)| kind == "table" && name == "later")
        );
        assert_eq!(db.count_accounts().await.unwrap(), 4);
    }

    #[tokio::test]
    async fn a_first_version_rusqlite_database_gets_the_later_migrations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        rusqlite_db(&path, 1).await;
        let db = Db::open(&path).await.unwrap();
        assert_eq!(applied(&db).await, skipped_up_to(1));
        assert_eq!(db.admin_totp().await.unwrap(), None);
        account(&db, 1, "a").await;
        assert_eq!(db.count_accounts().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_fresh_database_has_the_upgraded_schema() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = Db::open(&dir.path().join("fresh.db")).await.unwrap();
        let path = dir.path().join("old.db");
        rusqlite_db(&path, 2).await;
        let upgraded = Db::open(&path).await.unwrap();
        let fresh_schema = schema(&fresh).await;
        assert!(fresh_schema.len() > 10);
        assert_eq!(fresh_schema, schema(&upgraded).await);
        assert_eq!(applied(&fresh).await, skipped_up_to(0));
        assert_eq!(auto_vacuum(&fresh).await, 2);
        assert_eq!(auto_vacuum(&upgraded).await, 2);
    }
}
