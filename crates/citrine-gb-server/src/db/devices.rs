use super::{Db, Result};
use sqlx::SqliteConnection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    pub id: i64,
    pub name: String,
    pub added: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownDevice {
    Known(i64),
    SignedOut,
    Unknown,
}

async fn copy_chats(conn: &mut SqliteConnection, account: i64, device: i64) -> Result<()> {
    sqlx::query!(
        "INSERT OR IGNORE INTO message_deliveries (device_id, message_id)
           SELECT ?2, id FROM messages
            WHERE (to_id = ?1 AND receiver_keeps = 1) OR (from_id = ?1 AND sender_keeps = 1)",
        account,
        device,
    )
    .execute(conn)
    .await?;
    Ok(())
}

impl Db {
    pub async fn add_device(
        &self,
        account: i64,
        key_hash: &[u8; 32],
        name: &str,
        now: u64,
    ) -> Result<i64> {
        let key_hash = &key_hash[..];
        let now = now as i64;
        let mut tx = self.pool.begin().await?;
        let id = sqlx::query!(
            "INSERT INTO devices (account_id, key_hash, created_at, last_seen, name) VALUES (?1, ?2, ?3, ?3, ?4)",
            account,
            key_hash,
            now,
            name,
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        copy_chats(&mut tx, account, id).await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn known_device(
        &self,
        account: i64,
        key_hash: &[u8; 32],
        name: &str,
        now: u64,
    ) -> Result<KnownDevice> {
        let key_hash = &key_hash[..];
        let now = now as i64;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let found = sqlx::query!(
            r#"SELECT id AS "id!", left_at IS NOT NULL AS "left!: bool" FROM devices WHERE account_id = ?1 AND key_hash = ?2"#,
            account,
            key_hash,
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(found) = found else {
            let out = sqlx::query_scalar!(
                r#"SELECT 1 AS "one!: i64" FROM signed_out_devices WHERE account_id = ?1 AND key_hash = ?2"#,
                account,
                key_hash,
            )
            .fetch_optional(&mut *tx)
            .await?;
            return Ok(if out.is_some() {
                KnownDevice::SignedOut
            } else {
                KnownDevice::Unknown
            });
        };
        sqlx::query!(
            "UPDATE devices SET last_seen = ?2, name = ?3, left_at = NULL WHERE id = ?1",
            found.id,
            now,
            name,
        )
        .execute(&mut *tx)
        .await?;
        if found.left {
            copy_chats(&mut tx, account, found.id).await?;
        }
        tx.commit().await?;
        Ok(KnownDevice::Known(found.id))
    }

    pub async fn old_chats_wanted(&self, device: i64) -> Result<bool> {
        let wanted = sqlx::query_scalar!(
            r#"SELECT old_chats_wanted AS "wanted!: bool" FROM devices WHERE id = ?1"#,
            device,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(wanted.unwrap_or(false))
    }

    pub async fn old_chats_kept(&self, device: i64) -> Result<()> {
        sqlx::query!(
            "UPDATE devices SET old_chats_wanted = 0 WHERE id = ?1",
            device
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn devices_of(&self, account: i64) -> Result<Vec<DeviceRow>> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id!", name, created_at, last_seen FROM devices
                WHERE account_id = ?1 AND left_at IS NULL ORDER BY last_seen DESC, id"#,
            account,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| DeviceRow {
                id: r.id,
                name: r.name,
                added: r.created_at as u64,
                last_seen: r.last_seen as u64,
            })
            .collect())
    }

    pub async fn device_seen(&self, device: i64, now: u64) -> Result<()> {
        let now = now as i64;
        sqlx::query!(
            "UPDATE devices SET last_seen = ?2 WHERE id = ?1",
            device,
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn sign_out_device(&self, account: i64, device: i64, now: u64) -> Result<bool> {
        let now = now as i64;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let key = sqlx::query_scalar!(
            "SELECT key_hash FROM devices WHERE id = ?1 AND account_id = ?2 AND left_at IS NULL",
            device,
            account,
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(key) = key else {
            return Ok(false);
        };
        sqlx::query!(
            "INSERT OR REPLACE INTO signed_out_devices (key_hash, account_id, at) VALUES (?1, ?2, ?3)",
            key,
            account,
            now,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!("DELETE FROM devices WHERE id = ?1", device)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn leave_device(&self, account: i64, device: i64, now: u64) -> Result<bool> {
        let now = now as i64;
        let mut tx = self.pool.begin().await?;
        let done = sqlx::query!(
            "UPDATE devices SET left_at = ?3, last_seen = ?3 WHERE id = ?1 AND account_id = ?2 AND left_at IS NULL",
            device,
            account,
            now,
        )
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(false);
        }
        sqlx::query!(
            "DELETE FROM message_deliveries WHERE device_id = ?1",
            device
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{account, db};
    use citrine_gb_net::{NONCE_BYTES, Sealed};

    fn sealed(byte: u8) -> Sealed {
        Sealed {
            nonce: [byte; NONCE_BYTES],
            ciphertext: vec![byte; 3],
        }
    }

    #[tokio::test]
    async fn devices_belong_to_their_account() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let other = account(&db, 2, "other").await.id;
        let phone = db
            .add_device(me, &[7; 32], "Android phone", 20)
            .await
            .unwrap();
        let laptop = db.add_device(me, &[8; 32], "Linux PC", 20).await.unwrap();
        assert_ne!(phone, laptop);
        assert_eq!(
            db.known_device(me, &[7; 32], "Pixel", 30).await.unwrap(),
            KnownDevice::Known(phone)
        );
        assert_eq!(
            db.known_device(other, &[7; 32], "Pixel", 30).await.unwrap(),
            KnownDevice::Unknown
        );
        let list = db.devices_of(me).await.unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, phone);
        assert_eq!(list[0].name, "Pixel");
        assert_eq!(list[0].last_seen, 30);
        db.delete_account(me).await.unwrap();
        assert_eq!(
            db.known_device(me, &[7; 32], "Pixel", 30).await.unwrap(),
            KnownDevice::Unknown
        );
    }

    #[tokio::test]
    async fn a_signed_out_device_stays_out() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let other = account(&db, 2, "other").await.id;
        let phone = db
            .add_device(me, &[7; 32], "Android phone", 20)
            .await
            .unwrap();
        db.add_device(me, &[8; 32], "Linux PC", 20).await.unwrap();
        assert!(!db.sign_out_device(other, phone, 30).await.unwrap());
        assert!(db.sign_out_device(me, phone, 30).await.unwrap());
        assert!(!db.sign_out_device(me, phone, 30).await.unwrap());
        assert_eq!(
            db.known_device(me, &[7; 32], "Android phone", 40)
                .await
                .unwrap(),
            KnownDevice::SignedOut
        );
        assert_eq!(db.devices_of(me).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_new_device_gets_the_stored_chats_and_a_left_one_gets_them_again() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let friend = account(&db, 2, "friend").await.id;
        let phone = db
            .add_device(me, &[7; 32], "Android phone", 10)
            .await
            .unwrap();
        let theirs = db.add_device(friend, &[9; 32], "Mac", 10).await.unwrap();
        let first = db
            .insert_message(friend, me, Some(theirs), 11, None, &sealed(1))
            .await
            .unwrap();
        let second = db
            .insert_message(me, friend, Some(phone), 12, None, &sealed(2))
            .await
            .unwrap();
        db.ack_messages(phone, &[first]).await.unwrap();
        let pc = db.add_device(me, &[8; 32], "Linux PC", 13).await.unwrap();
        let ids: Vec<_> = db
            .pending_messages(pc, 0, 10)
            .await
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec![first, second]);
        db.ack_messages(pc, &ids).await.unwrap();
        assert!(db.leave_device(me, pc, 14).await.unwrap());
        assert_eq!(db.devices_of(me).await.unwrap().len(), 1);
        assert_eq!(
            db.known_device(me, &[8; 32], "Linux PC", 15).await.unwrap(),
            KnownDevice::Known(pc)
        );
        assert_eq!(db.pending_messages(pc, 0, 10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn the_last_device_leaving_keeps_the_whole_history() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let friend = account(&db, 2, "friend").await.id;
        let phone = db
            .add_device(me, &[7; 32], "Android phone", 10)
            .await
            .unwrap();
        let theirs = db.add_device(friend, &[9; 32], "Mac", 10).await.unwrap();
        let to_me = db
            .insert_message(friend, me, Some(theirs), 11, None, &sealed(1))
            .await
            .unwrap();
        let from_me = db
            .insert_message(me, friend, Some(phone), 12, None, &sealed(2))
            .await
            .unwrap();
        assert!(db.leave_device(me, phone, 13).await.unwrap());
        assert!(db.sign_out_device(friend, theirs, 14).await.unwrap());
        let ids = |list: Vec<crate::db::messages::StoredMessage>| {
            list.iter().map(|m| m.id).collect::<Vec<_>>()
        };
        let again = db
            .add_device(me, &[5; 32], "Linux PC", 400 * 86_400)
            .await
            .unwrap();
        assert_eq!(
            ids(db.pending_messages(again, 0, 10).await.unwrap()),
            [to_me, from_me]
        );
        let new_mac = db.add_device(friend, &[6; 32], "iPad", 15).await.unwrap();
        assert_eq!(
            ids(db.pending_messages(new_mac, 0, 10).await.unwrap()),
            [to_me, from_me]
        );
        assert_eq!(db.stored_messages().await.unwrap(), (2, 6));
    }

    #[tokio::test]
    async fn only_messages_no_device_got_yet_fill_the_inbox() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let friend = account(&db, 2, "friend").await.id;
        let phone = db
            .add_device(me, &[7; 32], "Android phone", 10)
            .await
            .unwrap();
        let old = db
            .insert_message(friend, me, None, 11, None, &sealed(1))
            .await
            .unwrap();
        db.ack_messages(phone, &[old]).await.unwrap();
        assert_eq!(db.waiting_between(friend, me).await.unwrap(), (0, 0));
        let pc = db.add_device(me, &[8; 32], "Linux PC", 12).await.unwrap();
        assert_eq!(db.pending_messages(pc, 0, 10).await.unwrap().len(), 1);
        assert_eq!(db.waiting_between(friend, me).await.unwrap(), (0, 0));
        assert!(db.leave_device(me, phone, 13).await.unwrap());
        assert!(db.leave_device(me, pc, 14).await.unwrap());
        assert_eq!(db.waiting_between(friend, me).await.unwrap(), (0, 0));
        db.insert_message(friend, me, None, 15, None, &sealed(2))
            .await
            .unwrap();
        assert_eq!(db.waiting_between(friend, me).await.unwrap(), (1, 3));
    }
    #[tokio::test]
    async fn only_devices_from_before_the_upgrade_are_asked_for_old_chats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
        sqlx::migrate::Migrator::with_migrations(
            crate::db::MIGRATOR
                .iter()
                .filter(|m| m.version < 9)
                .cloned()
                .collect(),
        )
        .run(&pool)
        .await
        .unwrap();
        let old = Db { pool };
        let me = account(&old, 1, "me").await.id;
        let phone = old
            .add_device(me, &[7; 32], "Android phone", 10)
            .await
            .unwrap();
        old.pool.close().await;

        let db = Db::open(&path).await.unwrap();
        assert!(db.old_chats_wanted(phone).await.unwrap());
        let pc = db.add_device(me, &[8; 32], "Linux PC", 20).await.unwrap();
        assert!(!db.old_chats_wanted(pc).await.unwrap());
        db.old_chats_kept(phone).await.unwrap();
        assert!(!db.old_chats_wanted(phone).await.unwrap());
    }
}
