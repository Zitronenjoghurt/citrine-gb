CREATE TABLE invites (
    id INTEGER PRIMARY KEY,
    code INTEGER NOT NULL UNIQUE,
    note TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    used_at INTEGER,
    used_by INTEGER REFERENCES accounts(id) ON DELETE SET NULL
);
