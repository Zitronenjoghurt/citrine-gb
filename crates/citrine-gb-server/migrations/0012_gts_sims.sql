CREATE TABLE gts_sims (
    pid INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    language INTEGER NOT NULL,
    version INTEGER NOT NULL,
    country INTEGER NOT NULL,
    region INTEGER NOT NULL,
    tid INTEGER NOT NULL,
    sid INTEGER NOT NULL,
    female INTEGER NOT NULL,
    avatar INTEGER NOT NULL,
    utc_offset INTEGER NOT NULL,
    seed INTEGER NOT NULL,
    started_at INTEGER NOT NULL,
    days INTEGER NOT NULL,
    next_at INTEGER NOT NULL
);
CREATE TABLE gts_sim_records (
    pid INTEGER PRIMARY KEY REFERENCES gts_sims(pid) ON DELETE CASCADE,
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
CREATE INDEX gts_sim_records_search ON gts_sim_records(generation, species, traded_at);
ALTER TABLE gts_records ADD COLUMN traded_with_sim INTEGER NOT NULL DEFAULT 0;
