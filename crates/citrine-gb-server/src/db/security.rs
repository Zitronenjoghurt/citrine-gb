use super::{Db, Result};

impl Db {
    pub async fn relay_secret(&self, made: [u8; 32]) -> Result<[u8; 32]> {
        let made = &made[..];
        sqlx::query!(
            "INSERT INTO relay_key (id, secret) VALUES (1, ?1) ON CONFLICT (id) DO NOTHING",
            made
        )
        .execute(&self.pool)
        .await?;
        let secret = sqlx::query_scalar!("SELECT secret FROM relay_key WHERE id = 1")
            .fetch_one(&self.pool)
            .await?;
        secret
            .try_into()
            .map_err(|_| sqlx::Error::Decode("the relay key isn't 32 bytes".into()))
    }

    pub async fn account_totp(&self, account: i64) -> Result<Option<Vec<u8>>> {
        sqlx::query_scalar!(
            "SELECT secret FROM account_totp WHERE account_id = ?1",
            account
        )
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn set_account_totp(
        &self,
        account: i64,
        secret: &[u8],
        step: u64,
        codes: &[String],
    ) -> Result<()> {
        let step = step as i64;
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "INSERT INTO account_totp (account_id, secret, last_step) VALUES (?1, ?2, ?3)
             ON CONFLICT (account_id) DO UPDATE SET secret = excluded.secret, last_step = excluded.last_step",
            account,
            secret,
            step,
        )
        .execute(&mut *tx)
        .await?;
        replace_codes(&mut tx, account, codes).await?;
        tx.commit().await
    }

    pub async fn use_account_step(&self, account: i64, step: u64) -> Result<bool> {
        let step = step as i64;
        let done = sqlx::query!(
            "UPDATE account_totp SET last_step = ?2 WHERE account_id = ?1 AND last_step < ?2",
            account,
            step
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn account_recovery_codes(&self, account: i64) -> Result<Vec<(i64, String)>> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", hash FROM account_recovery_codes WHERE account_id = ?1 ORDER BY id"#,
            account
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.id, r.hash)).collect())
    }

    pub async fn account_codes_left(&self, account: i64) -> Result<u64> {
        let left = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM account_recovery_codes WHERE account_id = ?1"#,
            account
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(left as u64)
    }

    pub async fn use_account_recovery_code(&self, account: i64, id: i64) -> Result<bool> {
        let done = sqlx::query!(
            "DELETE FROM account_recovery_codes WHERE account_id = ?1 AND id = ?2",
            account,
            id
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn replace_account_recovery_codes(
        &self,
        account: i64,
        codes: &[String],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        replace_codes(&mut tx, account, codes).await?;
        tx.commit().await
    }

    pub async fn clear_account_totp(&self, account: i64) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let removed = sqlx::query!("DELETE FROM account_totp WHERE account_id = ?1", account)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        replace_codes(&mut tx, account, &[]).await?;
        tx.commit().await?;
        Ok(removed > 0)
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

async fn replace_codes(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account: i64,
    codes: &[String],
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM account_recovery_codes WHERE account_id = ?1",
        account
    )
    .execute(&mut **tx)
    .await?;
    for hash in codes {
        sqlx::query!(
            "INSERT INTO account_recovery_codes (account_id, hash) VALUES (?1, ?2)",
            account,
            hash
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_net::PublicKey;

    async fn db() -> (tempfile::TempDir, Db, i64) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).await.unwrap();
        let id = db
            .create_account(&[1; 32], &PublicKey([2; 32]), "Clara", 10)
            .await
            .unwrap()
            .id;
        (dir, db, id)
    }

    #[tokio::test]
    async fn the_relay_key_is_made_once() {
        let (_dir, db, _) = db().await;
        assert_eq!(db.relay_secret([5; 32]).await.unwrap(), [5; 32]);
        assert_eq!(db.relay_secret([6; 32]).await.unwrap(), [5; 32]);
    }

    #[tokio::test]
    async fn totp_steps_only_move_forward_and_codes_go_once() {
        let (_dir, db, me) = db().await;
        assert_eq!(db.account_totp(me).await.unwrap(), None);
        db.set_account_totp(me, &[9; 20], 100, &["a".into(), "b".into()])
            .await
            .unwrap();
        assert_eq!(db.account_totp(me).await.unwrap(), Some(vec![9; 20]));
        assert!(!db.use_account_step(me, 100).await.unwrap());
        assert!(db.use_account_step(me, 101).await.unwrap());
        assert!(!db.use_account_step(me, 101).await.unwrap());
        let codes = db.account_recovery_codes(me).await.unwrap();
        assert_eq!(db.account_codes_left(me).await.unwrap(), 2);
        assert!(db.use_account_recovery_code(me, codes[0].0).await.unwrap());
        assert!(!db.use_account_recovery_code(me, codes[0].0).await.unwrap());
        assert_eq!(db.account_codes_left(me).await.unwrap(), 1);
        db.replace_account_recovery_codes(me, &["c".to_string(), "c".to_string(), "c".to_string()])
            .await
            .unwrap();
        assert_eq!(db.account_codes_left(me).await.unwrap(), 3);
        assert!(db.clear_account_totp(me).await.unwrap());
        assert_eq!(db.account_totp(me).await.unwrap(), None);
        assert_eq!(db.account_codes_left(me).await.unwrap(), 0);
        assert!(!db.clear_account_totp(me).await.unwrap());
    }
}
