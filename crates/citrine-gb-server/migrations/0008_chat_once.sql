ALTER TABLE messages ADD COLUMN sender_key INTEGER;
CREATE UNIQUE INDEX messages_sender_key ON messages(from_id, sender_key) WHERE sender_key IS NOT NULL;
