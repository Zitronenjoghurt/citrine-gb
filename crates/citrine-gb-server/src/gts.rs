mod crypt;
mod record;
mod sims;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use citrine_gb_net::{
    GTS_PAGE, GtsDeposit, GtsListing as Listing, GtsReply, GtsState, GtsWanted, OnlineId, ServerMsg,
};

use crate::db::{Db, GtsListing, GtsRow};
use crypt::Generation;
use record::{Record, Wanted};

const TOKEN_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const TOKEN_LENGTH: usize = 32;
const TOKEN_LIFE: Duration = Duration::from_secs(600);
const MAX_TOKENS: usize = 4096;
const PROFILE_BYTES: usize = 100;
const FINISH_BYTES: usize = 8;
const GEN5_UPLOAD_BYTES: usize = 432;
const MOST_RESULTS: usize = 7;

const OK: [u8; 2] = [1, 0];
const FAILED: [u8; 2] = [2, 0];
const NOT_TRADED: [u8; 2] = [3, 0];
const WAITING: [u8; 2] = [4, 0];
const NONE: [u8; 2] = [5, 0];
const REFUSED: [u8; 2] = [0x0C, 0];
const ALREADY: [u8; 2] = [0x0E, 0];

struct Token {
    pid: i32,
    account: i64,
    generation: Generation,
    issued: Instant,
}

enum Pending {
    Deposit(Record),
    Trade { offer: Record, target: i64 },
}

#[derive(Default)]
pub struct Gts {
    tokens: Mutex<HashMap<String, Token>>,
    pending: Mutex<HashMap<i64, (Instant, Pending)>>,
    notices: Mutex<Vec<(i64, ServerMsg)>>,
}

struct Call<'a> {
    db: &'a Db,
    generation: Generation,
    pid: i64,
    account: i64,
    now: u64,
}

fn query_value<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn listing(row: &GtsRow, friend: Option<OnlineId>) -> Option<Listing> {
    let generation = Generation::from_number(row.generation)?;
    let record = Record::new(generation, &row.record)?;
    let wanted = record.wanted();
    Some(Listing {
        offered: record.listed()?,
        wanted: GtsWanted {
            species: wanted.species,
            gender: wanted.gender,
            min_level: wanted.min,
            max_level: wanted.max,
        },
        trainer: record.trainer(),
        game: record.version(),
        country: record.country(),
        deposited: row.deposited_at,
        friend,
        simulated: row.sim,
    })
}

fn deposit(row: &GtsRow) -> Option<GtsDeposit> {
    let generation = Generation::from_number(row.generation)?;
    let state = match (&row.received, row.traded_at) {
        (Some(received), Some(at)) => {
            let received = Record::new(generation, received)?;
            GtsState::Traded {
                at,
                received: received.listed()?,
                trainer: received.trainer(),
                simulated: row.traded_with_sim,
            }
        }
        _ => GtsState::Waiting,
    };
    Some(GtsDeposit {
        generation: u8::try_from(row.generation).ok()?,
        listing: listing(row, None)?,
        state,
    })
}

fn token() -> String {
    let mut bytes = [0u8; TOKEN_LENGTH];
    getrandom::getrandom(&mut bytes).ok();
    bytes
        .iter()
        .map(|byte| char::from(TOKEN_CHARS[usize::from(*byte) % TOKEN_CHARS.len()]))
        .collect()
}

impl Gts {
    pub async fn handle(&self, db: &Db, account: i64, friends: &[i64], target: &str) -> GtsReply {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let Some((generation, endpoint)) = Generation::of_path(path) else {
            return GtsReply::Status(404);
        };
        let Some(pid) = query_value(query, "pid").and_then(|pid| pid.parse::<i32>().ok()) else {
            return GtsReply::Status(400);
        };
        let Some(hash) = query_value(query, "hash") else {
            return GtsReply::Body(self.issue(pid, account, generation).into_bytes());
        };
        if !self.redeem(hash, pid, account, generation) {
            return GtsReply::Status(400);
        }
        let Some(data) = query_value(query, "data").and_then(|data| crypt::open(generation, data))
        else {
            return GtsReply::Status(400);
        };
        if data.pid != pid {
            return GtsReply::Status(400);
        }
        let now = crate::now();
        let pid = i64::from(pid);
        match db.gts_claim(pid, account, generation.number(), now).await {
            Ok(true) => {}
            Ok(false) => return GtsReply::Status(400),
            Err(_) => return GtsReply::Status(500),
        }
        let call = Call {
            db,
            generation,
            pid,
            account,
            now,
        };
        match self.answer(&call, friends, endpoint, &data.payload).await {
            Ok(Some(body)) => GtsReply::Body(crypt::sign(generation, body)),
            Ok(None) => GtsReply::Status(404),
            Err(_) => GtsReply::Status(500),
        }
    }

    #[expect(clippy::too_many_arguments, reason = "one gts search")]
    pub async fn browse(
        &self,
        db: &Db,
        account: i64,
        friends: &[i64],
        generation: u8,
        species: Option<u16>,
        page: u32,
        simulated: bool,
    ) -> crate::db::Result<ServerMsg> {
        let mut found: Vec<Listing> = db
            .gts_listed(i64::from(generation), species.map(i64::from), account)
            .await?
            .iter()
            .filter_map(|(row, online)| {
                listing(row, (*online).filter(|_| friends.contains(&row.account)))
            })
            .collect();
        found.sort_by_key(|listed| listed.friend.is_none());
        let total = u32::try_from(found.len()).unwrap_or(u32::MAX);
        let skip = usize::try_from(page)
            .unwrap_or(usize::MAX)
            .saturating_mul(GTS_PAGE);
        Ok(ServerMsg::GtsListings {
            generation,
            species,
            page,
            total,
            listings: found.into_iter().skip(skip).take(GTS_PAGE).collect(),
            simulated,
        })
    }

    pub async fn mine(&self, db: &Db, account: i64) -> crate::db::Result<Vec<GtsDeposit>> {
        Ok(db
            .gts_mine(account)
            .await?
            .iter()
            .filter_map(deposit)
            .collect())
    }

    pub fn take_notices(&self) -> Vec<(i64, ServerMsg)> {
        std::mem::take(&mut *self.notices())
    }

    fn notices(&self) -> std::sync::MutexGuard<'_, Vec<(i64, ServerMsg)>> {
        self.notices
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub async fn simulate(&self, db: &Db, now: u64) -> crate::db::Result<Vec<(i64, ServerMsg)>> {
        let mut told = Vec::new();
        for pid in sims::tick(db, now).await? {
            if let Some(row) = db.gts_record(pid).await?
                && let Some(done) = deposit(&row)
            {
                told.push((row.account, ServerMsg::GtsTraded(done)));
            }
        }
        Ok(told)
    }

    fn issue(&self, pid: i32, account: i64, generation: Generation) -> String {
        let token = token();
        let mut tokens = self
            .tokens
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tokens.retain(|_, token| token.issued.elapsed() < TOKEN_LIFE);
        if tokens.len() < MAX_TOKENS {
            tokens.insert(
                crypt::hash(generation, &token),
                Token {
                    pid,
                    account,
                    generation,
                    issued: Instant::now(),
                },
            );
        }
        token
    }

    fn redeem(&self, hash: &str, pid: i32, account: i64, generation: Generation) -> bool {
        let mut tokens = self
            .tokens
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tokens.remove(hash).is_some_and(|token| {
            token.pid == pid
                && token.account == account
                && token.generation == generation
                && token.issued.elapsed() < TOKEN_LIFE
        })
    }

    fn hold(&self, pid: i64, pending: Pending) {
        let mut held = self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        held.retain(|_, (since, _)| since.elapsed() < TOKEN_LIFE);
        held.insert(pid, (Instant::now(), pending));
    }

    fn take(&self, pid: i64) -> Option<Pending> {
        let mut held = self
            .pending
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        held.remove(&pid)
            .filter(|(since, _)| since.elapsed() < TOKEN_LIFE)
            .map(|(_, pending)| pending)
    }

    async fn answer(
        &self,
        call: &Call<'_>,
        friends: &[i64],
        endpoint: &str,
        payload: &[u8],
    ) -> crate::db::Result<Option<Vec<u8>>> {
        let generation = call.generation;
        let db = call.db;
        let pid = call.pid;
        let mine = db.gts_record(pid).await?;
        let body = match endpoint {
            "worldexchange/info.asp" => OK.to_vec(),
            "common/setProfile.asp" => {
                if payload.len() == PROFILE_BYTES {
                    db.gts_set_profile(pid, payload).await?;
                }
                vec![0; 8]
            }
            "worldexchange/result.asp" => match mine {
                None => NONE.to_vec(),
                Some(GtsRow {
                    received: Some(received),
                    ..
                }) => received,
                Some(_) => WAITING.to_vec(),
            },
            "worldexchange/get.asp" => {
                mine.map_or_else(|| NONE.to_vec(), |row| row.received.unwrap_or(row.record))
            }
            "worldexchange/delete.asp" => {
                if db.gts_remove(pid, true).await? {
                    OK.to_vec()
                } else if generation == Generation::Five {
                    NONE.to_vec()
                } else {
                    NOT_TRADED.to_vec()
                }
            }
            "worldexchange/return.asp" => {
                if db.gts_remove(pid, false).await? {
                    OK.to_vec()
                } else {
                    FAILED.to_vec()
                }
            }
            "worldexchange/post.asp" => {
                let Some(mut record) = upload(generation, payload).map(|(record, _)| record) else {
                    return Ok(Some(REFUSED.to_vec()));
                };
                if mine.is_some() {
                    ALREADY.to_vec()
                } else if record.check().is_err() {
                    REFUSED.to_vec()
                } else {
                    record.stamp(pid as i32, call.now, None);
                    self.hold(pid, Pending::Deposit(record));
                    OK.to_vec()
                }
            }
            "worldexchange/post_finish.asp" => {
                if payload.len() != FINISH_BYTES {
                    return Ok(Some(FAILED.to_vec()));
                }
                match self.take(pid) {
                    Some(Pending::Deposit(record)) => {
                        let wanted = record.wanted();
                        let listing = GtsListing {
                            species: i64::from(record.species()),
                            gender: i64::from(record.gender()),
                            level: i64::from(record.level()),
                            wanted_species: i64::from(wanted.species),
                            wanted_gender: i64::from(wanted.gender),
                            wanted_min: i64::from(wanted.min),
                            wanted_max: i64::from(wanted.max),
                            country: i64::from(record.country()),
                        };
                        if db
                            .gts_deposit(pid, generation.number(), &record.bytes, listing, call.now)
                            .await?
                        {
                            OK.to_vec()
                        } else {
                            NONE.to_vec()
                        }
                    }
                    _ => FAILED.to_vec(),
                }
            }
            "worldexchange/search.asp" => {
                let records = self.search(call, friends, payload).await?;
                let mut body = match generation {
                    Generation::Four => Vec::new(),
                    Generation::Five => OK.to_vec(),
                };
                for record in records {
                    body.extend_from_slice(&record);
                }
                body
            }
            "worldexchange/exchange.asp" => self.exchange(call, payload).await?,
            "worldexchange/exchange_finish.asp" => {
                if payload.len() != FINISH_BYTES {
                    return Ok(Some(FAILED.to_vec()));
                }
                match self.take(pid) {
                    Some(Pending::Trade { offer, target }) => {
                        let searched = db.gts_last_search(pid).await?.unwrap_or_default();
                        let Some(row) = db.gts_record(target).await? else {
                            return Ok(Some(FAILED.to_vec()));
                        };
                        let mut received = offer;
                        received.stamp(target as i32, row.deposited_at, Some(call.now));
                        if db
                            .gts_trade(target, &received.bytes, pid, searched, call.now)
                            .await?
                        {
                            if row.sim {
                                if let Some(theirs) = Record::new(call.generation, &row.record)
                                    && let Some(received) = theirs.listed()
                                {
                                    let trainer = theirs.trainer();
                                    self.notices().push((
                                        call.account,
                                        ServerMsg::GtsTradedWithSim { trainer, received },
                                    ));
                                }
                            } else if let Some(row) = db.gts_record(target).await?
                                && let Some(done) = deposit(&row)
                            {
                                self.notices()
                                    .push((row.account, ServerMsg::GtsTraded(done)));
                            }
                            OK.to_vec()
                        } else {
                            FAILED.to_vec()
                        }
                    }
                    _ => FAILED.to_vec(),
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(body))
    }

    async fn search(
        &self,
        call: &Call<'_>,
        friends: &[i64],
        payload: &[u8],
    ) -> crate::db::Result<Vec<Vec<u8>>> {
        if payload.len() < 7 {
            return Ok(Vec::new());
        }
        let species = u16::from_le_bytes([payload[0], payload[1]]);
        let wanted = Wanted {
            species,
            gender: payload[2],
            min: payload[3],
            max: payload[4],
        };
        let count = usize::from(payload[6]).min(MOST_RESULTS);
        let country = payload.get(7).copied().unwrap_or(0);
        call.db.gts_searched(call.pid, call.now).await?;
        let mut found: Vec<(bool, Vec<u8>)> = call
            .db
            .gts_candidates(call.generation.number(), i64::from(species), call.account)
            .await?
            .into_iter()
            .filter_map(|row| {
                let record = Record::new(call.generation, &row.record)?;
                (record.fits(wanted) && (country == 0 || record.country() == country))
                    .then(|| (friends.contains(&row.account), row.record))
            })
            .collect();
        found.sort_by_key(|(friend, _)| !friend);
        Ok(found
            .into_iter()
            .take(count)
            .map(|(_, record)| record)
            .collect())
    }

    async fn exchange(&self, call: &Call<'_>, payload: &[u8]) -> crate::db::Result<Vec<u8>> {
        let Some((offer, target)) = upload(call.generation, payload) else {
            return Ok(REFUSED.to_vec());
        };
        let db = call.db;
        let Some(searched) = db.gts_last_search(call.pid).await? else {
            return Ok(FAILED.to_vec());
        };
        let target = i64::from(target);
        let Some(row) = db.gts_record(target).await? else {
            return Ok(FAILED.to_vec());
        };
        if row.traded_at.is_some()
            || row.deposited_at > searched
            || row.account == call.account
            || row.generation != call.generation.number()
        {
            return Ok(FAILED.to_vec());
        }
        let Some(theirs) = Record::new(call.generation, &row.record) else {
            return Ok(FAILED.to_vec());
        };
        if offer.check().is_err() || !offer.fits(theirs.wanted()) {
            return Ok(REFUSED.to_vec());
        }
        self.hold(call.pid, Pending::Trade { offer, target });
        Ok(row.record)
    }
}

fn upload(generation: Generation, payload: &[u8]) -> Option<(Record, i32)> {
    let size = generation.record_bytes();
    let expected = match generation {
        Generation::Four => [size, size + 4],
        Generation::Five => [GEN5_UPLOAD_BYTES, GEN5_UPLOAD_BYTES],
    };
    if !expected.contains(&payload.len()) {
        return None;
    }
    let record = Record::new(generation, &payload[..size])?;
    let target = payload.get(size..size + 4).map_or(0, |bytes| {
        i32::from_le_bytes(bytes.try_into().unwrap_or_default())
    });
    Some((record, target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_net::PublicKey;
    use record::tests::pokemon_record;

    const A: i32 = 1_000_001;
    const B: i32 = 2_000_002;

    struct Setup {
        gts: Gts,
        db: Db,
        a: i64,
        b: i64,
        _dir: tempfile::TempDir,
    }

    async fn setup() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("relay.db")).await.unwrap();
        let a = db
            .create_account(&[1; 32], &PublicKey([1; 32]), "A", 1)
            .await
            .unwrap()
            .id;
        let b = db
            .create_account(&[2; 32], &PublicKey([2; 32]), "B", 1)
            .await
            .unwrap()
            .id;
        Setup {
            gts: Gts::default(),
            db,
            a,
            b,
            _dir: dir,
        }
    }

    impl Setup {
        async fn call(&self, account: i64, pid: i32, endpoint: &str, payload: &[u8]) -> Vec<u8> {
            self.call_in(Generation::Four, account, pid, endpoint, payload)
                .await
        }

        async fn call5(&self, account: i64, pid: i32, endpoint: &str, payload: &[u8]) -> Vec<u8> {
            let mut body = self
                .call_in(Generation::Five, account, pid, endpoint, payload)
                .await;
            let hash = body.split_off(body.len() - 40);
            assert_eq!(
                crypt::sign(Generation::Five, body.clone()),
                [&body, &hash[..]].concat()
            );
            body
        }

        async fn call_in(
            &self,
            generation: Generation,
            account: i64,
            pid: i32,
            endpoint: &str,
            payload: &[u8],
        ) -> Vec<u8> {
            let path = match generation {
                Generation::Four => format!("/pokemondpds/{endpoint}"),
                Generation::Five => format!("/syachi2ds/web/{endpoint}"),
            };
            let GtsReply::Body(token) = self
                .gts
                .handle(&self.db, account, &[], &format!("{path}?pid={pid}"))
                .await
            else {
                panic!("no token");
            };
            let token = String::from_utf8(token).unwrap();
            assert_eq!(token.len(), 32);
            let target = format!(
                "{path}?pid={pid}&hash={}&data={}",
                crypt::hash(generation, &token),
                crypt::seal(generation, pid, payload)
            );
            match self.gts.handle(&self.db, account, &[], &target).await {
                GtsReply::Body(body) => body,
                GtsReply::Status(status) => panic!("{endpoint} answered {status}"),
            }
        }
    }

    fn search(species: u16) -> Vec<u8> {
        let mut asked = species.to_le_bytes().to_vec();
        asked.extend_from_slice(&[3, 0, 0, 0, 7, 0]);
        asked
    }

    fn offer(record: &Record, target: i32) -> Vec<u8> {
        let mut bytes = record.bytes.clone();
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes
    }

    #[tokio::test]
    async fn a_deposit_is_found_traded_and_collected_by_its_owner() {
        let s = setup().await;
        assert_eq!(s.call(s.a, A, "worldexchange/info.asp", &[]).await, OK);
        assert_eq!(
            s.call(s.a, A, "common/setProfile.asp", &[0; 100]).await,
            vec![0; 8]
        );
        assert_eq!(s.call(s.a, A, "worldexchange/result.asp", &[]).await, NONE);

        let starly = pokemon_record(Generation::Four, 396, 5);
        assert_eq!(
            s.call(s.a, A, "worldexchange/post.asp", &starly.bytes)
                .await,
            OK
        );
        assert_eq!(
            s.call(s.a, A, "worldexchange/post_finish.asp", &[0; 8])
                .await,
            OK
        );
        assert_eq!(
            s.call(s.a, A, "worldexchange/result.asp", &[]).await,
            WAITING
        );
        assert_eq!(
            s.call(s.a, A, "worldexchange/post.asp", &starly.bytes)
                .await,
            ALREADY
        );
        let listed = s.call(s.a, A, "worldexchange/get.asp", &[]).await;
        assert_eq!(Record::new(Generation::Four, &listed).unwrap().pid(), A);

        let pikachu = pokemon_record(Generation::Four, 25, 10);
        assert_eq!(
            s.call(s.b, B, "worldexchange/exchange.asp", &offer(&pikachu, A))
                .await,
            FAILED
        );
        assert!(
            s.call(s.a, A, "worldexchange/search.asp", &search(396))
                .await
                .is_empty()
        );
        let found = s
            .call(s.b, B, "worldexchange/search.asp", &search(396))
            .await;
        assert_eq!(found.len(), 292);
        assert_eq!(
            Record::new(Generation::Four, &found).unwrap().species(),
            396
        );
        assert!(
            s.call(s.b, B, "worldexchange/search.asp", &search(25))
                .await
                .is_empty()
        );

        let bidoof = pokemon_record(Generation::Four, 399, 10);
        assert_eq!(
            s.call(s.b, B, "worldexchange/exchange.asp", &offer(&bidoof, A))
                .await,
            REFUSED
        );
        let theirs = s
            .call(s.b, B, "worldexchange/exchange.asp", &offer(&pikachu, A))
            .await;
        assert_eq!(theirs, found);
        assert_eq!(
            s.call(s.b, B, "worldexchange/exchange_finish.asp", &[0; 8])
                .await,
            OK
        );
        assert!(
            s.call(s.b, B, "worldexchange/search.asp", &search(396))
                .await
                .is_empty()
        );

        let result = s.call(s.a, A, "worldexchange/result.asp", &[]).await;
        let received = Record::new(Generation::Four, &result).unwrap();
        assert_eq!(received.species(), 25);
        assert_eq!(received.pid(), A);
        assert_eq!(received.pokemon().unwrap().species, 25);
        assert_eq!(received.bytes[0x121], 1);
        assert_eq!(
            s.call(s.a, A, "worldexchange/return.asp", &[]).await,
            FAILED
        );
        assert_eq!(s.call(s.a, A, "worldexchange/delete.asp", &[]).await, OK);
        assert_eq!(s.call(s.a, A, "worldexchange/result.asp", &[]).await, NONE);
    }

    fn upload5(record: &Record, target: i32) -> Vec<u8> {
        let mut bytes = record.bytes.clone();
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.resize(GEN5_UPLOAD_BYTES, 0);
        bytes
    }

    #[tokio::test]
    async fn a_gen_5_deposit_is_found_traded_and_collected() {
        let s = setup().await;
        assert_eq!(s.call5(s.a, A, "worldexchange/info.asp", &[]).await, OK);
        assert_eq!(
            s.call5(s.a, A, "common/setProfile.asp", &[0; 100]).await,
            vec![0; 8]
        );
        assert_eq!(s.call5(s.a, A, "worldexchange/result.asp", &[]).await, NONE);

        let tepig = pokemon_record(Generation::Five, 498, 6);
        assert_eq!(
            s.call5(s.a, A, "worldexchange/post.asp", &upload5(&tepig, 0))
                .await,
            OK
        );
        assert_eq!(
            s.call5(s.a, A, "worldexchange/post_finish.asp", &[0; 8])
                .await,
            OK
        );
        assert_eq!(
            s.call5(s.a, A, "worldexchange/result.asp", &[]).await,
            WAITING
        );
        let listed = s.call5(s.a, A, "worldexchange/get.asp", &[]).await;
        assert_eq!(Record::new(Generation::Five, &listed).unwrap().pid(), A);

        assert_eq!(
            s.call5(s.a, A, "worldexchange/search.asp", &search(498))
                .await,
            OK
        );
        let found = s
            .call5(s.b, B, "worldexchange/search.asp", &search(498))
            .await;
        assert_eq!(found.len(), 2 + 296);
        assert_eq!(found[..2], OK);
        assert_eq!(
            Record::new(Generation::Five, &found[2..])
                .unwrap()
                .species(),
            498
        );
        assert_eq!(
            s.call5(s.b, B, "worldexchange/search.asp", &search(25))
                .await,
            OK
        );

        let pikachu = pokemon_record(Generation::Five, 25, 10);
        let theirs = s
            .call5(s.b, B, "worldexchange/exchange.asp", &upload5(&pikachu, A))
            .await;
        assert_eq!(theirs, found[2..]);
        assert_eq!(
            s.call5(s.b, B, "worldexchange/exchange_finish.asp", &[0; 8])
                .await,
            OK
        );

        let result = s.call5(s.a, A, "worldexchange/result.asp", &[]).await;
        let received = Record::new(Generation::Five, &result).unwrap();
        assert_eq!(received.species(), 25);
        assert_eq!(received.pid(), A);
        assert_eq!(received.bytes[0x123], 1);
        assert_eq!(s.call5(s.a, A, "worldexchange/delete.asp", &[]).await, OK);
        assert_eq!(s.call5(s.a, A, "worldexchange/delete.asp", &[]).await, NONE);
        assert_eq!(s.call5(s.a, A, "worldexchange/result.asp", &[]).await, NONE);
    }

    #[tokio::test]
    async fn an_untraded_deposit_comes_back_and_a_bad_one_is_refused() {
        let s = setup().await;
        let starly = pokemon_record(Generation::Four, 396, 5);
        assert_eq!(
            s.call(s.a, A, "worldexchange/post.asp", &starly.bytes)
                .await,
            OK
        );
        assert_eq!(
            s.call(s.a, A, "worldexchange/post_finish.asp", &[0; 8])
                .await,
            OK
        );
        assert_eq!(
            s.call(s.a, A, "worldexchange/delete.asp", &[]).await,
            NOT_TRADED
        );
        assert_eq!(s.call(s.a, A, "worldexchange/return.asp", &[]).await, OK);
        assert_eq!(s.call(s.a, A, "worldexchange/result.asp", &[]).await, NONE);
        let mut lying = starly.clone();
        lying.bytes[0xEF] = 99;
        assert_eq!(
            s.call(s.a, A, "worldexchange/post.asp", &lying.bytes).await,
            REFUSED
        );
        assert_eq!(
            s.call(s.a, A, "worldexchange/post_finish.asp", &[0; 8])
                .await,
            FAILED
        );
    }

    #[tokio::test]
    async fn a_token_works_once_and_a_pid_belongs_to_one_account() {
        let s = setup().await;
        assert_eq!(s.call(s.a, A, "worldexchange/info.asp", &[]).await, OK);
        let path = "/pokemondpds/worldexchange/info.asp";
        let GtsReply::Body(token) = s
            .gts
            .handle(&s.db, s.a, &[], &format!("{path}?pid={A}"))
            .await
        else {
            panic!("no token");
        };
        let target = format!(
            "{path}?pid={A}&hash={}&data={}",
            crypt::hash(Generation::Four, &String::from_utf8(token).unwrap()),
            crypt::seal(Generation::Four, A, &[])
        );
        assert_eq!(
            s.gts.handle(&s.db, s.b, &[], &target).await,
            GtsReply::Status(400)
        );
        assert_eq!(
            s.gts.handle(&s.db, s.a, &[], &target).await,
            GtsReply::Status(400)
        );
        let GtsReply::Body(token) = s
            .gts
            .handle(&s.db, s.b, &[], &format!("{path}?pid={A}"))
            .await
        else {
            panic!("no token");
        };
        let target = format!(
            "{path}?pid={A}&hash={}&data={}",
            crypt::hash(Generation::Four, &String::from_utf8(token).unwrap()),
            crypt::seal(Generation::Four, A, &[])
        );
        assert_eq!(
            s.gts.handle(&s.db, s.b, &[], &target).await,
            GtsReply::Status(400)
        );
        assert_eq!(
            s.gts.handle(&s.db, s.a, &[], "/other/info.asp?pid=1").await,
            GtsReply::Status(404)
        );
    }

    #[tokio::test]
    async fn friends_deposits_come_first_in_a_search() {
        let s = setup().await;
        let c =
            s.db.create_account(&[3; 32], &PublicKey([3; 32]), "C", 1)
                .await
                .unwrap()
                .id;
        let starly = pokemon_record(Generation::Four, 396, 5);
        for (account, pid) in [(s.a, A), (c, 3_000_003)] {
            s.call(account, pid, "worldexchange/post.asp", &starly.bytes)
                .await;
            s.call(account, pid, "worldexchange/post_finish.asp", &[0; 8])
                .await;
        }
        let call = Call {
            db: &s.db,
            generation: Generation::Four,
            pid: i64::from(B),
            account: s.b,
            now: crate::now(),
        };
        s.db.gts_claim(i64::from(B), s.b, 4, 1).await.unwrap();
        let mut asked = search(396);
        asked[6] = 1;
        for (friend, pid) in [(s.a, A), (c, 3_000_003)] {
            let found = s.gts.search(&call, &[friend], &asked).await.unwrap();
            assert_eq!(found.len(), 1);
            assert_eq!(Record::new(Generation::Four, &found[0]).unwrap().pid(), pid);
        }
    }

    #[tokio::test]
    async fn browse_lists_others_deposits_and_mine_follows_a_trade() {
        let s = setup().await;
        let starly = pokemon_record(Generation::Four, 396, 5);
        s.call(s.a, A, "worldexchange/post.asp", &starly.bytes)
            .await;
        s.call(s.a, A, "worldexchange/post_finish.asp", &[0; 8])
            .await;

        let ServerMsg::GtsListings {
            total, listings, ..
        } = s
            .gts
            .browse(&s.db, s.b, &[s.a], 4, None, 0, false)
            .await
            .unwrap()
        else {
            panic!("no listings");
        };
        assert_eq!(total, 1);
        assert_eq!(listings[0].offered.species, 396);
        assert_eq!(listings[0].offered.level, 5);
        assert!(listings[0].friend.is_some());
        let ServerMsg::GtsListings { total, .. } = s
            .gts
            .browse(&s.db, s.a, &[], 4, Some(396), 0, false)
            .await
            .unwrap()
        else {
            panic!("no listings");
        };
        assert_eq!(total, 0);
        let mine = s.gts.mine(&s.db, s.a).await.unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].state, GtsState::Waiting);

        s.call(s.b, B, "worldexchange/search.asp", &search(396))
            .await;
        let pikachu = pokemon_record(Generation::Four, 25, 10);
        s.call(s.b, B, "worldexchange/exchange.asp", &offer(&pikachu, A))
            .await;
        s.call(s.b, B, "worldexchange/exchange_finish.asp", &[0; 8])
            .await;
        let traded = s.gts.take_notices();
        assert_eq!(traded.len(), 1);
        assert_eq!(traded[0].0, s.a);
        let mine = s.gts.mine(&s.db, s.a).await.unwrap();
        let GtsState::Traded { received, .. } = &mine[0].state else {
            panic!("not traded");
        };
        assert_eq!(received.species, 25);
        assert_eq!(mine[0].listing.offered.species, 396);
        assert!(s.gts.take_notices().is_empty());
    }

    #[tokio::test]
    async fn a_sim_deposit_is_found_marked_and_traded() {
        let s = setup().await;
        let now = crate::now();
        let sim = crate::db::SimRow {
            pid: 77,
            name: "Ren".into(),
            language: 2,
            version: 12,
            country: 0,
            region: 0,
            tid: 1,
            sid: 2,
            female: false,
            avatar: 0,
            utc_offset: 0,
            seed: 1,
            started_at: now,
            days: 1,
            next_at: now + 86_400,
        };
        assert!(s.db.gts_sim_add(&sim).await.unwrap());
        let mut starly = pokemon_record(Generation::Four, 396, 5);
        starly.stamp(77, now - 60, None);
        assert!(
            s.db.gts_sim_deposit(77, 4, &starly.bytes, sims::listing(&starly), now - 60)
                .await
                .unwrap()
        );

        let ServerMsg::GtsListings {
            listings,
            simulated,
            ..
        } = s
            .gts
            .browse(&s.db, s.b, &[], 4, None, 0, true)
            .await
            .unwrap()
        else {
            panic!("no listings");
        };
        assert!(simulated);
        assert_eq!(listings.len(), 1);
        assert!(listings[0].simulated);
        assert!(listings[0].friend.is_none());

        let found = s
            .call(s.b, B, "worldexchange/search.asp", &search(396))
            .await;
        assert_eq!(found.len(), starly.bytes.len());
        let pikachu = pokemon_record(Generation::Four, 25, 10);
        s.call(s.b, B, "worldexchange/exchange.asp", &offer(&pikachu, 77))
            .await;
        assert_eq!(
            s.call(s.b, B, "worldexchange/exchange_finish.asp", &[0; 8])
                .await,
            OK
        );
        let notices = s.gts.take_notices();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].0, s.b);
        let ServerMsg::GtsTradedWithSim { received, .. } = &notices[0].1 else {
            panic!("no sim notice");
        };
        assert_eq!(received.species, 396);
        let row = s.db.gts_record(77).await.unwrap().unwrap();
        assert!(row.sim && row.traded_at.is_some());
        assert!(s.db.gts_mine(s.b).await.unwrap().is_empty());
    }
}
