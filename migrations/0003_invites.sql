CREATE TABLE invites (
    code TEXT PRIMARY KEY,
    created_by TEXT NOT NULL,
    joined_user_id TEXT,
    created_at DATETIME NOT NULL,
    FOREIGN KEY (created_by) REFERENCES users(id),
    FOREIGN KEY (joined_user_id) REFERENCES users(id)
);
