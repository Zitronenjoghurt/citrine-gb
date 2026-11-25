CREATE TABLE ds_profiles (
    pid INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);
CREATE INDEX ds_profiles_account ON ds_profiles(account_id);
