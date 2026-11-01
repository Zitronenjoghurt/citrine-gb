use super::{AccountRow, Db, Result, insert_account, like_pattern, online_id};
use citrine_gb_net::{InviteCode, OnlineId, PublicKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteRow {
    pub id: i64,
    pub code: InviteCode,
    pub note: String,
    pub created_at: u64,
    pub used: Option<InviteUse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteUse {
    pub at: u64,
    pub by: Option<(OnlineId, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InviteFilter {
    #[default]
    All,
    Unused,
    Used,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteRefusal {
    Wrong,
    Used,
}

fn code(value: i64) -> InviteCode {
    InviteCode::new(value as u64).unwrap_or(InviteCode::new(0).expect("zero fits"))
}

impl Db {
    pub async fn create_invite(&self, note: &str, now: u64) -> Result<InviteRow> {
        let created = now as i64;
        loop {
            let invite = InviteCode::random();
            let value = invite.value() as i64;
            let done = sqlx::query!(
                "INSERT INTO invites (code, note, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (code) DO NOTHING",
                value,
                note,
                created,
            )
            .execute(&self.pool)
            .await?;
            if done.rows_affected() == 1 {
                return Ok(InviteRow {
                    id: done.last_insert_rowid(),
                    code: invite,
                    note: note.to_string(),
                    created_at: now,
                    used: None,
                });
            }
        }
    }

    pub async fn invite_counts(&self) -> Result<(u64, u64)> {
        let row = sqlx::query!(
            r#"SELECT COUNT(*) AS "all!: i64", COALESCE(SUM(used_at IS NULL), 0) AS "unused!: i64" FROM invites"#
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((row.unused as u64, (row.all - row.unused) as u64))
    }

    pub async fn invites(
        &self,
        search: &str,
        filter: InviteFilter,
        offset: u64,
        limit: u64,
    ) -> Result<(Vec<InviteRow>, u64)> {
        let pattern = like_pattern(search);
        let typed = search.parse::<InviteCode>().ok().map(|c| c.value() as i64);
        let online = search.parse::<OnlineId>().ok().map(|o| o.value() as i64);
        let filter = filter as i64;
        let (limit, offset) = (limit as i64, offset as i64);
        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM invites
               LEFT JOIN accounts ON accounts.id = invites.used_by
               WHERE (?1 = '%%' OR invites.note LIKE ?1 ESCAPE '\' OR accounts.display_name LIKE ?1 ESCAPE '\'
                      OR invites.code = ?2 OR accounts.online_id = ?3)
                 AND (?4 = 0 OR (?4 = 1 AND invites.used_at IS NULL) OR (?4 = 2 AND invites.used_at IS NOT NULL))"#,
            pattern,
            typed,
            online,
            filter,
        )
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query!(
            r#"SELECT invites.id AS "id!", invites.code, invites.note, invites.created_at, invites.used_at,
                   accounts.online_id AS "online_id?", accounts.display_name AS "display_name?"
               FROM invites
               LEFT JOIN accounts ON accounts.id = invites.used_by
               WHERE (?1 = '%%' OR invites.note LIKE ?1 ESCAPE '\' OR accounts.display_name LIKE ?1 ESCAPE '\'
                      OR invites.code = ?2 OR accounts.online_id = ?3)
                 AND (?4 = 0 OR (?4 = 1 AND invites.used_at IS NULL) OR (?4 = 2 AND invites.used_at IS NOT NULL))
               ORDER BY invites.id DESC LIMIT ?5 OFFSET ?6"#,
            pattern,
            typed,
            online,
            filter,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|r| InviteRow {
            id: r.id,
            code: code(r.code),
            note: r.note,
            created_at: r.created_at as u64,
            used: r.used_at.map(|at| InviteUse {
                at: at as u64,
                by: r.online_id.map(online_id).zip(r.display_name),
            }),
        })
        .collect();
        Ok((rows, total as u64))
    }

    pub async fn revoke_invite(&self, id: i64) -> Result<bool> {
        let done = sqlx::query!("DELETE FROM invites WHERE id = ?1 AND used_at IS NULL", id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn create_account_with_invite(
        &self,
        auth_hash: &[u8; 32],
        public_key: &PublicKey,
        name: &str,
        now: u64,
        invite: InviteCode,
    ) -> Result<std::result::Result<AccountRow, InviteRefusal>> {
        let value = invite.value() as i64;
        let used_at = now as i64;
        let mut tx = self.pool.begin().await?;
        let claimed = sqlx::query!(
            "UPDATE invites SET used_at = ?1 WHERE code = ?2 AND used_at IS NULL",
            used_at,
            value,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !claimed {
            let exists = sqlx::query_scalar!(
                r#"SELECT EXISTS (SELECT 1 FROM invites WHERE code = ?1) AS "exists!: bool""#,
                value
            )
            .fetch_one(&mut *tx)
            .await?;
            tx.rollback().await?;
            return Ok(Err(if exists {
                InviteRefusal::Used
            } else {
                InviteRefusal::Wrong
            }));
        }
        let id = insert_account(&mut tx, auth_hash, public_key, name, now).await?;
        sqlx::query!("UPDATE invites SET used_by = ?1 WHERE code = ?2", id, value)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let account = self.account(id).await?.ok_or(sqlx::Error::RowNotFound)?;
        Ok(Ok(account))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    async fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).await.unwrap();
        (dir, db)
    }

    async fn join(
        db: &Db,
        seed: u8,
        invite: InviteCode,
    ) -> std::result::Result<AccountRow, InviteRefusal> {
        db.create_account_with_invite(&[seed; 32], &PublicKey([seed; 32]), "Mia", 50, invite)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_code_makes_one_account_and_records_who() {
        let (_dir, db) = db().await;
        let invite = db.create_invite("Mia from work", 10).await.unwrap();
        assert_eq!(db.invite_counts().await.unwrap(), (1, 0));
        let account = join(&db, 1, invite.code).await.unwrap();
        assert_eq!(
            join(&db, 2, invite.code).await.unwrap_err(),
            InviteRefusal::Used
        );
        assert_eq!(db.count_accounts().await.unwrap(), 1);
        let (rows, total) = db.invites("", InviteFilter::All, 0, 50).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].note, "Mia from work");
        assert_eq!(rows[0].created_at, 10);
        assert_eq!(
            rows[0].used,
            Some(InviteUse {
                at: 50,
                by: Some((account.online_id, "Mia".into())),
            })
        );
        assert_eq!(db.invite_counts().await.unwrap(), (0, 1));
    }

    #[tokio::test]
    async fn an_unknown_or_revoked_code_is_wrong() {
        let (_dir, db) = db().await;
        let invite = db.create_invite("Ben", 10).await.unwrap();
        let other = InviteCode::new(invite.code.value() ^ 1).unwrap();
        assert_eq!(join(&db, 1, other).await.unwrap_err(), InviteRefusal::Wrong);
        assert!(db.revoke_invite(invite.id).await.unwrap());
        assert!(!db.revoke_invite(invite.id).await.unwrap());
        assert_eq!(
            join(&db, 1, invite.code).await.unwrap_err(),
            InviteRefusal::Wrong
        );
        assert_eq!(db.count_accounts().await.unwrap(), 0);
        assert_eq!(db.invite_counts().await.unwrap(), (0, 0));
    }

    #[tokio::test]
    async fn a_used_code_cannot_be_revoked() {
        let (_dir, db) = db().await;
        let invite = db.create_invite("Ana", 10).await.unwrap();
        join(&db, 1, invite.code).await.unwrap();
        assert!(!db.revoke_invite(invite.id).await.unwrap());
        assert_eq!(db.invite_counts().await.unwrap(), (0, 1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_sign_ups_racing_for_one_code_make_one_account() {
        let (_dir, db) = db().await;
        let db = Arc::new(db);
        for round in 0..20u8 {
            let invite = db.create_invite("race", 10).await.unwrap();
            let tries: Vec<_> = (0..4u8)
                .map(|i| {
                    let db = db.clone();
                    let seed = round * 4 + i + 1;
                    tokio::spawn(async move { join(&db, seed, invite.code).await })
                })
                .collect();
            let mut made = 0;
            for t in tries {
                match t.await.unwrap() {
                    Ok(_) => made += 1,
                    Err(refusal) => assert_eq!(refusal, InviteRefusal::Used),
                }
            }
            assert_eq!(made, 1, "round {round}");
        }
        assert_eq!(db.count_accounts().await.unwrap(), 20);
    }

    #[tokio::test]
    async fn search_filters_and_pages() {
        let (_dir, db) = db().await;
        let mia = db.create_invite("Mia from work", 10).await.unwrap();
        let ben = db.create_invite("Ben 100%", 11).await.unwrap();
        db.create_invite("Cleo", 12).await.unwrap();
        let account = join(&db, 1, mia.code).await.unwrap();
        let names = |rows: Vec<InviteRow>| rows.into_iter().map(|r| r.note).collect::<Vec<_>>();
        let (rows, total) = db.invites("", InviteFilter::All, 0, 2).await.unwrap();
        assert_eq!(
            (names(rows), total),
            (vec!["Cleo".into(), "Ben 100%".into()], 3)
        );
        let (rows, _) = db.invites("", InviteFilter::All, 2, 2).await.unwrap();
        assert_eq!(names(rows), vec!["Mia from work".to_string()]);
        let (rows, total) = db.invites("", InviteFilter::Unused, 0, 50).await.unwrap();
        assert_eq!(
            (names(rows), total),
            (vec!["Cleo".into(), "Ben 100%".into()], 2)
        );
        let (rows, _) = db.invites("", InviteFilter::Used, 0, 50).await.unwrap();
        assert_eq!(names(rows), vec!["Mia from work".to_string()]);
        let (rows, _) = db.invites("%", InviteFilter::All, 0, 50).await.unwrap();
        assert_eq!(names(rows), vec!["Ben 100%".to_string()]);
        let typed = ben.code.to_string().to_lowercase().replace('-', "");
        let (rows, _) = db.invites(&typed, InviteFilter::All, 0, 50).await.unwrap();
        assert_eq!(names(rows), vec!["Ben 100%".to_string()]);
        let (rows, _) = db
            .invites(&account.online_id.to_string(), InviteFilter::All, 0, 50)
            .await
            .unwrap();
        assert_eq!(names(rows), vec!["Mia from work".to_string()]);
    }
}
