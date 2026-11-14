use super::{Db, GtsListing, GtsRow, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimRow {
    pub pid: i64,
    pub name: String,
    pub language: i64,
    pub version: i64,
    pub country: i64,
    pub region: i64,
    pub tid: i64,
    pub sid: i64,
    pub female: bool,
    pub avatar: i64,
    pub utc_offset: i64,
    pub seed: i64,
    pub started_at: u64,
    pub days: i64,
    pub next_at: u64,
}

fn sim_row(
    pid: i64,
    generation: i64,
    record: Vec<u8>,
    received: Option<Vec<u8>>,
    deposited_at: i64,
    traded_at: Option<i64>,
    traded_with: Option<i64>,
) -> GtsRow {
    GtsRow {
        pid,
        account: 0,
        generation,
        record,
        received,
        deposited_at: deposited_at as u64,
        traded_at: traded_at.map(|at| at as u64),
        traded_with,
        sim: true,
        traded_with_sim: false,
    }
}

impl Db {
    pub async fn gts_is_sim(&self, pid: i64) -> Result<bool> {
        let found = sqlx::query_scalar!("SELECT pid FROM gts_sims WHERE pid = ?1", pid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(found.is_some())
    }

    pub async fn gts_sim_record(&self, pid: i64) -> Result<Option<GtsRow>> {
        let row = sqlx::query!(
            r#"SELECT pid AS "pid!", generation, record, received, deposited_at, traded_at, traded_with
                 FROM gts_sim_records WHERE pid = ?1"#,
            pid
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| {
            sim_row(
                r.pid,
                r.generation,
                r.record,
                r.received,
                r.deposited_at,
                r.traded_at,
                r.traded_with,
            )
        }))
    }

    pub async fn gts_sim_candidates(&self, generation: i64, species: i64) -> Result<Vec<GtsRow>> {
        let rows = sqlx::query!(
            r#"SELECT pid AS "pid!", record, deposited_at FROM gts_sim_records
                WHERE generation = ?1 AND species = ?2 AND traded_at IS NULL
                ORDER BY deposited_at DESC LIMIT 500"#,
            generation,
            species,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                sim_row(
                    r.pid,
                    generation,
                    r.record,
                    None,
                    r.deposited_at,
                    None,
                    None,
                )
            })
            .collect())
    }

    pub async fn gts_sim_listed(
        &self,
        generation: i64,
        species: Option<i64>,
    ) -> Result<Vec<GtsRow>> {
        let rows = sqlx::query!(
            r#"SELECT pid AS "pid!", record, deposited_at FROM gts_sim_records
                WHERE generation = ?1 AND (?2 IS NULL OR species = ?2) AND traded_at IS NULL
                ORDER BY deposited_at DESC LIMIT 500"#,
            generation,
            species,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                sim_row(
                    r.pid,
                    generation,
                    r.record,
                    None,
                    r.deposited_at,
                    None,
                    None,
                )
            })
            .collect())
    }

    pub async fn gts_sim_traded(
        &self,
        target: i64,
        received: &[u8],
        with: i64,
        searched_at: u64,
        now: u64,
    ) -> Result<bool> {
        let (searched_at, now) = (searched_at as i64, now as i64);
        let done = sqlx::query!(
            "UPDATE gts_sim_records SET received = ?2, traded_at = ?3, traded_with = ?4
              WHERE pid = ?1 AND traded_at IS NULL AND deposited_at <= ?5",
            target,
            received,
            now,
            with,
            searched_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn gts_sims(&self) -> Result<Vec<SimRow>> {
        let rows = sqlx::query!(
            r#"SELECT pid AS "pid!", name, language, version, country, region, tid, sid, female, avatar,
                      utc_offset, seed, started_at, days, next_at
                 FROM gts_sims ORDER BY pid"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SimRow {
                pid: r.pid,
                name: r.name,
                language: r.language,
                version: r.version,
                country: r.country,
                region: r.region,
                tid: r.tid,
                sid: r.sid,
                female: r.female != 0,
                avatar: r.avatar,
                utc_offset: r.utc_offset,
                seed: r.seed,
                started_at: r.started_at as u64,
                days: r.days,
                next_at: r.next_at as u64,
            })
            .collect())
    }

    pub async fn gts_sim_add(&self, sim: &SimRow) -> Result<bool> {
        let taken = sqlx::query_scalar!("SELECT pid FROM gts_profiles WHERE pid = ?1", sim.pid)
            .fetch_optional(&self.pool)
            .await?;
        if taken.is_some() {
            return Ok(false);
        }
        let (started_at, next_at) = (sim.started_at as i64, sim.next_at as i64);
        let done = sqlx::query!(
            "INSERT INTO gts_sims (pid, name, language, version, country, region, tid, sid, female,
                                   avatar, utc_offset, seed, started_at, days, next_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT (pid) DO NOTHING",
            sim.pid,
            sim.name,
            sim.language,
            sim.version,
            sim.country,
            sim.region,
            sim.tid,
            sim.sid,
            sim.female,
            sim.avatar,
            sim.utc_offset,
            sim.seed,
            started_at,
            sim.days,
            next_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn gts_sim_played(&self, pid: i64, days: i64, seed: i64, next_at: u64) -> Result<()> {
        let next_at = next_at as i64;
        sqlx::query!(
            "UPDATE gts_sims SET days = ?2, seed = ?3, next_at = ?4 WHERE pid = ?1",
            pid,
            days,
            seed,
            next_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn gts_sim_deposit(
        &self,
        pid: i64,
        generation: i64,
        record: &[u8],
        listing: GtsListing,
        deposited_at: u64,
    ) -> Result<bool> {
        let deposited_at = deposited_at as i64;
        let done = sqlx::query!(
            "INSERT INTO gts_sim_records (pid, generation, record, species, gender, level,
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
            deposited_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn gts_sim_withdraw(&self, pid: i64) -> Result<()> {
        sqlx::query!("DELETE FROM gts_sim_records WHERE pid = ?1", pid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn gts_sim_open(&self) -> Result<i64> {
        let open = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "open!: i64" FROM gts_sim_records WHERE traded_at IS NULL"#
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(open)
    }

    pub async fn gts_sims_clear(&self) -> Result<()> {
        sqlx::query!("DELETE FROM gts_sims")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn gts_active_players(&self, since: u64) -> Result<i64> {
        let since = since as i64;
        let active = sqlx::query_scalar!(
            r#"SELECT COUNT(DISTINCT p.account_id) AS "active!: i64" FROM gts_profiles p
                WHERE p.last_search >= ?1 OR p.created_at >= ?1
                   OR EXISTS (SELECT 1 FROM gts_records r WHERE r.pid = p.pid AND r.deposited_at >= ?1)"#,
            since
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(active)
    }

    pub async fn gts_waiting(&self, generation: i64, before: u64) -> Result<Vec<GtsRow>> {
        let before = before as i64;
        let rows = sqlx::query!(
            r#"SELECT r.pid AS "pid!", p.account_id AS "account!", r.record AS "record!",
                      r.deposited_at AS "deposited_at!"
                 FROM gts_records r JOIN gts_profiles p ON p.pid = r.pid
                WHERE r.generation = ?1 AND r.traded_at IS NULL AND r.deposited_at <= ?2
                ORDER BY r.deposited_at"#,
            generation,
            before,
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
            .collect())
    }

    pub async fn gts_sim_took(
        &self,
        target: i64,
        received: &[u8],
        sim: i64,
        now: u64,
    ) -> Result<bool> {
        let now = now as i64;
        let done = sqlx::query!(
            "UPDATE gts_records SET received = ?2, traded_at = ?3, traded_with = ?4, traded_with_sim = 1
              WHERE pid = ?1 AND traded_at IS NULL",
            target,
            received,
            now,
            sim,
        )
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }
}
