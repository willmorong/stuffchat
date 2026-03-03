CREATE TABLE admin_log (
  id TEXT PRIMARY KEY,
  actor_user_id TEXT NOT NULL,
  action_type TEXT NOT NULL,
  action_info TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (actor_user_id) REFERENCES users(id)
);

CREATE INDEX idx_admin_log_created_at ON admin_log(created_at DESC);
CREATE INDEX idx_admin_log_actor_user_id ON admin_log(actor_user_id);
