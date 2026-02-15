-- 0008_message_search.sql

-- Membership and message lookup indexes used by search/context queries.
CREATE INDEX IF NOT EXISTS idx_channel_members_user_read_channel
ON channel_members(user_id, can_read, channel_id);

CREATE INDEX IF NOT EXISTS idx_messages_channel_created_id_active
ON messages(channel_id, created_at DESC, id DESC)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_messages_user_created_id_active
ON messages(user_id, created_at DESC, id DESC)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_users_username_nocase
ON users(username COLLATE NOCASE);

-- Full-text index for message content.
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
USING fts5(content, tokenize = 'unicode61 remove_diacritics 2');

-- Backfill active messages.
INSERT INTO messages_fts(rowid, content)
SELECT rowid, COALESCE(content, '')
FROM messages
WHERE deleted_at IS NULL;

-- Keep messages_fts synchronized with messages.
CREATE TRIGGER IF NOT EXISTS messages_fts_ai
AFTER INSERT ON messages
WHEN NEW.deleted_at IS NULL
BEGIN
  INSERT INTO messages_fts(rowid, content)
  VALUES (NEW.rowid, COALESCE(NEW.content, ''));
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_au_content_deleted
AFTER UPDATE OF content, deleted_at ON messages
BEGIN
  DELETE FROM messages_fts WHERE rowid = OLD.rowid;
  INSERT INTO messages_fts(rowid, content)
  SELECT NEW.rowid, COALESCE(NEW.content, '')
  WHERE NEW.deleted_at IS NULL;
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_ad
AFTER DELETE ON messages
BEGIN
  DELETE FROM messages_fts WHERE rowid = OLD.rowid;
END;
