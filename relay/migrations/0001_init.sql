CREATE TABLE relay_servers (
  server_id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  secret TEXT NOT NULL,
  created_at TEXT NOT NULL,
  revoked_at TEXT
);

CREATE TABLE push_deliveries (
  server_id TEXT NOT NULL,
  event_id TEXT NOT NULL,
  installation_id TEXT NOT NULL,
  status TEXT NOT NULL,
  last_error TEXT,
  apns_id TEXT,
  updated_at TEXT NOT NULL,
  delivered_at TEXT,
  PRIMARY KEY (server_id, event_id, installation_id)
);

CREATE TABLE request_nonces (
  server_id TEXT NOT NULL,
  nonce TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (server_id, nonce)
);

CREATE INDEX idx_request_nonces_created_at ON request_nonces(created_at);

