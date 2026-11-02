use super::{
    AccountRecord, AccountRow, Db, Result, UserOrder, UserRow, file_id, insert_account,
    like_pattern,
};
use citrine_gb_net::{FileId, OnlineId, PublicKey};

impl Db {
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
        let mut tx = self.pool.begin().await?;
        sqlx::query!("UPDATE accounts SET banned = ?2 WHERE id = ?1", id, banned)
            .execute(&mut *tx)
            .await?;
        if banned {
            sqlx::query!(
                "DELETE FROM friend_requests WHERE from_id = ?1 OR to_id = ?1",
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn ban_joined_since(&self, since: u64) -> Result<Vec<i64>> {
        let since = since as i64;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let ids = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM accounts WHERE created_at >= ?1 AND banned = 0 ORDER BY id"#,
            since,
        )
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query!(
            "DELETE FROM friend_requests
              WHERE from_id IN (SELECT id FROM accounts WHERE created_at >= ?1 AND banned = 0)
                 OR to_id IN (SELECT id FROM accounts WHERE created_at >= ?1 AND banned = 0)",
            since,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE accounts SET banned = 1 WHERE created_at >= ?1 AND banned = 0",
            since,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ids)
    }

    pub async fn count_joined_since(&self, since: u64) -> Result<u64> {
        let since = since as i64;
        let n = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM accounts WHERE created_at >= ?1 AND banned = 0"#,
            since,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(n as u64)
    }

    pub async fn delete_account(&self, id: i64) -> Result<Option<Vec<FileId>>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let files: Vec<FileId> = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM files WHERE owner = ?1 ORDER BY created_at, id"#,
            id,
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(file_id)
        .collect();
        let done = sqlx::query!("DELETE FROM accounts WHERE id = ?1", id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok((done.rows_affected() == 1).then_some(files))
    }

    pub async fn empty_accounts(&self, unseen_since: u64) -> Result<Vec<i64>> {
        let before = unseen_since as i64;
        sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM accounts
                WHERE last_seen < ?1 AND banned = 0
                  AND NOT EXISTS (SELECT 1 FROM friendships WHERE a = accounts.id OR b = accounts.id)
                  AND NOT EXISTS (SELECT 1 FROM files WHERE owner = accounts.id)
                  AND NOT EXISTS (SELECT 1 FROM saves WHERE owner = accounts.id)
                ORDER BY id"#,
            before,
        )
        .fetch_all(&self.pool)
        .await
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
        order: UserOrder,
        joined_since: u64,
        offset: u64,
        limit: u64,
    ) -> Result<(Vec<UserRow>, u64)> {
        let pattern = like_pattern(search);
        let online = search.parse::<OnlineId>().ok().map(|o| o.value() as i64);
        let (limit, offset) = (limit as i64, offset as i64);
        let newest = order == UserOrder::Newest;
        let since = joined_since as i64;
        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM accounts
               WHERE (?1 = '%%' OR display_name LIKE ?1 ESCAPE '\' OR online_id = ?2)
                 AND created_at >= ?3"#,
            pattern,
            online,
            since,
        )
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", online_id, public_key, display_name, created_at, last_seen, quota_bytes, banned AS "banned: bool",
                   (SELECT COALESCE(SUM(MAX(size, 4096)), 0) FROM files WHERE owner = accounts.id)
                     + (SELECT COALESCE(SUM(LENGTH(label)), 0) FROM saves WHERE owner = accounts.id) AS "storage_used!: i64",
                   (SELECT COUNT(*) FROM files WHERE owner = accounts.id) AS "files!: i64"
               FROM accounts
               WHERE (?1 = '%%' OR display_name LIKE ?1 ESCAPE '\' OR online_id = ?2)
                 AND created_at >= ?5
               ORDER BY CASE WHEN ?6 THEN created_at ELSE last_seen END DESC, id DESC LIMIT ?3 OFFSET ?4"#,
            pattern,
            online,
            limit,
            offset,
            since,
            newest,
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
            joined: r.created_at as u64,
            storage_used: r.storage_used as u64,
            files: r.files as u64,
        })
        .collect();
        Ok((rows, total as u64))
    }
}
