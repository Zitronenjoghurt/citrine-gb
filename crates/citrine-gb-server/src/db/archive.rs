use super::{AccountRecord, AccountRow, Db, Result};
use citrine_gb_net::ArchiveId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveRow {
    pub id: ArchiveId,
    pub sealed: Vec<u8>,
    pub wrapped_key: Vec<u8>,
    pub stored: u64,
    pub added_at: u64,
}

struct ArchiveRecord {
    id: Vec<u8>,
    sealed: Vec<u8>,
    wrapped_key: Vec<u8>,
    stored: i64,
    added_at: i64,
}

impl From<ArchiveRecord> for ArchiveRow {
    fn from(r: ArchiveRecord) -> Self {
        Self {
            id: archive_id(&r.id),
            sealed: r.sealed,
            wrapped_key: r.wrapped_key,
            stored: r.stored.max(0) as u64,
            added_at: r.added_at.max(0) as u64,
        }
    }
}

fn archive_id(bytes: &[u8]) -> ArchiveId {
    let mut id = [0; 16];
    if bytes.len() == 16 {
        id.copy_from_slice(bytes);
    }
    ArchiveId(id)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArchiveTotals {
    pub games: u64,
    pub stored: u64,
}

impl Db {
    pub async fn archive_add(&self, row: &ArchiveRow) -> Result<bool> {
        let id = row.id.0.as_slice();
        let stored = row.stored as i64;
        let added_at = row.added_at as i64;
        let done = sqlx::query!(
            "INSERT INTO archive_games (id, sealed, wrapped_key, stored, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (id) DO NOTHING",
            id,
            row.sealed,
            row.wrapped_key,
            stored,
            added_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn archive_game(&self, id: ArchiveId) -> Result<Option<ArchiveRow>> {
        let id = id.0.as_slice();
        let row = sqlx::query_as!(
            ArchiveRecord,
            r#"SELECT id AS "id!", sealed, wrapped_key, stored, added_at
               FROM archive_games WHERE id = ?1"#,
            id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(ArchiveRow::from))
    }

    pub async fn archive_games(&self) -> Result<Vec<ArchiveRow>> {
        let rows = sqlx::query_as!(
            ArchiveRecord,
            r#"SELECT id AS "id!", sealed, wrapped_key, stored, added_at
               FROM archive_games ORDER BY added_at, id"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(ArchiveRow::from).collect())
    }

    pub async fn archive_games_for(&self, account: i64) -> Result<Vec<ArchiveRow>> {
        let rows = sqlx::query_as!(
            ArchiveRecord,
            r#"SELECT g.id AS "id!", g.sealed AS "sealed!", g.wrapped_key AS "wrapped_key!",
                      g.stored AS "stored!", g.added_at AS "added_at!"
               FROM archive_games g
               WHERE EXISTS (SELECT 1 FROM archive_access a
                             WHERE a.account = ?1 AND (a.game IS NULL OR a.game = g.id))
               ORDER BY g.added_at, g.id"#,
            account,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(ArchiveRow::from).collect())
    }

    pub async fn archive_delete(&self, id: ArchiveId) -> Result<bool> {
        let id = id.0.as_slice();
        let done = sqlx::query!("DELETE FROM archive_games WHERE id = ?1", id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn archive_totals(&self) -> Result<ArchiveTotals> {
        let r = sqlx::query!(
            r#"SELECT COUNT(*) AS "games!: i64", COALESCE(SUM(stored), 0) AS "stored!: i64"
               FROM archive_games"#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(ArchiveTotals {
            games: r.games.max(0) as u64,
            stored: r.stored.max(0) as u64,
        })
    }

    pub async fn archive_grant(&self, account: i64, now: u64) -> Result<bool> {
        let now = now as i64;
        let done = sqlx::query!(
            "INSERT OR IGNORE INTO archive_access (account, game, granted_at)
             SELECT id, NULL, ?2 FROM accounts WHERE id = ?1",
            account,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn archive_revoke(&self, account: i64) -> Result<bool> {
        let done = sqlx::query!("DELETE FROM archive_access WHERE account = ?1", account)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn archive_has_access(&self, account: i64) -> Result<bool> {
        let found = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM archive_access WHERE account = ?1) AS "found!: bool""#,
            account,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(found)
    }

    pub async fn archive_can_get(&self, account: i64, id: ArchiveId) -> Result<bool> {
        let id = id.0.as_slice();
        let found = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM archive_access a JOIN archive_games g ON g.id = ?2
                              WHERE a.account = ?1 AND (a.game IS NULL OR a.game = g.id)) AS "found!: bool""#,
            account,
            id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(found)
    }

    pub async fn archive_accounts(&self) -> Result<Vec<AccountRow>> {
        let rows = sqlx::query_as!(
            AccountRecord,
            r#"SELECT id AS "id!", online_id, public_key, display_name, last_seen, quota_bytes, banned AS "banned: bool"
               FROM accounts
               WHERE EXISTS (SELECT 1 FROM archive_access WHERE account = accounts.id)
               ORDER BY display_name COLLATE NOCASE, id"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(AccountRow::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{account, db};

    fn row(byte: u8, stored: u64, at: u64) -> ArchiveRow {
        ArchiveRow {
            id: ArchiveId([byte; 16]),
            sealed: vec![byte; 40],
            wrapped_key: vec![byte; 72],
            stored,
            added_at: at,
        }
    }

    #[tokio::test]
    async fn a_game_is_kept_once_and_counts_toward_the_totals() {
        let (_dir, db) = db().await;
        assert!(db.archive_add(&row(1, 100, 5)).await.unwrap());
        assert!(!db.archive_add(&row(1, 999, 6)).await.unwrap());
        assert!(db.archive_add(&row(2, 50, 7)).await.unwrap());
        assert_eq!(
            db.archive_totals().await.unwrap(),
            ArchiveTotals {
                games: 2,
                stored: 150
            }
        );
        assert_eq!(
            db.archive_game(ArchiveId([1; 16])).await.unwrap(),
            Some(row(1, 100, 5))
        );
        assert!(db.archive_delete(ArchiveId([1; 16])).await.unwrap());
        assert!(!db.archive_delete(ArchiveId([1; 16])).await.unwrap());
        assert_eq!(db.archive_games().await.unwrap(), vec![row(2, 50, 7)]);
    }

    #[tokio::test]
    async fn only_granted_accounts_see_and_get_games() {
        let (_dir, db) = db().await;
        let ana = account(&db, 1, "ana").await.id;
        let ben = account(&db, 2, "ben").await.id;
        db.archive_add(&row(1, 100, 5)).await.unwrap();
        db.archive_add(&row(2, 100, 6)).await.unwrap();
        assert!(db.archive_grant(ana, 10).await.unwrap());
        assert!(!db.archive_grant(ana, 11).await.unwrap());
        assert!(!db.archive_grant(9999, 11).await.unwrap());
        assert_eq!(db.archive_games_for(ana).await.unwrap().len(), 2);
        assert!(db.archive_games_for(ben).await.unwrap().is_empty());
        assert!(db.archive_has_access(ana).await.unwrap());
        assert!(!db.archive_has_access(ben).await.unwrap());
        assert!(db.archive_can_get(ana, ArchiveId([2; 16])).await.unwrap());
        assert!(!db.archive_can_get(ana, ArchiveId([3; 16])).await.unwrap());
        assert!(!db.archive_can_get(ben, ArchiveId([2; 16])).await.unwrap());
        let granted: Vec<i64> = db
            .archive_accounts()
            .await
            .unwrap()
            .iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(granted, vec![ana]);
        assert!(db.archive_revoke(ana).await.unwrap());
        assert!(!db.archive_revoke(ana).await.unwrap());
        assert!(db.archive_games_for(ana).await.unwrap().is_empty());
        assert!(!db.archive_can_get(ana, ArchiveId([2; 16])).await.unwrap());
    }

    #[tokio::test]
    async fn a_deleted_account_loses_its_grant() {
        let (_dir, db) = db().await;
        let ana = account(&db, 1, "ana").await.id;
        db.archive_grant(ana, 10).await.unwrap();
        db.delete_account(ana).await.unwrap();
        assert!(db.archive_accounts().await.unwrap().is_empty());
    }
}
