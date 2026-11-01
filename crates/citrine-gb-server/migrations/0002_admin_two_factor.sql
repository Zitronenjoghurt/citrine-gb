CREATE TABLE admin_totp (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    secret BLOB NOT NULL,
    last_step INTEGER NOT NULL
);
CREATE TABLE admin_recovery_codes (
    id INTEGER PRIMARY KEY,
    hash TEXT NOT NULL
);
