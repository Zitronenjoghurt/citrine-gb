CREATE TABLE relay_key (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    secret BLOB NOT NULL
);
CREATE TABLE devices (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    key_hash BLOB NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_seen INTEGER NOT NULL
);
CREATE INDEX devices_account ON devices(account_id);
CREATE TABLE account_totp (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    secret BLOB NOT NULL,
    last_step INTEGER NOT NULL
);
CREATE TABLE account_recovery_codes (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    hash TEXT NOT NULL
);
CREATE INDEX account_recovery_codes_account ON account_recovery_codes(account_id);
