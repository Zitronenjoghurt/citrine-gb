CREATE TABLE gts_profiles (
    pid INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    profile BLOB,
    last_search INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX gts_profiles_account ON gts_profiles(account_id);
CREATE TABLE gts_records (
    pid INTEGER PRIMARY KEY REFERENCES gts_profiles(pid) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    record BLOB NOT NULL,
    received BLOB,
    species INTEGER NOT NULL,
    gender INTEGER NOT NULL,
    level INTEGER NOT NULL,
    wanted_species INTEGER NOT NULL,
    wanted_gender INTEGER NOT NULL,
    wanted_min INTEGER NOT NULL,
    wanted_max INTEGER NOT NULL,
    country INTEGER NOT NULL,
    deposited_at INTEGER NOT NULL,
    traded_at INTEGER,
    traded_with INTEGER
);
CREATE INDEX gts_records_search ON gts_records(generation, species, traded_at);
