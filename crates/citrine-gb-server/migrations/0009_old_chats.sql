ALTER TABLE messages ADD COLUMN old_key INTEGER;
CREATE UNIQUE INDEX messages_old_key ON messages(from_id, to_id, old_key) WHERE old_key IS NOT NULL;
ALTER TABLE devices ADD COLUMN old_chats_wanted INTEGER NOT NULL DEFAULT 0;
UPDATE devices SET old_chats_wanted = 1;
