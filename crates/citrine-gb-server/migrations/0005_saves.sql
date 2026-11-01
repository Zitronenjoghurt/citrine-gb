CREATE TABLE saves (
    owner INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    game BLOB NOT NULL,
    slot TEXT NOT NULL,
    version INTEGER NOT NULL,
    file BLOB NOT NULL UNIQUE REFERENCES files(id) ON DELETE CASCADE,
    label BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (owner, game, slot)
);
