-- 0002_presence_and_profile_pictures.sql

-- Presence: store last heartbeat and a desired status
-- Offline can be computed when last_heartbeat is older than a threshold.
CREATE TABLE IF NOT EXISTS presence (
  user_id TEXT PRIMARY KEY,
  last_heartbeat TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'online', -- 'online' | 'away' | 'dnd' | 'offline' | 'invisible'
  updated_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Profile pictures: map user -> file
CREATE TABLE IF NOT EXISTS profile_pictures (
  user_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  set_at TEXT NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (file_id) REFERENCES files(id)
);

-- Optional: backfill presence rows for existing users as offline-now
INSERT OR IGNORE INTO presence (user_id, last_heartbeat, status, updated_at)
SELECT id, DATETIME('now'), 'offline', DATETIME('now') FROM users;