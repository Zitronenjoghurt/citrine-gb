use super::{Db, Result, Usage, file_id};
use citrine_gb_net::{FileId, FileInfo};

impl Db {
    pub async fn usage(&self, owner: i64) -> Result<Usage> {
        let r = sqlx::query!(
            r#"SELECT
                   (SELECT COALESCE(SUM(MAX(size, 4096)), 0) FROM files WHERE owner = ?1)
                     + (SELECT COALESCE(SUM(LENGTH(label)), 0) FROM saves WHERE owner = ?1) AS "bytes!: i64",
                   (SELECT COUNT(*) FROM files WHERE owner = ?1 AND id NOT IN (SELECT file FROM saves)) AS "files!: i64",
                   (SELECT COUNT(*) FROM saves WHERE owner = ?1) AS "saves!: i64""#,
            owner,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(Usage {
            bytes: r.bytes as u64,
            files: r.files as u64,
            saves: r.saves as u64,
        })
    }

    pub async fn storage_used(&self, owner: i64) -> Result<u64> {
        Ok(self.usage(owner).await?.bytes)
    }

    pub async fn total_storage(&self) -> Result<u64> {
        let n = sqlx::query_scalar!(
            r#"SELECT (SELECT COALESCE(SUM(MAX(size, 4096)), 0) FROM files)
                 + (SELECT COALESCE(SUM(LENGTH(label)), 0) FROM saves)
                 + (SELECT COALESCE(SUM(LENGTH(body)), 0) FROM messages) AS "n!: i64""#
        )
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
}
