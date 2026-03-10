CREATE TABLE push_devices (
  user_id TEXT NOT NULL,
  installation_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  push_token TEXT NOT NULL,
  environment TEXT NOT NULL,
  message_notifications INTEGER NOT NULL DEFAULT 1,
  call_notifications INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (user_id, installation_id, platform),
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_push_devices_user_id ON push_devices(user_id);

CREATE TABLE push_events (
  id TEXT PRIMARY KEY,
  event_id TEXT NOT NULL UNIQUE,
  event_type TEXT NOT NULL,
  channel_id TEXT NOT NULL,
  actor_user_id TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  attempt_count INTEGER NOT NULL DEFAULT 0,
  next_attempt_at TEXT NOT NULL,
  last_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  dispatched_at TEXT,
  FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE,
  FOREIGN KEY (actor_user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_push_events_ready
ON push_events(dispatched_at, next_attempt_at, created_at);
