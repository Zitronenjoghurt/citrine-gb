use super::{Db, Result};
use std::collections::HashMap;

impl Db {
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
}
