-- 0006_custom_emojis.sql

CREATE TABLE custom_emojis (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    deleted_at TEXT,
    FOREIGN KEY (created_by) REFERENCES users(id)
);

CREATE INDEX idx_emoji_name ON custom_emojis(name);
