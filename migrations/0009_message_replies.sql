-- 0009_message_replies.sql

ALTER TABLE messages ADD COLUMN replying_to TEXT REFERENCES messages(id) ON DELETE SET NULL;
