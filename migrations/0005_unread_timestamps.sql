-- 0005_unread_timestamps.sql
ALTER TABLE channel_unread ADD COLUMN last_read_at TEXT;
ALTER TABLE channel_unread ADD COLUMN last_notified_at TEXT;
