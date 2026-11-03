use super::*;
use citrine_gb_net::{FileInfo, FriendState, Sealed};
use futures_util::future::join_all;
use sqlx::migrate::{Migration, MigrationType};
use sqlx::{AssertSqlSafe, SqlSafeStr};

const RUSQLITE_SQL: &[&str] = &[
    r"
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    online_id INTEGER NOT NULL UNIQUE,
    auth_hash BLOB NOT NULL UNIQUE,
    public_key BLOB NOT NULL,
    display_name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    quota_bytes INTEGER,
    banned INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE friend_requests (
    from_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    to_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (from_id, to_id)
);
CREATE TABLE friendships (
    a INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    b INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    since INTEGER NOT NULL,
    PRIMARY KEY (a, b),
    CHECK (a < b)
);
CREATE TABLE blocks (
    blocker INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    blocked INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    PRIMARY KEY (blocker, blocked)
);
CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    from_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    to_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    sent_at INTEGER NOT NULL,
    nonce BLOB NOT NULL,
    body BLOB NOT NULL
);
CREATE INDEX messages_to ON messages(to_id, id);
CREATE TABLE files (
    id BLOB PRIMARY KEY,
    owner INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    size INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX files_owner ON files(owner);
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
    ",
    r"
CREATE TABLE admin_totp (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    secret BLOB NOT NULL,
    last_step INTEGER NOT NULL
);
CREATE TABLE admin_recovery_codes (
    id INTEGER PRIMARY KEY,
    hash TEXT NOT NULL
);
    ",
];

pub(super) async fn db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("t.db")).await.unwrap();
    (dir, db)
}

pub(super) async fn account(db: &Db, seed: u8, name: &str) -> AccountRow {
    db.create_account(&[seed; 32], &PublicKey([seed; 32]), name, 10)
        .await
        .unwrap()
}

#[tokio::test]
async fn requests_are_added_once_counted_and_expire() {
    let (_dir, db) = db().await;
    let a = account(&db, 1, "a").await;
    let b = account(&db, 2, "b").await;
    let c = account(&db, 3, "c").await;
    assert!(db.add_request(a.id, b.id, 100).await.unwrap());
    assert!(!db.add_request(a.id, b.id, 200).await.unwrap());
    assert!(db.add_request(a.id, c.id, 300).await.unwrap());
    assert!(db.add_request(c.id, b.id, 300).await.unwrap());
    assert_eq!(db.requests_of(a.id).await.unwrap(), (2, 0));
    assert_eq!(db.requests_of(b.id).await.unwrap(), (0, 2));
    assert_eq!(db.purge_requests(200).await.unwrap(), 1);
    assert_eq!(db.requests_of(b.id).await.unwrap(), (0, 1));
}

#[tokio::test]
async fn banned_accounts_leave_every_list() {
    let (_dir, db) = db().await;
    let a = account(&db, 1, "a").await;
    let b = account(&db, 2, "b").await;
    let c = account(&db, 3, "c").await;
    let d = account(&db, 4, "d").await;
    db.befriend(a.id, b.id, 10).await.unwrap();
    db.add_request(c.id, a.id, 10).await.unwrap();
    db.add_request(a.id, d.id, 10).await.unwrap();
    assert_eq!(db.relations(a.id).await.unwrap().len(), 3);
    for id in [b.id, c.id, d.id] {
        db.set_banned(id, true).await.unwrap();
    }
    assert!(db.relations(a.id).await.unwrap().is_empty());
    assert_eq!(db.requests_of(a.id).await.unwrap(), (0, 0));
    db.set_banned(b.id, false).await.unwrap();
    assert_eq!(db.relations(a.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn migrations_run_once_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let db = Db::open(&path).await.unwrap();
        account(&db, 1, "a").await;
    }
    let db = Db::open(&path).await.unwrap();
    assert_eq!(db.count_accounts().await.unwrap(), 1);
}

#[tokio::test]
async fn accounts_by_auth_and_online_id() {
    let (_dir, db) = db().await;
    let a = account(&db, 1, "Clara").await;
    assert_eq!(
        db.account_by_auth(&[1; 32]).await.unwrap().unwrap().id,
        a.id
    );
    assert!(db.account_by_auth(&[2; 32]).await.unwrap().is_none());
    assert_eq!(
        db.account_by_online(a.online_id)
            .await
            .unwrap()
            .unwrap()
            .display_name,
        "Clara"
    );
    assert!(
        db.create_account(&[1; 32], &PublicKey([9; 32]), "x", 1)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn relations_cover_every_state() {
    let (_dir, db) = db().await;
    let me = account(&db, 1, "me").await;
    let friend = account(&db, 2, "friend").await;
    let incoming = account(&db, 3, "incoming").await;
    let outgoing = account(&db, 4, "outgoing").await;
    let blocked = account(&db, 5, "blocked").await;
    db.befriend(friend.id, me.id, 1).await.unwrap();
    db.add_request(incoming.id, me.id, 1).await.unwrap();
    db.add_request(me.id, outgoing.id, 1).await.unwrap();
    db.add_request(blocked.id, me.id, 1).await.unwrap();
    db.block(me.id, blocked.id).await.unwrap();
    let mut got: Vec<(String, FriendState)> = db
        .relations(me.id)
        .await
        .unwrap()
        .into_iter()
        .map(|(a, s)| (a.display_name, s))
        .collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        got,
        vec![
            ("blocked".into(), FriendState::Blocked),
            ("friend".into(), FriendState::Friend),
            ("incoming".into(), FriendState::Incoming),
            ("outgoing".into(), FriendState::Outgoing),
        ]
    );
    assert!(db.are_friends(me.id, friend.id).await.unwrap());
    assert_eq!(db.friend_ids(me.id).await.unwrap(), vec![friend.id]);
    db.unfriend(me.id, friend.id).await.unwrap();
    assert!(!db.are_friends(friend.id, me.id).await.unwrap());
}

#[tokio::test]
async fn search_treats_wildcards_as_text() {
    let (_dir, db) = db().await;
    account(&db, 1, "100%_lemon").await;
    account(&db, 2, "plain").await;
    assert_eq!(
        db.users("%", UserOrder::LastSeen, 0, 0, 10)
            .await
            .unwrap()
            .1,
        1
    );
    assert_eq!(
        db.users("_", UserOrder::LastSeen, 0, 0, 10)
            .await
            .unwrap()
            .1,
        1
    );
    assert_eq!(
        db.users("", UserOrder::LastSeen, 0, 0, 10).await.unwrap().1,
        2
    );
    assert_eq!(
        db.users("' OR 1=1 --", UserOrder::LastSeen, 0, 0, 10)
            .await
            .unwrap()
            .1,
        0
    );
}

async fn auto_vacuum(db: &Db) -> i64 {
    sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

async fn pages(db: &Db) -> i64 {
    sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn deleted_messages_give_their_space_back() {
    let (dir, db) = db().await;
    let a = account(&db, 1, "a").await;
    let b = account(&db, 2, "b").await;
    let empty = pages(&db).await;
    let sealed = Sealed {
        nonce: [3; 24],
        ciphertext: vec![0x5A; 16 << 10],
    };
    let mut ids = Vec::new();
    for i in 0..64 {
        ids.push(
            db.insert_message(a.id, b.id, None, i, &sealed)
                .await
                .unwrap(),
        );
    }
    db.reclaim().await.unwrap();
    let full = pages(&db).await;
    let full_bytes = std::fs::metadata(dir.path().join("t.db")).unwrap().len();
    assert!(full > empty + 200, "{empty} then {full} pages");
    assert_eq!(db.stored_messages().await.unwrap().0, ids.len() as u64);
    db.delete_account(a.id).await.unwrap();
    assert_eq!(db.stored_messages().await.unwrap().0, 0);
    let freed = db.reclaim().await.unwrap();
    assert!(freed >= 1 << 20, "{freed} bytes freed");
    assert!(pages(&db).await < empty + 10);
    let bytes = std::fs::metadata(dir.path().join("t.db")).unwrap().len();
    assert!(bytes < full_bytes / 4, "{full_bytes} then {bytes} bytes");
    assert_eq!(db.reclaim().await.unwrap(), 0);
}

async fn joined(db: &Db, seed: u8, at: u64) -> AccountRow {
    let name = format!("user {seed}");
    db.create_account(&[seed; 32], &PublicKey([seed; 32]), &name, at)
        .await
        .unwrap()
}

fn sealed() -> Sealed {
    Sealed {
        nonce: [0; citrine_gb_net::NONCE_BYTES],
        ciphertext: vec![1, 2, 3],
    }
}

#[tokio::test]
async fn users_sort_by_join_time_and_filter_by_it() {
    let (_dir, db) = db().await;
    let old = joined(&db, 1, 100).await;
    let new = joined(&db, 2, 200).await;
    db.set_last_seen(old.id, 900).await.unwrap();
    let ids = |rows: Vec<UserRow>| rows.iter().map(|r| r.account.id).collect::<Vec<_>>();
    let (seen, _) = db.users("", UserOrder::LastSeen, 0, 0, 10).await.unwrap();
    assert_eq!(ids(seen), vec![old.id, new.id]);
    let (newest, _) = db.users("", UserOrder::Newest, 0, 0, 10).await.unwrap();
    assert_eq!(newest[0].joined, 200);
    assert_eq!(ids(newest), vec![new.id, old.id]);
    let (since, total) = db.users("", UserOrder::Newest, 150, 0, 10).await.unwrap();
    assert_eq!((ids(since), total), (vec![new.id], 1));
}

#[tokio::test]
async fn a_bulk_ban_takes_exactly_the_accounts_after_the_cut() {
    let (_dir, db) = db().await;
    let old = joined(&db, 1, 100).await;
    let a = joined(&db, 2, 200).await;
    let b = joined(&db, 3, 300).await;
    db.add_request(a.id, old.id, 200).await.unwrap();
    db.set_banned(b.id, true).await.unwrap();
    assert_eq!(db.count_joined_since(200).await.unwrap(), 1);
    assert_eq!(db.ban_joined_since(200).await.unwrap(), vec![a.id]);
    assert!(db.account(a.id).await.unwrap().unwrap().banned);
    assert!(!db.account(old.id).await.unwrap().unwrap().banned);
    assert_eq!(db.requests_of(old.id).await.unwrap(), (0, 0));
    assert!(db.ban_joined_since(200).await.unwrap().is_empty());
}

#[tokio::test]
async fn deleting_an_account_removes_its_rows_everywhere() {
    let (_dir, db) = db().await;
    let a = account(&db, 1, "a").await;
    let b = account(&db, 2, "b").await;
    let c = account(&db, 3, "c").await;
    db.befriend(a.id, b.id, 10).await.unwrap();
    db.add_request(a.id, c.id, 10).await.unwrap();
    db.block(c.id, a.id).await.unwrap();
    db.add_device(b.id, &[9; 32], "Mac", 10).await.unwrap();
    db.insert_message(a.id, b.id, None, 10, &sealed())
        .await
        .unwrap();
    db.insert_message(b.id, a.id, None, 10, &sealed())
        .await
        .unwrap();
    db.insert_file(FileId([7; 16]), a.id, 10, 10).await.unwrap();
    assert_eq!(db.waiting_messages().await.unwrap(), (1, 3));
    assert_eq!(db.stored_messages().await.unwrap(), (2, 6));
    assert_eq!(
        db.delete_account(a.id).await.unwrap(),
        Some(vec![FileId([7; 16])])
    );
    assert!(db.account(a.id).await.unwrap().is_none());
    assert!(db.friend_ids(b.id).await.unwrap().is_empty());
    assert_eq!(db.requests_of(c.id).await.unwrap(), (0, 0));
    assert_eq!(db.stored_messages().await.unwrap(), (0, 0));
    assert_eq!(db.total_storage().await.unwrap(), 0);
    assert!(!db.is_blocked(c.id, a.id).await.unwrap());
    assert_eq!(db.delete_account(a.id).await.unwrap(), None);
}

#[tokio::test]
async fn empty_accounts_are_unseen_and_own_nothing() {
    let (_dir, db) = db().await;
    let empty = joined(&db, 1, 100).await;
    let friend = joined(&db, 2, 100).await;
    let other = joined(&db, 3, 100).await;
    let files = joined(&db, 4, 100).await;
    let banned = joined(&db, 5, 100).await;
    let recent = joined(&db, 6, 100).await;
    db.befriend(friend.id, other.id, 100).await.unwrap();
    db.insert_file(FileId([1; 16]), files.id, 10, 100)
        .await
        .unwrap();
    db.set_banned(banned.id, true).await.unwrap();
    db.set_last_seen(recent.id, 500).await.unwrap();
    assert_eq!(db.empty_accounts(400).await.unwrap(), vec![empty.id]);
    assert!(db.empty_accounts(50).await.unwrap().is_empty());
}

#[tokio::test]
async fn files_and_storage() {
    let (_dir, db) = db().await;
    let a = account(&db, 1, "a").await;
    db.insert_file(FileId([1; 16]), a.id, 100, 5).await.unwrap();
    db.insert_file(FileId([2; 16]), a.id, 50, 6).await.unwrap();
    assert_eq!(db.storage_used(a.id).await.unwrap(), 8192);
    assert_eq!(db.total_storage().await.unwrap(), 8192);
    assert!(!db.delete_file(FileId([1; 16]), a.id + 1).await.unwrap());
    assert!(db.delete_file(FileId([1; 16]), a.id).await.unwrap());
    assert_eq!(
        db.delete_files_of(a.id).await.unwrap(),
        vec![FileId([2; 16])]
    );
    assert_eq!(db.storage_used(a.id).await.unwrap(), 0);
}

#[tokio::test]
async fn settings_upsert_and_delete() {
    let (_dir, db) = db().await;
    db.set_setting("relay", "on").await.unwrap();
    db.set_setting("relay", "off").await.unwrap();
    assert_eq!(db.settings().await.unwrap()["relay"], "off");
    db.delete_setting("relay").await.unwrap();
    assert!(db.settings().await.unwrap().is_empty());
}

const RUSQLITE_ROWS: &str = r"
INSERT INTO accounts (id, online_id, auth_hash, public_key, display_name, created_at, last_seen, quota_bytes, banned) VALUES
    (1, 123456789, X'0101010101010101010101010101010101010101010101010101010101010101', X'1111111111111111111111111111111111111111111111111111111111111111', 'Clara', 100, 200, NULL, 0),
    (2, 987654321, X'0202020202020202020202020202020202020202020202020202020202020202', X'2222222222222222222222222222222222222222222222222222222222222222', 'Lime', 101, 201, 5242880, 0),
    (3, 555555555, X'0303030303030303030303030303030303030303030303030303030303030303', X'3333333333333333333333333333333333333333333333333333333333333333', 'Troll', 102, 202, NULL, 1),
    (4, 444444444, X'0404040404040404040404040404040404040404040404040404040404040404', X'4444444444444444444444444444444444444444444444444444444444444444', 'Pending', 103, 203, NULL, 0);
INSERT INTO friendships (a, b, since) VALUES (1, 2, 300);
INSERT INTO friend_requests (from_id, to_id, created_at) VALUES (4, 1, 301);
INSERT INTO blocks (blocker, blocked) VALUES (1, 3);
INSERT INTO messages (id, from_id, to_id, sent_at, nonce, body) VALUES (7, 2, 1, 400, X'ABABABABABABABABABABABABABABABABABABABABABABABAB', X'C0FFEE');
INSERT INTO files (id, owner, size, created_at) VALUES (X'5A5A5A5A5A5A5A5A5A5A5A5A5A5A5A5A', 2, 1234, 500);
INSERT INTO settings (key, value) VALUES ('name', 'Lemon relay'), ('relay_password', '$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA');
INSERT INTO admin_totp (id, secret, last_step) VALUES (1, X'7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E7E', 58726080);
INSERT INTO admin_recovery_codes (id, hash) VALUES (1, '$argon2id$one'), (2, '$argon2id$two');
";

async fn raw(path: &Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true);
    SqlitePool::connect_with(options).await.unwrap()
}

async fn rusqlite_db(path: &Path, version: usize) {
    let pool = raw(path).await;
    for (i, sql) in RUSQLITE_SQL.iter().enumerate().take(version) {
        let mut tx = pool.begin().await.unwrap();
        sqlx::raw_sql(*sql).execute(&mut *tx).await.unwrap();
        sqlx::raw_sql(AssertSqlSafe(format!("PRAGMA user_version = {}", i + 1)))
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    if version == RUSQLITE_SQL.len() {
        sqlx::raw_sql(RUSQLITE_ROWS).execute(&pool).await.unwrap();
    }
    pool.close().await;
}

async fn applied(db: &Db) -> Vec<(i64, bool)> {
    sqlx::query!(
        r#"SELECT version AS "version!", execution_time = -1 AS "skipped!: bool"
               FROM _sqlx_migrations ORDER BY version"#
    )
    .fetch_all(&db.pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| (r.version, r.skipped))
    .collect()
}

fn skipped_up_to(version: i64) -> Vec<(i64, bool)> {
    MIGRATOR
        .iter()
        .map(|m| (m.version, m.version <= version))
        .collect()
}

async fn schema(db: &Db) -> Vec<(String, String, String, Option<String>)> {
    sqlx::query!(
            r#"SELECT type AS "kind!: String", name AS "name!: String", tbl_name AS "table!: String", sql
               FROM sqlite_master ORDER BY type, name"#
        )
        .fetch_all(&db.pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| (r.kind, r.name, r.table, r.sql))
        .collect()
}

#[tokio::test]
async fn a_rusqlite_database_keeps_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    rusqlite_db(&path, 2).await;
    let db = Db::open(&path).await.unwrap();
    assert_eq!(applied(&db).await, skipped_up_to(2));

    assert_eq!(db.count_accounts().await.unwrap(), 4);
    let clara = db.account_by_auth(&[1; 32]).await.unwrap().unwrap();
    assert_eq!(clara.id, 1);
    assert_eq!(clara.online_id.value(), 123_456_789);
    assert_eq!(clara.public_key, PublicKey([0x11; 32]));
    assert_eq!(clara.display_name, "Clara");
    assert_eq!(clara.last_seen, 200);
    assert_eq!(clara.quota_bytes, None);
    assert!(!clara.banned);
    let lime = db.account(2).await.unwrap().unwrap();
    assert_eq!(lime.quota_bytes, Some(5_242_880));
    assert_eq!(lime.online_id.value(), 987_654_321);
    assert!(db.account(3).await.unwrap().unwrap().banned);

    let mut relations: Vec<(String, FriendState)> = db
        .relations(1)
        .await
        .unwrap()
        .into_iter()
        .map(|(a, s)| (a.display_name, s))
        .collect();
    relations.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        relations,
        vec![
            ("Lime".into(), FriendState::Friend),
            ("Pending".into(), FriendState::Incoming),
            ("Troll".into(), FriendState::Blocked),
        ]
    );

    let device = db.add_device(1, &[0x71; 32], "Mac", 600).await.unwrap();
    let pending = db.pending_messages(device, 0, 200).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, 7);
    assert_eq!(pending[0].from, lime.online_id);
    assert_eq!(pending[0].at, 400);
    assert_eq!(
        pending[0].sealed,
        Sealed {
            nonce: [0xAB; 24],
            ciphertext: vec![0xC0, 0xFF, 0xEE],
        }
    );

    assert_eq!(
        db.files_of(2).await.unwrap(),
        vec![FileInfo {
            id: FileId([0x5A; 16]),
            size: 1234,
            created: 500,
        }]
    );
    assert_eq!(db.storage_used(2).await.unwrap(), 4096);

    let settings = db.settings().await.unwrap();
    assert_eq!(settings.len(), 2);
    assert_eq!(settings["name"], "Lemon relay");
    assert_eq!(
        settings["relay_password"],
        "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
    );

    assert_eq!(db.admin_totp().await.unwrap(), Some(vec![0x7E; 20]));
    assert_eq!(
        db.admin_recovery_codes().await.unwrap(),
        vec![(1, "$argon2id$one".into()), (2, "$argon2id$two".into())]
    );
    assert!(!db.use_admin_step(58_726_080).await.unwrap());
    assert!(db.use_admin_step(58_726_081).await.unwrap());

    db.pool.close().await;
    let db = Db::open(&path).await.unwrap();
    assert_eq!(applied(&db).await, skipped_up_to(2));
    assert_eq!(db.count_accounts().await.unwrap(), 4);

    let later = MIGRATOR.iter().map(|m| m.version).max().unwrap() + 1;
    let next = Migration::new(
            later,
            "later".into(),
            MigrationType::Simple,
            "CREATE TABLE later (id INTEGER PRIMARY KEY); INSERT INTO later (id) SELECT id FROM accounts;"
                .into_sql_str(),
            false,
        );
    Migrator::with_migrations(MIGRATOR.iter().cloned().chain([next]).collect())
        .run(&db.pool)
        .await
        .unwrap();
    let mut expected = skipped_up_to(2);
    expected.push((later, false));
    assert_eq!(applied(&db).await, expected);
    assert!(
        schema(&db)
            .await
            .iter()
            .any(|(kind, name, _, _)| kind == "table" && name == "later")
    );
    assert_eq!(db.count_accounts().await.unwrap(), 4);
}

#[tokio::test]
async fn a_first_version_rusqlite_database_gets_the_later_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    rusqlite_db(&path, 1).await;
    let db = Db::open(&path).await.unwrap();
    assert_eq!(applied(&db).await, skipped_up_to(1));
    assert_eq!(db.admin_totp().await.unwrap(), None);
    account(&db, 1, "a").await;
    assert_eq!(db.count_accounts().await.unwrap(), 1);
}

#[tokio::test]
async fn a_fresh_database_has_the_upgraded_schema() {
    let dir = tempfile::tempdir().unwrap();
    let fresh = Db::open(&dir.path().join("fresh.db")).await.unwrap();
    let path = dir.path().join("old.db");
    rusqlite_db(&path, 2).await;
    let upgraded = Db::open(&path).await.unwrap();
    let fresh_schema = schema(&fresh).await;
    assert!(fresh_schema.len() > 10);
    assert_eq!(fresh_schema, schema(&upgraded).await);
    assert_eq!(applied(&fresh).await, skipped_up_to(0));
    assert_eq!(auto_vacuum(&fresh).await, 2);
    assert_eq!(auto_vacuum(&upgraded).await, 2);
}

#[tokio::test]
async fn writers_at_once_never_find_the_database_busy() {
    let (_dir, db) = db().await;
    let mut ids = Vec::new();
    for seed in 1..=40 {
        ids.push(joined(&db, seed, u64::from(seed)).await.id);
    }
    let deletes = ids[..20].iter().map(|&id| db.delete_account(id));
    let bans = (21..=40).rev().map(|since| db.ban_joined_since(since));
    let seen = ids[20..].iter().map(|&id| db.set_last_seen(id, 99));
    let (deleted, banned, seen) = tokio::join!(join_all(deletes), join_all(bans), join_all(seen));
    for done in deleted {
        assert!(done.unwrap().is_some());
    }
    for done in banned {
        done.unwrap();
    }
    for done in seen {
        done.unwrap();
    }
    assert_eq!(db.count_accounts().await.unwrap(), 20);
    for &id in &ids[20..] {
        assert!(db.account(id).await.unwrap().unwrap().banned);
    }
}
