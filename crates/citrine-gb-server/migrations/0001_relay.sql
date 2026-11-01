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
