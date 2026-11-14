use citrine_gb_net::OnlineId;

use super::{Db, Result, online_id};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GtsRow {
    pub pid: i64,
    pub account: i64,
    pub generation: i64,
    pub record: Vec<u8>,
    pub received: Option<Vec<u8>>,
    pub deposited_at: u64,
    pub traded_at: Option<u64>,
    pub traded_with: Option<i64>,
    pub sim: bool,
    pub traded_with_sim: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GtsListing {
    pub species: i64,
    pub gender: i64,
    pub level: i64,
    pub wanted_species: i64,
    pub wanted_gender: i64,
    pub wanted_min: i64,
    pub wanted_max: i64,
    pub country: i64,
}

impl Db {
    pub async fn gts_claim(
        &self,
        pid: i64,
        account: i64,
        generation: i64,
        now: u64,
    ) -> Result<bool> {
        let now = now as i64;
        if self.gts_is_sim(pid).await? {
            return Ok(false);
        }
        sqlx::query!(
            "INSERT INTO gts_profiles (pid, account_id, generation, created_at)
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT (pid) DO NOTHING",
            pid,
            account,
            generation,
            now,
        )
        .execute(&self.pool)
        .await?;
        let owner = sqlx::query_scalar!("SELECT account_id FROM gts_profiles WHERE pid = ?1", pid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(owner == Some(account))
    }

    pub async fn gts_set_profile(&self, pid: i64, profile: &[u8]) -> Result<()> {
        sqlx::query!(
            "UPDATE gts_profiles SET profile = ?2 WHERE pid = ?1",
            pid,
            profile,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn gts_searched(&self, pid: i64, now: u64) -> Result<()> {
        let now = now as i64;
        sqlx::query!(
            "UPDATE gts_profiles SET last_search = ?2 WHERE pid = ?1",
            pid,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn gts_last_search(&self, pid: i64) -> Result<Option<u64>> {
        let last = sqlx::query_scalar!("SELECT last_search FROM gts_profiles WHERE pid = ?1", pid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(last.flatten().map(|at| at as u64))
    }

    pub async fn gts_record(&self, pid: i64) -> Result<Option<GtsRow>> {
        let row = sqlx::query!(
            r#"SELECT r.pid AS "pid!", p.account_id AS "account!", r.generation AS "generation!",
                      r.record AS "record!", r.received, r.deposited_at AS "deposited_at!", r.traded_at, r.traded_with,
                      r.traded_with_sim AS "traded_with_sim!"
                 FROM gts_records r JOIN gts_profiles p ON p.pid = r.pid
                WHERE r.pid = ?1"#,
            pid
        )
        .fetch_optional(&self.pool)
        .await?;
        if let Some(r) = row {
            return Ok(Some(GtsRow {
                pid: r.pid,
                account: r.account,
                generation: r.generation,
                record: r.record,
                received: r.received,
                deposited_at: r.deposited_at as u64,
                traded_at: r.traded_at.map(|at| at as u64),
                traded_with: r.traded_with,
                sim: false,
                traded_with_sim: r.traded_with_sim != 0,
            }));
        }
        self.gts_sim_record(pid).await
    }

    pub async fn gts_deposit(
        &self,
        pid: i64,
        generation: i64,
        record: &[u8],
        listing: GtsListing,
        now: u64,
    ) -> Result<bool> {
        let now = now as i64;
        let done = sqlx::query!(
            "INSERT INTO gts_records (pid, generation, record, species, gender, level,
                                      wanted_species, wanted_gender, wanted_min, wanted_max,
                                      country, deposited_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT (pid) DO NOTHING",
            pid,
            generation,
            record,
            listing.species,
            listing.gender,
            listing.level,
            listing.wanted_species,
            listing.wanted_gender,
            listing.wanted_min,
            listing.wanted_max,
            listing.country,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn gts_remove(&self, pid: i64, traded: bool) -> Result<bool> {
        let done = sqlx::query!(
            "DELETE FROM gts_records WHERE pid = ?1 AND (traded_at IS NOT NULL) = ?2",
            pid,
            traded,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn gts_candidates(
        &self,
        generation: i64,
        species: i64,
        searcher: i64,
    ) -> Result<Vec<GtsRow>> {
        let rows = sqlx::query!(
            r#"SELECT r.pid AS "pid!", p.account_id AS "account!", r.record AS "record!",
                      r.deposited_at AS "deposited_at!"
                 FROM gts_records r JOIN gts_profiles p ON p.pid = r.pid
                WHERE r.generation = ?1 AND r.species = ?2 AND r.traded_at IS NULL
                  AND p.account_id != ?3
                ORDER BY r.deposited_at DESC
                LIMIT 500"#,
            generation,
            species,
            searcher,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| GtsRow {
                pid: r.pid,
                account: r.account,
                generation,
                record: r.record,
                received: None,
                deposited_at: r.deposited_at as u64,
                traded_at: None,
                traded_with: None,
                sim: false,
                traded_with_sim: false,
            })
            .chain(self.gts_sim_candidates(generation, species).await?)
            .collect())
    }

    pub async fn gts_trade(
        &self,
        target: i64,
        received: &[u8],
        with: i64,
        searched_at: u64,
        now: u64,
    ) -> Result<bool> {
        let (searched_at, now) = (searched_at as i64, now as i64);
        let done = sqlx::query!(
            "UPDATE gts_records SET received = ?2, traded_at = ?3, traded_with = ?4
              WHERE pid = ?1 AND traded_at IS NULL AND deposited_at <= ?5",
            target,
            received,
            now,
            with,
            searched_at,
        )
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 1 {
            return Ok(true);
        }
        self.gts_sim_traded(target, received, with, searched_at as u64, now as u64)
            .await
    }

    pub async fn gts_listed(
        &self,
        generation: i64,
        species: Option<i64>,
        searcher: i64,
    ) -> Result<Vec<(GtsRow, Option<OnlineId>)>> {
        let rows = sqlx::query!(
            r#"SELECT r.pid AS "pid!", p.account_id AS "account!", a.online_id AS "online_id!",
                      r.record AS "record!", r.deposited_at AS "deposited_at!"
                 FROM gts_records r JOIN gts_profiles p ON p.pid = r.pid
                 JOIN accounts a ON a.id = p.account_id
                WHERE r.generation = ?1 AND (?2 IS NULL OR r.species = ?2)
                  AND r.traded_at IS NULL AND p.account_id != ?3
                ORDER BY r.deposited_at DESC
                LIMIT 500"#,
            generation,
            species,
            searcher,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let row = GtsRow {
                    pid: r.pid,
                    account: r.account,
                    generation,
                    record: r.record,
                    received: None,
                    deposited_at: r.deposited_at as u64,
                    traded_at: None,
                    traded_with: None,
                    sim: false,
                    traded_with_sim: false,
                };
                (row, Some(online_id(r.online_id)))
            })
            .chain(
                self.gts_sim_listed(generation, species)
                    .await?
                    .into_iter()
                    .map(|row| (row, None)),
            )
            .collect())
    }

    pub async fn gts_mine(&self, account: i64) -> Result<Vec<GtsRow>> {
        let rows = sqlx::query!(
            r#"SELECT r.pid AS "pid!", r.generation AS "generation!", r.record AS "record!",
                      r.received, r.deposited_at AS "deposited_at!", r.traded_at, r.traded_with,
                      r.traded_with_sim AS "traded_with_sim!"
                 FROM gts_records r JOIN gts_profiles p ON p.pid = r.pid
                WHERE p.account_id = ?1
                ORDER BY r.deposited_at DESC"#,
            account
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| GtsRow {
                pid: r.pid,
                account,
                generation: r.generation,
                record: r.record,
                received: r.received,
                deposited_at: r.deposited_at as u64,
                traded_at: r.traded_at.map(|at| at as u64),
                traded_with: r.traded_with,
                sim: false,
                traded_with_sim: r.traded_with_sim != 0,
            })
            .collect())
    }
}
