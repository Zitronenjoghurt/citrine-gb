CREATE TABLE push_tokens (
    device_id INTEGER PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    token TEXT NOT NULL UNIQUE,
    kinds INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX push_tokens_account ON push_tokens(account_id);
