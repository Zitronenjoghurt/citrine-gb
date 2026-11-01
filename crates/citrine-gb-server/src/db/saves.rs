use super::{Db, Result, file_id};
use citrine_gb_net::{FileId, GameKey, SaveInfo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveWrite {
    Stored {
        info: SaveInfo,
        replaced: Option<FileId>,
    },
    Changed(Option<SaveInfo>),
}

fn game_key(bytes: Vec<u8>) -> GameKey {
    let mut key = [0; 32];
    if bytes.len() == 32 {
        key.copy_from_slice(&bytes);
    }
    GameKey(key)
}

pub struct NewSave<'a> {
    pub game: GameKey,
    pub slot: &'a str,
    pub base: u64,
    pub label: &'a [u8],
    pub file: FileId,
    pub size: u64,
}

impl Db {
    pub async fn saves_of(&self, owner: i64) -> Result<Vec<SaveInfo>> {
        let rows = sqlx::query!(
            r#"SELECT s.game AS "game!", s.slot AS "slot!", s.version AS "version!", s.label AS "label!",
                      s.updated_at AS "updated_at!", f.size AS "size!"
                 FROM saves s JOIN files f ON f.id = s.file
                WHERE s.owner = ?1
                ORDER BY s.game, s.slot"#,
            owner,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SaveInfo {
                game: game_key(r.game),
                slot: r.slot,
                version: r.version as u64,
                size: r.size as u64,
                updated: r.updated_at as u64,
                label: r.label,
            })
            .collect())
    }

    pub async fn save(
        &self,
        owner: i64,
        game: GameKey,
        slot: &str,
    ) -> Result<Option<(SaveInfo, FileId)>> {
        let key = &game.0[..];
        let row = sqlx::query!(
            r#"SELECT s.version AS "version!", s.label AS "label!", s.updated_at AS "updated_at!",
                      s.file AS "file!", f.size AS "size!"
                 FROM saves s JOIN files f ON f.id = s.file
                WHERE s.owner = ?1 AND s.game = ?2 AND s.slot = ?3"#,
            owner,
            key,
            slot,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            (
                SaveInfo {
                    game,
                    slot: slot.to_string(),
                    version: r.version as u64,
                    size: r.size as u64,
                    updated: r.updated_at as u64,
                    label: r.label,
                },
                file_id(r.file),
            )
        }))
    }

    pub async fn put_save(&self, owner: i64, new: NewSave<'_>, now: u64) -> Result<SaveWrite> {
        let current = self.save(owner, new.game, new.slot).await?;
        let version = current.as_ref().map_or(0, |(info, _)| info.version);
        if version != new.base {
            return Ok(SaveWrite::Changed(current.map(|(info, _)| info)));
        }
        let next = (version + 1) as i64;
        let key = &new.game.0[..];
        let file = &new.file.0[..];
        let (size, at) = (new.size as i64, now as i64);
        let base = new.base as i64;
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "INSERT INTO files (id, owner, size, created_at) VALUES (?1, ?2, ?3, ?4)",
            file,
            owner,
            size,
            at,
        )
        .execute(&mut *tx)
        .await?;
        let replaced = match &current {
            Some((_, old)) => {
                let done = sqlx::query!(
                    "UPDATE saves SET version = ?4, file = ?5, label = ?6, updated_at = ?7
                      WHERE owner = ?1 AND game = ?2 AND slot = ?3 AND version = ?8",
                    owner,
                    key,
                    new.slot,
                    next,
                    file,
                    new.label,
                    at,
                    base,
                )
                .execute(&mut *tx)
                .await?;
                if done.rows_affected() != 1 {
                    tx.rollback().await?;
                    let current = self.save(owner, new.game, new.slot).await?;
                    return Ok(SaveWrite::Changed(current.map(|(info, _)| info)));
                }
                let old_id = &old.0[..];
                sqlx::query!("DELETE FROM files WHERE id = ?1", old_id)
                    .execute(&mut *tx)
                    .await?;
                Some(*old)
            }
            None => {
                sqlx::query!(
                    "INSERT INTO saves (owner, game, slot, version, file, label, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    owner,
                    key,
                    new.slot,
                    next,
                    file,
                    new.label,
                    at,
                )
                .execute(&mut *tx)
                .await?;
                None
            }
        };
        tx.commit().await?;
        Ok(SaveWrite::Stored {
            info: SaveInfo {
                game: new.game,
                slot: new.slot.to_string(),
                version: next as u64,
                size: new.size,
                updated: now,
                label: new.label.to_vec(),
            },
            replaced,
        })
    }

    pub async fn delete_save(
        &self,
        owner: i64,
        game: GameKey,
        slot: &str,
        base: u64,
    ) -> Result<std::result::Result<FileId, Option<SaveInfo>>> {
        let Some((info, file)) = self.save(owner, game, slot).await? else {
            return Ok(Err(None));
        };
        if info.version != base {
            return Ok(Err(Some(info)));
        }
        let id = &file.0[..];
        sqlx::query!("DELETE FROM files WHERE id = ?1 AND owner = ?2", id, owner)
            .execute(&self.pool)
            .await?;
        Ok(Ok(file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> (Db, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("test.db")).await.unwrap();
        (db, dir)
    }

    async fn account(db: &Db, seed: u8) -> i64 {
        db.create_account(
            &[seed; 32],
            &citrine_gb_net::PublicKey([seed; 32]),
            "Clara",
            1,
        )
        .await
        .unwrap()
        .id
    }

    fn new(slot: &str, base: u64, file: u8, size: u64) -> NewSave<'_> {
        NewSave {
            game: GameKey([9; 32]),
            slot,
            base,
            label: b"label",
            file: FileId([file; 16]),
            size,
        }
    }

    #[tokio::test]
    async fn versions_count_up_and_a_stale_base_is_refused() {
        let (db, _dir) = db().await;
        let me = account(&db, 1).await;
        let first = db
            .put_save(me, new("battery", 0, 1, 100), 10)
            .await
            .unwrap();
        let SaveWrite::Stored { info, replaced } = first else {
            panic!("got {first:?}");
        };
        assert_eq!((info.version, replaced), (1, None));
        let stale = db.put_save(me, new("battery", 0, 2, 50), 11).await.unwrap();
        assert!(matches!(stale, SaveWrite::Changed(Some(ref i)) if i.version == 1));
        let second = db.put_save(me, new("battery", 1, 3, 60), 12).await.unwrap();
        assert_eq!(
            second,
            SaveWrite::Stored {
                info: SaveInfo {
                    game: GameKey([9; 32]),
                    slot: "battery".into(),
                    version: 2,
                    size: 60,
                    updated: 12,
                    label: b"label".to_vec(),
                },
                replaced: Some(FileId([1; 16])),
            }
        );
        assert_eq!(db.storage_used(me).await.unwrap(), 60);
        assert_eq!(db.saves_of(me).await.unwrap().len(), 1);
        assert!(db.files_of(me).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_needs_the_current_version() {
        let (db, _dir) = db().await;
        let me = account(&db, 1).await;
        db.put_save(me, new("slot-0", 0, 1, 100), 10).await.unwrap();
        let wrong = db
            .delete_save(me, GameKey([9; 32]), "slot-0", 5)
            .await
            .unwrap();
        assert!(matches!(wrong, Err(Some(ref i)) if i.version == 1));
        let right = db
            .delete_save(me, GameKey([9; 32]), "slot-0", 1)
            .await
            .unwrap();
        assert_eq!(right, Ok(FileId([1; 16])));
        assert!(db.saves_of(me).await.unwrap().is_empty());
        assert_eq!(db.storage_used(me).await.unwrap(), 0);
        let gone = db
            .delete_save(me, GameKey([9; 32]), "slot-0", 1)
            .await
            .unwrap();
        assert_eq!(gone, Err(None));
    }

    #[tokio::test]
    async fn deleting_everything_or_the_account_takes_the_saves() {
        let (db, _dir) = db().await;
        let me = account(&db, 1).await;
        let other = account(&db, 2).await;
        db.put_save(me, new("battery", 0, 1, 100), 10)
            .await
            .unwrap();
        db.put_save(other, new("battery", 0, 2, 100), 10)
            .await
            .unwrap();
        let removed = db.delete_files_of(me).await.unwrap();
        assert_eq!(removed, vec![FileId([1; 16])]);
        assert!(db.saves_of(me).await.unwrap().is_empty());
        assert_eq!(db.saves_of(other).await.unwrap().len(), 1);

        db.put_save(me, new("battery", 0, 3, 100), 10)
            .await
            .unwrap();
        sqlx::query!("DELETE FROM accounts WHERE id = ?1", me)
            .execute(&db.pool)
            .await
            .unwrap();
        let left = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "n!: i64" FROM saves"#)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
        assert_eq!(db.saves_of(other).await.unwrap().len(), 1);
    }
}
