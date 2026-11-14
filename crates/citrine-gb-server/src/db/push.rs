use super::{Db, Result};
use citrine_gb_net::PushKinds;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushTarget {
    pub device: i64,
    pub token: String,
    pub kinds: PushKinds,
}

impl Db {
    pub async fn set_push_token(
        &self,
        account: i64,
        device: i64,
        token: &str,
        kinds: PushKinds,
        now: u64,
    ) -> Result<()> {
        let (kinds, now) = (i64::from(kinds.bits()), now as i64);
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "DELETE FROM push_tokens WHERE token = ?1 AND device_id != ?2",
            token,
            device,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO push_tokens (device_id, account_id, token, kinds, updated_at)
               SELECT id, account_id, ?3, ?4, ?5 FROM devices
                WHERE id = ?1 AND account_id = ?2 AND left_at IS NULL
             ON CONFLICT (device_id) DO UPDATE
                SET token = excluded.token, kinds = excluded.kinds, updated_at = excluded.updated_at",
            device,
            account,
            token,
            kinds,
            now,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn remove_push_token(&self, device: i64) -> Result<()> {
        sqlx::query!("DELETE FROM push_tokens WHERE device_id = ?1", device)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn drop_push_token(&self, token: &str) -> Result<()> {
        sqlx::query!("DELETE FROM push_tokens WHERE token = ?1", token)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn push_targets(&self, account: i64) -> Result<Vec<PushTarget>> {
        let rows = sqlx::query!(
            r#"SELECT p.device_id AS "device!: i64", p.token, p.kinds AS "kinds!: i64"
                 FROM push_tokens p JOIN devices d ON d.id = p.device_id
                WHERE p.account_id = ?1 AND d.left_at IS NULL
                ORDER BY p.device_id"#,
            account,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| PushTarget {
                device: r.device,
                token: r.token,
                kinds: PushKinds::from_bits(r.kinds as u8),
            })
            .collect())
    }

    pub async fn unread_from(&self, device: i64, from: i64) -> Result<u32> {
        let count = sqlx::query_scalar!(
            r#"SELECT count(*) AS "count!: i64" FROM message_deliveries d
                 JOIN messages m ON m.id = d.message_id
                WHERE d.device_id = ?1 AND m.from_id = ?2"#,
            device,
            from,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::{account, db};

    const ONLY_CHATS: PushKinds = PushKinds {
        friend_requests: false,
        invites: false,
        chats: true,
    };

    #[tokio::test]
    async fn a_device_registers_refreshes_and_removes_its_token() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let phone = db.add_device(me, &[7; 32], "Pixel", 10).await.unwrap();
        assert!(db.push_targets(me).await.unwrap().is_empty());
        db.set_push_token(me, phone, "first", PushKinds::ALL, 11)
            .await
            .unwrap();
        db.set_push_token(me, phone, "second", ONLY_CHATS, 12)
            .await
            .unwrap();
        assert_eq!(
            db.push_targets(me).await.unwrap(),
            vec![PushTarget {
                device: phone,
                token: "second".into(),
                kinds: ONLY_CHATS,
            }]
        );
        db.remove_push_token(phone).await.unwrap();
        assert!(db.push_targets(me).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_token_follows_the_account_signed_in_on_the_phone() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let other = account(&db, 2, "other").await.id;
        let mine = db.add_device(me, &[7; 32], "Pixel", 10).await.unwrap();
        let theirs = db.add_device(other, &[8; 32], "Pixel", 10).await.unwrap();
        db.set_push_token(me, mine, "phone", PushKinds::ALL, 11)
            .await
            .unwrap();
        db.set_push_token(other, theirs, "phone", PushKinds::ALL, 12)
            .await
            .unwrap();
        assert!(db.push_targets(me).await.unwrap().is_empty());
        assert_eq!(db.push_targets(other).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn signing_out_or_leaving_removes_the_token() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let phone = db.add_device(me, &[7; 32], "Pixel", 10).await.unwrap();
        let tablet = db.add_device(me, &[8; 32], "Tab", 10).await.unwrap();
        db.set_push_token(me, phone, "phone", PushKinds::ALL, 11)
            .await
            .unwrap();
        db.set_push_token(me, tablet, "tablet", PushKinds::ALL, 11)
            .await
            .unwrap();
        assert!(db.sign_out_device(me, phone, 12).await.unwrap());
        assert!(db.leave_device(me, tablet, 12).await.unwrap());
        assert!(db.push_targets(me).await.unwrap().is_empty());
        db.set_push_token(me, tablet, "tablet", PushKinds::ALL, 13)
            .await
            .unwrap();
        assert!(db.push_targets(me).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn another_accounts_device_cannot_take_a_token() {
        let (_dir, db) = db().await;
        let me = account(&db, 1, "me").await.id;
        let other = account(&db, 2, "other").await.id;
        let theirs = db.add_device(other, &[8; 32], "Pixel", 10).await.unwrap();
        db.set_push_token(me, theirs, "phone", PushKinds::ALL, 11)
            .await
            .unwrap();
        assert!(db.push_targets(me).await.unwrap().is_empty());
        assert!(db.push_targets(other).await.unwrap().is_empty());
    }

    #[test]
    fn kinds_survive_their_bits() {
        for bits in 0..8 {
            assert_eq!(PushKinds::from_bits(bits).bits(), bits);
        }
        assert_eq!(PushKinds::ALL.bits(), 7);
    }
}
