CREATE TABLE archive_games (
    id BLOB PRIMARY KEY NOT NULL,
    sealed BLOB NOT NULL,
    wrapped_key BLOB NOT NULL,
    stored INTEGER NOT NULL,
    added_at INTEGER NOT NULL
);
CREATE TABLE archive_access (
    account INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    game BLOB REFERENCES archive_games(id) ON DELETE CASCADE,
    granted_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX archive_access_one ON archive_access(account, ifnull(game, x''));
CREATE INDEX archive_access_game ON archive_access(game);
