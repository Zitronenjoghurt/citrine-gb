use super::{Db, Result};

pub const MAX_DS_PROFILES: i64 = 64;

impl Db {
    pub async fn ds_profile_claim(&self, pid: i64, account: i64, now: u64) -> Result<bool> {
        let owner = sqlx::query_scalar!("SELECT account_id FROM ds_profiles WHERE pid = ?1", pid)
            .fetch_optional(&self.pool)
            .await?;
        if let Some(owner) = owner {
            return Ok(owner == account);
        }
        let held = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!: i64" FROM ds_profiles WHERE account_id = ?1"#,
            account
        )
        .fetch_one(&self.pool)
        .await?;
        if held >= MAX_DS_PROFILES {
            return Ok(false);
        }
        let now = now as i64;
        sqlx::query!(
            "INSERT INTO ds_profiles (pid, account_id, created_at)
             VALUES (?1, ?2, ?3) ON CONFLICT (pid) DO NOTHING",
            pid,
            account,
            now,
        )
        .execute(&self.pool)
        .await?;
        let owner = sqlx::query_scalar!("SELECT account_id FROM ds_profiles WHERE pid = ?1", pid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(owner == Some(account))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{account, db};

    #[tokio::test]
    async fn a_profile_belongs_to_the_first_account_that_claims_it() {
        let (_dir, db) = db().await;
        let ana = account(&db, 1, "ana").await.id;
        let ben = account(&db, 2, "ben").await.id;
        assert!(db.ds_profile_claim(500, ana, 1).await.unwrap());
        assert!(db.ds_profile_claim(500, ana, 2).await.unwrap());
        assert!(!db.ds_profile_claim(500, ben, 3).await.unwrap());
        assert!(db.ds_profile_claim(501, ben, 4).await.unwrap());
    }

    #[tokio::test]
    async fn an_account_holds_a_limited_number_of_profiles() {
        let (_dir, db) = db().await;
        let ana = account(&db, 1, "ana").await.id;
        for pid in 0..MAX_DS_PROFILES {
            assert!(db.ds_profile_claim(1000 + pid, ana, 1).await.unwrap());
        }
        assert!(!db.ds_profile_claim(5000, ana, 1).await.unwrap());
        assert!(db.ds_profile_claim(1000, ana, 1).await.unwrap());
    }
}
