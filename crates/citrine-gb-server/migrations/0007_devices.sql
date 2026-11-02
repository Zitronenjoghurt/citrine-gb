ALTER TABLE devices ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE devices ADD COLUMN left_at INTEGER;
CREATE TABLE signed_out_devices (
    key_hash BLOB PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    at INTEGER NOT NULL
);
ALTER TABLE messages ADD COLUMN sender_keeps INTEGER NOT NULL DEFAULT 1;
ALTER TABLE messages ADD COLUMN receiver_keeps INTEGER NOT NULL DEFAULT 1;
CREATE INDEX messages_from ON messages(from_id, id);
CREATE TABLE message_deliveries (
    device_id INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    PRIMARY KEY (device_id, message_id)
) WITHOUT ROWID;
CREATE INDEX message_deliveries_message ON message_deliveries(message_id);
INSERT INTO message_deliveries (device_id, message_id)
    SELECT d.id, m.id FROM messages m JOIN devices d ON d.account_id = m.to_id;
