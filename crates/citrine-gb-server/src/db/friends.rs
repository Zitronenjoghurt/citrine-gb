use super::{AccountRecord, AccountRow, Db, Result, ordered};
use citrine_gb_net::FriendState;

impl Db {
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

    pub async fn add_request(&self, from: i64, to: i64, now: u64) -> Result<bool> {
        let now = now as i64;
        let done = sqlx::query!(
            "INSERT OR IGNORE INTO friend_requests (from_id, to_id, created_at) VALUES (?1, ?2, ?3)",
            from,
            to,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn requests_of(&self, id: i64) -> Result<(u64, u64)> {
        let row = sqlx::query!(
            r#"SELECT (SELECT COUNT(*) FROM friend_requests WHERE from_id = ?1) AS "sent!: i64",
                      (SELECT COUNT(*) FROM friend_requests WHERE to_id = ?1) AS "received!: i64""#,
            id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((row.sent as u64, row.received as u64))
    }

    pub async fn purge_requests(&self, before: u64) -> Result<usize> {
        let before = before as i64;
        let done = sqlx::query!("DELETE FROM friend_requests WHERE created_at < ?1", before)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() as usize)
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
               FROM accounts WHERE banned = 0 AND id IN
                   (SELECT b FROM friendships WHERE a = ?1 UNION SELECT a FROM friendships WHERE b = ?1)
               UNION ALL
               SELECT id, online_id, public_key, display_name, last_seen, quota_bytes, banned, 1
               FROM accounts WHERE banned = 0 AND id IN
                   (SELECT from_id FROM friend_requests WHERE to_id = ?1
                    AND from_id NOT IN (SELECT blocked FROM blocks WHERE blocker = ?1))
               UNION ALL
               SELECT id, online_id, public_key, display_name, last_seen, quota_bytes, banned, 2
               FROM accounts WHERE banned = 0 AND id IN
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
}
