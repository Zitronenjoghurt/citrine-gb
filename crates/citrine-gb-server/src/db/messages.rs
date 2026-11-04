use super::{Db, Result, online_id};
use citrine_gb_net::{MessageId, NONCE_BYTES, OnlineId, Sealed};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    pub id: MessageId,
    pub from_id: i64,
    pub from: OnlineId,
    pub to: OnlineId,
    pub at: u64,
    pub sealed: Sealed,
    pub old_key: Option<u64>,
}

pub struct OldRow<'a> {
    pub friend: i64,
    pub key: u64,
    pub at: u64,
    pub sealed: &'a Sealed,
}

impl Db {
    pub async fn waiting_messages(&self) -> Result<(u64, u64)> {
        let r = sqlx::query!(
            r#"SELECT COUNT(*) AS "count!: i64", COALESCE(SUM(LENGTH(body)), 0) AS "bytes!: i64" FROM messages
                WHERE EXISTS (SELECT 1 FROM message_deliveries d JOIN devices v ON v.id = d.device_id
                               WHERE d.message_id = messages.id AND v.account_id = messages.to_id)"#
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((r.count as u64, r.bytes as u64))
    }

    pub async fn stored_messages(&self) -> Result<(u64, u64)> {
        let (count, bytes): (i64, i64) =
            sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(LENGTH(body)), 0) FROM messages")
                .fetch_one(&self.pool)
                .await?;
        Ok((count as u64, bytes as u64))
    }

    pub async fn insert_message(
        &self,
        from: i64,
        to: i64,
        from_device: Option<i64>,
        at: u64,
        key: Option<u64>,
        sealed: &Sealed,
    ) -> Result<MessageId> {
        let at = at as i64;
        let key = key.map(|k| k as i64);
        let nonce = &sealed.nonce[..];
        let from_device = from_device.unwrap_or(-1);
        let mut tx = self.pool.begin().await?;
        let id = sqlx::query!(
            "INSERT INTO messages (from_id, to_id, sent_at, nonce, body, sender_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            from,
            to,
            at,
            nonce,
            sealed.ciphertext,
            key,
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        sqlx::query!(
            "INSERT INTO message_deliveries (device_id, message_id)
               SELECT id, ?4 FROM devices
                WHERE left_at IS NULL AND (account_id = ?2 OR (account_id = ?1 AND id != ?3))",
            from,
            to,
            from_device,
            id,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id as MessageId)
    }

    pub async fn sent_once(&self, from: i64, key: u64) -> Result<Option<(MessageId, u64)>> {
        let key = key as i64;
        let row = sqlx::query!(
            r#"SELECT id AS "id!: i64", sent_at AS "sent_at!: i64" FROM messages
               WHERE from_id = ?1 AND sender_key = ?2"#,
            from,
            key,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.id as MessageId, r.sent_at as u64)))
    }

    pub async fn pending_messages(
        &self,
        device: i64,
        after: MessageId,
        limit: u32,
    ) -> Result<Vec<StoredMessage>> {
        let after = after as i64;
        let rows = sqlx::query!(
            r#"SELECT m.id AS "id!: i64", m.from_id AS "from_id!: i64", f.online_id AS "from_online!: i64",
                   t.online_id AS "to_online!: i64", m.sent_at AS "sent_at!: i64",
                   m.nonce AS "nonce!: Vec<u8>", m.body AS "body!: Vec<u8>", m.old_key AS "old_key: i64"
               FROM message_deliveries d
               JOIN messages m ON m.id = d.message_id
               JOIN accounts f ON f.id = m.from_id
               JOIN accounts t ON t.id = m.to_id
               WHERE d.device_id = ?1 AND d.message_id > ?2 ORDER BY d.message_id LIMIT ?3"#,
            device,
            after,
            limit,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let mut nonce = [0; NONCE_BYTES];
                if r.nonce.len() == nonce.len() {
                    nonce.copy_from_slice(&r.nonce);
                }
                StoredMessage {
                    id: r.id as MessageId,
                    from_id: r.from_id,
                    from: online_id(r.from_online),
                    to: online_id(r.to_online),
                    at: r.sent_at as u64,
                    sealed: Sealed {
                        nonce,
                        ciphertext: r.body,
                    },
                    old_key: r.old_key.map(|k| k as u64),
                }
            })
            .collect())
    }

    pub async fn keep_old_chats(
        &self,
        account: i64,
        device: i64,
        chats: &[OldRow<'_>],
    ) -> Result<Vec<(usize, MessageId)>> {
        let mut kept = Vec::new();
        let mut tx = self.pool.begin().await?;
        for (index, chat) in chats.iter().enumerate() {
            let key = chat.key as i64;
            let at = chat.at as i64;
            let nonce = &chat.sealed.nonce[..];
            let done = sqlx::query!(
                "INSERT OR IGNORE INTO messages (from_id, to_id, sent_at, nonce, body, receiver_keeps, old_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
                account,
                chat.friend,
                at,
                nonce,
                chat.sealed.ciphertext,
                key,
            )
            .execute(&mut *tx)
            .await?;
            if done.rows_affected() == 0 {
                continue;
            }
            let id = done.last_insert_rowid();
            sqlx::query!(
                "INSERT INTO message_deliveries (device_id, message_id)
                   SELECT id, ?3 FROM devices WHERE account_id = ?1 AND id != ?2 AND left_at IS NULL",
                account,
                device,
                id,
            )
            .execute(&mut *tx)
            .await?;
            kept.push((index, id as MessageId));
        }
        tx.commit().await?;
        Ok(kept)
    }

    pub async fn ack_messages(&self, device: i64, ids: &[MessageId]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for &id in ids {
            let id = id as i64;
            sqlx::query!(
                "DELETE FROM message_deliveries WHERE device_id = ?1 AND message_id = ?2",
                device,
                id
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    pub async fn restate_message(
        &self,
        from: i64,
        id: MessageId,
        sealed: Option<&Sealed>,
    ) -> Result<bool> {
        let id = id as i64;
        let done = match sealed {
            Some(sealed) => {
                let nonce = &sealed.nonce[..];
                sqlx::query!(
                    "UPDATE messages SET nonce = ?3, body = ?4 WHERE id = ?1 AND from_id = ?2",
                    id,
                    from,
                    nonce,
                    sealed.ciphertext,
                )
                .execute(&self.pool)
                .await?
            }
            None => {
                sqlx::query!(
                    "DELETE FROM messages WHERE id = ?1 AND from_id = ?2",
                    id,
                    from
                )
                .execute(&self.pool)
                .await?
            }
        };
        Ok(done.rows_affected() == 1)
    }

    pub async fn waiting_between(&self, from: i64, to: i64) -> Result<(u64, u64)> {
        let r = sqlx::query!(
            r#"SELECT COUNT(*) AS "count!: i64", COALESCE(SUM(LENGTH(body)), 0) AS "bytes!: i64"
               FROM messages m WHERE m.to_id = ?1 AND m.from_id = ?2 AND m.receiver_keeps = 1
                AND NOT EXISTS (SELECT 1 FROM devices v WHERE v.account_id = ?1 AND v.left_at IS NULL
                                 AND NOT EXISTS (SELECT 1 FROM message_deliveries d
                                                  WHERE d.device_id = v.id AND d.message_id = m.id))
                AND (EXISTS (SELECT 1 FROM devices WHERE account_id = ?1 AND left_at IS NULL)
                     OR m.sent_at > COALESCE((SELECT MAX(left_at) FROM devices WHERE account_id = ?1), 0))"#,
            to,
            from,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((r.count as u64, r.bytes as u64))
    }
}

#[cfg(test)]
mod tests {
    use crate::db::tests::{account, db};

    fn sealed(byte: u8) -> citrine_gb_net::Sealed {
        citrine_gb_net::Sealed {
            nonce: [byte; citrine_gb_net::NONCE_BYTES],
            ciphertext: vec![byte; 3],
        }
    }

    #[tokio::test]
    async fn every_device_gets_a_message_and_the_sender_gets_copies() {
        let (_dir, db) = db().await;
        let a = account(&db, 1, "a").await;
        let b = account(&db, 2, "b").await;
        let a_phone = db
            .add_device(a.id, &[1; 32], "Android phone", 5)
            .await
            .unwrap();
        let a_pc = db.add_device(a.id, &[2; 32], "Linux PC", 5).await.unwrap();
        let b_mac = db.add_device(b.id, &[3; 32], "Mac", 5).await.unwrap();
        let b_web = db
            .add_device(b.id, &[4; 32], "Firefox on Mac", 5)
            .await
            .unwrap();
        let first = db
            .insert_message(a.id, b.id, Some(a_phone), 100, None, &sealed(1))
            .await
            .unwrap();
        let second = db
            .insert_message(a.id, b.id, Some(a_phone), 200, None, &sealed(2))
            .await
            .unwrap();
        assert!(
            db.pending_messages(a_phone, 0, 10)
                .await
                .unwrap()
                .is_empty()
        );
        let copies = db.pending_messages(a_pc, 0, 10).await.unwrap();
        assert_eq!(copies.len(), 2);
        assert_eq!(copies[0].from, a.online_id);
        assert_eq!(copies[0].to, b.online_id);
        for device in [b_mac, b_web] {
            let got = db.pending_messages(device, 0, 10).await.unwrap();
            assert_eq!(
                got.iter().map(|m| m.id).collect::<Vec<_>>(),
                [first, second]
            );
            assert_eq!(got[0].sealed, sealed(1));
            assert_eq!(got[0].at, 100);
            assert_eq!(got[0].from_id, a.id);
        }
        let page = db.pending_messages(b_mac, first, 1).await.unwrap();
        assert_eq!(page[0].id, second);
        assert_eq!(db.waiting_between(a.id, b.id).await.unwrap(), (2, 6));
        assert_eq!(db.waiting_between(b.id, a.id).await.unwrap(), (0, 0));
        db.ack_messages(b_mac, &[first, second]).await.unwrap();
        assert_eq!(db.pending_messages(b_web, 0, 10).await.unwrap().len(), 2);
        assert_eq!(db.waiting_between(a.id, b.id).await.unwrap(), (0, 0));
        db.ack_messages(b_web, &[first, second]).await.unwrap();
        assert_eq!(db.waiting_between(a.id, b.id).await.unwrap(), (0, 0));
        assert_eq!(db.stored_messages().await.unwrap(), (2, 6));
    }

    #[tokio::test]
    async fn only_the_sender_restates_a_message() {
        let (_dir, db) = db().await;
        let a = account(&db, 1, "a").await;
        let b = account(&db, 2, "b").await;
        let b_mac = db.add_device(b.id, &[3; 32], "Mac", 5).await.unwrap();
        let id = db
            .insert_message(a.id, b.id, None, 100, None, &sealed(1))
            .await
            .unwrap();
        assert!(
            !db.restate_message(b.id, id, Some(&sealed(9)))
                .await
                .unwrap()
        );
        assert!(
            db.restate_message(a.id, id, Some(&sealed(2)))
                .await
                .unwrap()
        );
        let late = db.add_device(b.id, &[4; 32], "iPad", 6).await.unwrap();
        assert_eq!(
            db.pending_messages(late, 0, 10).await.unwrap()[0].sealed,
            sealed(2)
        );
        assert!(!db.restate_message(b.id, id, None).await.unwrap());
        assert!(db.restate_message(a.id, id, None).await.unwrap());
        assert!(db.pending_messages(b_mac, 0, 10).await.unwrap().is_empty());
        let later = db.add_device(b.id, &[5; 32], "Linux PC", 7).await.unwrap();
        assert!(db.pending_messages(later, 0, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_account_without_devices_still_gets_its_messages_later() {
        let (_dir, db) = db().await;
        let a = account(&db, 1, "a").await;
        let b = account(&db, 2, "b").await;
        let id = db
            .insert_message(a.id, b.id, None, 100, None, &sealed(1))
            .await
            .unwrap();
        assert_eq!(db.waiting_between(a.id, b.id).await.unwrap(), (1, 3));
        assert_eq!(db.waiting_messages().await.unwrap(), (0, 0));
        let first = db.add_device(b.id, &[3; 32], "Mac", 5).await.unwrap();
        assert_eq!(db.pending_messages(first, 0, 10).await.unwrap()[0].id, id);
        assert_eq!(db.waiting_messages().await.unwrap(), (1, 3));
    }
    #[tokio::test]
    async fn old_chats_reach_only_the_other_devices_of_their_account_once() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let friend = account(&db, 2, "friend").await.id;
        let phone = db
            .add_device(me, &[7; 32], "Android phone", 10)
            .await
            .unwrap();
        let pc = db.add_device(me, &[8; 32], "Linux PC", 10).await.unwrap();
        let theirs = db.add_device(friend, &[9; 32], "iPad", 10).await.unwrap();
        let body = sealed(4);
        let rows = [
            super::OldRow {
                friend,
                key: 41,
                at: 5,
                sealed: &body,
            },
            super::OldRow {
                friend,
                key: 42,
                at: 6,
                sealed: &body,
            },
        ];
        let kept = db.keep_old_chats(me, phone, &rows).await.unwrap();
        assert_eq!(kept.iter().map(|k| k.0).collect::<Vec<_>>(), [0, 1]);
        assert!(db.keep_old_chats(me, pc, &rows).await.unwrap().is_empty());
        assert!(db.pending_messages(phone, 0, 10).await.unwrap().is_empty());
        assert!(db.pending_messages(theirs, 0, 10).await.unwrap().is_empty());
        let got = db.pending_messages(pc, 0, 10).await.unwrap();
        assert_eq!(
            got.iter().map(|m| (m.old_key, m.at)).collect::<Vec<_>>(),
            [(Some(41), 5), (Some(42), 6)]
        );
        assert_eq!(db.waiting_between(me, friend).await.unwrap(), (0, 0));
        assert_eq!(db.waiting_between(friend, me).await.unwrap(), (0, 0));
        let later = db.add_device(me, &[5; 32], "Mac", 20).await.unwrap();
        assert_eq!(db.pending_messages(later, 0, 10).await.unwrap().len(), 2);
        let friend_later = db.add_device(friend, &[6; 32], "Phone", 20).await.unwrap();
        assert!(
            db.pending_messages(friend_later, 0, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
